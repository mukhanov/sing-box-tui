//! Tunnel interface discovery (T013).
//!
//! Prefer the config's tun `interface_name`; else the newest `utun*`
//! interface (highest-numbered — macOS assigns them incrementally).
//! Address and MTU come from `ifconfig`. An absent interface or a missing
//! address are distinct, reportable conditions (FR-003, spec Edge Cases).

use std::process::Stdio;

/// Tunnel interface details (data-model.md Tunnel).
#[derive(Debug, Clone, Default)]
pub struct TunnelInfo {
    /// Interface name (e.g., utun10).
    pub name: String,
    /// Assigned IP address (e.g., 172.18.0.1) — None is a distinct condition.
    pub address: Option<String>,
    /// MTU.
    pub mtu: Option<u32>,
}

/// Parsed address/MTU parts of `ifconfig <name>` output.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TunnelParts {
    pub address: Option<String>,
    pub mtu: Option<u32>,
}

/// Parse `ifconfig <name>` output into address/MTU parts (pure; tested).
pub fn parse_ifconfig(output: &str) -> TunnelParts {
    let mut parts = TunnelParts::default();
    for line in output.lines() {
        let l = line.trim();
        // "inet 172.18.0.1 netmask 0xfffffffc -> 172.18.0.2"
        if let Some(rest) = l.strip_prefix("inet ")
            && let Some(addr) = rest.split_whitespace().next()
        {
            // Skip IPv6 link-local lines ("inet6 ..."); "inet" is IPv4.
            if parts.address.is_none() {
                parts.address = Some(addr.to_string());
            }
        }
        // "mtu 1400" appears inside the status line.
        if let Some(idx) = l.find("mtu ")
            && let Some(mtu) = l[idx + 4..]
                .split_whitespace()
                .next()
                .and_then(|v| v.parse::<u32>().ok())
            && parts.mtu.is_none()
        {
            parts.mtu = Some(mtu);
        }
    }
    parts
}

/// Run `ifconfig <name>` and return its output, or None when the interface
/// does not exist.
async fn ifconfig(name: &str) -> Option<String> {
    let out = tokio::process::Command::new("/sbin/ifconfig")
        .arg(name)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Does the interface exist?
async fn interface_exists(name: &str) -> bool {
    ifconfig(name).await.is_some()
}

/// List `utun*` interface names via `ifconfig -l`, newest (highest N) first.
pub async fn list_utun() -> Vec<String> {
    let out = tokio::process::Command::new("/sbin/ifconfig")
        .arg("-l")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .output()
        .await;

    let mut names: Vec<String> = match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .split_whitespace()
            .filter(|n| n.starts_with("utun"))
            .map(str::to_string)
            .collect(),
        _ => Vec::new(),
    };
    names.sort_by_key(|n| n.trim_start_matches("utun").parse::<u32>().unwrap_or(0));
    names.reverse();
    names
}

/// Discover the tunnel: prefer `config_interface` when the interface exists,
/// else the newest utun (FR-003). A present interface with no address is
/// still returned — the missing address is a distinct degraded condition.
pub async fn discover(config_interface: Option<&str>) -> Option<TunnelInfo> {
    let name = match config_interface {
        Some(name) if !name.is_empty() && interface_exists(name).await => name.to_string(),
        _ => list_utun().await.into_iter().next()?,
    };

    let output = ifconfig(&name).await?;
    let parts = parse_ifconfig(&output);
    Some(TunnelInfo {
        name,
        address: parts.address,
        mtu: parts.mtu,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_address_and_mtu() {
        let out = "utun10: flags=8051<UP,POINTOPOINT,RUNNING,MULTICAST> mtu 1400\n\tinet 172.18.0.1 netmask 0xfffffffc -> 172.18.0.2\n";
        let parts = parse_ifconfig(out);
        assert_eq!(parts.address.as_deref(), Some("172.18.0.1"));
        assert_eq!(parts.mtu, Some(1400));
    }

    #[test]
    fn missing_address_is_distinct() {
        // Interface exists (flags/mtu present) but no inet line.
        let parts = parse_ifconfig("utun3: flags=8051<UP,POINTOPOINT,RUNNING> mtu 1380\n");
        assert_eq!(parts.address, None);
        assert_eq!(parts.mtu, Some(1380));
    }
}
