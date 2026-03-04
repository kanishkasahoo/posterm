use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::state::{AppState, ResponseSearchScope};

use super::response_body::{apply_search_overlays, search_matches_for_line};

pub fn render(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let lines = raw_lines(state);
    let inner_height = usize::from(area.height.saturating_sub(2).max(1));
    let inner_width = usize::from(area.width.saturating_sub(2).max(1));
    let max_vertical_scroll = lines.len().saturating_sub(inner_height);
    let vertical_scroll = state.response.scroll_offset.min(max_vertical_scroll);
    let horizontal_scroll = state.response.horizontal_scroll_offset;

    let mut rendered = Vec::with_capacity(inner_height);
    for row in 0..inner_height {
        let line_index = vertical_scroll + row;
        if let Some(line_text) = lines.get(line_index) {
            let mut line = Line::from(line_text.clone());
            let line_matches = search_matches_for_line(line_index, state, ResponseSearchScope::Raw);
            if !line_matches.is_empty() {
                line = apply_search_overlays(line, &line_matches);
            }

            if !state.response.wrap_lines {
                line = slice_line(&line, horizontal_scroll, inner_width);
            }

            rendered.push(line);
        }
    }

    if rendered.is_empty() {
        rendered.push(Line::from("No raw response available."));
    }

    let mut widget = Paragraph::new(Text::from(rendered)).block(
        Block::default()
            .title(if state.response.wrap_lines {
                "Raw (Wrap: On)"
            } else {
                "Raw (Wrap: Off)"
            })
            .borders(Borders::ALL),
    );

    if state.response.wrap_lines {
        widget = widget.wrap(Wrap { trim: false });
    }

    frame.render_widget(widget, area);
}

pub(crate) fn raw_lines(state: &AppState) -> Vec<String> {
    let mut lines = Vec::new();

    if let Some(metadata) = &state.response.metadata {
        let status = metadata
            .status_code
            .map(|code| code.to_string())
            .unwrap_or_else(|| String::from("?"));
        let reason = metadata.reason_phrase.as_deref().unwrap_or_default();
        lines.push(
            format!("{} {} {}", metadata.http_version, status, reason)
                .trim()
                .to_string(),
        );
        for (name, value) in &metadata.headers {
            lines.push(format!("{name}: {value}"));
        }
    }

    if !lines.is_empty() {
        lines.push(String::new());
    }

    if let Some(error) = &state.response.last_error {
        lines.push(format!("Request failed: {error}"));
    } else if state.response.cancelled {
        lines.push(String::from("Request cancelled."));
    } else if !state.response.buffer.is_empty() {
        lines.extend(
            state
                .response
                .buffer
                .as_text()
                .split('\n')
                .map(str::to_string),
        );
    }

    if lines.is_empty() {
        lines.push(String::from("No raw response available."));
    }

    lines
}

fn slice_line(line: &Line<'static>, start: usize, width: usize) -> Line<'static> {
    if width == 0 {
        return Line::from(String::new());
    }

    let mut chars = Vec::new();
    for span in &line.spans {
        for ch in span.content.chars() {
            chars.push((ch, span.style));
        }
    }

    if start >= chars.len() {
        return Line::from(String::new());
    }

    let end = (start + width).min(chars.len());
    let mut spans = Vec::new();
    let mut current_style = chars[start].1;
    let mut current = String::new();

    for (ch, style) in chars[start..end].iter().copied() {
        if style != current_style {
            spans.push(ratatui::text::Span::styled(current, current_style));
            current = String::new();
            current_style = style;
        }
        current.push(ch);
    }
    spans.push(ratatui::text::Span::styled(current, current_style));

    Line::from(spans)
}
