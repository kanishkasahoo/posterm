use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Clear, Paragraph};

use crate::state::AppState;

pub fn render(frame: &mut Frame<'_>, area: Rect, _state: &AppState) {
    let modal_width = area.width.saturating_sub(4).clamp(56, 100);
    let modal_height = area.height.saturating_sub(4).clamp(18, 24);
    let modal_area = centered_rect(modal_width, modal_height, area);

    frame.render_widget(Clear, modal_area);

    let block = Block::default()
        .title("Keyboard Shortcuts")
        .title_style(Style::default().fg(Color::Yellow))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(modal_area);

    frame.render_widget(block, modal_area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let lines = vec![
        Line::from("Global"),
        Line::from("  F1             Toggle help"),
        Line::from("  Ctrl+Q         Quit"),
        Line::from("  Ctrl+S         Send request"),
        Line::from("  Ctrl+C         Cancel request"),
        Line::from("  Tab/Shift+Tab  Move request focus"),
        Line::from("  Esc            Close help or search"),
        Line::from(""),
        Line::from("Response"),
        Line::from("  Ctrl+F         Open search input"),
        Line::from("  Enter/n        Next search match"),
        Line::from("  Shift+N        Previous search match"),
        Line::from("  Ctrl+H/Ctrl+L  Previous/next response tab"),
        Line::from("  Alt+1..3       Body/Headers/Raw tab"),
        Line::from("  Ctrl+Shift+Up/Down, Ctrl+PageUp/PageDown  Scroll response"),
        Line::from("  Ctrl+Left/Right Horizontal scroll"),
        Line::from("  Ctrl+W         Toggle response wrap"),
        Line::from(""),
        Line::from("Request"),
        Line::from("  Method focus: Left/Right/Up/Down or j/k"),
        Line::from("  Tabs focus: Left/Right or 1..4 (Params/Headers/Auth/Body)"),
        Line::from(""),
        Line::from("Editors"),
        Line::from("  Params/Headers/Form: Ctrl+N add, Ctrl+D remove, Space enable/disable"),
        Line::from("  Params/Headers/Form: Enter switch key/value, arrows/Home/End move cursor"),
    ];

    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Left)
            .style(Style::default().fg(Color::White)),
        inner,
    );
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .split(area);
    Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .split(vertical[0])[0]
}
