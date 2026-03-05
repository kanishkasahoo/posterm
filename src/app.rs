use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use base64::Engine;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Rect};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use crate::action::{Action, BodyContent};
use crate::components::Component;
use crate::components::help_modal;
use crate::components::layout_manager::LayoutManager;
use crate::components::overlay_manager;
use crate::components::request_builder::RequestBuilder;
use crate::components::response_viewer::ResponseViewer;
use crate::components::response_viewer::raw_view::raw_lines;
use crate::components::response_viewer::response_body::body_search_lines;
use crate::components::response_viewer::response_headers::header_lines;
use crate::components::sidebar::Sidebar;
use crate::components::status_bar::StatusBar;
use crate::event::{Event, EventHandler};
use crate::http::client::HttpClientPool;
use crate::http::execute_request;
use crate::persistence::{
    Collection, HistoryEntry, PersistTarget, PersistenceManager, SavedRequest,
    SerializedKeyValueRow, delete_collection_file, ensure_config_exists, is_sensitive_header,
    load_all_collections, load_history,
};
use crate::state::{
    AppState, AuthField, AuthMode, BodyField, BodyFormat, HttpMethod, InFlightRequest,
    KeyValueEditorState, KeyValueField, KeyValueRow, LayoutMode, MAX_AUTH_PASSWORD_LENGTH,
    MAX_AUTH_TOKEN_LENGTH, MAX_AUTH_USERNAME_LENGTH, MAX_BODY_FORM_ROWS, MAX_BODY_TEXT_LENGTH,
    MAX_HEADER_ROWS, MAX_KEY_LENGTH, MAX_QUERY_PARAM_ROWS, MAX_URL_LENGTH, MAX_VALUE_LENGTH,
    QueryParamToken, RequestTab, ResponseSearchScope, ResponseTab, SearchMatch, SidebarItem,
    SidebarPromptMode, SidebarPromptState, SyncDirection,
};
use crate::tui::Tui;
use crate::util::terminal_sanitize::sanitize_terminal_text;
use crate::util::url_parser::{parse_query_params, rebuild_url_with_params};

pub struct App {
    state: AppState,
    events: EventHandler,
    request_builder: RequestBuilder,
    response_viewer: ResponseViewer,
    status_bar: StatusBar,
    sidebar: Sidebar,
    http_pool: HttpClientPool,
    action_sender: mpsc::Sender<Action>,
    action_receiver: mpsc::Receiver<Action>,
    /// Per-request task handles, keyed by request_id.
    request_tasks: HashMap<u64, JoinHandle<()>>,
    /// Per-request cancellation senders, keyed by request_id.
    request_cancel_senders: HashMap<u64, watch::Sender<bool>>,
    next_request_id: u64,
    /// The request_id currently displayed in the response pane.
    active_response_id: Option<u64>,
    pending_response_search_bytes: usize,
    persistence: PersistenceManager,
}

const MAX_RESPONSE_SEARCH_QUERY_CHARS: usize = 256;
const MAX_RESPONSE_SEARCH_MATCHES: usize = 10_000;
const RESPONSE_SEARCH_RECOMPUTE_CHUNK_BYTES: usize = 8 * 1024;

impl App {
    pub fn new(initial_size: (u16, u16)) -> Self {
        let layout_mode = LayoutManager::mode_for_dimensions(initial_size.0, initial_size.1);
        let mut state = AppState::new(initial_size, layout_mode);
        let (action_sender, action_receiver) = mpsc::channel(256);

        // Load persistent data.
        state.config = ensure_config_exists();
        state.collections = load_all_collections();
        state.history = load_history();

        Self {
            state,
            events: EventHandler::new(Duration::from_millis(250), Duration::from_millis(33)),
            request_builder: RequestBuilder,
            response_viewer: ResponseViewer,
            status_bar: StatusBar,
            sidebar: Sidebar,
            http_pool: HttpClientPool::new().expect("failed to initialize HTTP client pool"),
            action_sender,
            action_receiver,
            request_tasks: HashMap::new(),
            request_cancel_senders: HashMap::new(),
            next_request_id: 1,
            active_response_id: None,
            pending_response_search_bytes: 0,
            persistence: PersistenceManager::new(),
        }
    }

    pub async fn run(&mut self, tui: &mut Tui) -> std::io::Result<()> {
        self.render(tui)?;

        loop {
            let events = &mut self.events;
            let action_receiver = &mut self.action_receiver;
            let next_actions = tokio::select! {
                maybe_event = events.next() => {
                    match maybe_event {
                        Some(event) => self.map_event_to_actions(event),
                        None => break,
                    }
                }
                maybe_action = action_receiver.recv() => {
                    match maybe_action {
                        Some(action) => vec![action, Action::Render],
                        None => Vec::new(),
                    }
                }
            };

            for action in next_actions {
                self.process_action(action, tui)?;
                if self.state.should_quit {
                    self.cancel_all_in_flight_requests();
                    return Ok(());
                }
            }
        }

        Ok(())
    }

    fn process_action(&mut self, action: Action, tui: &mut Tui) -> std::io::Result<()> {
        self.apply_action(action.clone());
        self.request_builder.handle_action(&action, &self.state);
        self.response_viewer.handle_action(&action, &self.state);
        self.status_bar.handle_action(&action, &self.state);
        self.sidebar.handle_action(&action, &self.state);

        if matches!(action, Action::Render) {
            self.render(tui)?;
        }

        Ok(())
    }

    fn render(&mut self, tui: &mut Tui) -> std::io::Result<()> {
        tui.draw(|frame| {
            let pane_layout = LayoutManager::compute(frame.area());

            const MIN_REQUEST_PANE_WIDTH: u16 = 56;
            const MIN_RESPONSE_PANE_WIDTH: u16 = 72;
            const HORIZONTAL_MIN_WIDTH: u16 = MIN_REQUEST_PANE_WIDTH + MIN_RESPONSE_PANE_WIDTH;
            const SIDEBAR_WIDTH: u16 = 30;

            let is_large = matches!(self.state.layout_mode, LayoutMode::Large);

            // In Large mode: sidebar | [request + response].
            // In Small/Medium: full area for request+response; sidebar overlaid when visible.
            let (content_area, sidebar_area_opt) = if is_large {
                let chunks = Layout::horizontal([
                    Constraint::Length(SIDEBAR_WIDTH),
                    Constraint::Min(HORIZONTAL_MIN_WIDTH),
                ])
                .split(pane_layout.main);
                (chunks[1], Some(chunks[0]))
            } else {
                (pane_layout.main, None)
            };

            // Render inline sidebar in Large mode.
            if let Some(sidebar_area) = sidebar_area_opt {
                self.sidebar.render(frame, sidebar_area, &self.state);
            }

            let is_small = matches!(self.state.layout_mode, LayoutMode::Small);
            let use_horizontal_split = !is_small && content_area.width >= HORIZONTAL_MIN_WIDTH;

            if is_small {
                // In Small mode: show only one pane at a time (full content_area).
                if self.state.small_mode_show_response {
                    self.response_viewer
                        .render(frame, content_area, &self.state);
                } else {
                    self.request_builder
                        .render(frame, content_area, &self.state);
                }
            } else {
                let main_chunks = if use_horizontal_split {
                    Layout::horizontal([
                        Constraint::Min(MIN_REQUEST_PANE_WIDTH),
                        Constraint::Min(MIN_RESPONSE_PANE_WIDTH),
                    ])
                    .split(content_area)
                } else {
                    Layout::vertical([Constraint::Percentage(52), Constraint::Percentage(48)])
                        .split(content_area)
                };
                self.request_builder
                    .render(frame, main_chunks[0], &self.state);
                self.response_viewer
                    .render(frame, main_chunks[1], &self.state);
            }
            self.status_bar
                .render(frame, pane_layout.status, &self.state);

            // Overlay sidebar in Small/Medium mode when visible.
            if !is_large && self.state.sidebar_visible {
                overlay_manager::render_sidebar_overlay(frame, content_area, &self.state);
            }

            if self.state.help_visible {
                help_modal::render(frame, pane_layout.main, &self.state);
            }
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
        if self.state.help_visible {
            if Self::is_help_shortcut(key_event) {
                return vec![Action::ToggleHelp, Action::Render];
            }
            if key_event.code == KeyCode::Down || key_event.code == KeyCode::Char('j') {
                return vec![Action::ScrollHelp(1), Action::Render];
            }
            if key_event.code == KeyCode::Up || key_event.code == KeyCode::Char('k') {
                return vec![Action::ScrollHelp(-1), Action::Render];
            }
            if key_event.code == KeyCode::PageDown {
                return vec![Action::ScrollHelp(10), Action::Render];
            }
            if key_event.code == KeyCode::PageUp {
                return vec![Action::ScrollHelp(-10), Action::Render];
            }
            if key_event.code == KeyCode::Esc {
                return vec![Action::CloseHelp, Action::Render];
            }
            return Vec::new();
        }

        if self.state.response.search.active {
            let actions = self.handle_active_response_search_keys(key_event);
            if !actions.is_empty() {
                return actions;
            }
        }

        if Self::is_help_shortcut(key_event) {
            return vec![Action::ToggleHelp, Action::Render];
        }

        if Self::is_response_search_shortcut(key_event) {
            return vec![Action::OpenResponseSearch, Action::Render];
        }

        // Ctrl+B toggles sidebar visibility (Small/Medium) or focus (Large).
        if key_event.code == KeyCode::Char('b')
            && key_event.modifiers.contains(KeyModifiers::CONTROL)
        {
            return vec![Action::ToggleSidebar, Action::Render];
        }

        // Ctrl+R toggles between request builder and response viewer in Small mode.
        if matches!(self.state.layout_mode, LayoutMode::Small)
            && key_event.code == KeyCode::Char('r')
            && key_event.modifiers.contains(KeyModifiers::CONTROL)
        {
            return vec![Action::ToggleSmallModePane, Action::Render];
        }

        // When sidebar is focused, delegate navigation keys to sidebar actions.
        if self.state.sidebar_focused {
            return self.handle_sidebar_focused_key(key_event);
        }

        match (key_event.code, key_event.modifiers) {
            (KeyCode::Char('q'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
                return vec![Action::Quit];
            }
            (KeyCode::Char('s'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
                return vec![Action::SendRequest, Action::Render];
            }
            (KeyCode::Char('c'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
                return vec![Action::CancelRequest, Action::Render];
            }
            (KeyCode::Tab, _) => {
                let mut acts = vec![Action::FocusNext, Action::Render];
                if self.state.request.focus == crate::state::RequestFocus::Editor
                    && self.state.request.active_tab == RequestTab::Params
                {
                    acts.insert(0, Action::SyncUrlFromParams);
                }
                return acts;
            }
            (KeyCode::BackTab, _) => {
                let mut acts = vec![Action::FocusPrev, Action::Render];
                if self.state.request.focus == crate::state::RequestFocus::Editor
                    && self.state.request.active_tab == RequestTab::Params
                {
                    acts.insert(0, Action::SyncUrlFromParams);
                }
                return acts;
            }
            (KeyCode::Esc, _) if self.state.response.search.active => {
                return vec![Action::CloseResponseSearch, Action::Render];
            }
            _ => {}
        }

        let response_actions = self.handle_response_navigation_keys(key_event);
        if !response_actions.is_empty() {
            return response_actions;
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

    /// Handles key events when the sidebar has focus.
    fn handle_sidebar_focused_key(&mut self, key_event: KeyEvent) -> Vec<Action> {
        if self.state.sidebar_prompt.is_some() {
            return self.handle_sidebar_prompt_key(key_event);
        }

        match key_event.code {
            KeyCode::Esc => vec![Action::SidebarClose, Action::Render],
            KeyCode::Up => vec![Action::SidebarFocusPrev, Action::Render],
            KeyCode::Down => vec![Action::SidebarFocusNext, Action::Render],
            KeyCode::Left => {
                vec![
                    Action::SidebarScrollCollectionsHorizontal(-2),
                    Action::Render,
                ]
            }
            KeyCode::Right => {
                vec![
                    Action::SidebarScrollCollectionsHorizontal(2),
                    Action::Render,
                ]
            }
            KeyCode::Enter => vec![Action::SidebarSelect, Action::Render],
            KeyCode::Char(' ') => {
                if let SidebarItem::HistoryEntry(idx) = self.state.sidebar_selected_item {
                    vec![Action::ToggleHistoryMark(idx), Action::Render]
                } else {
                    vec![Action::SidebarSelect, Action::Render]
                }
            }
            KeyCode::Char('c') if key_event.modifiers.is_empty() => {
                self.start_sidebar_prompt(SidebarPromptMode::CreateCollection, String::new());
                vec![Action::Render]
            }
            KeyCode::Char('r') if key_event.modifiers.is_empty() => {
                match self.selected_sidebar_rename_prompt() {
                    Some((mode, current_name)) => {
                        self.start_sidebar_prompt(mode, current_name);
                        vec![Action::Render]
                    }
                    None => vec![
                        Action::ShowNotification {
                            message: String::from("Select a collection or saved request to rename"),
                            kind: crate::state::NotificationKind::Error,
                        },
                        Action::Render,
                    ],
                }
            }
            KeyCode::Char('s') if key_event.modifiers.is_empty() => {
                match self.selected_collection_for_save() {
                    Some(collection_index) => {
                        let default_name = default_saved_request_name(&self.state.request);
                        self.start_sidebar_prompt(
                            SidebarPromptMode::SaveRequestToCollection { collection_index },
                            default_name,
                        );
                        vec![Action::Render]
                    }
                    None => vec![
                        Action::ShowNotification {
                            message: String::from(
                                "Select a collection to save the current request",
                            ),
                            kind: crate::state::NotificationKind::Error,
                        },
                        Action::Render,
                    ],
                }
            }
            KeyCode::Char('d') if key_event.modifiers.is_empty() => {
                self.delete_selected_sidebar_item_actions()
            }
            // 'X' (Shift+X) clears all history; match both NONE and SHIFT modifiers since
            // crossterm may or may not include the SHIFT modifier for uppercase characters.
            KeyCode::Char('X')
                if key_event.modifiers.is_empty() || key_event.modifiers == KeyModifiers::SHIFT =>
            {
                if self.state.history.is_empty() {
                    vec![
                        Action::ShowNotification {
                            message: String::from("History is already empty"),
                            kind: crate::state::NotificationKind::Info,
                        },
                        Action::Render,
                    ]
                } else {
                    let count = self.state.history.len();
                    vec![
                        Action::ClearHistory,
                        Action::ShowNotification {
                            message: format!(
                                "Cleared all {count} history entr{}",
                                if count == 1 { "y" } else { "ies" }
                            ),
                            kind: crate::state::NotificationKind::Info,
                        },
                        Action::Render,
                    ]
                }
            }
            _ => Vec::new(),
        }
    }

    fn handle_sidebar_prompt_key(&mut self, key_event: KeyEvent) -> Vec<Action> {
        let Some(prompt_mode) = self
            .state
            .sidebar_prompt
            .as_ref()
            .map(|prompt| prompt.mode.clone())
        else {
            return Vec::new();
        };

        match key_event.code {
            KeyCode::Esc => {
                let cancelled = prompt_mode.cancel_label().to_string();
                self.state.sidebar_prompt = None;
                vec![
                    Action::ShowNotification {
                        message: format!("{cancelled} cancelled"),
                        kind: crate::state::NotificationKind::Info,
                    },
                    Action::Render,
                ]
            }
            KeyCode::Enter => self.confirm_sidebar_prompt(),
            KeyCode::Backspace => {
                if let Some(prompt) = self.state.sidebar_prompt.as_mut() {
                    prompt.value.pop();
                }
                vec![Action::Render]
            }
            KeyCode::Char(ch)
                if !key_event.modifiers.contains(KeyModifiers::CONTROL)
                    && !key_event.modifiers.contains(KeyModifiers::ALT) =>
            {
                if let Some(prompt) = self.state.sidebar_prompt.as_mut() {
                    prompt.value.push(ch);
                }
                vec![Action::Render]
            }
            _ => Vec::new(),
        }
    }

    fn is_help_shortcut(key_event: KeyEvent) -> bool {
        key_event.code == KeyCode::F(1)
    }

    fn is_response_search_shortcut(key_event: KeyEvent) -> bool {
        key_event.code == KeyCode::Char('f') && key_event.modifiers.contains(KeyModifiers::CONTROL)
    }

    fn handle_active_response_search_keys(&self, key_event: KeyEvent) -> Vec<Action> {
        match key_event.code {
            KeyCode::Esc => vec![Action::CloseResponseSearch, Action::Render],
            KeyCode::Enter | KeyCode::Char('n') => vec![Action::NextSearchMatch, Action::Render],
            KeyCode::Char('N') => vec![Action::PrevSearchMatch, Action::Render],
            KeyCode::Backspace => {
                let mut query = self.state.response.search.query.clone();
                query.pop();
                vec![Action::SearchInResponse(query), Action::Render]
            }
            KeyCode::Char(ch) if !key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                let mut query = self.state.response.search.query.clone();
                query.push(ch);
                vec![Action::SearchInResponse(query), Action::Render]
            }
            _ => Vec::new(),
        }
    }

    fn handle_response_navigation_keys(&self, key_event: KeyEvent) -> Vec<Action> {
        match (key_event.code, key_event.modifiers) {
            (KeyCode::Char('h'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
                vec![
                    Action::SetResponseTab(self.state.response.active_tab.prev()),
                    Action::Render,
                ]
            }
            (KeyCode::Char('l'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
                vec![
                    Action::SetResponseTab(self.state.response.active_tab.next()),
                    Action::Render,
                ]
            }
            (KeyCode::Char('1'), modifiers) if modifiers.contains(KeyModifiers::ALT) => {
                vec![Action::SetResponseTab(ResponseTab::Body), Action::Render]
            }
            (KeyCode::Char('2'), modifiers) if modifiers.contains(KeyModifiers::ALT) => {
                vec![Action::SetResponseTab(ResponseTab::Headers), Action::Render]
            }
            (KeyCode::Char('3'), modifiers) if modifiers.contains(KeyModifiers::ALT) => {
                vec![Action::SetResponseTab(ResponseTab::Raw), Action::Render]
            }
            (KeyCode::Up, modifiers)
                if modifiers.contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT) =>
            {
                vec![Action::ScrollResponse(-1), Action::Render]
            }
            (KeyCode::Down, modifiers)
                if modifiers.contains(KeyModifiers::CONTROL | KeyModifiers::SHIFT) =>
            {
                vec![Action::ScrollResponse(1), Action::Render]
            }
            (KeyCode::PageUp, modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
                vec![Action::ScrollResponse(-10), Action::Render]
            }
            (KeyCode::PageDown, modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
                vec![Action::ScrollResponse(10), Action::Render]
            }
            (KeyCode::Left, modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
                vec![Action::ScrollResponseHorizontal(-4), Action::Render]
            }
            (KeyCode::Right, modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
                vec![Action::ScrollResponseHorizontal(4), Action::Render]
            }
            (KeyCode::Char('w'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
                vec![Action::ToggleResponseWrap, Action::Render]
            }
            _ => Vec::new(),
        }
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
            KeyCode::Char('n') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                match self.state.request.active_tab {
                    RequestTab::Params => {
                        if self.state.request.query_params.len() < MAX_QUERY_PARAM_ROWS {
                            vec![Action::AddQueryParam, Action::Render]
                        } else {
                            Vec::new()
                        }
                    }
                    RequestTab::Headers => {
                        if self.state.request.headers.len() < MAX_HEADER_ROWS {
                            vec![Action::AddHeader, Action::Render]
                        } else {
                            Vec::new()
                        }
                    }
                    RequestTab::Body => {
                        if self.state.request.body_format == BodyFormat::Form
                            && self.state.request.body_form.len() < MAX_BODY_FORM_ROWS
                        {
                            vec![
                                Action::SetBodyContent(BodyContent::AddFormRow),
                                Action::Render,
                            ]
                        } else {
                            Vec::new()
                        }
                    }
                    RequestTab::Auth => Vec::new(),
                }
            }
            _ => Vec::new(),
        }
    }

    fn handle_editor_key(&mut self, key_event: KeyEvent) -> Vec<Action> {
        match self.state.request.active_tab {
            RequestTab::Params => self.handle_kv_editor_key(key_event, true),
            RequestTab::Headers => self.handle_kv_editor_key(key_event, false),
            RequestTab::Auth => self.handle_auth_editor_key(key_event),
            RequestTab::Body => self.handle_body_editor_key(key_event),
        }
    }

    fn handle_auth_editor_key(&mut self, key_event: KeyEvent) -> Vec<Action> {
        let mut actions = Vec::new();
        let active = self.state.request.auth_editor.active_field;

        match key_event.code {
            KeyCode::Up => {
                self.state.request.auth_editor.active_field =
                    prev_auth_field(active, self.state.request.auth_mode);
            }
            KeyCode::Down => {
                self.state.request.auth_editor.active_field =
                    next_auth_field(active, self.state.request.auth_mode);
            }
            KeyCode::Left => {
                if active == AuthField::Mode {
                    actions.push(Action::SetAuthMode(self.state.request.auth_mode.prev()));
                } else {
                    let cursor = self.auth_cursor_mut(active);
                    *cursor = cursor.saturating_sub(1);
                }
            }
            KeyCode::Right => {
                if active == AuthField::Mode {
                    actions.push(Action::SetAuthMode(self.state.request.auth_mode.next()));
                } else {
                    let len = self.auth_field_len(active);
                    let cursor = self.auth_cursor_mut(active);
                    *cursor = (*cursor + 1).min(len);
                }
            }
            KeyCode::Home => {
                if active != AuthField::Mode {
                    *self.auth_cursor_mut(active) = 0;
                }
            }
            KeyCode::End => {
                if active != AuthField::Mode {
                    *self.auth_cursor_mut(active) = self.auth_field_len(active);
                }
            }
            KeyCode::Backspace => {
                self.handle_auth_backspace(active, &mut actions);
            }
            KeyCode::Delete => {
                self.handle_auth_delete(active, &mut actions);
            }
            KeyCode::Char(ch) if !key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                self.handle_auth_insert(active, ch, &mut actions);
            }
            _ => {}
        }

        actions
    }

    fn handle_body_editor_key(&mut self, key_event: KeyEvent) -> Vec<Action> {
        match self.state.request.body_editor.active_field {
            BodyField::Form => self.handle_body_form_editor_key(key_event),
            _ => self.handle_body_non_form_key(key_event),
        }
    }

    fn handle_body_non_form_key(&mut self, key_event: KeyEvent) -> Vec<Action> {
        let mut actions = Vec::new();
        let active = self.state.request.body_editor.active_field;

        match key_event.code {
            KeyCode::Up => {
                self.state.request.body_editor.active_field =
                    prev_body_field(active, self.state.request.body_format);
            }
            KeyCode::Down => {
                self.state.request.body_editor.active_field =
                    next_body_field(active, self.state.request.body_format);
            }
            KeyCode::Left => {
                if active == BodyField::Format {
                    actions.push(Action::SetBodyFormat(self.state.request.body_format.prev()));
                } else if active == BodyField::Json {
                    self.state.request.body_editor.json_cursor =
                        self.state.request.body_editor.json_cursor.saturating_sub(1);
                } else if active == BodyField::Text {
                    self.state.request.body_editor.text_cursor =
                        self.state.request.body_editor.text_cursor.saturating_sub(1);
                }
            }
            KeyCode::Right => {
                if active == BodyField::Format {
                    actions.push(Action::SetBodyFormat(self.state.request.body_format.next()));
                } else if active == BodyField::Json {
                    let max = self.state.request.body_json.chars().count();
                    self.state.request.body_editor.json_cursor =
                        (self.state.request.body_editor.json_cursor + 1).min(max);
                } else if active == BodyField::Text {
                    let max = self.state.request.body_text.chars().count();
                    self.state.request.body_editor.text_cursor =
                        (self.state.request.body_editor.text_cursor + 1).min(max);
                }
            }
            KeyCode::Home => {
                if active == BodyField::Json {
                    let cursor = self.state.request.body_editor.json_cursor;
                    self.state.request.body_editor.json_cursor =
                        line_start_index(&self.state.request.body_json, cursor);
                } else if active == BodyField::Text {
                    let cursor = self.state.request.body_editor.text_cursor;
                    self.state.request.body_editor.text_cursor =
                        line_start_index(&self.state.request.body_text, cursor);
                }
            }
            KeyCode::End => {
                if active == BodyField::Json {
                    let cursor = self.state.request.body_editor.json_cursor;
                    self.state.request.body_editor.json_cursor =
                        line_end_index(&self.state.request.body_json, cursor);
                } else if active == BodyField::Text {
                    let cursor = self.state.request.body_editor.text_cursor;
                    self.state.request.body_editor.text_cursor =
                        line_end_index(&self.state.request.body_text, cursor);
                }
            }
            KeyCode::Enter => {
                if active == BodyField::Json {
                    self.handle_json_insert_char('\n', &mut actions);
                } else if active == BodyField::Text {
                    self.handle_text_insert_char('\n', &mut actions);
                }
            }
            KeyCode::Backspace => {
                if active == BodyField::Json {
                    self.handle_json_backspace(&mut actions);
                } else if active == BodyField::Text {
                    self.handle_text_backspace(&mut actions);
                }
            }
            KeyCode::Delete => {
                if active == BodyField::Json {
                    self.handle_json_delete(&mut actions);
                } else if active == BodyField::Text {
                    self.handle_text_delete(&mut actions);
                }
            }
            KeyCode::Char(ch) if !key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                if active == BodyField::Json {
                    self.handle_json_insert_char(ch, &mut actions);
                } else if active == BodyField::Text {
                    self.handle_text_insert_char(ch, &mut actions);
                }
            }
            _ => {}
        }

        actions
    }

    fn handle_body_form_editor_key(&mut self, key_event: KeyEvent) -> Vec<Action> {
        if key_event.code == KeyCode::Up
            && self.state.request.body_editor.form_editor.selected_row == 0
        {
            self.state.request.body_editor.active_field = BodyField::Format;
            return Vec::new();
        }

        let rows_len = self.state.request.body_form.len();
        let mut editor = self.state.request.body_editor.form_editor;

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
                editor.cursor = (editor.cursor + 1).min(self.form_editor_field_len(editor));
            }
            KeyCode::Home => {
                editor.cursor = 0;
            }
            KeyCode::End => {
                editor.cursor = self.form_editor_field_len(editor);
            }
            KeyCode::Enter => {
                editor.active_field = editor.active_field.toggle();
                editor.cursor = self.form_editor_field_len(editor).min(editor.cursor);
            }
            KeyCode::Char('n') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                if rows_len < MAX_BODY_FORM_ROWS {
                    actions.push(Action::SetBodyContent(BodyContent::AddFormRow));
                    editor.selected_row = rows_len;
                    editor.active_field = KeyValueField::Key;
                    editor.cursor = 0;
                }
            }
            KeyCode::Char('d') if key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                if rows_len > 0 {
                    actions.push(Action::SetBodyContent(BodyContent::RemoveFormRow(
                        editor.selected_row,
                    )));
                    if editor.selected_row > 0 {
                        editor.selected_row -= 1;
                    }
                    editor.cursor = 0;
                }
            }
            KeyCode::Char(' ') => {
                if let Some(row) = self
                    .state
                    .request
                    .body_form
                    .get(editor.selected_row)
                    .cloned()
                {
                    actions.push(Action::SetBodyContent(BodyContent::SetFormRow {
                        index: editor.selected_row,
                        row: KeyValueRow {
                            enabled: !row.enabled,
                            ..row
                        },
                    }));
                }
            }
            KeyCode::Backspace => {
                self.handle_form_backspace(&mut editor, &mut actions);
            }
            KeyCode::Delete => {
                self.handle_form_delete(&mut editor, &mut actions);
            }
            KeyCode::Char(ch) if !key_event.modifiers.contains(KeyModifiers::CONTROL) => {
                self.handle_form_insert(ch, &mut editor, &mut actions);
            }
            _ => {}
        }

        self.state.request.body_editor.form_editor = editor;
        actions
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
                if is_params {
                    actions.push(Action::SyncUrlFromParams);
                }
            }
            KeyCode::Down => {
                if rows_len > 0 {
                    editor.selected_row = (editor.selected_row + 1).min(rows_len - 1);
                    editor.cursor = 0;
                }
                if is_params {
                    actions.push(Action::SyncUrlFromParams);
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
                if is_params {
                    actions.push(Action::SyncUrlFromParams);
                }
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
                if is_params {
                    actions.push(Action::SyncUrlFromParams);
                }
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
                    if is_params {
                        actions.push(Action::SyncUrlFromParams);
                    }

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

    fn auth_field_len(&self, field: AuthField) -> usize {
        match field {
            AuthField::Mode => 0,
            AuthField::Token => self.state.request.auth_token.chars().count(),
            AuthField::Username => self.state.request.auth_username.chars().count(),
            AuthField::Password => self.state.request.auth_password.chars().count(),
        }
    }

    fn auth_cursor_mut(&mut self, field: AuthField) -> &mut usize {
        match field {
            AuthField::Mode => &mut self.state.request.auth_editor.token_cursor,
            AuthField::Token => &mut self.state.request.auth_editor.token_cursor,
            AuthField::Username => &mut self.state.request.auth_editor.username_cursor,
            AuthField::Password => &mut self.state.request.auth_editor.password_cursor,
        }
    }

    fn handle_auth_insert(&mut self, field: AuthField, ch: char, actions: &mut Vec<Action>) {
        let (value, max_len, cursor) = match field {
            AuthField::Mode => return,
            AuthField::Token => (
                self.state.request.auth_token.clone(),
                MAX_AUTH_TOKEN_LENGTH,
                self.state.request.auth_editor.token_cursor,
            ),
            AuthField::Username => (
                self.state.request.auth_username.clone(),
                MAX_AUTH_USERNAME_LENGTH,
                self.state.request.auth_editor.username_cursor,
            ),
            AuthField::Password => (
                self.state.request.auth_password.clone(),
                MAX_AUTH_PASSWORD_LENGTH,
                self.state.request.auth_editor.password_cursor,
            ),
        };

        if value.chars().count() >= max_len {
            return;
        }

        let mut updated = value;
        insert_char(&mut updated, cursor, ch);
        *self.auth_cursor_mut(field) = cursor + 1;
        self.push_auth_update_action(actions, field, updated);
    }

    fn handle_auth_backspace(&mut self, field: AuthField, actions: &mut Vec<Action>) {
        if field == AuthField::Mode {
            return;
        }

        let cursor = *self.auth_cursor_mut(field);
        if cursor == 0 {
            return;
        }

        let mut updated = match field {
            AuthField::Token => self.state.request.auth_token.clone(),
            AuthField::Username => self.state.request.auth_username.clone(),
            AuthField::Password => self.state.request.auth_password.clone(),
            AuthField::Mode => String::new(),
        };

        remove_char(&mut updated, cursor - 1);
        *self.auth_cursor_mut(field) = cursor - 1;
        self.push_auth_update_action(actions, field, updated);
    }

    fn handle_auth_delete(&mut self, field: AuthField, actions: &mut Vec<Action>) {
        if field == AuthField::Mode {
            return;
        }

        let cursor = *self.auth_cursor_mut(field);
        let mut updated = match field {
            AuthField::Token => self.state.request.auth_token.clone(),
            AuthField::Username => self.state.request.auth_username.clone(),
            AuthField::Password => self.state.request.auth_password.clone(),
            AuthField::Mode => String::new(),
        };

        if cursor < updated.chars().count() {
            remove_char(&mut updated, cursor);
            self.push_auth_update_action(actions, field, updated);
        }
    }

    fn push_auth_update_action(
        &self,
        actions: &mut Vec<Action>,
        field: AuthField,
        updated: String,
    ) {
        match field {
            AuthField::Token => actions.push(Action::SetAuthToken(updated)),
            AuthField::Username => actions.push(Action::SetAuthCredentials {
                username: updated,
                password: self.state.request.auth_password.clone(),
            }),
            AuthField::Password => actions.push(Action::SetAuthCredentials {
                username: self.state.request.auth_username.clone(),
                password: updated,
            }),
            AuthField::Mode => {}
        }
    }

    fn handle_json_insert_char(&mut self, ch: char, actions: &mut Vec<Action>) {
        let cursor = self
            .state
            .request
            .body_editor
            .json_cursor
            .min(self.state.request.body_json.chars().count());
        if self.state.request.body_json.chars().count() >= MAX_BODY_TEXT_LENGTH {
            return;
        }
        let mut updated = self.state.request.body_json.clone();
        insert_char(&mut updated, cursor, ch);
        self.state.request.body_editor.json_cursor = cursor + 1;
        actions.push(Action::SetBodyContent(BodyContent::Json(updated)));
    }

    fn handle_json_backspace(&mut self, actions: &mut Vec<Action>) {
        let cursor = self.state.request.body_editor.json_cursor;
        if cursor == 0 {
            return;
        }
        let mut updated = self.state.request.body_json.clone();
        remove_char(&mut updated, cursor - 1);
        self.state.request.body_editor.json_cursor = cursor - 1;
        actions.push(Action::SetBodyContent(BodyContent::Json(updated)));
    }

    fn handle_json_delete(&mut self, actions: &mut Vec<Action>) {
        let cursor = self.state.request.body_editor.json_cursor;
        let mut updated = self.state.request.body_json.clone();
        if cursor < updated.chars().count() {
            remove_char(&mut updated, cursor);
            actions.push(Action::SetBodyContent(BodyContent::Json(updated)));
        }
    }

    fn handle_text_insert_char(&mut self, ch: char, actions: &mut Vec<Action>) {
        let cursor = self
            .state
            .request
            .body_editor
            .text_cursor
            .min(self.state.request.body_text.chars().count());
        if self.state.request.body_text.chars().count() >= MAX_BODY_TEXT_LENGTH {
            return;
        }
        let mut updated = self.state.request.body_text.clone();
        insert_char(&mut updated, cursor, ch);
        self.state.request.body_editor.text_cursor = cursor + 1;
        actions.push(Action::SetBodyContent(BodyContent::Text(updated)));
    }

    fn handle_text_backspace(&mut self, actions: &mut Vec<Action>) {
        let cursor = self.state.request.body_editor.text_cursor;
        if cursor == 0 {
            return;
        }
        let mut updated = self.state.request.body_text.clone();
        remove_char(&mut updated, cursor - 1);
        self.state.request.body_editor.text_cursor = cursor - 1;
        actions.push(Action::SetBodyContent(BodyContent::Text(updated)));
    }

    fn handle_text_delete(&mut self, actions: &mut Vec<Action>) {
        let cursor = self.state.request.body_editor.text_cursor;
        let mut updated = self.state.request.body_text.clone();
        if cursor < updated.chars().count() {
            remove_char(&mut updated, cursor);
            actions.push(Action::SetBodyContent(BodyContent::Text(updated)));
        }
    }

    fn form_editor_field_len(&self, editor: KeyValueEditorState) -> usize {
        let Some(row) = self.state.request.body_form.get(editor.selected_row) else {
            return 0;
        };
        match editor.active_field {
            KeyValueField::Key => row.key.chars().count(),
            KeyValueField::Value => row.value.chars().count(),
        }
    }

    fn handle_form_insert(
        &self,
        ch: char,
        editor: &mut KeyValueEditorState,
        actions: &mut Vec<Action>,
    ) {
        let row = self
            .state
            .request
            .body_form
            .get(editor.selected_row)
            .cloned()
            .unwrap_or_default();
        let mut updated = row;
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
        actions.push(Action::SetBodyContent(BodyContent::SetFormRow {
            index: editor.selected_row,
            row: updated,
        }));
    }

    fn handle_form_backspace(&self, editor: &mut KeyValueEditorState, actions: &mut Vec<Action>) {
        let Some(row) = self
            .state
            .request
            .body_form
            .get(editor.selected_row)
            .cloned()
        else {
            return;
        };
        if editor.cursor == 0 {
            return;
        }

        let mut updated = row;
        let field = match editor.active_field {
            KeyValueField::Key => &mut updated.key,
            KeyValueField::Value => &mut updated.value,
        };
        remove_char(field, editor.cursor - 1);
        editor.cursor -= 1;
        actions.push(Action::SetBodyContent(BodyContent::SetFormRow {
            index: editor.selected_row,
            row: updated,
        }));
    }

    fn handle_form_delete(&self, editor: &mut KeyValueEditorState, actions: &mut Vec<Action>) {
        let Some(row) = self
            .state
            .request
            .body_form
            .get(editor.selected_row)
            .cloned()
        else {
            return;
        };

        let mut updated = row;
        let field = match editor.active_field {
            KeyValueField::Key => &mut updated.key,
            KeyValueField::Value => &mut updated.value,
        };

        if editor.cursor < field.chars().count() {
            remove_char(field, editor.cursor);
            actions.push(Action::SetBodyContent(BodyContent::SetFormRow {
                index: editor.selected_row,
                row: updated,
            }));
        }
    }

    fn start_request_execution(&mut self) {
        let effective_url = rebuild_url_with_params(
            &self.state.request.url,
            &self.state.request.query_params,
            &self.state.request.query_param_tokens,
        );
        if effective_url.trim().is_empty() {
            self.state.response.last_error = Some(String::from("Request URL cannot be empty."));
            return;
        }

        if reqwest::Url::parse(&effective_url).is_err() {
            self.state.response.last_error = Some(format!(
                "Request URL is invalid: {}",
                sanitize_terminal_text(&effective_url)
            ));
            return;
        }

        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1).max(1);

        // This new request becomes the active context; clear display state.
        self.active_response_id = Some(request_id);
        self.state.response = crate::state::ResponseState::default();
        self.state.response.last_request_id = Some(request_id);

        let request_state = self.state.request.clone();
        let action_sender = self.action_sender.clone();
        let pool = self.http_pool.clone();
        let (cancel_sender, mut cancel_receiver) = watch::channel(false);

        self.request_cancel_senders
            .insert(request_id, cancel_sender);
        self.request_tasks.insert(
            request_id,
            tokio::spawn(async move {
                let _ = execute_request(
                    &pool,
                    &request_state,
                    request_id,
                    false,
                    &mut cancel_receiver,
                    &action_sender,
                )
                .await;
            }),
        );
    }

    /// Cancel only the active response context's in-flight request.
    fn cancel_active_in_flight_request(&self) {
        if let Some(id) = self.active_response_id
            && let Some(cancel_sender) = self.request_cancel_senders.get(&id)
        {
            let _ = cancel_sender.send(true);
        }
    }

    /// Cancel all in-flight requests (used on quit).
    fn cancel_all_in_flight_requests(&self) {
        for cancel_sender in self.request_cancel_senders.values() {
            let _ = cancel_sender.send(true);
        }
    }

    /// Clean up task and cancel sender for a completed/cancelled request.
    fn cleanup_request_handles(&mut self, request_id: u64) {
        self.request_tasks.remove(&request_id);
        self.request_cancel_senders.remove(&request_id);
    }

    fn apply_action(&mut self, action: Action) {
        match action {
            Action::Tick => {
                self.persistence.flush_pending(&self.state);
                // Tick down notification timer
                if self.state.notification_ticks_remaining > 0 {
                    self.state.notification_ticks_remaining -= 1;
                    if self.state.notification_ticks_remaining == 0 {
                        self.state.notification = None;
                    }
                }
            }
            Action::Render => {}
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
                self.state.request.url_error = None;
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
                track_content_type_manual_override_on_set(&mut self.state.request, index, &row);
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
                dedupe_named_header(&mut self.state.request, "Authorization", Some(index));
                dedupe_named_header(&mut self.state.request, "Content-Type", Some(index));
                reconcile_authorization_header(&mut self.state.request);
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
                reconcile_authorization_header(&mut self.state.request);
            }
            Action::RemoveHeader(index) => {
                if index < self.state.request.headers.len() {
                    track_content_type_manual_override_on_remove(&mut self.state.request, index);
                    self.state.request.headers.remove(index);
                    adjust_managed_header_indices_on_remove(&mut self.state.request, index);
                    normalize_editor_state(
                        &mut self.state.request.headers_editor,
                        &self.state.request.headers,
                    );
                    reconcile_authorization_header(&mut self.state.request);
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
            Action::SetAuthMode(mode) => {
                self.state.request.auth_mode = mode;
                clear_auth_credentials_for_mode(&mut self.state.request);
                self.state.request.auth_editor.active_field = match mode {
                    AuthMode::None => AuthField::Mode,
                    AuthMode::Bearer => {
                        if matches!(
                            self.state.request.auth_editor.active_field,
                            AuthField::Username | AuthField::Password
                        ) {
                            AuthField::Token
                        } else {
                            self.state.request.auth_editor.active_field
                        }
                    }
                    AuthMode::Basic => {
                        if self.state.request.auth_editor.active_field == AuthField::Token {
                            AuthField::Username
                        } else {
                            self.state.request.auth_editor.active_field
                        }
                    }
                };
                normalize_auth_editor(&mut self.state.request);
                reconcile_authorization_header(&mut self.state.request);
            }
            Action::SetAuthToken(token) => {
                if token.chars().count() > MAX_AUTH_TOKEN_LENGTH {
                    return;
                }
                self.state.request.auth_token = token;
                normalize_auth_editor(&mut self.state.request);
                reconcile_authorization_header(&mut self.state.request);
            }
            Action::SetAuthCredentials { username, password } => {
                if username.chars().count() > MAX_AUTH_USERNAME_LENGTH
                    || password.chars().count() > MAX_AUTH_PASSWORD_LENGTH
                {
                    return;
                }
                self.state.request.auth_username = username;
                self.state.request.auth_password = password;
                normalize_auth_editor(&mut self.state.request);
                reconcile_authorization_header(&mut self.state.request);
            }
            Action::SetBodyFormat(format) => {
                self.state.request.body_format = format;
                self.state.request.content_type_manual_override = false;
                match format {
                    BodyFormat::Json => {
                        if matches!(
                            self.state.request.body_editor.active_field,
                            BodyField::Form | BodyField::Text
                        ) {
                            self.state.request.body_editor.active_field = BodyField::Json;
                        }
                    }
                    BodyFormat::Form => {
                        if matches!(
                            self.state.request.body_editor.active_field,
                            BodyField::Json | BodyField::Text
                        ) {
                            self.state.request.body_editor.active_field = BodyField::Form;
                        }
                    }
                    BodyFormat::Text => {
                        if matches!(
                            self.state.request.body_editor.active_field,
                            BodyField::Json | BodyField::Form
                        ) {
                            self.state.request.body_editor.active_field = BodyField::Text;
                        }
                    }
                }
                apply_body_content_type_header(&mut self.state.request, true);
                normalize_body_editor(&mut self.state.request);
            }
            Action::SetBodyContent(content) => match content {
                BodyContent::Json(json) => {
                    if json.chars().count() > MAX_BODY_TEXT_LENGTH {
                        return;
                    }
                    self.state.request.body_json = json;
                    normalize_body_editor(&mut self.state.request);
                }
                BodyContent::Text(text) => {
                    if text.chars().count() > MAX_BODY_TEXT_LENGTH {
                        return;
                    }
                    self.state.request.body_text = text;
                    normalize_body_editor(&mut self.state.request);
                }
                BodyContent::SetFormRow { index, row } => {
                    upsert_row_with_limit(
                        &mut self.state.request.body_form,
                        index,
                        limit_key_value_row(row),
                        MAX_BODY_FORM_ROWS,
                    );
                    normalize_editor_state(
                        &mut self.state.request.body_editor.form_editor,
                        &self.state.request.body_form,
                    );
                }
                BodyContent::AddFormRow => {
                    if self.state.request.body_form.len() >= MAX_BODY_FORM_ROWS {
                        return;
                    }
                    self.state.request.body_form.push(KeyValueRow::default());
                    normalize_editor_state(
                        &mut self.state.request.body_editor.form_editor,
                        &self.state.request.body_form,
                    );
                }
                BodyContent::RemoveFormRow(index) => {
                    if index < self.state.request.body_form.len() {
                        self.state.request.body_form.remove(index);
                        normalize_editor_state(
                            &mut self.state.request.body_editor.form_editor,
                            &self.state.request.body_form,
                        );
                    }
                }
            },
            Action::SendRequest => {
                let url = self.state.request.url.trim().to_string();
                if url.is_empty() {
                    self.state.request.url_error = Some(String::from("URL cannot be empty"));
                    self.state.notification = Some((
                        String::from("URL cannot be empty — set a URL before sending"),
                        crate::state::NotificationKind::Error,
                    ));
                    self.state.notification_ticks_remaining = 50;
                    return;
                }
                if !url.starts_with("http://") && !url.starts_with("https://") {
                    self.state.request.url_error =
                        Some(String::from("URL must start with http:// or https://"));
                    self.state.notification = Some((
                        String::from("Invalid URL — must start with http:// or https://"),
                        crate::state::NotificationKind::Error,
                    ));
                    self.state.notification_ticks_remaining = 50;
                    return;
                }
                // Clear any previous error and send
                self.state.request.url_error = None;
                self.state.response.last_error = None;
                self.state.response.cancelled = false;
                self.start_request_execution();
            }
            Action::CancelRequest => {
                if let Some(in_flight) = self.state.response.in_flight.as_mut() {
                    in_flight.cancellation_requested = true;
                }
                self.cancel_active_in_flight_request();
            }
            Action::RequestStarted {
                request_id,
                method,
                url,
            } => {
                let record = InFlightRequest {
                    id: request_id,
                    method,
                    url: url.clone(),
                    cancellation_requested: false,
                };
                self.state
                    .in_flight_requests
                    .insert(request_id, record.clone());
                if self.active_response_id == Some(request_id) {
                    self.state.response.last_request_id = Some(request_id);
                    self.state.response.in_flight = Some(record);
                    self.state.response.buffer.clear();
                    self.state.response.scroll_offset = 0;
                    self.state.response.horizontal_scroll_offset = 0;
                    self.state.response.metadata = None;
                    self.state.response.last_error = None;
                    self.state.response.cancelled = false;
                    self.state.response.truncated = false;
                    self.recompute_response_search();
                }
            }
            Action::RequestCompleted {
                request_id,
                mut metadata,
            } => {
                self.state.in_flight_requests.remove(&request_id);
                if self.active_response_id == Some(request_id) {
                    metadata.truncated = self.state.response.truncated;
                    self.state.response.metadata = Some(metadata.clone());
                    self.state.response.in_flight = None;
                    self.state.response.cancelled = false;
                    let max_scroll = response_scroll_upper_bound(&self.state.response);
                    self.state.response.scroll_offset =
                        self.state.response.scroll_offset.min(max_scroll);
                    self.recompute_response_search();
                    // In Small mode, automatically show the response pane when a request completes.
                    if matches!(self.state.layout_mode, LayoutMode::Small) {
                        self.state.small_mode_show_response = true;
                    }
                    self.cleanup_request_handles(request_id);

                    // Auto-record to history.
                    let timestamp_secs = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs();
                    let entry = HistoryEntry {
                        id: uuid::Uuid::new_v4().to_string(),
                        timestamp_secs,
                        method: self.state.request.method.as_str().to_string(),
                        url: self.state.request.url.clone(),
                        status_code: metadata.status_code,
                        elapsed_ms: Some(metadata.duration_ms),
                        request: Some(snapshot_request(
                            &self.state.request,
                            self.state.config.persist_sensitive_headers,
                        )),
                    };
                    self.state.history.insert(0, entry);
                    self.state.history.truncate(self.state.config.history_limit);
                    self.persistence.schedule_save(PersistTarget::History);
                } else {
                    // Background request completed — clean up silently.
                    self.cleanup_request_handles(request_id);
                }
            }
            Action::RequestFailed { request_id, error } => {
                self.state.in_flight_requests.remove(&request_id);
                if self.active_response_id == Some(request_id) {
                    self.state.response.last_error = Some(sanitize_terminal_text(&error));
                    self.state.response.in_flight = None;
                    self.recompute_response_search();
                    // In Small mode, automatically show the response pane when a request fails.
                    if matches!(self.state.layout_mode, LayoutMode::Small) {
                        self.state.small_mode_show_response = true;
                    }
                    self.cleanup_request_handles(request_id);
                } else {
                    // Background request failed — clean up silently.
                    self.cleanup_request_handles(request_id);
                }
            }
            Action::RequestCancelled { request_id } => {
                self.state.in_flight_requests.remove(&request_id);
                if self.active_response_id == Some(request_id) {
                    self.state.response.in_flight = None;
                    self.state.response.cancelled = true;
                    self.recompute_response_search();
                }
                self.cleanup_request_handles(request_id);
            }
            Action::ResponseChunk { request_id, chunk } => {
                if self.active_response_id == Some(request_id) {
                    let chunk_len = chunk.len();
                    self.state.response.buffer.append_chunk(&chunk);
                    self.state.response.truncated = self.state.response.buffer.is_truncated();
                    let max_scroll = response_scroll_upper_bound(&self.state.response);
                    self.state.response.scroll_offset =
                        self.state.response.scroll_offset.min(max_scroll);
                    if self.state.response.search.active {
                        self.pending_response_search_bytes =
                            self.pending_response_search_bytes.saturating_add(chunk_len);
                        if self.pending_response_search_bytes
                            >= RESPONSE_SEARCH_RECOMPUTE_CHUNK_BYTES
                        {
                            self.recompute_response_search();
                        }
                    }
                }
                // Chunks for non-active requests are discarded — the background
                // task drains to completion but we don't buffer the data.
            }
            Action::ScrollResponse(delta) => {
                let max_scroll = response_scroll_upper_bound(&self.state.response);
                if delta >= 0 {
                    self.state.response.scroll_offset = self
                        .state
                        .response
                        .scroll_offset
                        .saturating_add(delta as usize)
                        .min(max_scroll);
                } else {
                    self.state.response.scroll_offset = self
                        .state
                        .response
                        .scroll_offset
                        .saturating_sub(delta.unsigned_abs() as usize);
                }
            }
            Action::ScrollResponseHorizontal(delta) => {
                if delta >= 0 {
                    self.state.response.horizontal_scroll_offset = self
                        .state
                        .response
                        .horizontal_scroll_offset
                        .saturating_add(delta as usize);
                } else {
                    self.state.response.horizontal_scroll_offset = self
                        .state
                        .response
                        .horizontal_scroll_offset
                        .saturating_sub(delta.unsigned_abs() as usize);
                }
            }
            Action::ToggleResponseWrap => {
                self.state.response.wrap_lines = !self.state.response.wrap_lines;
                if self.state.response.wrap_lines {
                    self.state.response.horizontal_scroll_offset = 0;
                }
            }
            Action::SetResponseTab(tab) => {
                self.state.response.active_tab = tab;
                if self.state.response.search.active {
                    self.state.response.search.scope = search_scope_for_tab(tab);
                    self.recompute_response_search();
                }
            }
            Action::OpenResponseSearch => {
                self.state.response.search.active = true;
                self.state.response.search.scope =
                    search_scope_for_tab(self.state.response.active_tab);
                self.recompute_response_search();
            }
            Action::CloseResponseSearch => {
                self.state.response.search.active = false;
                self.state.response.search.query.clear();
                self.state.response.search.matches.clear();
                self.state.response.search.current_match = None;
            }
            Action::SearchInResponse(query) => {
                let mut bounded_query = query;
                truncate_to_char_limit(&mut bounded_query, MAX_RESPONSE_SEARCH_QUERY_CHARS);
                self.state.response.search.query = bounded_query;
                self.recompute_response_search();
            }
            Action::NextSearchMatch => {
                let match_count = self.state.response.search.matches.len();
                if match_count == 0 {
                    return;
                }
                let current = self
                    .state
                    .response
                    .search
                    .current_match
                    .filter(|index| *index < match_count);
                let next = match current {
                    Some(index) => (index + 1) % match_count,
                    None => 0,
                };
                self.state.response.search.current_match = Some(next);
                self.scroll_to_search_match(next);
            }
            Action::PrevSearchMatch => {
                let match_count = self.state.response.search.matches.len();
                if match_count == 0 {
                    return;
                }
                let current = self
                    .state
                    .response
                    .search
                    .current_match
                    .filter(|index| *index < match_count);
                let prev = match current {
                    Some(0) | None => match_count - 1,
                    Some(index) => index - 1,
                };
                self.state.response.search.current_match = Some(prev);
                self.scroll_to_search_match(prev);
            }
            Action::ToggleHelp => {
                self.state.help_visible = !self.state.help_visible;
                if !self.state.help_visible {
                    self.state.help_scroll = 0;
                }
            }
            Action::CloseHelp => {
                self.state.help_visible = false;
                self.state.help_scroll = 0;
            }
            Action::ScrollHelp(delta) => {
                let terminal_area =
                    Rect::new(0, 0, self.state.terminal_size.0, self.state.terminal_size.1);
                let main_area = LayoutManager::compute(terminal_area).main;
                let max_scroll =
                    help_modal::max_scroll_for_area(main_area, help_modal::line_count());
                if delta < 0 {
                    self.state.help_scroll = self
                        .state
                        .help_scroll
                        .saturating_sub(delta.unsigned_abs() as usize);
                } else {
                    self.state.help_scroll =
                        (self.state.help_scroll + delta as usize).min(max_scroll);
                }
            }

            // ── Collections ──────────────────────────────────────────────────
            Action::CreateCollection { name } => {
                let id = uuid::Uuid::new_v4().to_string();
                let col = Collection {
                    id: id.clone(),
                    name,
                    expanded: true,
                    requests: Vec::new(),
                };
                self.state.collections.push(col);
                self.persistence
                    .schedule_save(PersistTarget::Collection(id));
            }
            Action::RenameCollection { index, name } => {
                if let Some(col) = self.state.collections.get_mut(index) {
                    col.name = name;
                    let id = col.id.clone();
                    self.persistence
                        .schedule_save(PersistTarget::Collection(id));
                }
            }
            Action::DeleteCollection(index) => {
                if index < self.state.collections.len() {
                    let col = self.state.collections.remove(index);
                    let _ = delete_collection_file(&col.id);
                    // Reset sidebar selection if it pointed into this collection.
                    match &self.state.sidebar_selected_item {
                        SidebarItem::Collection(i) if *i == index => {
                            self.state.sidebar_selected_item = SidebarItem::None;
                        }
                        SidebarItem::Request { collection, .. } if *collection == index => {
                            self.state.sidebar_selected_item = SidebarItem::None;
                        }
                        _ => {}
                    }
                }
            }
            Action::ToggleCollectionExpanded(index) => {
                if let Some(col) = self.state.collections.get_mut(index) {
                    col.expanded = !col.expanded;
                }
            }
            Action::SaveRequestToCollection {
                collection_index,
                name,
            } => {
                if let Some(col) = self.state.collections.get_mut(collection_index) {
                    let mut saved = snapshot_request(
                        &self.state.request,
                        self.state.config.persist_sensitive_headers,
                    );
                    saved.name = name;
                    col.requests.push(saved);
                    let id = col.id.clone();
                    self.persistence
                        .schedule_save(PersistTarget::Collection(id));
                }
            }
            Action::RenameCollectionRequest {
                collection,
                request,
                name,
            } => {
                if let Some(col) = self.state.collections.get_mut(collection)
                    && let Some(req) = col.requests.get_mut(request)
                {
                    req.name = name;
                    let id = col.id.clone();
                    self.persistence
                        .schedule_save(PersistTarget::Collection(id));
                }
            }
            Action::DeleteCollectionRequest {
                collection,
                request,
            } => {
                if let Some(col) = self.state.collections.get_mut(collection)
                    && request < col.requests.len()
                {
                    col.requests.remove(request);
                    let id = col.id.clone();
                    self.persistence
                        .schedule_save(PersistTarget::Collection(id));
                }
                // Reset sidebar selection if it pointed at the deleted request.
                match &self.state.sidebar_selected_item {
                    SidebarItem::Request {
                        collection: c,
                        request: r,
                    } if *c == collection && *r == request => {
                        self.state.sidebar_selected_item = SidebarItem::Collection(collection);
                    }
                    _ => {}
                }
            }
            Action::LoadCollectionRequest {
                collection,
                request,
            } => {
                let saved = self
                    .state
                    .collections
                    .get(collection)
                    .and_then(|col| col.requests.get(request))
                    .cloned();
                if let Some(saved) = saved {
                    load_saved_request_into_state(&mut self.state.request, &saved);
                }
            }

            // ── History ───────────────────────────────────────────────────────
            Action::RecordHistory(entry) => {
                self.state.history.insert(0, *entry);
                self.state.history.truncate(self.state.config.history_limit);
                self.persistence.schedule_save(PersistTarget::History);
            }
            Action::LoadFromHistory(index) => {
                let saved = self
                    .state
                    .history
                    .get(index)
                    .and_then(|e| e.request.as_ref())
                    .cloned();
                if let Some(saved) = saved {
                    load_saved_request_into_state(&mut self.state.request, &saved);
                }
            }
            Action::ClearHistory => {
                self.state.history.clear();
                self.state.history_marked_indices.clear();
                self.persistence.schedule_save(PersistTarget::History);
            }
            Action::DeleteHistoryEntry(index) => {
                if index < self.state.history.len() {
                    self.state.history.remove(index);
                    // Shift down all marked indices that were above this index.
                    self.state.history_marked_indices = self
                        .state
                        .history_marked_indices
                        .iter()
                        .filter_map(|&i| match i.cmp(&index) {
                            std::cmp::Ordering::Less => Some(i),
                            std::cmp::Ordering::Equal => None,
                            std::cmp::Ordering::Greater => Some(i - 1),
                        })
                        .collect();
                    // Adjust sidebar selection.
                    let new_len = self.state.history.len();
                    if let SidebarItem::HistoryEntry(sel) = self.state.sidebar_selected_item {
                        self.state.sidebar_selected_item = if new_len == 0 {
                            SidebarItem::None
                        } else if sel >= new_len {
                            SidebarItem::HistoryEntry(new_len - 1)
                        } else {
                            SidebarItem::HistoryEntry(sel)
                        };
                    }
                    self.persistence.schedule_save(PersistTarget::History);
                }
            }
            Action::DeleteHistoryEntries(indices) => {
                if indices.is_empty() {
                    return;
                }
                // Sort descending so removals don't shift remaining indices.
                let mut sorted = indices.clone();
                sorted.sort_unstable_by(|a, b| b.cmp(a));
                sorted.dedup();
                for idx in &sorted {
                    if *idx < self.state.history.len() {
                        self.state.history.remove(*idx);
                    }
                }
                self.state.history_marked_indices.clear();
                // Adjust sidebar selection.
                let new_len = self.state.history.len();
                if let SidebarItem::HistoryEntry(sel) = self.state.sidebar_selected_item {
                    self.state.sidebar_selected_item = if new_len == 0 {
                        SidebarItem::None
                    } else if sel >= new_len {
                        SidebarItem::HistoryEntry(new_len - 1)
                    } else {
                        SidebarItem::HistoryEntry(sel)
                    };
                }
                self.persistence.schedule_save(PersistTarget::History);
            }
            Action::ToggleHistoryMark(index) => {
                if index < self.state.history.len() {
                    if self.state.history_marked_indices.contains(&index) {
                        self.state.history_marked_indices.remove(&index);
                    } else {
                        self.state.history_marked_indices.insert(index);
                    }
                }
            }

            // ── Sidebar navigation ────────────────────────────────────────────
            Action::ToggleSidebar => {
                if matches!(self.state.layout_mode, LayoutMode::Large) {
                    // Sidebar is always visible in Large; toggle focus instead.
                    self.state.sidebar_focused = !self.state.sidebar_focused;
                    if !self.state.sidebar_focused {
                        self.state.sidebar_prompt = None;
                    }
                } else {
                    self.state.sidebar_visible = !self.state.sidebar_visible;
                    self.state.sidebar_focused = self.state.sidebar_visible;
                    if !self.state.sidebar_visible {
                        self.state.sidebar_prompt = None;
                    }
                }
            }
            Action::ToggleSmallModePane => {
                self.state.small_mode_show_response = !self.state.small_mode_show_response;
            }
            Action::SidebarFocusNext => {
                self.sidebar_navigate(1);
            }
            Action::SidebarFocusPrev => {
                self.sidebar_navigate(-1);
            }
            Action::SidebarScrollCollectionsHorizontal(delta) => {
                self.scroll_sidebar_horizontal(delta);
            }
            Action::SidebarSelect => {
                match self.state.sidebar_selected_item.clone() {
                    SidebarItem::Collection(idx) => {
                        self.apply_action(Action::ToggleCollectionExpanded(idx));
                    }
                    SidebarItem::Request {
                        collection,
                        request,
                    } => {
                        self.apply_action(Action::LoadCollectionRequest {
                            collection,
                            request,
                        });
                        // Close/defocus sidebar after loading.
                        self.state.sidebar_focused = false;
                        if !matches!(self.state.layout_mode, LayoutMode::Large) {
                            self.state.sidebar_visible = false;
                        }
                    }
                    SidebarItem::HistoryEntry(idx) => {
                        self.apply_action(Action::LoadFromHistory(idx));
                        self.state.sidebar_focused = false;
                        if !matches!(self.state.layout_mode, LayoutMode::Large) {
                            self.state.sidebar_visible = false;
                        }
                    }
                    SidebarItem::None => {}
                }
            }
            Action::SidebarClose => {
                self.state.sidebar_focused = false;
                self.state.sidebar_prompt = None;
                if !matches!(self.state.layout_mode, LayoutMode::Large) {
                    self.state.sidebar_visible = false;
                }
            }

            // ── Persistence ───────────────────────────────────────────────────
            Action::PersistenceError(msg) => {
                eprintln!("[posterm] persistence error: {msg}");
                self.state.notification = Some((
                    sanitize_terminal_text(&msg),
                    crate::state::NotificationKind::Error,
                ));
                self.state.notification_ticks_remaining = 50;
            }

            // ── Notifications ─────────────────────────────────────────────────
            Action::ShowNotification { message, kind } => {
                self.state.notification = Some((sanitize_terminal_text(&message), kind));
                self.state.notification_ticks_remaining = 50;
            }
            Action::DismissNotification => {
                self.state.notification = None;
                self.state.notification_ticks_remaining = 0;
            }
        }
    }

    fn recompute_response_search(&mut self) {
        self.pending_response_search_bytes = 0;

        if !self.state.response.search.active {
            self.state.response.search.matches.clear();
            self.state.response.search.current_match = None;
            return;
        }

        let mut query = self.state.response.search.query.clone();
        truncate_to_char_limit(&mut query, MAX_RESPONSE_SEARCH_QUERY_CHARS);
        if query != self.state.response.search.query {
            self.state.response.search.query = query.clone();
        }
        if query.is_empty() {
            self.state.response.search.matches.clear();
            self.state.response.search.current_match = None;
            return;
        }

        let lines = response_search_lines(&self.state, self.state.response.search.scope);
        let matches = find_line_matches(&lines, &query, MAX_RESPONSE_SEARCH_MATCHES);
        self.state.response.search.matches = matches;
        self.state.response.search.current_match = if self.state.response.search.matches.is_empty()
        {
            None
        } else {
            Some(0)
        };
        if self.state.response.search.current_match.is_some() {
            self.scroll_to_search_match(0);
        }
    }

    fn scroll_to_search_match(&mut self, match_index: usize) {
        let Some(entry) = self.state.response.search.matches.get(match_index) else {
            return;
        };
        self.state.response.scroll_offset = entry.line_index;
    }

    /// Moves sidebar selection by `delta` (+1 = down, -1 = up) through the
    /// flat list of visible sidebar items.
    fn sidebar_navigate(&mut self, delta: i32) {
        // Build the flat ordered list of selectable items (same ordering as the
        // renderer uses).
        let mut items: Vec<SidebarItem> = Vec::new();
        for (col_idx, col) in self.state.collections.iter().enumerate() {
            items.push(SidebarItem::Collection(col_idx));
            if col.expanded {
                for req_idx in 0..col.requests.len() {
                    items.push(SidebarItem::Request {
                        collection: col_idx,
                        request: req_idx,
                    });
                }
            }
        }
        for hist_idx in 0..self.state.history.len() {
            items.push(SidebarItem::HistoryEntry(hist_idx));
        }

        if items.is_empty() {
            return;
        }

        let current_pos = items
            .iter()
            .position(|item| *item == self.state.sidebar_selected_item)
            .unwrap_or(0);

        let next_pos = if delta > 0 {
            (current_pos + delta as usize).min(items.len() - 1)
        } else {
            current_pos.saturating_sub(delta.unsigned_abs() as usize)
        };

        self.state.sidebar_selected_item = items[next_pos].clone();
    }

    fn scroll_sidebar_horizontal(&mut self, delta: i16) {
        let offset = match self.state.sidebar_selected_item {
            SidebarItem::HistoryEntry(_) => &mut self.state.sidebar_history_horizontal_offset,
            SidebarItem::Collection(_) | SidebarItem::Request { .. } | SidebarItem::None => {
                &mut self.state.sidebar_collections_horizontal_offset
            }
        };

        if delta >= 0 {
            *offset = offset.saturating_add(delta as usize);
        } else {
            *offset = offset.saturating_sub(delta.unsigned_abs() as usize);
        }
    }

    fn start_sidebar_prompt(&mut self, mode: SidebarPromptMode, value: String) {
        self.state.sidebar_prompt = Some(SidebarPromptState { mode, value });
    }

    fn selected_sidebar_rename_prompt(&self) -> Option<(SidebarPromptMode, String)> {
        match self.state.sidebar_selected_item {
            SidebarItem::Collection(index) => self.state.collections.get(index).map(|col| {
                (
                    SidebarPromptMode::RenameCollection { index },
                    col.name.clone(),
                )
            }),
            SidebarItem::Request {
                collection,
                request,
            } => self
                .state
                .collections
                .get(collection)
                .and_then(|col| col.requests.get(request))
                .map(|req| {
                    (
                        SidebarPromptMode::RenameCollectionRequest {
                            collection,
                            request,
                        },
                        req.name.clone(),
                    )
                }),
            _ => None,
        }
    }

    fn selected_collection_for_save(&self) -> Option<usize> {
        match self.state.sidebar_selected_item {
            SidebarItem::Collection(index) => Some(index),
            SidebarItem::Request { collection, .. } => Some(collection),
            SidebarItem::HistoryEntry(_) | SidebarItem::None => None,
        }
    }

    fn confirm_sidebar_prompt(&mut self) -> Vec<Action> {
        let Some(prompt) = self.state.sidebar_prompt.clone() else {
            return Vec::new();
        };

        let name = prompt.value.trim().to_string();
        if name.is_empty() {
            return vec![
                Action::ShowNotification {
                    message: String::from("Name cannot be empty"),
                    kind: crate::state::NotificationKind::Error,
                },
                Action::Render,
            ];
        }

        let mut actions = Vec::new();
        match prompt.mode {
            SidebarPromptMode::CreateCollection => {
                actions.push(Action::CreateCollection { name: name.clone() });
                actions.push(Action::ShowNotification {
                    message: format!("Created collection '{name}'"),
                    kind: crate::state::NotificationKind::Info,
                });
            }
            SidebarPromptMode::RenameCollection { index } => {
                if self.state.collections.get(index).is_none() {
                    actions.push(Action::ShowNotification {
                        message: String::from("Collection no longer exists"),
                        kind: crate::state::NotificationKind::Error,
                    });
                } else {
                    actions.push(Action::RenameCollection {
                        index,
                        name: name.clone(),
                    });
                    actions.push(Action::ShowNotification {
                        message: format!("Renamed collection to '{name}'"),
                        kind: crate::state::NotificationKind::Info,
                    });
                }
            }
            SidebarPromptMode::SaveRequestToCollection { collection_index } => {
                if self.state.collections.get(collection_index).is_none() {
                    actions.push(Action::ShowNotification {
                        message: String::from("Collection no longer exists"),
                        kind: crate::state::NotificationKind::Error,
                    });
                } else {
                    actions.push(Action::SaveRequestToCollection {
                        collection_index,
                        name: name.clone(),
                    });
                    actions.push(Action::ShowNotification {
                        message: format!("Saved current request as '{name}'"),
                        kind: crate::state::NotificationKind::Info,
                    });
                }
            }
            SidebarPromptMode::RenameCollectionRequest {
                collection,
                request,
            } => {
                let exists = self
                    .state
                    .collections
                    .get(collection)
                    .and_then(|col| col.requests.get(request))
                    .is_some();
                if !exists {
                    actions.push(Action::ShowNotification {
                        message: String::from("Saved request no longer exists"),
                        kind: crate::state::NotificationKind::Error,
                    });
                } else {
                    actions.push(Action::RenameCollectionRequest {
                        collection,
                        request,
                        name: name.clone(),
                    });
                    actions.push(Action::ShowNotification {
                        message: format!("Renamed saved request to '{name}'"),
                        kind: crate::state::NotificationKind::Info,
                    });
                }
            }
        }

        self.state.sidebar_prompt = None;
        actions.push(Action::Render);
        actions
    }

    fn delete_selected_sidebar_item_actions(&self) -> Vec<Action> {
        match self.state.sidebar_selected_item {
            SidebarItem::Collection(index) => {
                if let Some(collection) = self.state.collections.get(index) {
                    vec![
                        Action::DeleteCollection(index),
                        Action::ShowNotification {
                            message: format!("Deleted collection '{}'", collection.name),
                            kind: crate::state::NotificationKind::Info,
                        },
                        Action::Render,
                    ]
                } else {
                    vec![
                        Action::ShowNotification {
                            message: String::from("Collection no longer exists"),
                            kind: crate::state::NotificationKind::Error,
                        },
                        Action::Render,
                    ]
                }
            }
            SidebarItem::Request {
                collection,
                request,
            } => {
                let Some(req_name) = self
                    .state
                    .collections
                    .get(collection)
                    .and_then(|col| col.requests.get(request))
                    .map(|req| req.name.clone())
                else {
                    return vec![
                        Action::ShowNotification {
                            message: String::from("Saved request no longer exists"),
                            kind: crate::state::NotificationKind::Error,
                        },
                        Action::Render,
                    ];
                };

                vec![
                    Action::DeleteCollectionRequest {
                        collection,
                        request,
                    },
                    Action::ShowNotification {
                        message: format!("Deleted saved request '{req_name}'"),
                        kind: crate::state::NotificationKind::Info,
                    },
                    Action::Render,
                ]
            }
            SidebarItem::HistoryEntry(index) => {
                // If any entries are marked, delete all marked; otherwise delete this one entry.
                if !self.state.history_marked_indices.is_empty() {
                    let indices: Vec<usize> =
                        self.state.history_marked_indices.iter().copied().collect();
                    let count = indices.len();
                    vec![
                        Action::DeleteHistoryEntries(indices),
                        Action::ShowNotification {
                            message: format!(
                                "Deleted {count} history entr{}",
                                if count == 1 { "y" } else { "ies" }
                            ),
                            kind: crate::state::NotificationKind::Info,
                        },
                        Action::Render,
                    ]
                } else if self.state.history.get(index).is_some() {
                    vec![
                        Action::DeleteHistoryEntry(index),
                        Action::ShowNotification {
                            message: String::from("Deleted history entry"),
                            kind: crate::state::NotificationKind::Info,
                        },
                        Action::Render,
                    ]
                } else {
                    vec![
                        Action::ShowNotification {
                            message: String::from("History entry no longer exists"),
                            kind: crate::state::NotificationKind::Error,
                        },
                        Action::Render,
                    ]
                }
            }
            SidebarItem::None => vec![
                Action::ShowNotification {
                    message: String::from(
                        "Select a collection, saved request, or history entry to delete",
                    ),
                    kind: crate::state::NotificationKind::Error,
                },
                Action::Render,
            ],
        }
    }
}

// ── Phase 6 helper functions ──────────────────────────────────────────────────

fn default_saved_request_name(req: &crate::state::RequestState) -> String {
    let url = req.url.trim();
    if url.is_empty() {
        format!("{} request", req.method.as_str())
    } else {
        format!("{} {url}", req.method.as_str())
    }
}

/// Creates a [`SavedRequest`] snapshot from the current [`RequestState`].
///
/// When `persist_sensitive` is `false`, sensitive header values are replaced
/// with `"[REDACTED]"` and auth credentials are cleared.
fn snapshot_request(req: &crate::state::RequestState, persist_sensitive: bool) -> SavedRequest {
    let mut headers: Vec<SerializedKeyValueRow> = req
        .headers
        .iter()
        .map(|row| SerializedKeyValueRow {
            key: row.key.clone(),
            value: row.value.clone(),
            enabled: row.enabled,
        })
        .collect();

    if !persist_sensitive {
        for h in &mut headers {
            if is_sensitive_header(&h.key) {
                h.value = String::from("[REDACTED]");
            }
        }
    }

    SavedRequest {
        id: uuid::Uuid::new_v4().to_string(),
        name: String::new(),
        method: req.method.as_str().to_string(),
        url: req.url.clone(),
        query_params: req
            .query_params
            .iter()
            .map(|r| SerializedKeyValueRow {
                key: r.key.clone(),
                value: r.value.clone(),
                enabled: r.enabled,
            })
            .collect(),
        headers,
        auth_mode: req.auth_mode.as_str().to_string(),
        auth_token: if persist_sensitive {
            req.auth_token.clone()
        } else {
            String::new()
        },
        auth_username: if persist_sensitive {
            req.auth_username.clone()
        } else {
            String::new()
        },
        auth_password: if persist_sensitive {
            req.auth_password.clone()
        } else {
            String::new()
        },
        body_format: req.body_format.as_str().to_string(),
        body_json: req.body_json.clone(),
        body_text: req.body_text.clone(),
        body_form: req
            .body_form
            .iter()
            .map(|r| SerializedKeyValueRow {
                key: r.key.clone(),
                value: r.value.clone(),
                enabled: r.enabled,
            })
            .collect(),
    }
}

/// Loads a [`SavedRequest`] snapshot into `state`, overwriting the current
/// request fields.
fn load_saved_request_into_state(state: &mut crate::state::RequestState, saved: &SavedRequest) {
    state.method = http_method_from_str(&saved.method);
    state.url = saved.url.clone();
    state.url_cursor = state.url.chars().count();
    state.query_params = saved
        .query_params
        .iter()
        .map(|r| KeyValueRow {
            key: r.key.clone(),
            value: r.value.clone(),
            enabled: r.enabled,
        })
        .collect();
    state.query_param_tokens = state
        .query_params
        .iter()
        .map(|_| QueryParamToken::KeyValue)
        .collect();
    state.headers = saved
        .headers
        .iter()
        .map(|r| KeyValueRow {
            key: r.key.clone(),
            value: r.value.clone(),
            enabled: r.enabled,
        })
        .collect();
    state.auth_mode = auth_mode_from_str(&saved.auth_mode);
    state.auth_token = saved.auth_token.clone();
    state.auth_username = saved.auth_username.clone();
    state.auth_password = saved.auth_password.clone();
    state.body_format = body_format_from_str(&saved.body_format);
    state.body_json = saved.body_json.clone();
    state.body_text = saved.body_text.clone();
    state.body_form = saved
        .body_form
        .iter()
        .map(|r| KeyValueRow {
            key: r.key.clone(),
            value: r.value.clone(),
            enabled: r.enabled,
        })
        .collect();
    // Reset cursor / editor state so UX is clean after load.
    state.query_editor = KeyValueEditorState::default();
    state.headers_editor = KeyValueEditorState::default();
}

fn http_method_from_str(s: &str) -> HttpMethod {
    match s {
        "POST" => HttpMethod::Post,
        "PUT" => HttpMethod::Put,
        "PATCH" => HttpMethod::Patch,
        "DELETE" => HttpMethod::Delete,
        "HEAD" => HttpMethod::Head,
        "OPTIONS" => HttpMethod::Options,
        _ => HttpMethod::Get,
    }
}

fn auth_mode_from_str(s: &str) -> AuthMode {
    match s {
        "Bearer" => AuthMode::Bearer,
        "Basic" => AuthMode::Basic,
        _ => AuthMode::None,
    }
}

fn body_format_from_str(s: &str) -> BodyFormat {
    match s {
        "Form" => BodyFormat::Form,
        "Text" => BodyFormat::Text,
        _ => BodyFormat::Json,
    }
}

fn response_scroll_upper_bound(response: &crate::state::ResponseState) -> usize {
    response
        .buffer
        .total_bytes()
        .saturating_add(response.buffer.total_lines())
}

fn search_scope_for_tab(tab: ResponseTab) -> ResponseSearchScope {
    match tab {
        ResponseTab::Body => ResponseSearchScope::Body,
        ResponseTab::Headers => ResponseSearchScope::Headers,
        ResponseTab::Raw => ResponseSearchScope::Raw,
    }
}

fn response_search_lines(state: &AppState, scope: ResponseSearchScope) -> Vec<String> {
    match scope {
        ResponseSearchScope::Body => body_search_lines(state),
        ResponseSearchScope::Headers => header_lines(state),
        ResponseSearchScope::Raw => raw_lines(state),
    }
}

fn find_line_matches(lines: &[String], query: &str, max_matches: usize) -> Vec<SearchMatch> {
    if query.is_empty() || max_matches == 0 {
        return Vec::new();
    }

    let needle_len = query.chars().count();
    let mut matches = Vec::new();

    'line_scan: for (line_index, line) in lines.iter().enumerate() {
        let mut search_from_byte = 0;
        while search_from_byte <= line.len() {
            let Some(relative_byte_index) = line[search_from_byte..].find(query) else {
                break;
            };
            let start_byte = search_from_byte + relative_byte_index;
            let start_char = line[..start_byte].chars().count();
            matches.push(SearchMatch {
                line_index,
                start_char,
                end_char: start_char + needle_len,
            });
            if matches.len() >= max_matches {
                break 'line_scan;
            }
            search_from_byte = start_byte.saturating_add(query.len());
        }
    }

    matches
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

fn next_auth_field(field: AuthField, mode: AuthMode) -> AuthField {
    match mode {
        AuthMode::None => AuthField::Mode,
        AuthMode::Bearer => match field {
            AuthField::Mode => AuthField::Token,
            AuthField::Token => AuthField::Mode,
            AuthField::Username | AuthField::Password => AuthField::Mode,
        },
        AuthMode::Basic => match field {
            AuthField::Mode => AuthField::Username,
            AuthField::Username => AuthField::Password,
            AuthField::Password => AuthField::Mode,
            AuthField::Token => AuthField::Mode,
        },
    }
}

fn prev_auth_field(field: AuthField, mode: AuthMode) -> AuthField {
    match mode {
        AuthMode::None => AuthField::Mode,
        AuthMode::Bearer => match field {
            AuthField::Mode => AuthField::Token,
            AuthField::Token => AuthField::Mode,
            AuthField::Username | AuthField::Password => AuthField::Mode,
        },
        AuthMode::Basic => match field {
            AuthField::Mode => AuthField::Password,
            AuthField::Username => AuthField::Mode,
            AuthField::Password => AuthField::Username,
            AuthField::Token => AuthField::Mode,
        },
    }
}

fn next_body_field(field: BodyField, format: BodyFormat) -> BodyField {
    match format {
        BodyFormat::Json => match field {
            BodyField::Format => BodyField::Json,
            BodyField::Json => BodyField::Format,
            BodyField::Form | BodyField::Text => BodyField::Format,
        },
        BodyFormat::Form => match field {
            BodyField::Format => BodyField::Form,
            BodyField::Form => BodyField::Format,
            BodyField::Json | BodyField::Text => BodyField::Format,
        },
        BodyFormat::Text => match field {
            BodyField::Format => BodyField::Text,
            BodyField::Text => BodyField::Format,
            BodyField::Json | BodyField::Form => BodyField::Format,
        },
    }
}

fn prev_body_field(field: BodyField, format: BodyFormat) -> BodyField {
    next_body_field(field, format)
}

fn normalize_auth_editor(request: &mut crate::state::RequestState) {
    let editor = &mut request.auth_editor;
    let token_len = request.auth_token.chars().count();
    let username_len = request.auth_username.chars().count();
    let password_len = request.auth_password.chars().count();
    editor.token_cursor = editor.token_cursor.min(token_len);
    editor.username_cursor = editor.username_cursor.min(username_len);
    editor.password_cursor = editor.password_cursor.min(password_len);

    editor.active_field = match request.auth_mode {
        AuthMode::None => AuthField::Mode,
        AuthMode::Bearer => match editor.active_field {
            AuthField::Username | AuthField::Password => AuthField::Token,
            _ => editor.active_field,
        },
        AuthMode::Basic => match editor.active_field {
            AuthField::Token => AuthField::Username,
            _ => editor.active_field,
        },
    };
}

fn normalize_body_editor(request: &mut crate::state::RequestState) {
    let max_json_cursor = request.body_json.chars().count();
    request.body_editor.json_cursor = request.body_editor.json_cursor.min(max_json_cursor);
    normalize_editor_state(&mut request.body_editor.form_editor, &request.body_form);
    let json_line_count = request.body_json.split('\n').count().max(1);
    request.body_editor.json_scroll = request.body_editor.json_scroll.min(json_line_count - 1);

    let max_text_cursor = request.body_text.chars().count();
    request.body_editor.text_cursor = request.body_editor.text_cursor.min(max_text_cursor);
    let text_line_count = request.body_text.split('\n').count().max(1);
    request.body_editor.text_scroll = request.body_editor.text_scroll.min(text_line_count - 1);

    request.body_editor.active_field = match request.body_format {
        BodyFormat::Json => {
            if matches!(
                request.body_editor.active_field,
                BodyField::Form | BodyField::Text
            ) {
                BodyField::Json
            } else {
                request.body_editor.active_field
            }
        }
        BodyFormat::Form => {
            if matches!(
                request.body_editor.active_field,
                BodyField::Json | BodyField::Text
            ) {
                BodyField::Form
            } else {
                request.body_editor.active_field
            }
        }
        BodyFormat::Text => {
            if matches!(
                request.body_editor.active_field,
                BodyField::Json | BodyField::Form
            ) {
                BodyField::Text
            } else {
                request.body_editor.active_field
            }
        }
    };
}

fn computed_authorization_value(request: &crate::state::RequestState) -> Option<String> {
    match request.auth_mode {
        AuthMode::None => None,
        AuthMode::Bearer => {
            if request.auth_token.trim().is_empty() {
                None
            } else {
                Some(format!("Bearer {}", request.auth_token))
            }
        }
        AuthMode::Basic => {
            if request.auth_username.trim().is_empty() {
                return None;
            }
            let raw = format!("{}:{}", request.auth_username, request.auth_password);
            let encoded = base64::engine::general_purpose::STANDARD.encode(raw);
            Some(format!("Basic {encoded}"))
        }
    }
}

fn reconcile_authorization_header(request: &mut crate::state::RequestState) {
    let desired = computed_authorization_value(request);
    if let Some(value) = desired {
        let index = upsert_named_header(
            &mut request.headers,
            "Authorization",
            value,
            request.managed_auth_header_index,
            MAX_HEADER_ROWS,
        );
        request.managed_auth_header_index = dedupe_named_header(request, "Authorization", index);
    } else if let Some(index) = request.managed_auth_header_index.take()
        && index < request.headers.len()
        && header_name_matches(&request.headers[index].key, "Authorization")
    {
        request.headers.remove(index);
        adjust_managed_header_indices_on_remove(request, index);
    }
}

fn apply_body_content_type_header(request: &mut crate::state::RequestState, force: bool) {
    if !force && request.content_type_manual_override {
        return;
    }

    let value = request.body_format.content_type().to_string();
    let index = upsert_named_header(
        &mut request.headers,
        "Content-Type",
        value,
        request.managed_content_type_header_index,
        MAX_HEADER_ROWS,
    );
    request.managed_content_type_header_index = dedupe_named_header(request, "Content-Type", index);
}

fn upsert_named_header(
    headers: &mut Vec<KeyValueRow>,
    name: &str,
    value: String,
    preferred_index: Option<usize>,
    max_headers: usize,
) -> Option<usize> {
    let target = canonical_header_name(name);

    if let Some(index) = preferred_index
        && index < headers.len()
        && canonical_header_name(&headers[index].key) == target
    {
        headers[index] = KeyValueRow {
            enabled: true,
            key: name.to_string(),
            value,
        };
        return Some(index);
    }

    if let Some(index) = headers
        .iter()
        .position(|row| canonical_header_name(&row.key) == target)
    {
        headers[index] = KeyValueRow {
            enabled: true,
            key: name.to_string(),
            value,
        };
        return Some(index);
    }

    if headers.len() >= max_headers {
        return None;
    }

    headers.push(KeyValueRow {
        enabled: true,
        key: name.to_string(),
        value,
    });
    Some(headers.len() - 1)
}

fn track_content_type_manual_override_on_set(
    request: &mut crate::state::RequestState,
    index: usize,
    row: &KeyValueRow,
) {
    let touches_managed_index = request.managed_content_type_header_index == Some(index);
    let touches_content_type_name = header_name_matches(&row.key, "Content-Type")
        || request
            .headers
            .get(index)
            .map(|existing| header_name_matches(&existing.key, "Content-Type"))
            .unwrap_or(false);

    if touches_managed_index || touches_content_type_name {
        request.content_type_manual_override = true;
    }
}

fn track_content_type_manual_override_on_remove(
    request: &mut crate::state::RequestState,
    index: usize,
) {
    let touches_managed_index = request.managed_content_type_header_index == Some(index);
    let touches_content_type_name = request
        .headers
        .get(index)
        .map(|row| header_name_matches(&row.key, "Content-Type"))
        .unwrap_or(false);

    if touches_managed_index || touches_content_type_name {
        request.content_type_manual_override = true;
        request.managed_content_type_header_index = None;
    }
}

fn adjust_managed_header_indices_on_remove(
    request: &mut crate::state::RequestState,
    removed: usize,
) {
    request.managed_auth_header_index =
        shift_index_after_remove(request.managed_auth_header_index, removed);
    request.managed_content_type_header_index =
        shift_index_after_remove(request.managed_content_type_header_index, removed);
}

fn dedupe_named_header(
    request: &mut crate::state::RequestState,
    name: &str,
    preferred_index: Option<usize>,
) -> Option<usize> {
    let target = canonical_header_name(name);
    let mut matching_indices: Vec<usize> = request
        .headers
        .iter()
        .enumerate()
        .filter_map(|(index, row)| (canonical_header_name(&row.key) == target).then_some(index))
        .collect();

    if matching_indices.is_empty() {
        return None;
    }

    let mut keep_index = preferred_index
        .filter(|index| matching_indices.binary_search(index).is_ok())
        .unwrap_or(matching_indices[0]);

    while let Some(index) = matching_indices.pop() {
        if index == keep_index {
            continue;
        }

        request.headers.remove(index);
        adjust_managed_header_indices_on_remove(request, index);
        if index < keep_index {
            keep_index -= 1;
        }
    }

    Some(keep_index)
}

fn clear_auth_credentials_for_mode(request: &mut crate::state::RequestState) {
    match request.auth_mode {
        AuthMode::None => {
            request.auth_token.clear();
            request.auth_username.clear();
            request.auth_password.clear();
        }
        AuthMode::Bearer => {
            request.auth_username.clear();
            request.auth_password.clear();
        }
        AuthMode::Basic => {
            request.auth_token.clear();
        }
    }
}

fn canonical_header_name(name: &str) -> String {
    name.trim().to_ascii_lowercase()
}

fn header_name_matches(actual: &str, expected: &str) -> bool {
    canonical_header_name(actual) == canonical_header_name(expected)
}

fn shift_index_after_remove(index: Option<usize>, removed: usize) -> Option<usize> {
    match index {
        Some(current) if current == removed => None,
        Some(current) if current > removed => Some(current - 1),
        other => other,
    }
}

fn line_start_index(value: &str, cursor: usize) -> usize {
    let chars: Vec<char> = value.chars().collect();
    let mut index = cursor.min(chars.len());
    while index > 0 && chars[index - 1] != '\n' {
        index -= 1;
    }
    index
}

fn line_end_index(value: &str, cursor: usize) -> usize {
    let chars: Vec<char> = value.chars().collect();
    let mut index = cursor.min(chars.len());
    while index < chars.len() && chars[index] != '\n' {
        index += 1;
    }
    index
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
        App, MAX_RESPONSE_SEARCH_MATCHES, MAX_RESPONSE_SEARCH_QUERY_CHARS,
        RESPONSE_SEARCH_RECOMPUTE_CHUNK_BYTES, canonical_header_name, derive_query_token,
        find_line_matches, limit_key_value_row, truncate_to_char_limit,
        upsert_query_row_with_token, upsert_row_with_limit,
    };
    use crate::action::{Action, BodyContent};
    use crate::components::{help_modal, layout_manager::LayoutManager};
    use crate::persistence::{Collection, SavedRequest};
    use crate::state::{
        AuthMode, BodyFormat, KeyValueRow, MAX_KEY_LENGTH, MAX_VALUE_LENGTH, QueryParamToken,
        RequestFocus, ResponseMetadata, ResponseSearchScope, ResponseTab, SidebarItem,
        SidebarPromptMode,
    };
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
    use ratatui::layout::Rect;

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

    #[test]
    fn find_line_matches_respects_global_cap() {
        let lines = vec!["a".repeat(MAX_RESPONSE_SEARCH_MATCHES + 32)];

        let matches = find_line_matches(&lines, "a", MAX_RESPONSE_SEARCH_MATCHES);
        assert_eq!(matches.len(), MAX_RESPONSE_SEARCH_MATCHES);
        assert_eq!(matches.first().map(|m| m.start_char), Some(0));
        assert_eq!(
            matches.last().map(|m| m.start_char),
            Some(MAX_RESPONSE_SEARCH_MATCHES - 1)
        );
    }

    #[tokio::test]
    async fn ctrl_slash_does_not_open_response_search() {
        let mut app = App::new((120, 40));

        let actions =
            app.map_key_event_to_actions(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::CONTROL));

        assert_ne!(actions, vec![Action::OpenResponseSearch, Action::Render]);
    }

    #[tokio::test]
    async fn ctrl_unit_separator_does_not_open_response_search() {
        let mut app = App::new((120, 40));

        let actions = app.map_key_event_to_actions(KeyEvent::new(
            KeyCode::Char('\u{1f}'),
            KeyModifiers::CONTROL,
        ));

        assert_ne!(actions, vec![Action::OpenResponseSearch, Action::Render]);
    }

    #[tokio::test]
    async fn ctrl_f_opens_response_search_fallback() {
        let mut app = App::new((120, 40));

        let actions =
            app.map_key_event_to_actions(KeyEvent::new(KeyCode::Char('f'), KeyModifiers::CONTROL));

        assert_eq!(actions, vec![Action::OpenResponseSearch, Action::Render]);
    }

    #[tokio::test]
    async fn ctrl_shift_slash_does_not_toggle_help_modal() {
        let mut app = App::new((120, 40));

        let open_actions_shifted_slash = app.map_key_event_to_actions(KeyEvent::new(
            KeyCode::Char('/'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ));
        assert_ne!(
            open_actions_shifted_slash,
            vec![Action::ToggleHelp, Action::Render]
        );
    }

    #[tokio::test]
    async fn ctrl_question_mark_does_not_toggle_help_modal() {
        let mut app = App::new((120, 40));

        let actions = app.map_key_event_to_actions(KeyEvent::new(
            KeyCode::Char('?'),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        ));

        assert_ne!(actions, vec![Action::ToggleHelp, Action::Render]);
    }

    #[tokio::test]
    async fn f1_toggles_help_modal_fallback() {
        let mut app = App::new((120, 40));

        let actions =
            app.map_key_event_to_actions(KeyEvent::new(KeyCode::F(1), KeyModifiers::NONE));

        assert_eq!(actions, vec![Action::ToggleHelp, Action::Render]);
    }

    #[tokio::test]
    async fn esc_closes_help_modal_and_blocks_other_shortcuts() {
        let mut app = App::new((120, 40));
        app.state.help_visible = true;

        let ignored =
            app.map_key_event_to_actions(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
        assert!(ignored.is_empty());

        let close_actions =
            app.map_key_event_to_actions(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(close_actions, vec![Action::CloseHelp, Action::Render]);
    }

    #[tokio::test]
    async fn scroll_help_is_bounded_by_rendered_modal_capacity_when_height_is_clamped() {
        let terminal_size = (140, 120);
        let mut app = App::new(terminal_size);

        app.apply_action(Action::ScrollHelp(i16::MAX));

        let main_area =
            LayoutManager::compute(Rect::new(0, 0, terminal_size.0, terminal_size.1)).main;
        let expected_max_scroll =
            help_modal::max_scroll_for_area(main_area, help_modal::line_count());

        assert_eq!(help_modal::visible_line_capacity(main_area), 36);
        assert!(expected_max_scroll > 0);
        assert_eq!(app.state.help_scroll, expected_max_scroll);
    }

    #[tokio::test]
    async fn slash_in_url_input_inserts_slash() {
        let mut app = App::new((120, 40));
        app.state.request.focus = RequestFocus::Url;
        app.state.request.url.clear();
        app.state.request.url_cursor = 0;

        let actions =
            app.map_key_event_to_actions(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

        assert_eq!(
            actions,
            vec![
                Action::SetUrl(String::from("/")),
                Action::SyncParamsFromUrl,
                Action::Render,
            ]
        );
    }

    #[tokio::test]
    async fn plain_slash_does_not_open_response_search() {
        let mut app = App::new((120, 40));

        let actions =
            app.map_key_event_to_actions(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));

        assert_ne!(actions, vec![Action::OpenResponseSearch, Action::Render]);
    }

    #[tokio::test]
    async fn alt_four_no_longer_selects_response_tab() {
        let mut app = App::new((120, 40));

        let actions =
            app.map_key_event_to_actions(KeyEvent::new(KeyCode::Char('4'), KeyModifiers::ALT));

        assert!(actions.is_empty());
    }

    #[tokio::test]
    async fn active_search_keeps_esc_and_n_navigation() {
        let mut app = App::new((120, 40));
        app.state.response.search.active = true;

        let next_actions =
            app.map_key_event_to_actions(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::NONE));
        assert_eq!(next_actions, vec![Action::NextSearchMatch, Action::Render]);

        let prev_actions =
            app.map_key_event_to_actions(KeyEvent::new(KeyCode::Char('N'), KeyModifiers::SHIFT));
        assert_eq!(prev_actions, vec![Action::PrevSearchMatch, Action::Render]);

        let close_actions =
            app.map_key_event_to_actions(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(
            close_actions,
            vec![Action::CloseResponseSearch, Action::Render]
        );
    }

    #[tokio::test]
    async fn sidebar_create_shortcut_opens_prompt() {
        let mut app = App::new((120, 40));
        app.state.sidebar_focused = true;

        let actions =
            app.map_key_event_to_actions(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::NONE));

        assert_eq!(actions, vec![Action::Render]);
        assert!(matches!(
            app.state.sidebar_prompt.as_ref().map(|prompt| &prompt.mode),
            Some(SidebarPromptMode::CreateCollection)
        ));
    }

    #[tokio::test]
    async fn sidebar_left_right_shortcuts_scroll_collections_horizontally() {
        let mut app = App::new((120, 40));
        app.state.sidebar_focused = true;

        let right = app.map_key_event_to_actions(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        assert_eq!(
            right,
            vec![
                Action::SidebarScrollCollectionsHorizontal(2),
                Action::Render
            ]
        );

        let left = app.map_key_event_to_actions(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        assert_eq!(
            left,
            vec![
                Action::SidebarScrollCollectionsHorizontal(-2),
                Action::Render
            ]
        );
    }

    #[tokio::test]
    async fn sidebar_prompt_mode_keeps_left_right_for_prompt_flow() {
        let mut app = App::new((120, 40));
        app.state.sidebar_focused = true;
        app.state.sidebar_prompt = Some(crate::state::SidebarPromptState {
            mode: SidebarPromptMode::CreateCollection,
            value: String::from("name"),
        });

        let left = app.map_key_event_to_actions(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
        let right = app.map_key_event_to_actions(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));

        assert!(left.is_empty());
        assert!(right.is_empty());
        assert_eq!(app.state.sidebar_collections_horizontal_offset, 0);
    }

    #[tokio::test]
    async fn sidebar_horizontal_scroll_action_saturates_at_zero() {
        let mut app = App::new((120, 40));

        app.apply_action(Action::SidebarScrollCollectionsHorizontal(5));
        assert_eq!(app.state.sidebar_collections_horizontal_offset, 5);

        app.apply_action(Action::SidebarScrollCollectionsHorizontal(-20));
        assert_eq!(app.state.sidebar_collections_horizontal_offset, 0);

        app.state.sidebar_selected_item = SidebarItem::HistoryEntry(0);
        app.apply_action(Action::SidebarScrollCollectionsHorizontal(3));
        assert_eq!(app.state.sidebar_history_horizontal_offset, 3);

        app.apply_action(Action::SidebarScrollCollectionsHorizontal(-10));
        assert_eq!(app.state.sidebar_history_horizontal_offset, 0);
    }

    #[tokio::test]
    async fn sidebar_horizontal_scroll_routes_to_selected_section() {
        let mut app = App::new((120, 40));

        app.state.sidebar_selected_item = SidebarItem::HistoryEntry(2);
        app.apply_action(Action::SidebarScrollCollectionsHorizontal(4));
        assert_eq!(app.state.sidebar_history_horizontal_offset, 4);
        assert_eq!(app.state.sidebar_collections_horizontal_offset, 0);

        app.state.sidebar_selected_item = SidebarItem::Request {
            collection: 0,
            request: 0,
        };
        app.apply_action(Action::SidebarScrollCollectionsHorizontal(6));
        assert_eq!(app.state.sidebar_collections_horizontal_offset, 6);
        assert_eq!(app.state.sidebar_history_horizontal_offset, 4);
    }

    #[tokio::test]
    async fn sidebar_horizontal_scroll_defaults_to_collections_when_nothing_selected() {
        let mut app = App::new((120, 40));
        app.state.sidebar_selected_item = SidebarItem::None;

        app.apply_action(Action::SidebarScrollCollectionsHorizontal(2));

        assert_eq!(app.state.sidebar_collections_horizontal_offset, 2);
        assert_eq!(app.state.sidebar_history_horizontal_offset, 0);
    }

    #[tokio::test]
    async fn sidebar_save_shortcut_uses_selected_collection() {
        let mut app = App::new((120, 40));
        app.state.sidebar_focused = true;
        app.state.request.method = crate::state::HttpMethod::Post;
        app.state.request.url = String::from("https://api.example.com/items");
        app.state.collections.push(Collection {
            id: String::from("c1"),
            name: String::from("API"),
            expanded: true,
            requests: Vec::new(),
        });
        app.state.sidebar_selected_item = SidebarItem::Collection(0);

        let start_actions =
            app.map_key_event_to_actions(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
        assert_eq!(start_actions, vec![Action::Render]);

        let confirm_actions =
            app.map_key_event_to_actions(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(confirm_actions.contains(&Action::SaveRequestToCollection {
            collection_index: 0,
            name: String::from("POST https://api.example.com/items"),
        }));
    }

    #[tokio::test]
    async fn sidebar_delete_shortcut_removes_selected_saved_request() {
        let mut app = App::new((120, 40));
        app.state.sidebar_focused = true;
        app.state.collections.push(Collection {
            id: String::from("c1"),
            name: String::from("API"),
            expanded: true,
            requests: vec![SavedRequest {
                id: String::from("r1"),
                name: String::from("Get users"),
                ..SavedRequest::default()
            }],
        });
        app.state.sidebar_selected_item = SidebarItem::Request {
            collection: 0,
            request: 0,
        };

        let actions =
            app.map_key_event_to_actions(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));

        assert!(actions.contains(&Action::DeleteCollectionRequest {
            collection: 0,
            request: 0,
        }));
    }

    #[tokio::test]
    async fn response_search_works_in_headers_tab() {
        let mut app = App::new((120, 40));
        app.state.response.metadata = Some(ResponseMetadata {
            headers: vec![
                (String::from("server"), String::from("nginx")),
                (String::from("x-id"), String::from("abc-123")),
            ],
            ..ResponseMetadata::default()
        });
        app.apply_action(Action::SetResponseTab(ResponseTab::Headers));

        app.apply_action(Action::OpenResponseSearch);
        app.apply_action(Action::SearchInResponse(String::from("abc")));

        assert_eq!(
            app.state.response.search.scope,
            ResponseSearchScope::Headers
        );
        assert_eq!(app.state.response.search.matches.len(), 1);
        assert_eq!(app.state.response.search.current_match, Some(0));
    }

    #[tokio::test]
    async fn switching_response_tabs_recomputes_search_scope_and_matches() {
        let mut app = App::new((120, 40));
        app.state.response.buffer.append_chunk(b"body-match");
        app.state.response.metadata = Some(ResponseMetadata {
            headers: vec![(String::from("x-trace"), String::from("header-match"))],
            ..ResponseMetadata::default()
        });

        app.apply_action(Action::OpenResponseSearch);
        app.apply_action(Action::SearchInResponse(String::from("match")));

        assert_eq!(app.state.response.search.scope, ResponseSearchScope::Body);
        assert_eq!(app.state.response.search.matches.len(), 1);

        app.apply_action(Action::SetResponseTab(ResponseTab::Headers));
        assert_eq!(
            app.state.response.search.scope,
            ResponseSearchScope::Headers
        );
        assert_eq!(app.state.response.search.matches.len(), 1);

        app.apply_action(Action::SetResponseTab(ResponseTab::Raw));
        assert_eq!(app.state.response.search.scope, ResponseSearchScope::Raw);
        assert_eq!(app.state.response.search.matches.len(), 2);
    }

    #[tokio::test]
    async fn search_query_is_clamped_to_max_length() {
        let mut app = App::new((120, 40));
        app.apply_action(Action::OpenResponseSearch);
        app.apply_action(Action::SearchInResponse(
            "x".repeat(MAX_RESPONSE_SEARCH_QUERY_CHARS + 32),
        ));

        assert_eq!(
            app.state.response.search.query.chars().count(),
            MAX_RESPONSE_SEARCH_QUERY_CHARS
        );
    }

    #[tokio::test]
    async fn closing_response_search_clears_query_and_matches() {
        let mut app = App::new((120, 40));
        app.state.response.buffer.append_chunk(b"hello hello");

        app.apply_action(Action::OpenResponseSearch);
        app.apply_action(Action::SearchInResponse(String::from("hello")));
        assert!(!app.state.response.search.matches.is_empty());

        app.apply_action(Action::CloseResponseSearch);
        assert!(!app.state.response.search.active);
        assert!(app.state.response.search.query.is_empty());
        assert!(app.state.response.search.matches.is_empty());
        assert_eq!(app.state.response.search.current_match, None);
    }

    #[tokio::test]
    async fn response_chunks_do_not_recompute_search_when_inactive() {
        let mut app = App::new((120, 40));
        app.active_response_id = Some(7);
        app.state.response.last_request_id = Some(7);
        app.state.response.search.query = String::from("a");

        app.apply_action(Action::ResponseChunk {
            request_id: 7,
            chunk: b"a a a".to_vec(),
        });

        assert!(app.state.response.search.matches.is_empty());
        assert_eq!(app.state.response.search.current_match, None);
    }

    #[tokio::test]
    async fn response_chunks_throttle_search_recompute_until_threshold() {
        let mut app = App::new((120, 40));
        app.active_response_id = Some(7);
        app.state.response.last_request_id = Some(7);
        app.apply_action(Action::OpenResponseSearch);
        app.apply_action(Action::SearchInResponse(String::from("a")));

        let almost_threshold = vec![b'a'; RESPONSE_SEARCH_RECOMPUTE_CHUNK_BYTES - 1];
        app.apply_action(Action::ResponseChunk {
            request_id: 7,
            chunk: almost_threshold,
        });

        assert!(app.state.response.search.matches.is_empty());
        assert_eq!(
            app.pending_response_search_bytes,
            RESPONSE_SEARCH_RECOMPUTE_CHUNK_BYTES - 1
        );

        app.apply_action(Action::ResponseChunk {
            request_id: 7,
            chunk: vec![b'a'],
        });

        assert!(!app.state.response.search.matches.is_empty());
        assert_eq!(app.pending_response_search_bytes, 0);
    }

    #[tokio::test]
    async fn request_completion_forces_final_search_recompute() {
        let mut app = App::new((120, 40));
        app.active_response_id = Some(7);
        app.state.response.last_request_id = Some(7);
        app.apply_action(Action::OpenResponseSearch);
        app.apply_action(Action::SearchInResponse(String::from("z")));

        app.apply_action(Action::ResponseChunk {
            request_id: 7,
            chunk: vec![b'z'; RESPONSE_SEARCH_RECOMPUTE_CHUNK_BYTES - 1],
        });

        assert!(app.state.response.search.matches.is_empty());

        app.apply_action(Action::RequestCompleted {
            request_id: 7,
            metadata: ResponseMetadata::default(),
        });

        assert!(!app.state.response.search.matches.is_empty());
        assert_eq!(app.pending_response_search_bytes, 0);
    }

    #[tokio::test]
    async fn search_navigation_handles_out_of_range_current_match() {
        let mut app = App::new((120, 40));
        app.state
            .response
            .buffer
            .append_chunk("a".repeat(MAX_RESPONSE_SEARCH_MATCHES + 4).as_bytes());

        app.apply_action(Action::OpenResponseSearch);
        app.apply_action(Action::SearchInResponse(String::from("a")));
        assert_eq!(
            app.state.response.search.matches.len(),
            MAX_RESPONSE_SEARCH_MATCHES
        );

        app.state.response.search.current_match = Some(MAX_RESPONSE_SEARCH_MATCHES + 10);
        app.apply_action(Action::NextSearchMatch);
        assert_eq!(app.state.response.search.current_match, Some(0));

        app.state.response.search.current_match = Some(MAX_RESPONSE_SEARCH_MATCHES + 10);
        app.apply_action(Action::PrevSearchMatch);
        assert_eq!(
            app.state.response.search.current_match,
            Some(MAX_RESPONSE_SEARCH_MATCHES - 1)
        );
    }

    #[tokio::test]
    async fn basic_auth_base64_is_correct() {
        let mut app = App::new((120, 40));
        app.apply_action(Action::SetAuthMode(AuthMode::Basic));
        app.apply_action(Action::SetAuthCredentials {
            username: String::from("alice"),
            password: String::from("secret"),
        });

        let value = header_value(&app, "Authorization");
        assert_eq!(value, Some("Basic YWxpY2U6c2VjcmV0"));
    }

    #[tokio::test]
    async fn authorization_header_transitions_between_modes() {
        let mut app = App::new((120, 40));

        app.apply_action(Action::SetAuthMode(AuthMode::Bearer));
        app.apply_action(Action::SetAuthToken(String::from("tkn")));
        assert_eq!(header_value(&app, "Authorization"), Some("Bearer tkn"));

        app.apply_action(Action::SetAuthMode(AuthMode::Basic));
        app.apply_action(Action::SetAuthCredentials {
            username: String::from("u"),
            password: String::from("p"),
        });
        assert_eq!(header_value(&app, "Authorization"), Some("Basic dTpw"));

        app.apply_action(Action::SetAuthMode(AuthMode::None));
        assert_eq!(header_value(&app, "Authorization"), None);

        app.apply_action(Action::AddHeader);
        app.apply_action(Action::SetHeader {
            index: 0,
            row: KeyValueRow {
                enabled: true,
                key: String::from("Authorization"),
                value: String::from("Manual abc"),
            },
        });

        app.apply_action(Action::SetAuthMode(AuthMode::None));
        assert_eq!(header_value(&app, "Authorization"), Some("Manual abc"));
    }

    #[tokio::test]
    async fn switching_auth_mode_clears_irrelevant_credentials() {
        let mut app = App::new((120, 40));

        app.apply_action(Action::SetAuthMode(AuthMode::Basic));
        app.apply_action(Action::SetAuthCredentials {
            username: String::from("alice"),
            password: String::from("secret"),
        });

        app.apply_action(Action::SetAuthMode(AuthMode::Bearer));
        assert_eq!(app.state.request.auth_username, "");
        assert_eq!(app.state.request.auth_password, "");

        app.apply_action(Action::SetAuthToken(String::from("tkn")));
        app.apply_action(Action::SetAuthMode(AuthMode::Basic));
        assert_eq!(app.state.request.auth_token, "");

        app.apply_action(Action::SetAuthCredentials {
            username: String::from("bob"),
            password: String::from("pw"),
        });
        app.apply_action(Action::SetAuthMode(AuthMode::None));
        assert_eq!(app.state.request.auth_token, "");
        assert_eq!(app.state.request.auth_username, "");
        assert_eq!(app.state.request.auth_password, "");
    }

    #[tokio::test]
    async fn empty_auth_inputs_do_not_emit_managed_authorization_header() {
        let mut app = App::new((120, 40));

        app.apply_action(Action::SetAuthMode(AuthMode::Bearer));
        app.apply_action(Action::SetAuthToken(String::from("   ")));
        assert_eq!(header_value(&app, "Authorization"), None);

        app.apply_action(Action::SetAuthMode(AuthMode::Basic));
        app.apply_action(Action::SetAuthCredentials {
            username: String::from("   "),
            password: String::from("password"),
        });
        assert_eq!(header_value(&app, "Authorization"), None);

        app.apply_action(Action::SetAuthCredentials {
            username: String::from("alice"),
            password: String::new(),
        });
        assert_eq!(header_value(&app, "Authorization"), Some("Basic YWxpY2U6"));
    }

    #[tokio::test]
    async fn content_type_respects_manual_override_until_format_changes() {
        let mut app = App::new((120, 40));

        app.apply_action(Action::SetBodyFormat(BodyFormat::Json));
        assert_eq!(header_value(&app, "Content-Type"), Some("application/json"));

        let content_type_index = app
            .state
            .request
            .headers
            .iter()
            .position(|row| row.key.eq_ignore_ascii_case("Content-Type"))
            .expect("content-type header should exist");

        app.apply_action(Action::SetHeader {
            index: content_type_index,
            row: KeyValueRow {
                enabled: true,
                key: String::from("Content-Type"),
                value: String::from("text/plain"),
            },
        });

        app.apply_action(Action::SetBodyContent(BodyContent::Json(String::from(
            "{}",
        ))));
        assert_eq!(header_value(&app, "Content-Type"), Some("text/plain"));

        app.apply_action(Action::SetBodyFormat(BodyFormat::Form));
        assert_eq!(
            header_value(&app, "Content-Type"),
            Some("application/x-www-form-urlencoded")
        );
    }

    #[tokio::test]
    async fn canonical_header_name_prevents_managed_header_duplicates() {
        let mut app = App::new((120, 40));

        app.apply_action(Action::AddHeader);
        app.apply_action(Action::SetHeader {
            index: 0,
            row: KeyValueRow {
                enabled: true,
                key: String::from("  authorization  "),
                value: String::from("Manual"),
            },
        });

        app.apply_action(Action::SetAuthMode(AuthMode::Bearer));
        app.apply_action(Action::SetAuthToken(String::from("token")));

        let auth_rows = app
            .state
            .request
            .headers
            .iter()
            .filter(|row| canonical_header_name(&row.key) == "authorization")
            .count();
        assert_eq!(auth_rows, 1);
        assert_eq!(header_value(&app, "Authorization"), Some("Bearer token"));

        app.apply_action(Action::SetBodyFormat(BodyFormat::Json));
        app.apply_action(Action::AddHeader);
        let next_index = app.state.request.headers.len() - 1;
        app.apply_action(Action::SetHeader {
            index: next_index,
            row: KeyValueRow {
                enabled: true,
                key: String::from("\tcontent-type\t"),
                value: String::from("text/plain"),
            },
        });

        app.apply_action(Action::SetBodyContent(BodyContent::Json(String::from(
            "{}",
        ))));
        assert_eq!(header_value(&app, "Content-Type"), Some("text/plain"));
    }

    fn header_value<'a>(app: &'a App, name: &str) -> Option<&'a str> {
        app.state
            .request
            .headers
            .iter()
            .find(|row| canonical_header_name(&row.key) == canonical_header_name(name))
            .map(|row| row.value.as_str())
    }

    // ── Phase 6: snapshot / round-trip tests ─────────────────────────────────

    #[test]
    fn snapshot_request_round_trips_method_url_and_body() {
        use super::{load_saved_request_into_state, snapshot_request};
        use crate::state::{HttpMethod, RequestState};

        let req = RequestState {
            method: HttpMethod::Post,
            url: String::from("https://api.example.com/items"),
            body_format: BodyFormat::Json,
            body_json: String::from(r#"{"key":"value"}"#),
            ..Default::default()
        };

        let saved = snapshot_request(&req, true);
        assert_eq!(saved.method, "POST");
        assert_eq!(saved.url, "https://api.example.com/items");
        assert_eq!(saved.body_json, r#"{"key":"value"}"#);

        let mut restored = RequestState::default();
        load_saved_request_into_state(&mut restored, &saved);
        assert_eq!(restored.method, HttpMethod::Post);
        assert_eq!(restored.url, "https://api.example.com/items");
        assert_eq!(restored.body_json, r#"{"key":"value"}"#);
    }

    #[test]
    fn snapshot_request_redacts_sensitive_headers_when_persist_false() {
        use super::snapshot_request;
        use crate::state::{HttpMethod, KeyValueRow, RequestState};

        let req = RequestState {
            method: HttpMethod::Get,
            url: String::from("https://example.com"),
            headers: vec![
                KeyValueRow {
                    key: String::from("Authorization"),
                    value: String::from("Bearer top-secret"),
                    enabled: true,
                },
                KeyValueRow {
                    key: String::from("Accept"),
                    value: String::from("application/json"),
                    enabled: true,
                },
            ],
            auth_token: String::from("top-secret"),
            auth_username: String::from("admin"),
            auth_password: String::from("password"),
            ..Default::default()
        };

        let saved = snapshot_request(&req, false);

        let auth_header = saved
            .headers
            .iter()
            .find(|h| h.key == "Authorization")
            .unwrap();
        let accept_header = saved.headers.iter().find(|h| h.key == "Accept").unwrap();

        assert_eq!(auth_header.value, "[REDACTED]");
        assert_eq!(accept_header.value, "application/json");
        assert!(saved.auth_token.is_empty());
        assert!(saved.auth_username.is_empty());
        assert!(saved.auth_password.is_empty());
    }

    #[test]
    fn snapshot_request_preserves_sensitive_fields_when_persist_true() {
        use super::snapshot_request;
        use crate::state::{AuthMode, HttpMethod, KeyValueRow, RequestState};

        let req = RequestState {
            method: HttpMethod::Get,
            url: String::from("https://example.com"),
            auth_mode: AuthMode::Bearer,
            auth_token: String::from("my-token"),
            headers: vec![KeyValueRow {
                key: String::from("Cookie"),
                value: String::from("session=abc"),
                enabled: true,
            }],
            ..Default::default()
        };

        let saved = snapshot_request(&req, true);

        let cookie = saved.headers.iter().find(|h| h.key == "Cookie").unwrap();
        assert_eq!(cookie.value, "session=abc");
        assert_eq!(saved.auth_token, "my-token");
    }

    #[test]
    fn load_saved_request_resets_url_cursor_to_end() {
        use super::{load_saved_request_into_state, snapshot_request};
        use crate::state::RequestState;

        let req = RequestState {
            url: String::from("https://example.com/path"),
            ..Default::default()
        };
        let saved = snapshot_request(&req, true);

        let mut target = RequestState::default();
        load_saved_request_into_state(&mut target, &saved);

        assert_eq!(target.url_cursor, target.url.chars().count());
    }

    // ── History deletion tests ────────────────────────────────────────────────

    #[tokio::test]
    async fn delete_history_entry_removes_correct_entry() {
        let mut app = App::new((120, 40));
        app.state.history = vec![
            crate::persistence::HistoryEntry {
                id: String::from("1"),
                timestamp_secs: 1,
                method: String::from("GET"),
                url: String::from("https://a.com"),
                status_code: Some(200),
                elapsed_ms: Some(10),
                request: None,
            },
            crate::persistence::HistoryEntry {
                id: String::from("2"),
                timestamp_secs: 2,
                method: String::from("POST"),
                url: String::from("https://b.com"),
                status_code: Some(201),
                elapsed_ms: Some(20),
                request: None,
            },
        ];
        app.apply_action(Action::DeleteHistoryEntry(0));
        assert_eq!(app.state.history.len(), 1);
        assert_eq!(app.state.history[0].id, "2");
    }

    #[tokio::test]
    async fn delete_history_entries_removes_all_marked() {
        let mut app = App::new((120, 40));
        app.state.history = vec![
            crate::persistence::HistoryEntry {
                id: String::from("1"),
                timestamp_secs: 1,
                method: String::from("GET"),
                url: String::from("https://a.com"),
                status_code: Some(200),
                elapsed_ms: None,
                request: None,
            },
            crate::persistence::HistoryEntry {
                id: String::from("2"),
                timestamp_secs: 2,
                method: String::from("POST"),
                url: String::from("https://b.com"),
                status_code: Some(201),
                elapsed_ms: None,
                request: None,
            },
            crate::persistence::HistoryEntry {
                id: String::from("3"),
                timestamp_secs: 3,
                method: String::from("PUT"),
                url: String::from("https://c.com"),
                status_code: Some(200),
                elapsed_ms: None,
                request: None,
            },
        ];
        app.apply_action(Action::DeleteHistoryEntries(vec![0, 2]));
        assert_eq!(app.state.history.len(), 1);
        assert_eq!(app.state.history[0].id, "2");
        assert!(app.state.history_marked_indices.is_empty());
    }

    #[tokio::test]
    async fn toggle_history_mark_adds_and_removes() {
        let mut app = App::new((120, 40));
        app.state.history = vec![crate::persistence::HistoryEntry {
            id: String::from("1"),
            timestamp_secs: 1,
            method: String::from("GET"),
            url: String::from("https://a.com"),
            status_code: Some(200),
            elapsed_ms: None,
            request: None,
        }];
        app.apply_action(Action::ToggleHistoryMark(0));
        assert!(app.state.history_marked_indices.contains(&0));
        app.apply_action(Action::ToggleHistoryMark(0));
        assert!(!app.state.history_marked_indices.contains(&0));
    }

    #[tokio::test]
    async fn clear_history_also_clears_marked_indices() {
        let mut app = App::new((120, 40));
        app.state.history = vec![crate::persistence::HistoryEntry {
            id: String::from("1"),
            timestamp_secs: 1,
            method: String::from("GET"),
            url: String::from("https://a.com"),
            status_code: Some(200),
            elapsed_ms: None,
            request: None,
        }];
        app.state.history_marked_indices.insert(0);
        app.apply_action(Action::ClearHistory);
        assert!(app.state.history.is_empty());
        assert!(app.state.history_marked_indices.is_empty());
    }

    #[tokio::test]
    async fn sidebar_space_on_history_entry_toggles_mark() {
        let mut app = App::new((120, 40));
        app.state.history = vec![crate::persistence::HistoryEntry {
            id: String::from("1"),
            timestamp_secs: 1,
            method: String::from("GET"),
            url: String::from("https://a.com"),
            status_code: Some(200),
            elapsed_ms: None,
            request: None,
        }];
        app.state.sidebar_focused = true;
        app.state.sidebar_selected_item = SidebarItem::HistoryEntry(0);

        let actions =
            app.map_key_event_to_actions(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(actions.contains(&Action::ToggleHistoryMark(0)));
    }

    #[tokio::test]
    async fn sidebar_d_on_unmarked_history_entry_deletes_single() {
        let mut app = App::new((120, 40));
        app.state.history = vec![crate::persistence::HistoryEntry {
            id: String::from("1"),
            timestamp_secs: 1,
            method: String::from("GET"),
            url: String::from("https://a.com"),
            status_code: Some(200),
            elapsed_ms: None,
            request: None,
        }];
        app.state.sidebar_focused = true;
        app.state.sidebar_selected_item = SidebarItem::HistoryEntry(0);

        let actions =
            app.map_key_event_to_actions(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        assert!(actions.contains(&Action::DeleteHistoryEntry(0)));
    }

    #[tokio::test]
    async fn sidebar_d_with_marked_entries_deletes_all_marked() {
        let mut app = App::new((120, 40));
        app.state.history = vec![
            crate::persistence::HistoryEntry {
                id: String::from("1"),
                timestamp_secs: 1,
                method: String::from("GET"),
                url: String::from("https://a.com"),
                status_code: Some(200),
                elapsed_ms: None,
                request: None,
            },
            crate::persistence::HistoryEntry {
                id: String::from("2"),
                timestamp_secs: 2,
                method: String::from("POST"),
                url: String::from("https://b.com"),
                status_code: Some(201),
                elapsed_ms: None,
                request: None,
            },
        ];
        app.state.sidebar_focused = true;
        app.state.sidebar_selected_item = SidebarItem::HistoryEntry(1);
        app.state.history_marked_indices.insert(0);

        let actions =
            app.map_key_event_to_actions(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        // Should dispatch DeleteHistoryEntries with the marked indices.
        assert!(
            actions
                .iter()
                .any(|a| matches!(a, Action::DeleteHistoryEntries(_)))
        );
    }

    #[tokio::test]
    async fn sidebar_shift_x_clears_all_history() {
        let mut app = App::new((120, 40));
        app.state.history = vec![crate::persistence::HistoryEntry {
            id: String::from("1"),
            timestamp_secs: 1,
            method: String::from("GET"),
            url: String::from("https://a.com"),
            status_code: Some(200),
            elapsed_ms: None,
            request: None,
        }];
        app.state.sidebar_focused = true;
        app.state.sidebar_selected_item = SidebarItem::HistoryEntry(0);

        let actions =
            app.map_key_event_to_actions(KeyEvent::new(KeyCode::Char('X'), KeyModifiers::SHIFT));
        assert!(actions.contains(&Action::ClearHistory));
    }
}
