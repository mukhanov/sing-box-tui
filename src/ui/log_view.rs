//! Log panel (T038; FR-018, FR-024).
//!
//! Recent lines with the level visible, new lines appended as produced, `p`
//! pauses this panel only. An externally started instance has no captured
//! stream — the panel says so instead of showing nothing.

use crate::app::App;
use crate::service::LifecycleState;
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let lines = build_lines(app);
    // The log view scrolls from the end: offset 0 follows the newest line.
    let view = area.height.saturating_sub(2) as usize;
    let max_offset = app.logs.len().saturating_sub(1) as u16;
    let offset = app.log_offset_from_end.min(max_offset);
    // Render the window ending `offset` lines before the newest.
    let end = app.logs.len().saturating_sub(offset as usize);
    let start = end.saturating_sub(view);
    let visible: Vec<Line> = app
        .logs
        .iter()
        .skip(start)
        .take(end - start)
        .map(entry_line)
        .collect();

    let paragraph = Paragraph::new(if visible.is_empty() { lines } else { visible }).block(
        crate::ui::panel_block("LOG", app.focused == crate::app::Panel::Log),
    );
    f.render_widget(paragraph, area);
}

fn entry_line(entry: &crate::logging::LogEntry) -> Line<'static> {
    let level = entry.level.as_str();
    let style = match level {
        "error" | "fatal" | "panic" => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        "warn" => Style::default().fg(Color::Yellow),
        "info" => Style::default().fg(Color::Green),
        "debug" | "trace" => Style::default().fg(Color::Cyan),
        _ => Style::default().add_modifier(Modifier::DIM),
    };
    Line::from(vec![
        Span::styled(format!("{level:5} "), style),
        Span::raw(entry.message.clone()),
    ])
}

fn build_lines(app: &App) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    if app.log_paused {
        lines.push(Line::from(Span::styled(
            "[PAUSED] 'p' resumes",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
    }
    if app.service.state == LifecycleState::Running
        && !app.service.owned_by_app
        && app.logs.is_empty()
    {
        lines.push(Line::from(Span::raw(
            "service started externally —\nno captured output available",
        )));
    } else if app.logs.is_empty() {
        lines.push(Line::from(Span::styled(
            "no log output yet",
            Style::default().add_modifier(Modifier::DIM),
        )));
    }
    lines
}
