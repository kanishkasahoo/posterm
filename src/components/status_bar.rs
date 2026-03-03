use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::action::Action;
use crate::components::Component;
use crate::state::{AppState, LayoutMode};

#[derive(Default)]
pub struct StatusBar;

impl Component for StatusBar {
    fn handle_action(&mut self, _action: &Action, _state: &AppState) {}

    fn render(&self, frame: &mut Frame<'_>, area: Rect, state: &AppState) {
        let layout_label = match state.layout_mode {
            LayoutMode::Small => "Small",
            LayoutMode::Medium => "Medium",
            LayoutMode::Large => "Large",
        };

        let line = Line::from(vec![
            Span::raw(
                "Ctrl+Q Quit  |  Tab/Shift+Tab Focus  |  Enter Toggle Field  |  Ctrl+N Add Row  |  Ctrl+D Delete Row  |  Layout: ",
            ),
            Span::raw(layout_label),
        ]);

        frame.render_widget(Paragraph::new(line), area);
    }
}
