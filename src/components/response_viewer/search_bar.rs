use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::state::AppState;

pub fn render(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let search = &state.response.search;
    let search_prompt = "Ctrl+F";
    let current = search.current_match.map(|index| index + 1).unwrap_or(0);
    let total = search.matches.len();
    let prompt = vec![
        Span::styled(search_prompt, Style::default().fg(Color::Cyan)),
        Span::raw(search.query.clone()),
        Span::raw("  "),
        Span::styled(
            format!("{current}/{total}"),
            Style::default().fg(Color::DarkGray),
        ),
        Span::raw("  n/N navigate  Esc close"),
    ];

    let widget = Paragraph::new(Line::from(prompt))
        .block(Block::default().title("Search").borders(Borders::ALL));
    frame.render_widget(widget, area);

    let cursor_x = area
        .x
        .saturating_add(1)
        .saturating_add(search_prompt.chars().count().min(u16::MAX as usize) as u16)
        .saturating_add(search.query.chars().count().min(u16::MAX as usize) as u16);
    let cursor_y = area.y.saturating_add(1);
    if cursor_x < area.x.saturating_add(area.width) && cursor_y < area.y.saturating_add(area.height)
    {
        frame.set_cursor_position((cursor_x, cursor_y));
    }
}
