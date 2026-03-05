use ratatui::layout::{Alignment, Constraint, Flex, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::Line;
use ratatui::widgets::{
    Block, Borders, Clear, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};
use ratatui::Frame;

use crate::state::AppState;

const MODAL_MARGIN: u16 = 4;
const MIN_MODAL_WIDTH: u16 = 60;
const MAX_MODAL_WIDTH: u16 = 100;
const MIN_MODAL_HEIGHT: u16 = 20;
const MAX_MODAL_HEIGHT: u16 = 38;

pub fn line_count() -> usize {
    shortcut_lines().len()
}

pub fn visible_line_capacity(area: Rect) -> usize {
    usize::from(content_area(area).height)
}

pub fn max_scroll_for_area(area: Rect, total_lines: usize) -> usize {
    total_lines.saturating_sub(visible_line_capacity(area))
}

pub fn render(frame: &mut Frame<'_>, area: Rect, state: &AppState) {
    let modal_area = modal_area(area);

    frame.render_widget(Clear, modal_area);

    let block = modal_block();
    let inner = block.inner(modal_area);

    frame.render_widget(block, modal_area);
    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let all_lines = shortcut_lines();

    let total_lines = all_lines.len();
    let inner_height = usize::from(inner.height);
    let max_scroll = max_scroll_for_area(area, total_lines);
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

fn modal_block() -> Block<'static> {
    Block::default()
        .title("Keyboard Shortcuts  (↑↓/j/k/PgUp/PgDn to scroll)")
        .title_style(Style::default().fg(Color::Yellow))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
}

fn modal_area(area: Rect) -> Rect {
    let modal_width = area
        .width
        .saturating_sub(MODAL_MARGIN)
        .clamp(MIN_MODAL_WIDTH, MAX_MODAL_WIDTH);
    let modal_height = area
        .height
        .saturating_sub(MODAL_MARGIN)
        .clamp(MIN_MODAL_HEIGHT, MAX_MODAL_HEIGHT);
    centered_rect(modal_width, modal_height, area)
}

fn content_area(area: Rect) -> Rect {
    modal_block().inner(modal_area(area))
}

fn shortcut_lines() -> Vec<Line<'static>> {
    vec![
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
        Line::from("  Left/Right     Horizontal scroll selected section"),
        Line::from("  Enter          Expand collection or load request/history entry"),
        Line::from("  Space          Mark/unmark history entry for bulk delete (or load)"),
        Line::from("  c              Create collection"),
        Line::from("  r              Rename selected collection/request"),
        Line::from("  d              Delete selected collection/request/history entry"),
        Line::from("  X              Clear all history"),
        Line::from("  s              Save current request into selected collection"),
        Line::from("  Prompt mode: type + Backspace, Enter confirm, Esc cancel"),
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
        Line::from("  Body: Left/Right on Format field cycles formats (JSON/Form/Text)"),
    ]
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([Constraint::Length(height)])
        .flex(Flex::Center)
        .split(area);
    Layout::horizontal([Constraint::Length(width)])
        .flex(Flex::Center)
        .split(vertical[0])[0]
}

#[cfg(test)]
mod tests {
    use super::{line_count, max_scroll_for_area, visible_line_capacity};
    use ratatui::layout::Rect;

    #[test]
    fn visible_line_capacity_respects_modal_height_clamps() {
        let max_clamped_area = Rect::new(0, 0, 140, 80);
        let min_clamped_area = Rect::new(0, 0, 100, 23);

        assert_eq!(visible_line_capacity(max_clamped_area), 36);
        assert_eq!(visible_line_capacity(min_clamped_area), 18);
    }

    #[test]
    fn max_scroll_uses_visible_modal_capacity() {
        let area = Rect::new(0, 0, 140, 80);
        let expected = line_count().saturating_sub(36);

        assert_eq!(max_scroll_for_area(area, line_count()), expected);
    }
}
