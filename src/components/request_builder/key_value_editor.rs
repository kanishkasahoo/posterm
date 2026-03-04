use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::widgets::{
    Block, Borders, Cell, Row, Scrollbar, ScrollbarOrientation, ScrollbarState, Table, TableState,
};

use crate::state::{KeyValueEditorState, KeyValueField, KeyValueRow, RequestFocus};

pub fn render_editor(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &str,
    rows: &[KeyValueRow],
    editor_state: KeyValueEditorState,
    focus: RequestFocus,
) {
    let mut rendered_rows: Vec<Row<'_>> = rows
        .iter()
        .enumerate()
        .map(|(idx, row)| {
            let enabled = if row.enabled { "[x]" } else { "[ ]" };
            let mut key_cell = Cell::from(row.key.clone());
            let mut value_cell = Cell::from(row.value.clone());

            if focus == RequestFocus::Editor && idx == editor_state.selected_row {
                match editor_state.active_field {
                    KeyValueField::Key => {
                        key_cell = key_cell.style(Style::default().fg(Color::Yellow));
                    }
                    KeyValueField::Value => {
                        value_cell = value_cell.style(Style::default().fg(Color::Yellow));
                    }
                }
            }

            Row::new([Cell::from(enabled), key_cell, value_cell])
        })
        .collect();

    if rendered_rows.is_empty() {
        rendered_rows.push(Row::new([
            Cell::from("[ ]"),
            Cell::from(""),
            Cell::from(""),
        ]));
    }

    let title_style = if focus == RequestFocus::Editor {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default()
    };

    let table = Table::new(
        rendered_rows,
        [
            Constraint::Length(5),
            Constraint::Percentage(45),
            Constraint::Percentage(50),
        ],
    )
    .header(Row::new(["On", "Key", "Value"]).bold())
    .block(
        Block::default()
            .title(title)
            .title_style(title_style)
            .borders(Borders::ALL),
    )
    .row_highlight_style(Style::default().bg(Color::DarkGray));

    // Use stateful render so ratatui auto-scrolls to keep selected_row visible.
    let selected = if focus == RequestFocus::Editor {
        Some(editor_state.selected_row)
    } else {
        None
    };
    let mut table_state = TableState::default().with_selected(selected);
    frame.render_stateful_widget(table, area, &mut table_state);

    // Vertical scrollbar when rows overflow the visible area.
    // Header takes 1 row + 2 border rows = 3 overhead rows.
    let total_data_rows = rows.len().max(1);
    let visible_rows = usize::from(area.height.saturating_sub(3)); // borders + header
    if total_data_rows > visible_rows {
        let mut sb_state = ScrollbarState::new(total_data_rows.saturating_sub(visible_rows))
            .position(
                editor_state
                    .selected_row
                    .min(total_data_rows.saturating_sub(1)),
            );
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            area,
            &mut sb_state,
        );
    }
}
