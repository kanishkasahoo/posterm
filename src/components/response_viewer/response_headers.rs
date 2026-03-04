use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Text};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::state::{AppState, ResponseSearchScope};

use super::response_body::{apply_search_overlays, search_matches_for_line};

pub fn render(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let lines = header_lines(state);
    let inner_height = usize::from(area.height.saturating_sub(2).max(1));
    let inner_width = usize::from(area.width.saturating_sub(2).max(1));
    let max_scroll = lines.len().saturating_sub(inner_height);
    let scroll = state.response.scroll_offset.min(max_scroll);
    let horizontal_scroll = state.response.horizontal_scroll_offset;

    let mut rendered = Vec::with_capacity(inner_height);
    for row in 0..inner_height {
        let line_index = scroll + row;
        if let Some(line_text) = lines.get(line_index) {
            let mut line = Line::from(line_text.clone());
            let line_matches =
                search_matches_for_line(line_index, state, ResponseSearchScope::Headers);
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
        rendered.push(Line::from("No response headers available."));
    }

    let mut widget = Paragraph::new(Text::from(rendered)).block(
        Block::default()
            .title(if state.response.wrap_lines {
                "Headers (Wrap: On)"
            } else {
                "Headers (Wrap: Off)"
            })
            .borders(Borders::ALL),
    );

    if state.response.wrap_lines {
        widget = widget.wrap(Wrap { trim: false });
    }

    frame.render_widget(widget, area);
}

pub(crate) fn header_lines(state: &AppState) -> Vec<String> {
    match &state.response.metadata {
        Some(metadata) if metadata.headers.is_empty() => {
            vec![String::from("No headers in response.")]
        }
        Some(metadata) => metadata
            .headers
            .iter()
            .map(|(name, value)| format!("{name}: {value}"))
            .collect(),
        None if state.response.in_flight.is_some() => {
            vec![String::from("Waiting for response headers...")]
        }
        None => vec![String::from("No response headers available.")],
    }
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
