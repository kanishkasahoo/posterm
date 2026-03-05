use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Text;
use ratatui::widgets::{
    Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};
use ratatui::Frame;

use crate::components::request_builder::key_value_editor::render_editor;
use crate::state::{BodyField, BodyFormat, RequestFocus, RequestState};

pub struct BodyEditor;

impl BodyEditor {
    pub fn render(frame: &mut Frame<'_>, area: Rect, request: &RequestState) {
        let sections = Layout::vertical([Constraint::Length(3), Constraint::Min(4)]).split(area);
        render_format(frame, sections[0], request);

        match request.body_format {
            BodyFormat::Json => render_json(frame, sections[1], request),
            BodyFormat::Form => render_form(frame, sections[1], request),
            BodyFormat::Text => render_text(frame, sections[1], request),
        }
    }
}

fn render_format(frame: &mut Frame<'_>, area: Rect, request: &RequestState) {
    let is_active = request.focus == RequestFocus::Editor
        && request.body_editor.active_field == BodyField::Format;
    let title_style = if is_active {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };

    let widget = Paragraph::new(request.body_format.as_str()).block(
        Block::default()
            .title("Body Format")
            .title_style(title_style)
            .borders(Borders::ALL),
    );
    frame.render_widget(widget, area);
}

fn render_json(frame: &mut Frame<'_>, area: Rect, request: &RequestState) {
    let is_active = request.focus == RequestFocus::Editor
        && request.body_editor.active_field == BodyField::Json;
    let title_style = if is_active {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };

    let inner_height = usize::from(area.height.saturating_sub(2));
    let lines: Vec<&str> = request.body_json.split('\n').collect();
    let total_lines = lines.len().max(1);
    let max_scroll = total_lines.saturating_sub(inner_height.max(1));
    let scroll = request.body_editor.json_scroll.min(max_scroll);

    let visible = if inner_height == 0 {
        String::new()
    } else {
        lines
            .iter()
            .skip(scroll)
            .take(inner_height)
            .copied()
            .collect::<Vec<_>>()
            .join("\n")
    };

    let paragraph = Paragraph::new(Text::from(visible)).block(
        Block::default()
            .title("JSON Body")
            .title_style(title_style)
            .borders(Borders::ALL),
    );
    frame.render_widget(paragraph, area);

    // Vertical scrollbar for the JSON body editor.
    if total_lines > inner_height {
        let mut sb_state =
            ScrollbarState::new(total_lines.saturating_sub(inner_height)).position(scroll);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            area,
            &mut sb_state,
        );
    }

    if is_active {
        let (line, col) = cursor_line_col(&request.body_json, request.body_editor.json_cursor);
        let clamped_line = line.saturating_sub(scroll);
        let cursor_x = area.x.saturating_add(1).saturating_add(col as u16);
        let cursor_y = area.y.saturating_add(1).saturating_add(clamped_line as u16);
        let max_x = area.x.saturating_add(area.width.saturating_sub(1));
        let max_y = area.y.saturating_add(area.height.saturating_sub(1));
        frame.set_cursor_position(Position::new(cursor_x.min(max_x), cursor_y.min(max_y)));
    }
}

fn render_form(frame: &mut Frame<'_>, area: Rect, request: &RequestState) {
    let focus = if request.focus == RequestFocus::Editor
        && request.body_editor.active_field == BodyField::Form
    {
        RequestFocus::Editor
    } else {
        RequestFocus::Tabs
    };

    render_editor(
        frame,
        area,
        "Form Body",
        &request.body_form,
        request.body_editor.form_editor,
        focus,
    );
}

fn render_text(frame: &mut Frame<'_>, area: Rect, request: &RequestState) {
    let is_active = request.focus == RequestFocus::Editor
        && request.body_editor.active_field == BodyField::Text;
    let title_style = if is_active {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };

    let inner_height = usize::from(area.height.saturating_sub(2));
    let lines: Vec<&str> = request.body_text.split('\n').collect();
    let total_lines = lines.len().max(1);
    let max_scroll = total_lines.saturating_sub(inner_height.max(1));
    let scroll = request.body_editor.text_scroll.min(max_scroll);

    let visible = if inner_height == 0 {
        String::new()
    } else {
        lines
            .iter()
            .skip(scroll)
            .take(inner_height)
            .copied()
            .collect::<Vec<_>>()
            .join("\n")
    };

    let paragraph = Paragraph::new(Text::from(visible)).block(
        Block::default()
            .title("Text Body")
            .title_style(title_style)
            .borders(Borders::ALL),
    );
    frame.render_widget(paragraph, area);

    // Vertical scrollbar for the text body editor.
    if total_lines > inner_height {
        let mut sb_state =
            ScrollbarState::new(total_lines.saturating_sub(inner_height)).position(scroll);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            area,
            &mut sb_state,
        );
    }

    if is_active {
        let (line, col) = cursor_line_col(&request.body_text, request.body_editor.text_cursor);
        let clamped_line = line.saturating_sub(scroll);
        let cursor_x = area.x.saturating_add(1).saturating_add(col as u16);
        let cursor_y = area.y.saturating_add(1).saturating_add(clamped_line as u16);
        let max_x = area.x.saturating_add(area.width.saturating_sub(1));
        let max_y = area.y.saturating_add(area.height.saturating_sub(1));
        frame.set_cursor_position(Position::new(cursor_x.min(max_x), cursor_y.min(max_y)));
    }
}

fn cursor_line_col(value: &str, cursor: usize) -> (usize, usize) {
    let mut line = 0usize;
    let mut col = 0usize;
    for (idx, ch) in value.chars().enumerate() {
        if idx >= cursor {
            break;
        }
        if ch == '\n' {
            line += 1;
            col = 0;
        } else {
            col += 1;
        }
    }
    (line, col)
}
