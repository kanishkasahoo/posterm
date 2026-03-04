use ratatui::layout::{Alignment, Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{
    Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};
use ratatui::Frame;

use crate::state::AppState;

pub fn render(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let modal_width = area.width.saturating_sub(4).clamp(60, 100);
    let modal_height = area.height.saturating_sub(4).clamp(20, 38);
    let modal_area = centered_rect(modal_width, modal_height, area);

    frame.render_widget(Clear, modal_area);

    let block = Block::default()
        .title("Keyboard Shortcuts  (↑↓/j/k/PgUp/PgDn to scroll)")
        .title_style(Style::default().fg(Color::Yellow))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let inner = block.inner(modal_area);

    frame.render_widget(block, modal_area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let all_lines: Vec<Line> = vec![
        Line::from("Global"),
        Line::from("  F1             Toggle help"),
        Line::from("  Ctrl+Q         Quit"),
        Line::from("  Ctrl+S         Send request"),
        Line::from("  Ctrl+C         Cancel request"),
        Line::from("  Tab/Shift+Tab  Move request focus"),
        Line::from("  Esc            Close help, search, or sidebar"),
        Line::from(""),
        Line::from("Sidebar (Collections & History)"),
        Line::from("  Ctrl+B         Toggle sidebar (Small/Medium) / focus (Large)"),
        Line::from("  Up/Down        Navigate items"),
        Line::from("  Enter/Space    Expand collection or load request/history entry"),
        Line::from("  Esc            Close / unfocus sidebar"),
        Line::from(""),
        Line::from("Small Mode"),
        Line::from("  Ctrl+R         Toggle between Request Builder and Response Viewer"),
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

    let total_lines = all_lines.len();
    let inner_height = usize::from(inner.height);
    let max_scroll = total_lines.saturating_sub(inner_height);
    let scroll = state.help_scroll.min(max_scroll);

    let visible_lines: Vec<Line> = all_lines
        .into_iter()
        .skip(scroll)
        .take(inner_height)
        .collect();

    frame.render_widget(
        Paragraph::new(visible_lines)
            .alignment(Alignment::Left)
            .style(Style::default().fg(Color::White)),
        inner,
    );

    // Scrollbar overlay on the modal border (right side).
    if total_lines > inner_height {
        let mut sb_state =
            ScrollbarState::new(total_lines.saturating_sub(inner_height)).position(scroll);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            modal_area,
            &mut sb_state,
        );
    }
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .split(area);
    Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .split(vertical[0])[0]
}
