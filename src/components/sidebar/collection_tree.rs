use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, ListState};
use ratatui::Frame;

use crate::state::{AppState, SidebarItem};

/// Renders the expandable collection tree into `area`.
pub fn render(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let mut items: Vec<ListItem> = Vec::new();
    // Track which flat index corresponds to which SidebarItem so we can
    // highlight the selected one.
    let mut index_map: Vec<SidebarItem> = Vec::new();

    for (col_idx, col) in state.collections.iter().enumerate() {
        let arrow = if col.expanded { "▼" } else { "▶" };
        let col_label = format!("{arrow} {}", col.name);
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

        items.push(ListItem::new(Line::from(Span::styled(col_label, style))));
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

                let method_style = method_color(&req.method);
                let req_label = format!("  ● {} {}", req.method, req.name);
                let line = if is_req_selected && state.sidebar_focused {
                    Line::from(Span::styled(req_label, req_style))
                } else {
                    Line::from(vec![
                        Span::styled(format!("  ● "), req_style),
                        Span::styled(req.method.clone(), method_style),
                        Span::styled(format!(" {}", req.name), req_style),
                    ])
                };
                items.push(ListItem::new(line));
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
