mod headers_editor;
mod key_value_editor;
mod query_params;
mod url_bar;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Tabs};

use crate::action::Action;
use crate::components::Component;
use crate::components::request_builder::headers_editor::HeadersEditor;
use crate::components::request_builder::query_params::QueryParams;
use crate::components::request_builder::url_bar::UrlBar;
use crate::state::{AppState, RequestFocus, RequestTab};

#[derive(Default)]
pub struct RequestBuilder;

impl Component for RequestBuilder {
    fn handle_action(&mut self, _action: &Action, _state: &AppState) {}

    fn render(&self, frame: &mut Frame<'_>, area: Rect, state: &AppState) {
        let request = &state.request;

        let sections = Layout::vertical([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(5),
        ])
        .split(area);

        UrlBar::render(frame, sections[0], request);

        let titles = RequestTab::ALL
            .iter()
            .map(|tab| Line::from((*tab).title()))
            .collect::<Vec<_>>();

        let tab_title_style = if request.focus == RequestFocus::Tabs {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let tabs = Tabs::new(titles)
            .select(
                RequestTab::ALL
                    .iter()
                    .position(|tab| *tab == request.active_tab)
                    .unwrap_or(0),
            )
            .block(
                Block::default()
                    .title("Tabs")
                    .title_style(tab_title_style)
                    .borders(Borders::ALL),
            )
            .highlight_style(Style::default().fg(Color::Cyan));
        frame.render_widget(tabs, sections[1]);

        match request.active_tab {
            RequestTab::Params => QueryParams::render(frame, sections[2], request),
            RequestTab::Headers => HeadersEditor::render(frame, sections[2], request),
            RequestTab::Auth => {
                render_placeholder(frame, sections[2], "Auth editor comes in Phase 3")
            }
            RequestTab::Body => {
                render_placeholder(frame, sections[2], "Body editor comes in Phase 3")
            }
        }
    }
}

fn render_placeholder(frame: &mut Frame<'_>, area: Rect, message: &str) {
    let paragraph =
        Paragraph::new(message).block(Block::default().title("Placeholder").borders(Borders::ALL));
    frame.render_widget(paragraph, area);
}
