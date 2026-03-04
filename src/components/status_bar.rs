use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::action::Action;
use crate::components::Component;
use crate::state::{AppState, NotificationKind, ResponseSearchScope};

#[derive(Default)]
pub struct StatusBar;

impl Component for StatusBar {
    fn handle_action(&mut self, _action: &Action, _state: &AppState) {}

    fn render(&self, frame: &mut Frame<'_>, area: Rect, state: &AppState) {
        let mut spans: Vec<Span<'static>> = Vec::new();

        // Notification segment (if active) — prepended before regular content.
        if let Some((msg, kind)) = &state.notification {
            let (icon, color) = match kind {
                NotificationKind::Error => ("⚠ ", Color::Red),
                NotificationKind::Info => ("ℹ ", Color::Cyan),
            };
            spans.push(Span::styled(
                format!("{icon}{msg}"),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::raw(String::from(" | ")));
        }

        // Response / in-flight status segment.
        if let Some(in_flight) = state.response.in_flight.as_ref() {
            let status = if in_flight.cancellation_requested {
                "cancelling"
            } else {
                "loading"
            };
            spans.push(Span::raw(format!(
                "Request: {} {} ({status})",
                in_flight.method.as_str(),
                in_flight.url
            )));
        } else if let Some(error) = state.response.last_error.as_deref() {
            spans.push(Span::raw(format!("Error: {error}")));
        } else if let Some(metadata) = state.response.metadata.as_ref() {
            let status = metadata
                .status_code
                .map_or_else(|| String::from("-"), |code| code.to_string());
            spans.push(Span::raw(format!(
                "Response: {status} in {}ms · {} bytes",
                metadata.duration_ms, metadata.total_bytes
            )));
        } else {
            spans.push(Span::raw(String::from("Response: idle")));
        }

        spans.push(Span::raw(String::from(" | ")));

        // Search segment.
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
            spans.push(Span::raw(format!(
                "Search: \"{}\" [{match_position}/{} in {scope}]",
                state.response.search.query,
                state.response.search.matches.len()
            )));
        } else {
            spans.push(Span::raw(String::from("Search: inactive")));
        }

        spans.push(Span::raw(String::from(" | ")));

        spans.push(Span::raw(format!("Update: {}", state.updater.status_text)));

        spans.push(Span::raw(String::from(" | ")));

        if state.help_visible {
            spans.push(Span::raw(String::from("Close help: Esc")));
            spans.push(Span::raw(String::from(" | ")));
        }

        spans.push(Span::raw(String::from("Help: F1")));
        spans.push(Span::raw(String::from(" | ")));
        spans.push(Span::raw(String::from("Exit: Ctrl+Q")));

        frame.render_widget(Paragraph::new(Line::from(spans)), area);
    }
}
