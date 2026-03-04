use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Borders, List, ListItem, ListState, Scrollbar, ScrollbarOrientation, ScrollbarState,
};
use ratatui::Frame;

use crate::state::{AppState, SidebarItem};
use crate::util::terminal_sanitize::sanitize_terminal_text;

/// Renders the expandable collection tree into `area`.
pub fn render(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let visible_items = usize::from(area.height.saturating_sub(2));
    let total_items = collection_tree_item_count(state);
    let has_vertical_scrollbar = total_items > visible_items;
    let content_width = usize::from(area.width).saturating_sub(usize::from(has_vertical_scrollbar));

    let mut items: Vec<ListItem> = Vec::new();
    // Track which flat index corresponds to which SidebarItem so we can
    // highlight the selected one.
    let mut index_map: Vec<SidebarItem> = Vec::new();

    for (col_idx, col) in state.collections.iter().enumerate() {
        let arrow = if col.expanded { "▼" } else { "▶" };
        let sanitized_collection_name = sanitize_terminal_text(&col.name);
        let col_label = format!("{arrow} {sanitized_collection_name}");
        let is_col_selected = state.sidebar_selected_item == SidebarItem::Collection(col_idx);

        let style = if is_col_selected && state.sidebar_focused {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else if is_col_selected {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        };

        let collection_line = Line::from(Span::styled(col_label, style));
        items.push(ListItem::new(clamp_line_horizontal(
            collection_line,
            state.sidebar_collections_horizontal_offset,
            content_width,
        )));
        index_map.push(SidebarItem::Collection(col_idx));

        if col.expanded {
            for (req_idx, req) in col.requests.iter().enumerate() {
                let is_req_selected = state.sidebar_selected_item
                    == SidebarItem::Request {
                        collection: col_idx,
                        request: req_idx,
                    };

                let req_style = if is_req_selected && state.sidebar_focused {
                    Style::default().fg(Color::Black).bg(Color::Green)
                } else if is_req_selected {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::Gray)
                };

                let sanitized_method = sanitize_terminal_text(&req.method);
                let sanitized_request_name = sanitize_terminal_text(&req.name);
                let method_style = method_color(&sanitized_method);
                let req_label = format!("  ● {sanitized_method} {sanitized_request_name}");
                let line = if is_req_selected && state.sidebar_focused {
                    Line::from(Span::styled(req_label, req_style))
                } else {
                    Line::from(vec![
                        Span::styled("  ● ".to_string(), req_style),
                        Span::styled(sanitized_method, method_style),
                        Span::styled(format!(" {sanitized_request_name}"), req_style),
                    ])
                };
                items.push(ListItem::new(clamp_line_horizontal(
                    line,
                    state.sidebar_collections_horizontal_offset,
                    content_width,
                )));
                index_map.push(SidebarItem::Request {
                    collection: col_idx,
                    request: req_idx,
                });
            }
        }
    }

    if items.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            "  (no collections)",
            Style::default().fg(Color::DarkGray),
        ))));
    }

    // Find the flat index of the currently selected item.
    let selected_flat = index_map
        .iter()
        .position(|item| *item == state.sidebar_selected_item);

    let mut list_state = ListState::default();
    list_state.select(selected_flat);

    let list = List::new(items).block(
        Block::default()
            .title("Collections")
            .borders(Borders::BOTTOM),
    );

    frame.render_stateful_widget(list, area, &mut list_state);

    // Vertical scrollbar when the collection list overflows.
    let total_items = index_map.len();
    if total_items > visible_items {
        let mut sb_state = ScrollbarState::new(total_items.saturating_sub(visible_items))
            .position(selected_flat.unwrap_or(0));
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            area,
            &mut sb_state,
        );
    }
}

fn collection_tree_item_count(state: &AppState) -> usize {
    let mut count = 0;
    for collection in &state.collections {
        count += 1;
        if collection.expanded {
            count += collection.requests.len();
        }
    }
    count
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

#[cfg(test)]
mod tests {
    use super::clamp_line_horizontal;
    use ratatui::style::Style;
    use ratatui::text::{Line, Span};

    #[test]
    fn horizontal_clamp_applies_offset_and_width() {
        let line = Line::from(vec![
            Span::styled("  ● GET ", Style::default()),
            Span::raw("https://example.com/very/long/path"),
        ]);

        let clipped = clamp_line_horizontal(line, 5, 12);
        let rendered: String = clipped
            .spans
            .into_iter()
            .map(|s| s.content.into_owned())
            .collect();

        assert_eq!(rendered, "ET https://e");
    }

    #[test]
    fn horizontal_clamp_returns_empty_when_offset_exceeds_content() {
        let line = Line::from(Span::raw("short"));
        let clipped = clamp_line_horizontal(line, 32, 10);
        let rendered: String = clipped
            .spans
            .into_iter()
            .map(|s| s.content.into_owned())
            .collect();
        assert!(rendered.is_empty());
    }
}
