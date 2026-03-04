use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use ratatui::Frame;

use crate::state::{AppState, SidebarItem};

/// Renders the history list into `area`.
///
/// Entries are assumed to already be in most-recent-first order
/// (as maintained by `AppState.history`).
pub fn render(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
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

        // Truncate URL for display.
        let url_display = truncate_url(&entry.url, 22);

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

        items.push(ListItem::new(line));
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

/// Truncates a URL for display, keeping a leading protocol and path fragment.
fn truncate_url(url: &str, max_chars: usize) -> String {
    // Strip scheme for brevity.
    let display = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url);

    if display.chars().count() <= max_chars {
        display.to_string()
    } else {
        let truncated: String = display.chars().take(max_chars.saturating_sub(1)).collect();
        format!("{truncated}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn truncate_url_strips_https_scheme() {
        assert_eq!(
            truncate_url("https://example.com/path", 50),
            "example.com/path"
        );
    }

    #[test]
    fn truncate_url_strips_http_scheme() {
        assert_eq!(truncate_url("http://example.com", 50), "example.com");
    }

    #[test]
    fn truncate_url_truncates_long_urls_with_ellipsis() {
        let url = "https://example.com/very/long/path/that/exceeds/limit";
        let result = truncate_url(url, 10);
        // Result should be ≤ 10 chars (9 chars + ellipsis)
        assert!(result.chars().count() <= 10);
        assert!(result.ends_with('…'));
    }

    #[test]
    fn truncate_url_leaves_short_urls_intact() {
        let url = "https://a.io";
        assert_eq!(truncate_url(url, 10), "a.io");
    }

    #[test]
    fn truncate_url_handles_no_scheme() {
        assert_eq!(truncate_url("example.com/api", 50), "example.com/api");
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
