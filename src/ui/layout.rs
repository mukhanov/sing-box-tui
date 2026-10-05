//! Dashboard layout (T006, FR-013, FR-022, contracts/ui.md).
//!
//! Status is a fixed-height top row; the remaining space splits into three
//! equal columns (Ping, Config, Log). Minimum 80×24 keeps all four panels
//! visible with Status fully visible.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Minimum supported terminal size (contracts/ui.md).
pub const MIN_WIDTH: u16 = 80;
pub const MIN_HEIGHT: u16 = 24;

/// Vertical split: [status, rest]. Status height adapts to its content
/// (banner/error lines) but never shrinks below the 6-line contract minimum.
pub fn dashboard_layout(area: Rect, status_height: u16) -> Vec<Rect> {
    let status_height = status_height.clamp(6, area.height.saturating_sub(6).max(6));
    Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(status_height), Constraint::Fill(1)])
        .split(area)
        .to_vec()
}

/// Three equal columns for Ping/Config/Log.
pub fn three_columns(area: Rect) -> Vec<Rect> {
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
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
