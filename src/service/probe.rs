//! Process probing (T012): find a running instance via `pgrep`/`ps`
//! (matching the `sing-box run` command line), read pid and start time for
//! uptime, and derive the config in use from the process command line
//! (FR-002, FR-025).

use std::process::Stdio;
use std::time::{Duration, SystemTime};

/// What a probe observed about the sing-box process.
#[derive(Debug, Clone, Default)]
pub struct ProbedInstance {
    /// Pid of the `sing-box run` process, when one is alive.
    pub pid: Option<u32>,
    /// Whether a running instance was observed at all.
    pub alive: bool,
    /// Process start time, for uptime.
    pub start_time: Option<SystemTime>,
    /// Config path from the process command line (FR-025).
    pub config_path: Option<String>,
}

/// Probe for a running `sing-box run` instance.
///
/// `known_pid` short-circuits the pgrep scan: the app checks its own child
/// first so an owned instance is never mistaken for an external one.
pub async fn probe(known_pid: Option<u32>) -> anyhow::Result<Option<ProbedInstance>> {
    let pid = match known_pid {
        Some(pid) if process_alive(pid).await => Some(pid),
        _ => find_run_pid().await?,
    };
    let Some(pid) = pid else {
        return Ok(None);
    };

    // `ps -o etime=` gives elapsed time without timezone parsing.
    let cmdline = ps_field(pid, "command=").await.unwrap_or_default();
    let etime = ps_field(pid, "etime=").await.unwrap_or_default();

    Ok(Some(ProbedInstance {
        pid: Some(pid),
        alive: true,
        start_time: parse_etime(&etime).map(|elapsed| SystemTime::now() - elapsed),
        config_path: extract_config_path(&cmdline),
    }))
}

/// Is a process with this pid alive? (`ps -p <pid> -o pid=`)
async fn process_alive(pid: u32) -> bool {
    ps_field(pid, "pid=").await.is_some()
}

/// Find the pid of a `sing-box run` process via pgrep -f.
async fn find_run_pid() -> anyhow::Result<Option<u32>> {
    let out = tokio::process::Command::new("/usr/bin/pgrep")
        .args(["-f", "sing-box run"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .output()
        .await?;

    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.trim().parse::<u32>().ok())
        .find(|&pid| {
            // Exclude zombies/the probe itself is unnecessary (pgrep does);
            // keep the first live match.
            let _ = pid;
            true
        }))
}

/// Read one `ps -o <field>` column for a pid; None when the process is gone.
async fn ps_field(pid: u32, field: &str) -> Option<String> {
    let out = tokio::process::Command::new("/bin/ps")
        .args(["-o", field, "-p"])
        .arg(pid.to_string())
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
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() { None } else { Some(text) }
}

/// Parse `ps -o etime=` output ("MM:SS", "HH:MM:SS", or "D-HH:MM:SS").
pub fn parse_etime(etime: &str) -> Option<Duration> {
    let t = etime.trim();
    let (days, rest) = match t.split_once('-') {
        Some((d, r)) => (d.parse::<u64>().ok()?, r),
        None => (0, t),
    };
    let parts: Vec<u64> = rest
        .split(':')
        .map(|p| p.trim().parse::<u64>().ok())
        .collect::<Option<Vec<_>>>()?;
    let (h, m, s) = match parts.as_slice() {
        [s] => (0, 0, *s),
        [m, s] => (0, *m, *s),
        [h, m, s] => (*h, *m, *s),
        _ => return None,
    };
    Some(Duration::from_secs(days * 86_400 + h * 3_600 + m * 60 + s))
}

/// Extract the config path from a `sing-box run` command line: the value
/// after `-c`/`--config` (FR-025).
pub fn extract_config_path(cmdline: &str) -> Option<String> {
    if !cmdline.contains("sing-box") {
        return None;
    }
    let mut args = cmdline
        .split_whitespace()
        .skip_while(|a| !a.starts_with('-'));
    while let Some(flag) = args.next() {
        if flag == "-c" || flag == "--config" {
            return args.next().map(str::to_string);
        }
        // `--config=path` form.
        if let Some(v) = flag.strip_prefix("--config=") {
            return Some(v.to_string());
        }
    }
    None
}

/// Run `sing-box version` once and return its first line.
pub async fn singbox_version(bin: &str) -> Option<String> {
    let out = tokio::process::Command::new(bin)
        .arg("version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .ok()?;
    let first = String::from_utf8_lossy(&out.stdout)
        .lines()
        .next()?
        .trim()
        .to_string();
    if first.is_empty() { None } else { Some(first) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn etime_formats() {
        assert_eq!(parse_etime("05:12"), Some(Duration::from_secs(5 * 60 + 12)));
        assert_eq!(parse_etime("1:05:12"), Some(Duration::from_secs(3_912)));
        assert_eq!(
            parse_etime("2-01:00:00"),
            Some(Duration::from_secs(2 * 86_400 + 3_600))
        );
        assert_eq!(parse_etime(""), None);
        assert_eq!(parse_etime("garbage"), None);
    }

    #[test]
    fn config_path_from_cmdline() {
        assert_eq!(
            extract_config_path(
                "/opt/homebrew/bin/sing-box run -c /Users/u/.config/sing-box/config.json"
            ),
            Some("/Users/u/.config/sing-box/config.json".to_string())
        );
        assert_eq!(
            extract_config_path("sing-box run --config /etc/sb.json"),
            Some("/etc/sb.json".to_string())
        );
        assert_eq!(
            extract_config_path("sing-box run --config=/etc/sb.json"),
            Some("/etc/sb.json".to_string())
        );
        assert_eq!(extract_config_path("sing-box run"), None);
        assert_eq!(extract_config_path("some unrelated process"), None);
    }
}
