//! Log panel (T038; FR-018, FR-024).
//!
//! Recent lines with the level visible, new lines appended as produced, `p`
//! pauses this panel only. Lines render compactly — dim `HH:MM:SS`, a
//! one-letter colored level, then the message — so connection addresses fit
//! in a third-width panel. An externally started instance has no captured
//! stream: the panel says so instead of showing nothing.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::App;
use crate::logging::LogEntry;
use crate::service::LifecycleState;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let placeholder = build_placeholder(app);
    // The log view scrolls from the end: offset 0 follows the newest line.
    let view = area.height.saturating_sub(2) as usize;
    let max_offset = app.logs.len().saturating_sub(1) as u16;
    let offset = app.log_offset_from_end.min(max_offset);
    let end = app.logs.len().saturating_sub(offset as usize);
    let start = end.saturating_sub(view);
    let visible: Vec<Line> = app
        .logs
        .iter()
        .skip(start)
        .take(end - start)
        .map(entry_line)
        .collect();

    let paragraph = Paragraph::new(if visible.is_empty() {
        placeholder
    } else {
        visible
    })
    .block(crate::ui::panel_block(
        "LOG",
        app.focused == crate::app::Panel::Log,
    ));
    f.render_widget(paragraph, area);
}

/// One compact line: `19:57:45 i inbound/tun: accepted … tcp:ya.ru:443`.
fn entry_line(entry: &LogEntry) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::with_capacity(5);
    if let Some(time) = &entry.time {
        spans.push(Span::styled(
            format!("{time} "),
            Style::default().add_modifier(Modifier::DIM),
        ));
    }
    let (letter, style) = level_glyph(&entry.level);
    spans.push(Span::styled(letter, style));
    spans.push(Span::raw(" "));
    spans.push(Span::raw(entry.message.clone()));
    Line::from(spans)
}

/// Colored one-letter level marker.
fn level_glyph(level: &str) -> (String, Style) {
    match level {
        "error" | "fatal" | "panic" => (
            "E".into(),
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ),
        "warn" => ("W".into(), Style::default().fg(Color::Yellow)),
        "info" => ("I".into(), Style::default().fg(Color::Green)),
        "debug" => ("D".into(), Style::default().fg(Color::Cyan)),
        "trace" => ("T".into(), Style::default().fg(Color::Cyan)),
        _ => ("·".into(), Style::default().add_modifier(Modifier::DIM)),
    }
}

fn build_placeholder(app: &App) -> Vec<Line<'static>> {
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
