//! Measured address check (T024, FR-010a, SC-015): on demand, fetch the
//! apparent source address through the tunnel and without it, side by side,
//! via `/usr/bin/curl`, answering within 5 s.
//!
//! "Reference unavailable" is distinguished from a tunnel failure and never
//! reported as the tunnel failing (FR-011, spec Edge Cases): when the echo
//! reference cannot be reached even directly, the tunnel measurement is
//! reported as reference-unavailable, not as a tunnel fault.

use std::process::Stdio;

/// Result of a measured address check (data-model.md ObservedAddress).
#[derive(Debug, Clone, Default)]
pub struct MeasuredAddress {
    /// Address traffic appears from when sent through the tunnel.
    pub through_tunnel: Option<String>,
    /// Reason when the tunnel measurement is unavailable.
    pub through_tunnel_reason: Option<String>,
    /// Address traffic appears from when sent without the tunnel.
    pub direct: Option<String>,
    /// Reason when the direct measurement is unavailable.
    pub direct_reason: Option<String>,
}

impl MeasuredAddress {
    /// Do the two measurements differ? (The point of the check.)
    pub fn differs(&self) -> Option<bool> {
        match (&self.through_tunnel, &self.direct) {
            (Some(a), Some(b)) => Some(a != b),
            _ => None,
        }
    }
}

const ECHO_URL: &str = "https://api.ipify.org";

/// Run the measured address check. Both fetches run concurrently so the
/// whole check answers within the 5 s curl timeout (SC-015).
pub async fn measure(tunnel_name: Option<&str>) -> MeasuredAddress {
    let default_if = default_interface().await;

    let (tunnel_res, direct_res) = tokio::join!(
        async {
            match tunnel_name {
                // Tunnel down is a distinct, expected condition — not an error.
                Some(name) => fetch_ip(Some(name)).await,
                None => Err("tunnel not active".to_string()),
            }
        },
        async {
            match default_if.as_deref() {
                Some(iface) if tunnel_name.is_some() => fetch_ip(Some(iface)).await,
                // Without a tunnel, a plain fetch IS the direct measurement.
                _ => fetch_ip(None).await,
            }
        }
    );

    let mut result = MeasuredAddress {
        through_tunnel: tunnel_res.clone().ok(),
        direct: direct_res.clone().ok(),
        ..MeasuredAddress::default()
    };

    // Reference availability: if the direct fetch also failed, the echo
    // reference itself is unreachable — never blame the tunnel (FR-010a).
    match (tunnel_res, direct_res) {
        (Err(te), Err(_)) => {
            result.through_tunnel_reason = Some("reference unavailable".to_string());
            result.direct_reason = Some(te);
        }
        (Err(te), Ok(_)) => {
            result.through_tunnel_reason = Some(te);
            result.direct_reason = None;
        }
        (Ok(_), Err(de)) => {
            result.through_tunnel_reason = None;
            result.direct_reason = Some(de);
        }
        (Ok(_), Ok(_)) => {}
    }
    result
}

/// Fetch the apparent public IP, optionally bound to an interface.
async fn fetch_ip(interface: Option<&str>) -> Result<String, String> {
    let mut cmd = tokio::process::Command::new("/usr/bin/curl");
    cmd.args(["-s", "--max-time", "5"]);
    if let Some(iface) = interface {
        cmd.args(["--interface", iface]);
    }
    let output = cmd
        .arg(ECHO_URL)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| format!("curl failed to start: {e}"))?;

    let ip = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if output.status.success() && !ip.is_empty() && ip.contains('.') || ip.contains(':') {
        // Sanity: an echo response looks like an address, not an error page.
        if ip.len() <= 45 && !ip.contains(' ') {
            return Ok(ip);
        }
    }
    Err(format!("no reply from {ECHO_URL}"))
}

/// The default outbound physical interface (`route -n get default`).
async fn default_interface() -> Option<String> {
    let out = tokio::process::Command::new("/sbin/route")
        .args(["-n", "get", "default"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .find_map(|l| l.trim().strip_prefix("interface:").map(str::trim))
        .filter(|i| !i.is_empty())
        .map(str::to_string)
}
