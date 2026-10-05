//! Ping panel (T025; FR-008–FR-012).
//!
//! Per-target rows with stats + inferred route, the measured-address result
//! area, cancel key C, custom-target input (opened via `t`, rendered as an
//! overlay by ui::draw). When the service is stopped, results are still
//! produced but the panel clearly says the tunnel is down so the numbers
//! must not be read as VPN results (US2 scenario 9); stale numbers read as
//! "not measured" (US1 scenario 5).

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::App;
use crate::service::{CheckStatus, FailureReason, LifecycleState};
use ratatui::Frame;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let lines = build_lines(app);
    let scroll = clamp_scroll(
        app.scroll_ping,
        lines.len(),
        area.height.saturating_sub(2) as usize,
    );
    let paragraph = Paragraph::new(lines)
        .block(crate::ui::panel_block(
            "PING",
            app.focused == crate::app::Panel::Ping,
        ))
        .scroll((scroll, 0));
    f.render_widget(paragraph, area);
}

fn dim(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), Style::default().add_modifier(Modifier::DIM))
}

fn build_lines(app: &App) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let stopped = app.service.state == LifecycleState::Stopped;

    if stopped {
        lines.push(Line::from(Span::styled(
            "tunnel DOWN — results are NOT via VPN",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )));
    }

    if app.checks.is_empty() {
        lines.push(Line::from(dim("press 'c' to run connectivity test")));
        lines.push(Line::from(dim("'t' adds a custom target")));
    } else {
        for check in &app.checks {
            let glyph = match check.status {
                CheckStatus::Complete => Span::styled("✓", Style::default().fg(Color::Green)),
                CheckStatus::Failed => Span::styled(
                    "✗",
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                CheckStatus::Running => Span::styled("…", Style::default().fg(Color::Yellow)),
                CheckStatus::Cancelled => Span::styled("⨯", Style::default().fg(Color::Yellow)),
                CheckStatus::Interrupted => Span::styled("⏹", Style::default().fg(Color::Yellow)),
                CheckStatus::Pending => dim("·"),
            };
            lines.push(Line::from(vec![
                glyph,
                Span::raw(format!(" {} ", check.target)),
                dim(format!("({})", check.inferred_route)),
            ]));

            // Detail line: stats, failure reason, or not-measured.
            let detail = match check.status {
                CheckStatus::Running => format!("   sent {} recv {}", check.sent, check.received),
                _ if stopped && check.failure_reason.is_none() && check.received > 0 => {
                    "   not measured (service stopped)".to_string()
                }
                _ if stopped && check.received == 0 && check.status != CheckStatus::Complete => {
                    "   not measured (service stopped)".to_string()
                }
                _ if check.received > 0 && check.latency_max_ms == 0 => format!(
                    "   {}/{} pkts {}% loss sub-ms (avg {:.2} ms)",
                    check.received,
                    check.sent,
                    check.loss.round(),
                    check.latency_avg_ms
                ),
                _ if check.received > 0 => format!(
                    "   {}/{} pkts {}% loss {}/{}/{} ms",
                    check.received,
                    check.sent,
                    check.loss.round(),
                    check.latency_min_ms,
                    check.latency_avg_ms.round(),
                    check.latency_max_ms
                ),
                _ => {
                    let reason = check
                        .failure_reason
                        .map_or("no reply".to_string(), |r| match r {
                            FailureReason::NoReply => format!(
                                "no reply ({} sent, {}% loss)",
                                check.sent,
                                check.loss.round()
                            ),
                            other => other.as_str().to_string(),
                        });
                    format!("   {reason}")
                }
            };
            let detail_style = if check.status == CheckStatus::Failed {
                Span::styled(detail, Style::default().fg(Color::Red))
            } else {
                dim(detail)
            };
            lines.push(Line::from(detail_style));
        }
    }

    // Measured address check area (FR-010a).
    if app.measuring {
        lines.push(Line::from(dim("address: measuring…")));
    } else if let Some(m) = &app.measured {
        lines.push(Line::from(dim("address:")));
        match (&m.through_tunnel, &m.through_tunnel_reason) {
            (Some(ip), _) => lines.push(Line::from(format!("  via tunnel {ip}"))),
            (None, Some(r)) => lines.push(Line::from(vec![
                dim("  via tunnel "),
                dim(format!("— {r}")),
            ])),
            (None, None) => lines.push(Line::from(dim("  via tunnel —"))),
        }
        match (&m.direct, &m.direct_reason) {
            (Some(ip), _) => lines.push(Line::from(format!("  direct     {ip}"))),
            (None, Some(r)) => lines.push(Line::from(vec![
                dim("  direct     "),
                dim(format!("— {r}")),
            ])),
            (None, None) => lines.push(Line::from(dim("  direct     —"))),
        }
        match m.differs() {
            Some(true) => lines.push(Line::from(Span::styled(
                "  differ: tunnel in use",
                Style::default().fg(Color::Green),
            ))),
            Some(false) => lines.push(Line::from(Span::styled(
                "  same: traffic NOT via tunnel",
                Style::default().fg(Color::Yellow),
            ))),
            None => {}
        }
    } else {
        lines.push(Line::from(dim("'m' measures source address")));
    }

    lines.push(Line::from(dim("c test · C cancel · t target · m addr")));
    lines
}

fn clamp_scroll(scroll: u16, content: usize, view: usize) -> u16 {
    let max = content.saturating_sub(view.max(1)) as u16;
    scroll.min(max)
}
