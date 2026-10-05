//! Tunnel traffic sampling from interface byte counters.
//!
//! The tunnel has no management API — throughput comes from the kernel's
//! per-interface counters via `netstat -I <iface> -ibdn`, sampled on each
//! status tick; the caller diffs consecutive samples into a rate.

use std::process::Stdio;

/// Read cumulative (in_bytes, out_bytes) counters for an interface.
/// In = traffic arriving from the tunnel (download), out = sent into it.
pub async fn interface_bytes(iface: &str) -> Option<(u64, u64)> {
    let out = tokio::process::Command::new("/usr/sbin/netstat")
        .args(["-ibdn", "-I"])
        .arg(iface)
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
    parse_netstat(&String::from_utf8_lossy(&out.stdout))
}

/// Parse `netstat -ibdn -I <iface>` output: skip the header, take the first
/// data row, read Ibytes/Obytes (pure; tested).
pub fn parse_netstat(output: &str) -> Option<(u64, u64)> {
    let data_line = output.lines().skip(1).find(|l| !l.trim().is_empty())?;
    let fields: Vec<&str> = data_line.split_whitespace().collect();
    // Name Mtu Network Address Ipkts Ierrs Ibytes Opkts Oerrs Obytes Coll Drop
    let in_bytes = fields.get(5)?.parse::<u64>().ok()?;
    let out_bytes = fields.get(8)?.parse::<u64>().ok()?;
    Some((in_bytes, out_bytes))
}

/// Human-readable rate, e.g. "1.2 MB/s", "340 KB/s", "512 B/s".
pub fn fmt_rate(bytes_per_sec: f64) -> String {
    let bps = bytes_per_sec;
    if bps >= 1024.0 * 1024.0 {
        format!("{:.1} MB/s", bps / (1024.0 * 1024.0))
    } else if bps >= 1024.0 {
        format!("{:.0} KB/s", bps / 1024.0)
    } else {
        format!("{:.0} B/s", bps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_counters() {
        let out = "Name       Mtu   Network       Address            Ipkts Ierrs     Ibytes    Opkts Oerrs     Obytes  Coll Drop\nutun7      1400  <Link#26>                         9250     0    4742392     4625     0    2444994     0   0\nutun7      1400  172.18/30     172.18.0.1          9250     -    4742392     4625     -    2444994     -   -\n";
        assert_eq!(parse_netstat(out), Some((4_742_392, 2_444_994)));
        assert_eq!(parse_netstat("Name Mtu\n"), None);
        assert_eq!(parse_netstat(""), None);
    }

    #[test]
    fn formats_rates() {
        assert_eq!(fmt_rate(512.0), "512 B/s");
        assert_eq!(fmt_rate(348_160.0), "340 KB/s");
        assert_eq!(fmt_rate(1_258_291.2), "1.2 MB/s");
    }
}
