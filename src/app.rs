use std::time::Duration;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::action::Action;
use crate::components::Component;
use crate::components::layout_manager::LayoutManager;
use crate::components::request_builder::RequestBuilder;
use crate::components::status_bar::StatusBar;
use crate::event::{Event, EventHandler};
use crate::state::{
    AppState, KeyValueEditorState, KeyValueField, KeyValueRow, MAX_HEADER_ROWS, MAX_KEY_LENGTH,
    MAX_QUERY_PARAM_ROWS, MAX_URL_LENGTH, MAX_VALUE_LENGTH, QueryParamToken, RequestTab,
    SyncDirection,
};
use crate::tui::Tui;
use crate::util::url_parser::{parse_query_params, rebuild_url_with_params};

pub struct App {
    state: AppState,
    events: EventHandler,
    request_builder: RequestBuilder,
    status_bar: StatusBar,
}

impl App {
    pub fn new(initial_size: (u16, u16)) -> Self {
        let layout_mode = LayoutManager::mode_for_dimensions(initial_size.0, initial_size.1);
        let state = AppState::new(initial_size, layout_mode);

        Self {
            state,
            events: EventHandler::new(Duration::from_millis(250), Duration::from_millis(33)),
            request_builder: RequestBuilder,
            status_bar: StatusBar,
        }
    }

    pub async fn run(&mut self, tui: &mut Tui) -> std::io::Result<()> {
        self.render(tui)?;

        while let Some(event) = self.events.next().await {
            let actions = self.map_event_to_actions(event);

            for action in actions {
                self.apply_action(action.clone());
                self.request_builder.handle_action(&action, &self.state);
                self.status_bar.handle_action(&action, &self.state);

                if matches!(action, Action::Render) {
                    self.render(tui)?;
                }

                if self.state.should_quit {
                    return Ok(());
                }
            }
        }

        Ok(())
    }

    fn render(&mut self, tui: &mut Tui) -> std::io::Result<()> {
        tui.draw(|frame| {
            let pane_layout = LayoutManager::compute(frame.area());
            self.request_builder
                .render(frame, pane_layout.main, &self.state);
            self.status_bar
                .render(frame, pane_layout.status, &self.state);
        })
    }

    fn map_event_to_actions(&mut self, event: Event) -> Vec<Action> {
        match event {
            Event::Tick => vec![Action::Tick],
            Event::Render => vec![Action::Render],
            Event::Resize(width, height) => vec![Action::Resize(width, height), Action::Render],
            Event::Key(key_event) => self.map_key_event_to_actions(key_event),
        }
    }

    fn map_key_event_to_actions(&mut self, key_event: KeyEvent) -> Vec<Action> {
        match (key_event.code, key_event.modifiers) {
            (KeyCode::Char('q'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
                return vec![Action::Quit];
            }
            (KeyCode::Tab, _) => {
                return vec![Action::FocusNext, Action::Render];
            }
            (KeyCode::BackTab, _) => {
                return vec![Action::FocusPrev, Action::Render];
            }
            _ => {}
        }

        let mut actions = match self.state.request.focus {
            crate::state::RequestFocus::Method => self.handle_method_key(key_event),
            crate::state::RequestFocus::Url => self.handle_url_key(key_event),
            crate::state::RequestFocus::Tabs => self.handle_tab_key(key_event),
            crate::state::RequestFocus::Editor => self.handle_editor_key(key_event),
        };

        if !actions.is_empty() {
            actions.push(Action::Render);
        }

        actions
    }

    fn handle_method_key(&self, key_event: KeyEvent) -> Vec<Action> {
        match key_event.code {
            KeyCode::Left | KeyCode::Up => {
                vec![Action::SetMethod(self.state.request.method.prev())]
            }
            KeyCode::Right | KeyCode::Down => {
                vec![Action::SetMethod(self.state.request.method.next())]
            }
            KeyCode::Char('j') => vec![Action::SetMethod(self.state.request.method.next())],
            KeyCode::Char('k') => vec![Action::SetMethod(self.state.request.method.prev())],
            _ => Vec::new(),
        }
    }

    fn handle_url_key(&mut self, key_event: KeyEvent) -> Vec<Action> {
        let mut actions = Vec::new();
        let mut cursor = self
            .state
            .request
            .url_cursor
            .min(self.state.request.url.chars().count());

        match key_event.code {
            KeyCode::Left => {
                self.state.request.url_cursor = cursor.saturating_sub(1);
            }
            KeyCode::Right => {
                self.state.request.url_cursor =
                    (cursor + 1).min(self.state.request.url.chars().count());
            }
            KeyCode::Home => {
                self.state.request.url_cursor = 0;
            }
            KeyCode::End => {
                self.state.request.url_cursor = self.state.request.url.chars().count();
            }
            KeyCode::Backspace => {
                if cursor > 0 {
                    let mut chars: Vec<char> = self.state.request.url.chars().collect();
                    chars.remove(cursor - 1);
                    cursor -= 1;
                    self.state.request.url_cursor = cursor;
                    actions.push(Action::SetUrl(chars.into_iter().collect()));
                    actions.push(Action::SyncParamsFromUrl);
                }
            }
            KeyCode::Delete => {
                let mut chars: Vec<char> = self.state.request.url.chars().collect();
                if cursor < chars.len() {
                    chars.remove(cursor);
                    actions.push(Action::SetUrl(chars.into_iter().collect()));
                    actions.push(Action::SyncParamsFromUrl);
                }
            }
            KeyCode::Char(ch) if !key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                let mut chars: Vec<char> = self.state.request.url.chars().collect();
                if chars.len() >= MAX_URL_LENGTH {
                    return actions;
                }
                chars.insert(cursor, ch);
                cursor += 1;
                self.state.request.url_cursor = cursor;
                actions.push(Action::SetUrl(chars.into_iter().collect()));
                actions.push(Action::SyncParamsFromUrl);
            }
            _ => {}
        }

        actions
    }

    fn handle_tab_key(&mut self, key_event: KeyEvent) -> Vec<Action> {
        match key_event.code {
            KeyCode::Left => {
                self.state.request.active_tab = self.state.request.active_tab.prev();
                vec![Action::Render]
            }
            KeyCode::Right => {
                self.state.request.active_tab = self.state.request.active_tab.next();
                vec![Action::Render]
            }
            KeyCode::Char('1') => {
                self.state.request.active_tab = RequestTab::Params;
                vec![Action::Render]
            }
            KeyCode::Char('2') => {
                self.state.request.active_tab = RequestTab::Headers;
                vec![Action::Render]
            }
            KeyCode::Char('3') => {
                self.state.request.active_tab = RequestTab::Auth;
                vec![Action::Render]
            }
            KeyCode::Char('4') => {
                self.state.request.active_tab = RequestTab::Body;
                vec![Action::Render]
            }
            _ => Vec::new(),
        }
    }

    fn handle_editor_key(&mut self, key_event: KeyEvent) -> Vec<Action> {
        match self.state.request.active_tab {
            RequestTab::Params => self.handle_kv_editor_key(key_event, true),
            RequestTab::Headers => self.handle_kv_editor_key(key_event, false),
            RequestTab::Auth | RequestTab::Body => Vec::new(),
        }
    }

    fn handle_kv_editor_key(&mut self, key_event: KeyEvent, is_params: bool) -> Vec<Action> {
        let rows_len = if is_params {
            self.state.request.query_params.len()
        } else {
            self.state.request.headers.len()
        };

        let mut editor = if is_params {
            self.state.request.query_editor
        } else {
            self.state.request.headers_editor
        };

        if rows_len == 0 {
            editor.selected_row = 0;
            editor.cursor = 0;
        } else if editor.selected_row >= rows_len {
            editor.selected_row = rows_len - 1;
            editor.cursor = 0;
        }

        let mut actions = Vec::new();

        match key_event.code {
            KeyCode::Up => {
                if editor.selected_row > 0 {
                    editor.selected_row -= 1;
                    editor.cursor = 0;
                }
            }
            KeyCode::Down => {
                if rows_len > 0 {
                    editor.selected_row = (editor.selected_row + 1).min(rows_len - 1);
                    editor.cursor = 0;
                }
            }
            KeyCode::Left => {
                editor.cursor = editor.cursor.saturating_sub(1);
            }
            KeyCode::Right => {
                let len = self.editor_field_len(is_params, editor);
                editor.cursor = (editor.cursor + 1).min(len);
            }
            KeyCode::Home => {
                editor.cursor = 0;
            }
            KeyCode::End => {
                editor.cursor = self.editor_field_len(is_params, editor);
            }
            KeyCode::Enter => {
                editor.active_field = editor.active_field.toggle();
                editor.cursor = self.editor_field_len(is_params, editor).min(editor.cursor);
            }
            KeyCode::Char('n') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                let max_rows = if is_params {
                    MAX_QUERY_PARAM_ROWS
                } else {
                    MAX_HEADER_ROWS
                };
                if rows_len >= max_rows {
                    return actions;
                }
                actions.push(if is_params {
                    Action::AddQueryParam
                } else {
                    Action::AddHeader
                });
                let next_row = rows_len;
                editor.selected_row = next_row;
                editor.active_field = KeyValueField::Key;
                editor.cursor = 0;
            }
            KeyCode::Char('d') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                if rows_len > 0 {
                    let index = editor.selected_row;
                    actions.push(if is_params {
                        Action::RemoveQueryParam(index)
                    } else {
                        Action::RemoveHeader(index)
                    });

                    if index > 0 {
                        editor.selected_row = index - 1;
                    }
                    editor.cursor = 0;
                }
            }
            KeyCode::Char(' ') => {
                if let Some(row) = self.selected_row(is_params, editor.selected_row).cloned() {
                    let updated = KeyValueRow {
                        enabled: !row.enabled,
                        ..row
                    };
                    actions.push(if is_params {
                        Action::SetQueryParam {
                            index: editor.selected_row,
                            row: updated,
                        }
                    } else {
                        Action::SetHeader {
                            index: editor.selected_row,
                            row: updated,
                        }
                    });
                    if is_params {
                        actions.push(Action::SyncUrlFromParams);
                    }
                }
            }
            KeyCode::Backspace => {
                self.handle_editor_backspace(is_params, &mut editor, &mut actions);
            }
            KeyCode::Delete => {
                self.handle_editor_delete(is_params, &mut editor, &mut actions);
            }
            KeyCode::Char(ch) if !key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                self.handle_editor_insert_char(is_params, ch, &mut editor, &mut actions);
            }
            _ => {}
        }

        if is_params {
            self.state.request.query_editor = editor;
        } else {
            self.state.request.headers_editor = editor;
        }

        actions
    }

    fn handle_editor_insert_char(
        &self,
        is_params: bool,
        ch: char,
        editor: &mut KeyValueEditorState,
        actions: &mut Vec<Action>,
    ) {
        let row = self.selected_row(is_params, editor.selected_row).cloned();
        let current = row.unwrap_or_default();
        let mut updated = current;

        let field = match editor.active_field {
            KeyValueField::Key => &mut updated.key,
            KeyValueField::Value => &mut updated.value,
        };

        let max_len = match editor.active_field {
            KeyValueField::Key => MAX_KEY_LENGTH,
            KeyValueField::Value => MAX_VALUE_LENGTH,
        };

        if field.chars().count() >= max_len {
            return;
        }

        insert_char(field, editor.cursor, ch);
        editor.cursor += 1;
        self.push_set_row_action(is_params, editor.selected_row, updated, actions);
    }

    fn handle_editor_backspace(
        &self,
        is_params: bool,
        editor: &mut KeyValueEditorState,
        actions: &mut Vec<Action>,
    ) {
        let Some(row) = self.selected_row(is_params, editor.selected_row).cloned() else {
            return;
        };

        let mut updated = row;
        let field = match editor.active_field {
            KeyValueField::Key => &mut updated.key,
            KeyValueField::Value => &mut updated.value,
        };

        if editor.cursor > 0 {
            remove_char(field, editor.cursor - 1);
            editor.cursor -= 1;
            self.push_set_row_action(is_params, editor.selected_row, updated, actions);
        }
    }

    fn handle_editor_delete(
        &self,
        is_params: bool,
        editor: &mut KeyValueEditorState,
        actions: &mut Vec<Action>,
    ) {
        let Some(row) = self.selected_row(is_params, editor.selected_row).cloned() else {
            return;
        };

        let mut updated = row;
        let field = match editor.active_field {
            KeyValueField::Key => &mut updated.key,
            KeyValueField::Value => &mut updated.value,
        };

        if editor.cursor < field.chars().count() {
            remove_char(field, editor.cursor);
            self.push_set_row_action(is_params, editor.selected_row, updated, actions);
        }
    }

    fn push_set_row_action(
        &self,
        is_params: bool,
        index: usize,
        row: KeyValueRow,
        actions: &mut Vec<Action>,
    ) {
        if is_params {
            actions.push(Action::SetQueryParam { index, row });
        } else {
            actions.push(Action::SetHeader { index, row });
        }

        if is_params {
            actions.push(Action::SyncUrlFromParams);
        }
    }

    fn editor_field_len(&self, is_params: bool, editor: KeyValueEditorState) -> usize {
        let Some(row) = self.selected_row(is_params, editor.selected_row) else {
            return 0;
        };

        match editor.active_field {
            KeyValueField::Key => row.key.chars().count(),
            KeyValueField::Value => row.value.chars().count(),
        }
    }

    fn selected_row(&self, is_params: bool, index: usize) -> Option<&KeyValueRow> {
        if is_params {
            self.state.request.query_params.get(index)
        } else {
            self.state.request.headers.get(index)
        }
    }

    fn apply_action(&mut self, action: Action) {
        match action {
            Action::Tick | Action::Render => {}
            Action::Resize(width, height) => {
                self.state.terminal_size = (width, height);
                self.state.layout_mode = LayoutManager::mode_for_dimensions(width, height);
            }
            Action::Quit => {
                self.state.should_quit = true;
            }
            Action::FocusNext => {
                self.state.request.focus = self.state.request.focus.next();
            }
            Action::FocusPrev => {
                self.state.request.focus = self.state.request.focus.prev();
            }
            Action::SetMethod(method) => {
                self.state.request.method = method;
            }
            Action::SetUrl(url) => {
                if url.chars().count() > MAX_URL_LENGTH {
                    return;
                }
                self.state.request.url = url;
                let len = self.state.request.url.chars().count();
                self.state.request.url_cursor = self.state.request.url_cursor.min(len);
            }
            Action::SyncUrlFromParams => {
                if self.state.request.sync_guard == Some(SyncDirection::UrlToParams) {
                    return;
                }

                let guard = SyncGuard::new(&mut self.state.request, SyncDirection::ParamsToUrl);
                let rebuilt = rebuild_url_with_params(
                    &guard.request.url,
                    &guard.request.query_params,
                    &guard.request.query_param_tokens,
                );
                if rebuilt.chars().count() <= MAX_URL_LENGTH {
                    guard.request.url = rebuilt;
                    let len = guard.request.url.chars().count();
                    guard.request.url_cursor = guard.request.url_cursor.min(len);
                }
            }
            Action::SyncParamsFromUrl => {
                if self.state.request.sync_guard == Some(SyncDirection::ParamsToUrl) {
                    return;
                }

                let guard = SyncGuard::new(&mut self.state.request, SyncDirection::UrlToParams);
                let (parsed_rows, parsed_tokens) = parse_query_params(&guard.request.url);

                let mut limited_rows = Vec::new();
                let mut limited_tokens = Vec::new();
                for (row, token) in parsed_rows
                    .into_iter()
                    .zip(parsed_tokens.into_iter())
                    .take(MAX_QUERY_PARAM_ROWS)
                {
                    limited_rows.push(limit_key_value_row(row));
                    limited_tokens.push(token);
                }

                guard.request.query_params = limited_rows;
                guard.request.query_param_tokens = limited_tokens;

                let len = guard.request.query_params.len();
                if len == 0 {
                    guard.request.query_editor.selected_row = 0;
                    guard.request.query_editor.cursor = 0;
                } else {
                    guard.request.query_editor.selected_row =
                        guard.request.query_editor.selected_row.min(len - 1);
                    let selected =
                        &guard.request.query_params[guard.request.query_editor.selected_row];
                    let field_len = match guard.request.query_editor.active_field {
                        KeyValueField::Key => selected.key.chars().count(),
                        KeyValueField::Value => selected.value.chars().count(),
                    };
                    guard.request.query_editor.cursor =
                        guard.request.query_editor.cursor.min(field_len);
                }
            }
            Action::SetHeader { index, row } => {
                upsert_row_with_limit(
                    &mut self.state.request.headers,
                    index,
                    limit_key_value_row(row),
                    MAX_HEADER_ROWS,
                );
                normalize_editor_state(
                    &mut self.state.request.headers_editor,
                    &self.state.request.headers,
                );
            }
            Action::AddHeader => {
                if self.state.request.headers.len() >= MAX_HEADER_ROWS {
                    return;
                }
                self.state.request.headers.push(KeyValueRow::default());
                normalize_editor_state(
                    &mut self.state.request.headers_editor,
                    &self.state.request.headers,
                );
            }
            Action::RemoveHeader(index) => {
                if index < self.state.request.headers.len() {
                    self.state.request.headers.remove(index);
                    normalize_editor_state(
                        &mut self.state.request.headers_editor,
                        &self.state.request.headers,
                    );
                }
            }
            Action::SetQueryParam { index, row } => {
                upsert_query_row_with_token(
                    &mut self.state.request.query_params,
                    &mut self.state.request.query_param_tokens,
                    index,
                    limit_key_value_row(row),
                    MAX_QUERY_PARAM_ROWS,
                );
                normalize_editor_state(
                    &mut self.state.request.query_editor,
                    &self.state.request.query_params,
                );
            }
            Action::AddQueryParam => {
                if self.state.request.query_params.len() >= MAX_QUERY_PARAM_ROWS {
                    return;
                }
                self.state.request.query_params.push(KeyValueRow::default());
                self.state
                    .request
                    .query_param_tokens
                    .push(QueryParamToken::KeyValue);
                normalize_editor_state(
                    &mut self.state.request.query_editor,
                    &self.state.request.query_params,
                );
            }
            Action::RemoveQueryParam(index) => {
                if index < self.state.request.query_params.len() {
                    self.state.request.query_params.remove(index);
                    if index < self.state.request.query_param_tokens.len() {
                        self.state.request.query_param_tokens.remove(index);
                    }
                    normalize_editor_state(
                        &mut self.state.request.query_editor,
                        &self.state.request.query_params,
                    );
                }
            }
        }
    }
}

struct SyncGuard<'a> {
    request: &'a mut crate::state::RequestState,
}

impl<'a> SyncGuard<'a> {
    fn new(request: &'a mut crate::state::RequestState, direction: SyncDirection) -> Self {
        request.sync_guard = Some(direction);
        Self { request }
    }
}

impl Drop for SyncGuard<'_> {
    fn drop(&mut self) {
        self.request.sync_guard = None;
    }
}

fn normalize_editor_state(editor: &mut KeyValueEditorState, rows: &[KeyValueRow]) {
    if rows.is_empty() {
        editor.selected_row = 0;
        editor.cursor = 0;
        return;
    }

    editor.selected_row = editor.selected_row.min(rows.len() - 1);
    let row = &rows[editor.selected_row];
    let max_cursor = match editor.active_field {
        KeyValueField::Key => row.key.chars().count(),
        KeyValueField::Value => row.value.chars().count(),
    };
    editor.cursor = editor.cursor.min(max_cursor);
}

fn upsert_row_with_limit(rows: &mut Vec<KeyValueRow>, index: usize, row: KeyValueRow, max: usize) {
    if index > rows.len() {
        return;
    }

    if index < rows.len() {
        rows[index] = row;
        return;
    }

    if rows.len() < max {
        rows.push(row);
    }
}

fn upsert_query_row_with_token(
    rows: &mut Vec<KeyValueRow>,
    tokens: &mut Vec<QueryParamToken>,
    index: usize,
    row: KeyValueRow,
    max: usize,
) {
    if index > rows.len() {
        return;
    }

    if index < rows.len() {
        let prior = tokens
            .get(index)
            .copied()
            .unwrap_or(QueryParamToken::KeyValue);
        rows[index] = row;
        if index >= tokens.len() {
            tokens.resize(index + 1, QueryParamToken::KeyValue);
        }
        tokens[index] = derive_query_token(prior, &rows[index]);
        return;
    }

    if rows.len() < max {
        rows.push(row);
        tokens.push(QueryParamToken::KeyValue);
    }
}

fn derive_query_token(previous: QueryParamToken, row: &KeyValueRow) -> QueryParamToken {
    if row.key.is_empty() && row.value.is_empty() {
        return previous;
    }

    match previous {
        QueryParamToken::EmptySegment => {
            if row.value.is_empty() {
                QueryParamToken::KeyOnly
            } else {
                QueryParamToken::KeyValue
            }
        }
        QueryParamToken::KeyOnly => {
            if row.value.is_empty() {
                QueryParamToken::KeyOnly
            } else {
                QueryParamToken::KeyValue
            }
        }
        QueryParamToken::KeyValue => QueryParamToken::KeyValue,
    }
}

fn limit_key_value_row(mut row: KeyValueRow) -> KeyValueRow {
    truncate_to_char_limit(&mut row.key, MAX_KEY_LENGTH);
    truncate_to_char_limit(&mut row.value, MAX_VALUE_LENGTH);
    row
}

fn truncate_to_char_limit(value: &mut String, max_len: usize) {
    if value.chars().count() <= max_len {
        return;
    }

    *value = value.chars().take(max_len).collect();
}

fn insert_char(value: &mut String, cursor: usize, ch: char) {
    let mut chars: Vec<char> = value.chars().collect();
    let index = cursor.min(chars.len());
    chars.insert(index, ch);
    *value = chars.into_iter().collect();
}

fn remove_char(value: &mut String, cursor: usize) {
    let mut chars: Vec<char> = value.chars().collect();
    if cursor < chars.len() {
        chars.remove(cursor);
        *value = chars.into_iter().collect();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        derive_query_token, limit_key_value_row, truncate_to_char_limit, upsert_query_row_with_token,
        upsert_row_with_limit,
    };
    use crate::state::{KeyValueRow, QueryParamToken, MAX_KEY_LENGTH, MAX_VALUE_LENGTH};

    #[test]
    fn upsert_respects_row_limit() {
        let mut rows = vec![KeyValueRow::default()];
        upsert_row_with_limit(&mut rows, 1, KeyValueRow::default(), 1);
        assert_eq!(rows.len(), 1);
    }

    #[test]
    fn limit_key_value_row_truncates_key_and_value() {
        let row = KeyValueRow {
            enabled: true,
            key: "k".repeat(MAX_KEY_LENGTH + 3),
            value: "v".repeat(MAX_VALUE_LENGTH + 5),
        };

        let limited = limit_key_value_row(row);
        assert_eq!(limited.key.chars().count(), MAX_KEY_LENGTH);
        assert_eq!(limited.value.chars().count(), MAX_VALUE_LENGTH);
    }

    #[test]
    fn truncate_to_char_limit_handles_unicode_boundaries() {
        let mut value = String::from("a🙂b🙂c");
        truncate_to_char_limit(&mut value, 3);
        assert_eq!(value, "a🙂b");
    }

    #[test]
    fn upsert_query_row_promotes_key_only_to_key_value_when_value_added() {
        let mut rows = vec![KeyValueRow {
            enabled: true,
            key: String::from("flag"),
            value: String::new(),
        }];
        let mut tokens = vec![QueryParamToken::KeyOnly];

        upsert_query_row_with_token(
            &mut rows,
            &mut tokens,
            0,
            KeyValueRow {
                enabled: true,
                key: String::from("flag"),
                value: String::from("1"),
            },
            8,
        );

        assert_eq!(tokens[0], QueryParamToken::KeyValue);
    }

    #[test]
    fn derive_query_token_keeps_key_only_without_value() {
        let token = derive_query_token(
            QueryParamToken::KeyOnly,
            &KeyValueRow {
                enabled: true,
                key: String::from("flag"),
                value: String::new(),
            },
        );

        assert_eq!(token, QueryParamToken::KeyOnly);
    }
}
