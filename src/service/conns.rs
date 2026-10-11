//! urltest member observation. A urltest group picks its member at runtime
//! (lowest latency, re-tested on an interval); the config alone cannot name
//! it and no management API is assumed. The honest signal available outside
//! sing-box is the process's own established TCP connections: the member
//! actually carrying traffic is the one whose server the sing-box process is
//! connected to. The group's own health probes open short-lived connections
//! to every member, so a single snapshot can show several — callers keep a
//! sticky pick and confirm switches across consecutive observations.

use std::collections::HashSet;
use std::net::IpAddr;
use std::process::Stdio;

/// One member of a urltest group as an observable endpoint:
/// (tag, server host, server port).
pub type MemberEndpoint = (String, String, u16);

/// Remote endpoints of the process's established TCP connections:
/// `lsof -nP -i TCP -a -p <pid>` parsed into (remote ip, remote port).
pub async fn established_endpoints(pid: u32) -> anyhow::Result<Vec<(IpAddr, u16)>> {
    let out = tokio::process::Command::new("lsof")
        .args(["-nP", "-i", "TCP", "-a", "-p", &pid.to_string()])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .await?;
    Ok(parse_lsof(&String::from_utf8_lossy(&out.stdout)))
}

/// Parse `lsof -nP -i TCP` output. Rows of interest name the connection as
/// `local->remote (ESTABLISHED)` — keep the remote endpoint only; LISTEN and
/// stateless rows are ignored.
pub fn parse_lsof(output: &str) -> Vec<(IpAddr, u16)> {
    let mut out = Vec::new();
    for line in output.lines() {
        let toks: Vec<&str> = line.split_whitespace().collect();
        let Some(idx) = toks.iter().position(|t| *t == "(ESTABLISHED)") else {
            continue;
        };
        // The NAME column sits right before the state; nothing to attribute
        // when it is missing.
        let Some(name) = idx.checked_sub(1).map(|i| toks[i]) else {
            continue;
        };
        // "laddr->raddr" (IPv6 raddr is bracketed); take the remote side.
        let remote = name.rsplit("->").next().unwrap_or(name);
        let remote = remote.trim_start_matches('[');
        // Split once from the right: everything before the last ':' is the
        // address.
        let Some((ip, port)) = remote.rsplit_once(':') else {
            continue;
        };
        let ip = ip.trim_end_matches(']');
        if let (Ok(ip), Ok(port)) = (ip.parse::<IpAddr>(), port.parse::<u16>()) {
            out.push((ip, port));
        }
    }
    out
}

/// Which members have at least one established connection right now.
/// Returns None when observation fails (lsof error, DNS failure) so the
/// caller keeps its sticky pick instead of mistaking "cannot see" for "idle".
pub async fn members_with_live_conns(pid: u32, members: &[MemberEndpoint]) -> Option<Vec<String>> {
    let endpoints = established_endpoints(pid).await.ok()?;
    let mut live = Vec::new();
    for (tag, host, port) in members {
        let addrs: HashSet<IpAddr> = tokio::net::lookup_host((host.as_str(), *port))
            .await
            .ok()?
            .map(|sa| sa.ip())
            .collect();
        if endpoints
            .iter()
            .any(|(ip, p)| *p == *port && addrs.contains(ip))
        {
            live.push(tag.clone());
        }
    }
    Some(live)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_established_remote_endpoints() {
        let out = "\
COMMAND   PID USER   FD   TYPE  DEVICE SIZE/OFF NODE NAME
sing-box 1234 root   18u  IPv4 0xabc      0t0 TCP 172.18.0.1:52344->91.2.3.4:8443 (ESTABLISHED)
sing-box 1234 root   19u  IPv6 0xabd      0t0 TCP [fd00::1]:523->[2001:db8::7]:443 (ESTABLISHED)
sing-box 1234 root   20u  IPv4 0xabe      0t0 TCP *:9090 (LISTEN)
";
        assert_eq!(
            parse_lsof(out),
            vec![
                ("91.2.3.4".parse::<IpAddr>().unwrap(), 8443),
                ("2001:db8::7".parse::<IpAddr>().unwrap(), 443),
            ]
        );
    }

    #[test]
    fn ignores_unstyled_and_unparseable_rows() {
        assert!(parse_lsof("COMMAND PID USER NODE NAME").is_empty());
        assert!(parse_lsof("x x x TCP *:9090 (LISTEN)").is_empty());
        assert!(parse_lsof("x x x TCP not-an-addr (ESTABLISHED)").is_empty());
    }
}
