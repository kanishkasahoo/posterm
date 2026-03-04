pub mod collection_tree;
pub mod history_list;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use ratatui::widgets::{Block, BorderType, Borders};

use crate::action::Action;
use crate::components::Component;
use crate::state::AppState;
use crate::util::terminal_sanitize::sanitize_terminal_text;

/// The sidebar component displays the collections tree (top ~60%) and the
/// request history list (bottom ~40%).
pub struct Sidebar;

impl Component for Sidebar {
    fn handle_action(&mut self, _action: &Action, _state: &AppState) {
        // Navigation state is mutated directly in app.rs via apply_action.
    }

    fn render(&self, frame: &mut Frame<'_>, area: Rect, state: &AppState) {
        // Outer border with title.
        let border_style = if state.sidebar_focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let outer_block = Block::default()
            .title("Collections / History")
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(border_style);

        let inner = outer_block.inner(area);
        frame.render_widget(outer_block, area);

        let (lists_area, prompt_area) = if state.sidebar_prompt.is_some() && inner.height >= 5 {
            let chunks = Layout::vertical([Constraint::Min(2), Constraint::Length(3)]).split(inner);
            (chunks[0], Some(chunks[1]))
        } else {
            (inner, None)
        };

        // Split main list area: 60% collections, 40% history.
        let chunks = Layout::vertical([Constraint::Percentage(60), Constraint::Percentage(40)])
            .split(lists_area);

        collection_tree::render(frame, chunks[0], state);
        history_list::render(frame, chunks[1], state);

        if let (Some(prompt), Some(area)) = (state.sidebar_prompt.as_ref(), prompt_area) {
            let title = prompt.mode.title();
            let display_value = sanitize_terminal_text(&prompt.value);
            let line = Line::from(vec![
                Span::raw(display_value.clone()),
                Span::styled(
                    "  Enter confirm  Esc cancel",
                    Style::default().fg(Color::DarkGray),
                ),
            ]);
            let widget =
                Paragraph::new(line).block(Block::default().title(title).borders(Borders::TOP));
            frame.render_widget(widget, area);

            let cursor_x = area
                .x
                .saturating_add(1)
                .saturating_add(display_value.chars().count().min(u16::MAX as usize) as u16);
            let cursor_y = area.y.saturating_add(1);
            if cursor_x < area.x.saturating_add(area.width)
                && cursor_y < area.y.saturating_add(area.height)
            {
                frame.set_cursor_position((cursor_x, cursor_y));
            }
        }
    }
}
