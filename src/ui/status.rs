//! Status panel (T015, T017, T036; FR-002–FR-007).
//!
//! Fixed region, refreshed in place — information never scrolls or moves.
//! Shows lifecycle state, pid, uptime, version, config path, tunnel, active
//! vs idle outbounds, DNS, and the health badge with the reason for anything
//! not Healthy. Stopped means "not measured" for connectivity (US1 scenario
//! 5); the unprivileged banner names the needed action (FR-020a).

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::App;
use crate::service::{HealthStatus, LifecycleState};

/// How many rows the Status panel needs for its current content (used by the
/// layout before rendering; keep in sync with `build_lines`).
pub fn content_height(app: &App) -> u16 {
    let mut lines = 6; // state/config/tunnel/outbound/dns/health
    if !app.elevated {
        lines += 1;
    }
    lines += app
        .action_error
        .as_ref()
        .map(|e| e.lines().count().min(3) as u16)
        .unwrap_or(0);
    lines += u16::from(app.action_note.is_some());
    lines + 2 // borders
}

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let lines = build_lines(app);
    let paragraph = Paragraph::new(lines).block(crate::ui::panel_block(
        "STATUS",
        app.focused == crate::app::Panel::Status,
    ));
    f.render_widget(paragraph, area);
}

fn label(text: &str) -> Span<'static> {
    Span::styled(
        format!("{text}: "),
        Style::default().add_modifier(Modifier::BOLD),
    )
}

fn dim(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), Style::default().add_modifier(Modifier::DIM))
}

fn good(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), Style::default().fg(Color::Green))
}

fn bad(text: impl Into<String>) -> Span<'static> {
    Span::styled(
        text.into(),
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
    )
}

fn warn(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), Style::default().fg(Color::Yellow))
}

fn build_lines(app: &App) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let s = &app.service;

    // Unprivileged banner: name the needed action (FR-020a, SC-013).
    if !app.elevated {
        lines.push(Line::from(warn(
            "unprivileged — start/stop/restart unavailable; relaunch: sudo sing-box-tui",
        )));
    }

    // L1: state, pid, uptime (stale when stopped), version, not-responding.
    let mut l1 = vec![label("State")];
    match s.state {
        LifecycleState::Running => l1.push(good(s.state.as_str().to_string())),
        LifecycleState::Stopped => l1.push(bad(s.state.as_str().to_string())),
        _ => l1.push(warn(s.state.as_str().to_string())),
    }
    let mut extras: Vec<String> = Vec::new();
    if let Some(pid) = s.pid {
        extras.push(format!("pid {pid}"));
    }
    if let Some(up) = s.uptime_secs() {
        extras.push(format!(
            "up {}{}",
            format_duration(up),
            if s.uptime_stale {
                " (no longer current)"
            } else {
                ""
            }
        ));
    }
    if let Some(v) = &app.version {
        extras.push(v.clone());
    }
    if !extras.is_empty() {
        l1.push(Span::raw(format!(" ({})", extras.join(", "))));
    }
    if s.not_responding {
        l1.push(bad(" NOT RESPONDING"));
    }
    lines.push(Line::from(l1));

    // L2: config path + validation summary + change flag.
    let mut l2 = vec![label("Config")];
    match &app.config_path {
        Some(p) => l2.push(Span::raw(truncate_path(&p.to_string_lossy(), 46))),
        None => l2.push(dim("unresolved")),
    }
    if let Some(_err) = &app.config_error {
        l2.push(bad(" unreadable"));
    } else if app.config_changed {
        l2.push(warn(" CHANGED — 'l' to reload"));
    } else if let Some(v) = &app.validation {
        if v.ok {
            l2.push(dim(format!(" · {}", v.summary())));
        } else {
            l2.push(bad(" · invalid"));
        }
    } else if app.validating {
        l2.push(dim(" · checking…"));
    }
    lines.push(Line::from(l2));

    // L3: tunnel — absent interface and missing address are distinct (FR-003).
    let mut l3 = vec![label("Tunnel")];
    if s.state == LifecycleState::Stopped {
        l3.push(dim("— (service stopped)"));
    } else {
        match &s.tunnel {
            Some(t) => {
                let mut parts = vec![Span::raw(t.name.clone())];
                match &t.address {
                    Some(a) => parts.push(Span::raw(format!(" {a}"))),
                    None => parts.push(warn(" no address (degraded)")),
                }
                if let Some(mtu) = t.mtu {
                    parts.push(dim(format!(" mtu {mtu}")));
                }
                l3.extend(parts);
            }
            None => l3.push(warn("not found (degraded)")),
        }
    }
    lines.push(Line::from(l3));

    // L4: outbounds — active vs idle with type + destination (FR-004).
    let mut l4 = vec![label("Out")];
    if let Some(cfg) = &app.config {
        let active = s.active_outbound.as_deref().unwrap_or_default();
        let shown: Vec<String> = cfg
            .outbounds
            .iter()
            .map(|o| {
                let dest = o.destination();
                let base = if dest.is_empty() {
                    format!("{} ({})", o.tag, o.ty)
                } else {
                    format!("{} ({} → {})", o.tag, o.ty, dest)
                };
                if o.tag == active && !active.is_empty() {
                    format!("{base} ●")
                } else {
                    base
                }
            })
            .collect();
        l4.push(Span::raw(join_fit(&shown, 60)));
    } else {
        l4.push(dim("config not loaded"));
    }
    lines.push(Line::from(l4));

    // L5: DNS servers + strategy (FR-005).
    let mut l5 = vec![label("DNS")];
    if let Some(cfg) = &app.config {
        let servers: Vec<String> = cfg
            .dns
            .servers
            .iter()
            .map(|srv| format!("{}:{}", srv.tag, srv.display()))
            .collect();
        let mut text = join_fit(&servers, 46);
        if !cfg.dns.strategy.is_empty() {
            text.push_str(&format!(" strategy {}", cfg.dns.strategy));
        }
        if !cfg.dns.final_.is_empty() {
            text.push_str(&format!(" final {}", cfg.dns.final_));
        }
        l5.push(Span::raw(text));
    } else {
        l5.push(dim("—"));
    }
    lines.push(Line::from(l5));

    // L6: health badge + reason on the same screen (FR-007, SC-009).
    let mut l6 = vec![label("Health")];
    match s.health {
        HealthStatus::Healthy => l6.push(good("Healthy")),
        HealthStatus::Degraded => l6.push(warn("Degraded")),
        HealthStatus::Down => l6.push(bad("Down")),
    }
    if s.state == LifecycleState::Stopped {
        l6.push(dim(" — connectivity not measured while stopped"));
    } else if let Some(reason) = &s.health_reason {
        l6.push(Span::raw(format!(" — {reason}")));
    }
    lines.push(Line::from(l6));

    // Action note / verbatim action error (FR-020, SC-008: exact text).
    if let Some(err) = &app.action_error {
        for line in err.lines().take(3) {
            lines.push(Line::from(bad(line.to_string())));
        }
    } else if let Some(note) = &app.action_note {
        lines.push(Line::from(dim(note.clone())));
    }

    lines
}

/// Join items with " | ", trimming to `fit` chars.
fn join_fit(items: &[String], fit: usize) -> String {
    let mut out = String::new();
    for item in items {
        let sep = if out.is_empty() { "" } else { " | " };
        if out.len() + sep.len() + item.len() > fit {
            if out.is_empty() {
                out.push_str(&item[..fit.min(item.len())]);
            } else {
                out.push_str(" | …");
            }
            break;
        }
        out.push_str(sep);
        out.push_str(item);
    }
    out
}

/// Keep the tail of a path (the filename plus enough parent context).
fn truncate_path(path: &str, fit: usize) -> String {
    if path.len() <= fit {
        path.to_string()
    } else {
        format!("…{}", &path[path.len() - fit + 1..])
    }
}

fn format_duration(secs: u64) -> String {
    let hours = secs / 3600;
    let mins = (secs % 3600) / 60;
    let secs = secs % 60;
    if hours > 0 {
        format!("{hours}h {mins}m {secs}s")
    } else if mins > 0 {
        format!("{mins}m {secs}s")
    } else {
        format!("{secs}s")
    }
}
