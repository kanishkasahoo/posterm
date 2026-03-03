use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::state::{RequestFocus, RequestState};

pub struct UrlBar;

impl UrlBar {
    pub fn render(frame: &mut Frame<'_>, area: Rect, request: &RequestState) {
        let sections = Layout::horizontal([Constraint::Length(12), Constraint::Min(1)]).split(area);

        let method_title_style = if request.focus == RequestFocus::Method {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };
        let method = Paragraph::new(request.method.as_str())
            .block(
                Block::default()
                    .title("Method")
                    .title_style(method_title_style)
                    .borders(Borders::ALL),
            )
            .style(Style::default().fg(Color::Cyan));
        frame.render_widget(method, sections[0]);

        let url_title_style = if request.focus == RequestFocus::Url {
            Style::default().fg(Color::Yellow)
        } else {
            Style::default()
        };

        let input_width = usize::from(sections[1].width.saturating_sub(2));
        let cursor = request.url_cursor.min(request.url.chars().count());
        let scroll = if cursor >= input_width {
            cursor.saturating_sub(input_width.saturating_sub(1))
        } else {
            0
        };
        let visible = slice_chars(&request.url, scroll, input_width);

        let url = Paragraph::new(visible).block(
            Block::default()
                .title("URL")
                .title_style(url_title_style)
                .borders(Borders::ALL),
        );
        frame.render_widget(url, sections[1]);

        if request.focus == RequestFocus::Url {
            let cursor_x = sections[1]
                .x
                .saturating_add(1)
                .saturating_add((cursor.saturating_sub(scroll)) as u16);
            let cursor_y = sections[1].y.saturating_add(1);
            frame.set_cursor_position(Position::new(cursor_x, cursor_y));
        }
    }
}

fn slice_chars(value: &str, start: usize, width: usize) -> String {
    value.chars().skip(start).take(width).collect()
}
