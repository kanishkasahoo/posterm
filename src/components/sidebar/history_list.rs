use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, List, ListItem, ListState, Scrollbar, ScrollbarOrientation, ScrollbarState,
};
use ratatui::Frame;

use crate::state::{AppState, SidebarItem};

/// Renders the history list into `area`.
///
/// Entries are assumed to already be in most-recent-first order
/// (as maintained by `AppState.history`).
pub fn render(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let visible_items = usize::from(area.height.saturating_sub(2));
    let has_vertical_scrollbar = state.history.len() > visible_items;
    let content_width = usize::from(area.width).saturating_sub(usize::from(has_vertical_scrollbar));

    let mut items: Vec<ListItem> = Vec::new();

    for (idx, entry) in state.history.iter().enumerate() {
        let is_selected = state.sidebar_selected_item == SidebarItem::HistoryEntry(idx);

        let time_str = format_unix_time(entry.timestamp_secs);
        let status_str = entry
            .status_code
            .map(|c| c.to_string())
            .unwrap_or_else(|| String::from("---"));

        let status_style = entry
            .status_code
            .map(status_color)
            .unwrap_or_else(|| Style::default().fg(Color::DarkGray));

        let method_style = method_color(&entry.method);

        let url_display = display_url(&entry.url);

        let base_style = if is_selected && state.sidebar_focused {
            Style::default().fg(Color::Black).bg(Color::White)
        } else if is_selected {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::UNDERLINED)
        } else {
            Style::default().fg(Color::Gray)
        };

        let line = if is_selected && state.sidebar_focused {
            // When highlighted, render as a single flat span for contrast.
            Line::from(Span::styled(
                format!(
                    "{} {} {}  {}",
                    time_str, entry.method, url_display, status_str
                ),
                base_style,
            ))
        } else {
            Line::from(vec![
                Span::styled(
                    format!("{} ", time_str),
                    Style::default().fg(Color::DarkGray),
                ),
                Span::styled(format!("{} ", entry.method), method_style),
                Span::styled(format!("{} ", url_display), base_style),
                Span::styled(status_str, status_style),
            ])
        };

        items.push(ListItem::new(clamp_line_horizontal(
            line,
            state.sidebar_history_horizontal_offset,
            content_width,
        )));
    }

    if items.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            "  (no history)",
            Style::default().fg(Color::DarkGray),
        ))));
    }

    let selected_flat = if let SidebarItem::HistoryEntry(idx) = state.sidebar_selected_item {
        Some(idx)
    } else {
        None
    };

    let mut list_state = ListState::default();
    list_state.select(selected_flat);

    let list = List::new(items).block(Block::default().title("History").borders(Borders::TOP));

    frame.render_stateful_widget(list, area, &mut list_state);

    // Vertical scrollbar when history overflows.
    let total_items = state.history.len();
    if total_items > visible_items {
        let scroll_pos = selected_flat.unwrap_or(0);
        let mut sb_state =
            ScrollbarState::new(total_items.saturating_sub(visible_items)).position(scroll_pos);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            area,
            &mut sb_state,
        );
    }
}

/// Returns a style appropriate for the HTTP status code.
fn status_color(code: u16) -> Style {
    match code {
        200..=299 => Style::default().fg(Color::Green),
        300..=399 => Style::default().fg(Color::Yellow),
        400..=499 => Style::default().fg(Color::Red),
        500..=599 => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        _ => Style::default().fg(Color::Gray),
    }
}

fn method_color(method: &str) -> Style {
    match method {
        "GET" => Style::default().fg(Color::Green),
        "POST" => Style::default().fg(Color::Yellow),
        "PUT" => Style::default().fg(Color::Blue),
        "PATCH" => Style::default().fg(Color::Magenta),
        "DELETE" => Style::default().fg(Color::Red),
        _ => Style::default().fg(Color::Gray),
    }
}

/// Formats a Unix timestamp as `HH:MM`.
fn format_unix_time(secs: u64) -> String {
    let total_secs_in_day = secs % 86_400;
    let hours = total_secs_in_day / 3600;
    let minutes = (total_secs_in_day % 3600) / 60;
    format!("{:02}:{:02}", hours, minutes)
}

/// Strips URL scheme for compact display in the history sidebar.
fn display_url(url: &str) -> String {
    url.strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url)
        .to_string()
}

fn clamp_line_horizontal(line: Line<'static>, offset: usize, max_chars: usize) -> Line<'static> {
    if max_chars == 0 {
        return Line::default();
    }

    let mut remaining_offset = offset;
    let mut remaining_chars = max_chars;
    let mut spans: Vec<Span<'static>> = Vec::new();

    for span in line.spans {
        if remaining_chars == 0 {
            break;
        }

        let text = span.content.into_owned();
        let text_len = text.chars().count();

        if remaining_offset >= text_len {
            remaining_offset -= text_len;
            continue;
        }

        let start = remaining_offset;
        remaining_offset = 0;
        let take = (text_len - start).min(remaining_chars);
        if take == 0 {
            continue;
        }

        let clipped: String = text.chars().skip(start).take(take).collect();
        remaining_chars -= take;
        spans.push(Span::styled(clipped, span.style));
    }

    Line::from(spans)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::style::Style;

    #[test]
    fn format_unix_time_returns_hh_mm() {
        // 0 seconds from epoch → 00:00 UTC
        assert_eq!(format_unix_time(0), "00:00");
        // 3661 seconds = 1h 1m 1s
        assert_eq!(format_unix_time(3661), "01:01");
        // Exactly one day should wrap back to 00:00
        assert_eq!(format_unix_time(86_400), "00:00");
        // 23h 59m (86_340 seconds)
        assert_eq!(format_unix_time(86_340), "23:59");
    }

    #[test]
    fn display_url_strips_https_scheme() {
        assert_eq!(display_url("https://example.com/path"), "example.com/path");
    }

    #[test]
    fn display_url_strips_http_scheme() {
        assert_eq!(display_url("http://example.com"), "example.com");
    }

    #[test]
    fn display_url_handles_no_scheme() {
        assert_eq!(display_url("example.com/api"), "example.com/api");
    }

    #[test]
    fn clamp_line_horizontal_respects_offset_across_spans() {
        let line = Line::from(vec![
            Span::styled("12:34 ", Style::default()),
            Span::styled("GET ", Style::default()),
            Span::styled("example.com/very/long/path ", Style::default()),
            Span::styled("200", Style::default()),
        ]);

        let clipped = clamp_line_horizontal(line, 10, 14);
        let rendered: String = clipped
            .spans
            .into_iter()
            .map(|s| s.content.into_owned())
            .collect();
        assert_eq!(rendered, "example.com/ve");
    }

    #[test]
    fn clamp_line_horizontal_returns_empty_when_offset_is_past_end() {
        let line = Line::from(Span::raw("history"));
        let clipped = clamp_line_horizontal(line, 20, 8);
        assert!(clipped.spans.is_empty());
    }

    #[test]
    fn status_color_2xx_is_green() {
        let style = status_color(200);
        assert_eq!(style.fg, Some(ratatui::style::Color::Green));
    }

    #[test]
    fn status_color_4xx_is_red() {
        let style = status_color(404);
        assert_eq!(style.fg, Some(ratatui::style::Color::Red));
    }

    #[test]
    fn status_color_5xx_is_bold_red() {
        use ratatui::style::Modifier;
        let style = status_color(500);
        assert_eq!(style.fg, Some(ratatui::style::Color::Red));
        assert!(style.add_modifier.contains(Modifier::BOLD));
    }
}
