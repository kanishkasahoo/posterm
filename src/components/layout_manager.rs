use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::state::LayoutMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PaneLayout {
    pub main: Rect,
    pub status: Rect,
    pub mode: LayoutMode,
}

pub struct LayoutManager;

impl LayoutManager {
    pub fn mode_for_dimensions(width: u16, height: u16) -> LayoutMode {
        if width < 80 || height < 24 {
            LayoutMode::Small
        } else if width < 120 || height < 36 {
            LayoutMode::Medium
        } else {
            LayoutMode::Large
        }
    }

    pub fn compute(area: Rect) -> PaneLayout {
        let sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1), Constraint::Length(1)])
            .split(area);

        PaneLayout {
            main: sections[0],
            status: sections[1],
            mode: Self::mode_for_dimensions(area.width, area.height),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LayoutManager;
    use crate::state::LayoutMode;

    #[test]
    fn computes_layout_mode_thresholds() {
        assert_eq!(
            LayoutManager::mode_for_dimensions(60, 20),
            LayoutMode::Small
        );
        assert_eq!(
            LayoutManager::mode_for_dimensions(100, 30),
            LayoutMode::Medium
        );
        assert_eq!(
            LayoutManager::mode_for_dimensions(140, 40),
            LayoutMode::Large
        );
    }
}
