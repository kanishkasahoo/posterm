pub mod help_modal;
pub mod layout_manager;
pub mod request_builder;
pub mod response_viewer;
pub mod status_bar;

use ratatui::Frame;
use ratatui::layout::Rect;

use crate::action::Action;
use crate::state::AppState;

pub trait Component {
    fn handle_action(&mut self, action: &Action, state: &AppState);
    fn render(&self, frame: &mut Frame<'_>, area: Rect, state: &AppState);
}
