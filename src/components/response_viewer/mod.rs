mod metadata_bar;
pub(crate) mod raw_view;
pub(crate) mod response_body;
pub(crate) mod response_headers;
mod search_bar;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Tabs};

use crate::action::Action;
use crate::components::Component;
use crate::state::{AppState, ResponseTab};

#[derive(Default)]
pub struct ResponseViewer;

impl Component for ResponseViewer {
    fn handle_action(&mut self, _action: &Action, _state: &AppState) {}

    fn render(&self, frame: &mut Frame<'_>, area: Rect, state: &AppState) {
        let block = Block::default().title("Response").borders(Borders::ALL);
        frame.render_widget(block.clone(), area);

        let inner = block.inner(area);
        if inner.height == 0 || inner.width == 0 {
            return;
        }

        let sections = Layout::vertical([
            Constraint::Length(1),
            Constraint::Length(3),
            Constraint::Length(if state.response.search.active { 3 } else { 0 }),
            Constraint::Min(1),
        ])
        .split(inner);

        metadata_bar::render(frame, sections[0], state);

        let compact_tabs = sections[1].width < 36;
        let tab_titles = ResponseTab::ALL
            .iter()
            .map(|tab| {
                if compact_tabs {
                    let label = match tab {
                        ResponseTab::Body => "B",
                        ResponseTab::Headers => "H",
                        ResponseTab::Raw => "R",
                    };
                    Line::from(label)
                } else {
                    Line::from((*tab).title())
                }
            })
            .collect::<Vec<_>>();
        let tabs = Tabs::new(tab_titles)
            .select(
                ResponseTab::ALL
                    .iter()
                    .position(|tab| *tab == state.response.active_tab)
                    .unwrap_or(0),
            )
            .highlight_style(Style::default().fg(Color::Cyan))
            .divider(if compact_tabs { "" } else { "|" })
            .block(Block::default().borders(Borders::ALL).title("Tabs"));
        frame.render_widget(tabs, sections[1]);

        if state.response.search.active {
            search_bar::render(frame, sections[2], state);
        }

        match state.response.active_tab {
            ResponseTab::Body => response_body::render(frame, sections[3], state),
            ResponseTab::Headers => response_headers::render(frame, sections[3], state),
            ResponseTab::Raw => raw_view::render(frame, sections[3], state),
        }
    }
}
