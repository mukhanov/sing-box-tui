//! Dashboard layout (FR-013, FR-022, contracts/ui.md; revised 2026-10-06).
//!
//! Quadrant layout: Status top-left, Ping top-right, Config bottom-left
//! (1/3 of the width — the wide Log gets 2/3 so connection addresses fit
//! whole). Minimum 80×24 keeps all four panels visible with Status fully
//! visible.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Minimum supported terminal size (contracts/ui.md).
pub const MIN_WIDTH: u16 = 80;
pub const MIN_HEIGHT: u16 = 24;

/// Vertical split into [top row, bottom row]. The top row never shrinks
/// below 4 lines nor eats the bottom below 4.
pub fn dashboard_layout(area: Rect, top_height: u16) -> Vec<Rect> {
    let top_height = top_height.clamp(4, area.height.saturating_sub(4).max(4));
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(top_height), Constraint::Fill(1)])
        .split(area)
        .to_vec()
}

/// Horizontal split by ratio (`left`:`right`), e.g. (1, 1) halves,
/// (1, 2) one-third / two-thirds.
pub fn split_columns(area: Rect, left: u32, right: u32) -> Vec<Rect> {
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Ratio(left, left + right),
            Constraint::Ratio(right, left + right),
        ])
        .split(area)
        .to_vec()
}

/// Centered popup area (dialogs, input).
pub fn popup_area(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width.saturating_sub(4));
    let h = height.min(area.height.saturating_sub(4));
    Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    }
}

/// Is the terminal at least the minimum size?
pub fn is_minimum_size(area: Rect) -> bool {
    area.width >= MIN_WIDTH && area.height >= MIN_HEIGHT
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(w: u16, h: u16) -> Rect {
        Rect {
            x: 0,
            y: 0,
            width: w,
            height: h,
        }
    }

    #[test]
    fn quadrants_at_80x24() {
        let rows = dashboard_layout(area(80, 24), 12);
        assert_eq!(rows[0].height, 12, "top row keeps its height");
        assert_eq!(rows[1].height, 12, "bottom fills the rest");

        let top = split_columns(rows[0], 1, 1);
        assert_eq!(top[0].width, 40, "Status = half");
        assert_eq!(top[1].width, 40, "Ping = half");

        let bottom = split_columns(rows[1], 1, 2);
        assert_eq!(bottom[0].width, 27, "Config ≈ 1/3 (rounded)");
        assert_eq!(bottom[1].width, 53, "Log ≈ 2/3");
    }

    #[test]
    fn top_row_clamped_on_tiny_terminals() {
        let rows = dashboard_layout(area(80, 8), 12);
        assert!(rows[0].height >= 4, "top never collapses");
        assert!(rows[1].height >= 4, "bottom never collapses");
    }
}
