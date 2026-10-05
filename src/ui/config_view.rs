//! Config panel (T029, T030, T032; FR-013a, FR-014–FR-017).
//!
//! Grouped, labelled name/value entries covering general/log, inbounds,
//! outbounds, DNS, and routing rules in evaluation order with the
//! final/default outcome. Internal scrolling only — the panel never pushes
//! other panels off screen. Credentials masked by default (FR-015); `v`
//! reveals deliberately. A missing/unreadable config names the path tried
//! and the error, never an empty panel (spec Edge Cases).

use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::App;
use crate::config::mask::{MASK, is_credential_key};
use crate::config::parse::RouteRule;
use ratatui::Frame;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let lines = build_lines(app);
    let scroll = clamp_scroll(
        app.scroll_config,
        lines.len(),
        area.height.saturating_sub(2) as usize,
    );
    let paragraph = Paragraph::new(lines)
        .block(crate::ui::panel_block(
            "CONFIG",
            app.focused == crate::app::Panel::Config,
        ))
        .scroll((scroll, 0));
    f.render_widget(paragraph, area);
}

fn head(text: &str) -> Span<'static> {
    Span::styled(
        format!("{text}:"),
        Style::default().add_modifier(Modifier::BOLD),
    )
}

fn dim(text: impl Into<String>) -> Span<'static> {
    Span::styled(text.into(), Style::default().add_modifier(Modifier::DIM))
}

fn entry(key: &str, value: String, visible: bool) -> Line<'static> {
    let value_span = if is_credential_key(key) && !visible {
        Span::styled(MASK.to_string(), Style::default().fg(Color::Magenta))
    } else {
        Span::raw(value)
    };
    Line::from(vec![dim(format!("  {key}: ")), value_span])
}

fn build_lines(app: &App) -> Vec<Line<'static>> {
    let mut lines: Vec<Line<'static>> = Vec::new();
    let visible = app.credentials_visible;

    // Missing/unreadable config: name the path tried and the error (T032).
    if let Some(err) = &app.config_error {
        lines.push(Line::from(Span::styled(
            "CONFIG UNAVAILABLE",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )));
        for l in err.lines() {
            lines.push(Line::from(Span::raw(l.to_string())));
        }
        lines.push(Line::from(dim(
            "the service is not shown healthy on this config",
        )));
        return lines;
    }

    let Some(cfg) = &app.config else {
        lines.push(Line::from(dim("config not loaded")));
        return lines;
    };

    // File line: path + validation + change state (FR-016, FR-017).
    if let Some(p) = &app.config_path {
        lines.push(Line::from(vec![
            dim("file: "),
            Span::raw(p.to_string_lossy().to_string()),
        ]));
    }
    let validation = match (&app.validation, app.validating) {
        (_, true) => dim("  validation: checking…".to_string()),
        (Some(v), _) => {
            if v.ok {
                dim(format!("  validation: {}", v.summary()))
            } else {
                Span::styled(
                    format!("  validation: {}", v.summary()),
                    Style::default().fg(Color::Red),
                )
            }
        }
        (None, false) => dim("  validation: not run".to_string()),
    };
    lines.push(Line::from(validation));
    if app.config_changed {
        lines.push(Line::from(Span::styled(
            "  CHANGED on disk — 'l' to reload",
            Style::default().fg(Color::Yellow),
        )));
    }

    // General / log.
    lines.push(Line::from(head("log")));
    lines.push(entry("level", cfg.log.level.clone(), visible));
    lines.push(entry("timestamp", cfg.log.timestamp.to_string(), visible));
    for (k, v) in &cfg.log.extra {
        lines.push(entry(k, render_value(v, visible), visible));
    }

    // Inbounds (tun decoded; others raw).
    lines.push(Line::from(""));
    lines.push(Line::from(head("inbounds")));
    if cfg.inbounds.is_empty() {
        lines.push(Line::from(dim("  (none)")));
    }
    for inbound in &cfg.inbounds {
        let ty = inbound.get("type").and_then(|t| t.as_str()).unwrap_or("?");
        let tag = inbound.get("tag").and_then(|t| t.as_str()).unwrap_or("");
        lines.push(Line::from(format!("  {ty} {tag}")));
        if ty == "tun" {
            if let Some(tun) = cfg.tun_inbound() {
                if let Some(name) = &tun.interface_name {
                    lines.push(entry("interface_name", name.clone(), visible));
                }
                lines.push(entry("address", tun.address.join(", "), visible));
                lines.push(entry(
                    "mtu",
                    tun.mtu.map(|m| m.to_string()).unwrap_or_default(),
                    visible,
                ));
                for (k, v) in &tun.extra {
                    lines.push(entry(k, render_value(v, visible), visible));
                }
            }
        } else {
            for (k, v) in inbound.as_object().into_iter().flatten() {
                if k != "type" && k != "tag" {
                    lines.push(entry(k, render_value(v, visible), visible));
                }
            }
        }
    }

    // Outbounds.
    lines.push(Line::from(""));
    lines.push(Line::from(head("outbounds")));
    for o in &cfg.outbounds {
        let dest = o.destination();
        let dest_str = if dest.is_empty() {
            String::new()
        } else {
            format!(" → {dest}")
        };
        lines.push(Line::from(vec![
            dim("  - "),
            Span::raw(format!("{} ({}{})", o.tag, o.ty, dest_str)),
        ]));
        for (k, v) in &o.extra {
            lines.push(entry(k, render_value(v, visible), visible));
        }
    }

    // DNS.
    lines.push(Line::from(""));
    lines.push(Line::from(head("dns")));
    lines.push(entry("strategy", cfg.dns.strategy.clone(), visible));
    if !cfg.dns.final_.is_empty() {
        lines.push(entry("final", cfg.dns.final_.clone(), visible));
    }
    for srv in &cfg.dns.servers {
        lines.push(Line::from(vec![
            dim("  - "),
            Span::raw(format!("{} {} ({})", srv.tag, srv.display(), srv.ty)),
        ]));
        for (k, v) in &srv.extra {
            lines.push(entry(k, render_value(v, visible), visible));
        }
    }
    for (k, v) in &cfg.dns.extra {
        lines.push(entry(k, render_value(v, visible), visible));
    }

    // Routing rules in evaluation order + final (FR-014, T030).
    lines.push(Line::from(""));
    lines.push(Line::from(head("route rules (evaluation order)")));
    for (i, rule) in cfg.route.rules.iter().enumerate() {
        lines.push(Line::from(vec![
            dim(format!("  {}. ", i + 1)),
            Span::raw(format!("{} {}", conditions(rule), outcome(rule))),
        ]));
    }
    lines.push(Line::from(vec![
        dim("  [final] ".to_string()),
        Span::raw(cfg.route.final_.clone()),
    ]));
    for (k, v) in &cfg.route.extra {
        lines.push(entry(k, render_value(v, visible), visible));
    }

    // Unknown top-level sections, preserved for display (FR-028).
    if !cfg.extra.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(head("other")));
        for (k, v) in &cfg.extra {
            lines.push(entry(k, render_value(v, visible), visible));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(dim(format!(
        "'v' {} credentials",
        if visible { "mask" } else { "reveal" }
    ))));
    lines
}

/// Human-readable condition summary for one rule.
fn conditions(rule: &RouteRule) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !rule.domain.is_empty() {
        parts.push(format!("domain={}", join(&rule.domain)));
    }
    if !rule.domain_suffix.is_empty() {
        parts.push(format!("suffix={}", join(&rule.domain_suffix)));
    }
    if !rule.domain_keyword.is_empty() {
        parts.push(format!("keyword={}", join(&rule.domain_keyword)));
    }
    if !rule.ip_cidr.is_empty() {
        parts.push(format!("ip={}", join(&rule.ip_cidr)));
    }
    if rule.ip_is_private == Some(true) {
        parts.push("private_ip".to_string());
    }
    if let Some(p) = &rule.port
        && !p.is_null()
    {
        parts.push(format!("port={p}"));
    }
    if !rule.protocol.is_empty() {
        parts.push(format!("protocol={}", rule.protocol));
    }
    if !rule.network.is_empty() {
        parts.push(format!("network={}", rule.network));
    }
    if !rule.inbound.is_empty() {
        parts.push(format!("inbound={}", join(&rule.inbound)));
    }
    for (k, v) in &rule.extra {
        if !v.is_null() {
            parts.push(format!("{k}={v}"));
        }
    }
    if parts.is_empty() {
        "any".to_string()
    } else {
        parts.join(" & ")
    }
}

/// The rule's outcome: selected outbound or reject (FR-014).
fn outcome(rule: &RouteRule) -> String {
    let arrow = "→";
    if rule.action == "reject" {
        format!("{arrow} REJECT")
    } else if rule.action == "hijack-dns" {
        format!("{arrow} hijack-dns")
    } else if !rule.outbound.is_empty() {
        format!("{arrow} {}", rule.outbound)
    } else if !rule.action.is_empty() {
        rule.action.clone()
    } else {
        format!("{arrow} (unspecified)")
    }
}

fn join(items: &[String]) -> String {
    items.join(",")
}

/// Compact one-line JSON rendering for raw values.
fn compact(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => serde_json::to_string(other).unwrap_or_else(|_| other.to_string()),
    }
}

/// Render a raw JSON value one-line, masking any nested credentials unless
/// deliberately revealed (FR-015: 100% masked on first display).
fn render_value(v: &serde_json::Value, visible: bool) -> String {
    let masked = if visible {
        v.clone()
    } else {
        crate::config::mask::mask_json(v)
    };
    compact(&masked)
}

fn clamp_scroll(scroll: u16, content: usize, view: usize) -> u16 {
    let max = content.saturating_sub(view.max(1)) as u16;
    scroll.min(max)
}
