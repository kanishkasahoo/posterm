pub mod collection_tree;
pub mod history_list;

use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, BorderType, Borders};
use ratatui::Frame;

use crate::action::Action;
use crate::components::Component;
use crate::state::AppState;

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

        // Split inner area: 60% collections, 40% history.
        let chunks =
            Layout::vertical([Constraint::Percentage(60), Constraint::Percentage(40)]).split(inner);

        collection_tree::render(frame, chunks[0], state);
        history_list::render(frame, chunks[1], state);
    }
}
