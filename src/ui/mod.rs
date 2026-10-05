//! UI rendering modules (T006).
//!
//! `draw()` assembles the four-panel dashboard with color-independent focus
//! marking (bold borders), renders the target-input and confirmation-dialog
//! overlays, and shows a size warning below the 80×24 minimum without
//! dropping the layout (contracts/ui.md).

pub mod config_view;
pub mod layout;
pub mod log_view;
pub mod ping;
pub mod status;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::app::App;

/// Draw the full dashboard.
pub fn draw(f: &mut Frame, app: &App) {
    let area = f.area();

    // Below minimum: warn loudly but keep rendering the basic layout
    // (contracts/ui.md "Terminal too small").
    if !layout::is_minimum_size(area) {
        let warn = Paragraph::new(Line::from(vec![Span::styled(
            format!(
                " terminal too small: {}×{} (need {}×{}) — resize for full view ",
                area.width,
                area.height,
                layout::MIN_WIDTH,
                layout::MIN_HEIGHT
            ),
            Style::default().add_modifier(Modifier::BOLD),
        )]));
        let banner = Rect { height: 1, ..area };
        f.render_widget(warn, banner);
    }

    let status_height = status::content_height(app);
    let chunks = layout::dashboard_layout(area, status_height);
    let columns = layout::three_columns(chunks[1]);

    status::render(f, chunks[0], app);
    ping::render(f, columns[0], app);
    config_view::render(f, columns[1], app);
    log_view::render(f, columns[2], app);

    // Overlays last so they sit on top.
    if app.input.is_some() {
        render_target_input(f, area, app);
    }
    if let Some(dialog) = &app.dialog {
        render_dialog(f, area, &dialog.message());
    }
}

/// A focused panel's border style: bold, no color (FR-013, FR-023).
pub fn focus_style(focused: bool) -> Style {
    if focused {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default()
    }
}

/// Panel title with a bracket marker on the focused panel.
pub fn panel_title(name: &str, focused: bool) -> String {
    if focused {
        format!(" ▸ {name} ")
    } else {
        format!(" {name} ")
    }
}

/// Standard bordered block for a panel.
pub fn panel_block(name: &str, focused: bool) -> Block<'static> {
    Block::default()
        .title(panel_title(name, focused))
        .borders(Borders::ALL)
        .border_style(focus_style(focused))
}

fn render_target_input(f: &mut Frame, area: ratatui::layout::Rect, app: &App) {
    let buf = app.input.as_ref().map(|i| i.buffer.as_str()).unwrap_or("");
    let text = vec![
        Line::from("Add ping target"),
        Line::from(""),
        Line::from(format!("  {buf}▏")),
        Line::from(""),
        Line::from(Span::styled(
            "Enter submit · Esc cancel",
            Style::default().add_modifier(Modifier::DIM),
        )),
    ];
    let popup = layout::popup_area(area, 46, 7);
    f.render_widget(Clear, popup);
    f.render_widget(
        Paragraph::new(text)
            .block(panel_block(" TARGET ", true))
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn render_dialog(f: &mut Frame, area: ratatui::layout::Rect, message: &str) {
    let lines: Vec<Line> = message
        .lines()
        .map(Line::from)
        .chain([
            Line::from(""),
            Line::from(Span::styled(
                "y confirm · n/Esc decline",
                Style::default().add_modifier(Modifier::DIM),
            )),
        ])
        .collect();
    let height = (lines.len() as u16 + 2).min(area.height.saturating_sub(4));
    let popup = layout::popup_area(area, 48, height);
    f.render_widget(Clear, popup);
    f.render_widget(
        Paragraph::new(lines)
            .block(panel_block(" CONFIRM ", true))
            .wrap(Wrap { trim: false }),
        popup,
    );
}
