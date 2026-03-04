use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::state::{AuthField, AuthMode, RequestFocus, RequestState};

pub struct AuthPanel;

impl AuthPanel {
    pub fn render(frame: &mut Frame<'_>, area: Rect, request: &RequestState) {
        let rows = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(area);

        render_mode(frame, rows[0], request);

        match request.auth_mode {
            AuthMode::None => {
                let info = Paragraph::new("No Authorization header is auto-managed.")
                    .block(Block::default().title("Auth").borders(Borders::ALL));
                frame.render_widget(info, rows[1]);
            }
            AuthMode::Bearer => {
                render_text_input(
                    frame,
                    rows[1],
                    "Token",
                    &request.auth_token,
                    request,
                    AuthField::Token,
                    request.auth_editor.token_cursor,
                );
            }
            AuthMode::Basic => {
                render_text_input(
                    frame,
                    rows[1],
                    "Username",
                    &request.auth_username,
                    request,
                    AuthField::Username,
                    request.auth_editor.username_cursor,
                );
                render_text_input(
                    frame,
                    rows[2],
                    "Password",
                    &mask_secret(&request.auth_password),
                    request,
                    AuthField::Password,
                    request.auth_editor.password_cursor,
                );
            }
        }
    }
}

fn render_mode(frame: &mut Frame<'_>, area: Rect, request: &RequestState) {
    let is_active = request.focus == RequestFocus::Editor
        && request.auth_editor.active_field == AuthField::Mode;
    let title_style = if is_active {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };
    let mode = Paragraph::new(request.auth_mode.as_str()).block(
        Block::default()
            .title("Auth Mode")
            .title_style(title_style)
            .borders(Borders::ALL),
    );
    frame.render_widget(mode, area);
}

fn render_text_input(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    value: &str,
    request: &RequestState,
    field: AuthField,
    cursor: usize,
) {
    let is_active =
        request.focus == RequestFocus::Editor && request.auth_editor.active_field == field;
    let title_style = if is_active {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };

    let input_width = usize::from(area.width.saturating_sub(2));
    let cursor = cursor.min(value.chars().count());
    let scroll = if cursor >= input_width {
        cursor.saturating_sub(input_width.saturating_sub(1))
    } else {
        0
    };
    let visible = slice_chars(value, scroll, input_width);

    let input = Paragraph::new(visible).block(
        Block::default()
            .title(title)
            .title_style(title_style)
            .borders(Borders::ALL),
    );
    frame.render_widget(input, area);

    if is_active {
        let cursor_x = area
            .x
            .saturating_add(1)
            .saturating_add((cursor.saturating_sub(scroll)) as u16);
        let cursor_y = area.y.saturating_add(1);
        frame.set_cursor_position(Position::new(cursor_x, cursor_y));
    }
}

fn slice_chars(value: &str, start: usize, width: usize) -> String {
    value.chars().skip(start).take(width).collect()
}

fn mask_secret(value: &str) -> String {
    "*".repeat(value.chars().count())
}

#[cfg(test)]
mod tests {
    use super::mask_secret;

    #[test]
    fn mask_secret_preserves_length_and_hides_content() {
        assert_eq!(mask_secret("s3cr3t"), "******");
        assert_eq!(mask_secret(""), "");
    }
}
