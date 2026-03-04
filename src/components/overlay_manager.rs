use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::widgets::{Block, Clear};

use crate::components::Component;
use crate::components::sidebar::Sidebar;
use crate::state::AppState;

/// Renders the sidebar as a floating overlay panel on the left ~35% of `area`.
///
/// This is used in Small and Medium layout modes when `state.sidebar_visible`
/// is `true`.
pub fn render_sidebar_overlay(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let chunks =
        Layout::horizontal([Constraint::Percentage(35), Constraint::Percentage(65)]).split(area);

    let sidebar_area = chunks[0];

    // Clear the background behind the overlay so it appears floating.
    frame.render_widget(Clear, sidebar_area);
    frame.render_widget(Block::default(), sidebar_area);

    let sidebar = Sidebar;
    sidebar.render(frame, sidebar_area, state);
}
