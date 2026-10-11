//! Read-only configuration module.
//!
//! Provides:
//! - `resolve_config_path()` — path resolution against the invoking user's
//!   home via `SUDO_USER`, never the elevated account's home (FR-020b, FR-025)
//! - `active_outbound()` — outbound currently handling traffic (FR-004)
//! - `parse` / `validate` / `mask` / `watch` submodules

pub mod mask;
pub mod parse;
pub mod validate;
pub mod vless;
pub mod watch;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use parse::{Outbound, SingBoxConfig};

/// Home directory of the invoking user (FR-020b).
///
/// Under sudo, `SUDO_USER` names the invoking user: resolve *their* home via
/// `dscl` and never touch root's. Outside sudo, `$HOME` is correct as-is.
fn invoking_user_home() -> Option<PathBuf> {
    if let Ok(sudo_user) = std::env::var("SUDO_USER")
        && !sudo_user.is_empty()
        && sudo_user != "root"
    {
        // dscl: the authoritative source for a macOS user's home.
        if let Ok(out) = Command::new("/usr/bin/dscl")
            .args([
                ".",
                "-read",
                &format!("/Users/{sudo_user}"),
                "NFSHomeDirectory",
            ])
            .stdin(Stdio::null())
            .output()
            && out.status.success()
        {
            let text = String::from_utf8_lossy(&out.stdout);
            if let Some(home) = text.split_whitespace().nth(1) {
                return Some(PathBuf::from(home.to_string()));
            }
        }
        // Conventional fallback: /Users/<name>.
        return Some(PathBuf::from(format!("/Users/{sudo_user}")));
    }
    std::env::var("HOME").ok().map(PathBuf::from)
}

/// Resolve `~/.config/sing-box/config.json` against the invoking user's home.
pub fn resolve_config_path() -> PathBuf {
    invoking_user_home()
        .unwrap_or_else(|| PathBuf::from("/"))
        .join(".config")
        .join("sing-box")
        .join("config.json")
}

/// The outbound currently handling traffic, derived from `route.final` and
/// selector defaults (FR-004). Falls back to the first non-direct outbound,
/// then `direct` — matching sing-box's own default resolution order.
pub fn active_outbound(config: &SingBoxConfig) -> Option<&Outbound> {
    // 1. route.final names the default outbound.
    let final_tag = config.route.final_.trim();
    let resolved = if final_tag.is_empty() {
        // sing-box defaults final to the first outbound.
        config.outbounds.first()
    } else {
        config.outbounds.iter().find(|o| o.tag == final_tag)
    };

    // 2. A selector resolves to its default (or first) member.
    if let Some(out) = resolved
        && out.ty == "selector"
    {
        let chosen = out
            .default
            .as_deref()
            .filter(|d| !d.is_empty())
            .and_then(|d| config.outbounds.iter().find(|o| o.tag == d))
            .or_else(|| {
                out.outbounds
                    .first()
                    .and_then(|t| config.outbounds.iter().find(|o| o.tag == *t))
            });
        return chosen.or(Some(out));
    }
    resolved
}

/// The urltest group acting as the active outbound, when it is one. A
/// urltest is a purely technical construct: it picks its member at runtime,
/// so display layers resolve to the member instead (observed live by the
/// app; see `service::conns`).
pub fn urltest_group(config: &SingBoxConfig) -> Option<&Outbound> {
    let active = active_outbound(config)?;
    (active.ty == "urltest").then_some(active)
}

/// The concrete (server-bearing) members of a group outbound, in the order
/// the group lists them.
pub fn group_members<'a>(config: &'a SingBoxConfig, group: &Outbound) -> Vec<&'a Outbound> {
    group
        .outbounds
        .iter()
        .filter_map(|tag| config.outbounds.iter().find(|o| &o.tag == tag))
        .filter(|o| !o.server.is_empty() && o.server_port.is_some())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_outbound_resolves_final_and_selectors() {
        let cfg: SingBoxConfig = serde_json::from_str(
            r#"{
          "outbounds": [
            {"type":"selector","tag":"select","outbounds":["proxy","direct"],"default":"direct"},
            {"type":"vless","tag":"proxy","server":"s.example"},
            {"type":"direct","tag":"direct"}
          ],
          "route": {"final": "select", "rules": []}
        }"#,
        )
        .unwrap();
        assert_eq!(
            active_outbound(&cfg).map(|o| o.tag.as_str()),
            Some("direct")
        );

        // No final → first outbound (the selector) with no default → its first member.
        let cfg2: SingBoxConfig = serde_json::from_str(
            r#"{
          "outbounds": [
            {"type":"selector","tag":"select","outbounds":["proxy","direct"]},
            {"type":"vless","tag":"proxy","server":"s.example"},
            {"type":"direct","tag":"direct"}
          ],
          "route": {"rules": []}
        }"#,
        )
        .unwrap();
        assert_eq!(
            active_outbound(&cfg2).map(|o| o.tag.as_str()),
            Some("proxy")
        );
    }

    #[test]
    fn urltest_group_and_members_resolve() {
        let cfg: SingBoxConfig = serde_json::from_str(
            r#"{
          "outbounds": [
            {"type":"naive","tag":"naive-a","server":"a.example","server_port":8443},
            {"type":"naive","tag":"naive-b","server":"b.example","server_port":8443},
            {"type":"urltest","tag":"proxy","outbounds":["naive-a","naive-b","direct"]},
            {"type":"direct","tag":"direct"}
          ],
          "route": {"final": "proxy", "rules": []}
        }"#,
        )
        .unwrap();
        let group = urltest_group(&cfg).expect("urltest final is the group");
        assert_eq!(group.tag, "proxy");
        // Concrete members only, in group order — `direct` carries no server.
        let members = group_members(&cfg, group);
        assert_eq!(
            members.iter().map(|o| o.tag.as_str()).collect::<Vec<_>>(),
            vec!["naive-a", "naive-b"]
        );

        // A non-group active outbound is not a urltest group.
        let plain: SingBoxConfig = serde_json::from_str(
            r#"{"outbounds":[{"type":"vless","tag":"proxy","server":"s"}],"route":{"final":"proxy"}}"#,
        )
        .unwrap();
        assert!(urltest_group(&plain).is_none());
    }
}
