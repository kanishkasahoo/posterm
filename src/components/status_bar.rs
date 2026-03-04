use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::Paragraph;

use crate::action::Action;
use crate::components::Component;
use crate::state::{AppState, ResponseSearchScope};

#[derive(Default)]
pub struct StatusBar;

impl Component for StatusBar {
    fn handle_action(&mut self, _action: &Action, _state: &AppState) {}

    fn render(&self, frame: &mut Frame<'_>, area: Rect, state: &AppState) {
        let mut segments = Vec::new();

        if let Some(in_flight) = state.response.in_flight.as_ref() {
            let status = if in_flight.cancellation_requested {
                "cancelling"
            } else {
                "loading"
            };
            segments.push(format!(
                "Request: {} {} ({status})",
                in_flight.method.as_str(),
                in_flight.url
            ));
        } else if let Some(error) = state.response.last_error.as_deref() {
            segments.push(format!("Error: {error}"));
        } else if let Some(metadata) = state.response.metadata.as_ref() {
            let status = metadata
                .status_code
                .map_or_else(|| String::from("-"), |code| code.to_string());
            segments.push(format!(
                "Response: {status} in {}ms · {} bytes",
                metadata.duration_ms, metadata.total_bytes
            ));
        } else {
            segments.push(String::from("Response: idle"));
        }

        segments.push(format!("Tab: {}", state.response.active_tab.title()));

        if state.response.search.active {
            let scope = match state.response.search.scope {
                ResponseSearchScope::Body => "body",
                ResponseSearchScope::Headers => "headers",
                ResponseSearchScope::Raw => "raw",
            };
            let match_position = state
                .response
                .search
                .current_match
                .map_or(0, |idx| idx.saturating_add(1));
            segments.push(format!(
                "Search: \"{}\" [{match_position}/{} in {scope}]",
                state.response.search.query,
                state.response.search.matches.len()
            ));
        } else {
            segments.push(String::from("Search: inactive"));
        }

        if state.help_visible {
            segments.push(String::from("Close help: Esc"));
        }

        segments.push(String::from("Help: F1"));
        segments.push(String::from("Exit: Ctrl+Q"));

        let line = Line::from(segments.join(" | "));

        frame.render_widget(Paragraph::new(line), area);
    }
}
