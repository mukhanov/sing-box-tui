//! Service start/stop/restart (T033, T034).
//!
//! - Start: validate first (SC-008, via `config::validate`), then spawn
//!   `sing-box run -c <path>` with piped stdout/stderr; the instance is
//!   owned-by-app (FR-019, FR-019a).
//! - Stop: SIGTERM, wait 5 s, then SIGKILL (research.md Task 2).
//! - Restart: a full stop → start cycle (SC-011).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

use tokio::process::Child;

/// Why a start failed — the validation variant carries `sing-box check`
/// output verbatim for display (FR-020, SC-008).
#[derive(Debug)]
pub enum StartError {
    /// Config failed validation; the string is the verbatim tool output.
    Validation(String),
    /// The process could not be spawned.
    Spawn(String),
}

impl std::fmt::Display for StartError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StartError::Validation(raw) => write!(f, "{raw}"),
            StartError::Spawn(e) => write!(f, "failed to start sing-box: {e}"),
        }
    }
}

/// A freshly started, app-owned service process.
pub struct StartedService {
    pub pid: u32,
    pub child: Child,
}

/// Validate the config, then spawn `sing-box run -c <path>` (T033).
///
/// Validation runs first so an invalid config never leaves a half-started
/// process behind (spec US4 scenario 4).
pub async fn start(bin: &Path, config_path: &Path) -> Result<StartedService, StartError> {
    let check = crate::config::validate::check(bin, config_path).await;
    if !check.ok {
        let raw = if check.raw.trim().is_empty() {
            format!("sing-box check failed for {}", config_path.display())
        } else {
            check.raw.trim_end().to_string()
        };
        return Err(StartError::Validation(raw));
    }

    let child = tokio::process::Command::new(bin)
        .args(["run", "-c"])
        .arg(config_path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| StartError::Spawn(e.to_string()))?;

    let pid = child
        .id()
        .ok_or_else(|| StartError::Spawn("child pid unknown".into()))?;
    Ok(StartedService { pid, child })
}

/// Stop an app-owned child: SIGTERM, wait up to 5 s, then SIGKILL.
/// Consumes the child; the process is fully reaped before returning.
pub async fn stop_child(child: Child) -> Result<(), String> {
    stop_impl(child.id(), Some(child)).await
}

/// Stop an arbitrary pid (used only after confirmation for external
/// instances, FR-019a): SIGTERM → 5 s → SIGKILL.
pub async fn stop_pid(pid: u32) -> Result<(), String> {
    stop_impl(Some(pid), None).await
}

async fn stop_impl(pid: Option<u32>, child: Option<Child>) -> Result<(), String> {
    const TERM_WAIT: Duration = Duration::from_secs(5);

    // Send SIGTERM first.
    if let Some(pid) = pid
        && unsafe { libc::kill(pid as i32, libc::SIGTERM) } != 0
    {
        let err = std::io::Error::last_os_error();
        // ESRCH: already gone — treat as stopped.
        if err.raw_os_error() != Some(libc::ESRCH) {
            return Err(format!("SIGTERM failed: {err}"));
        }
    }

    let mut child = child;
    let deadline = Instant::now() + TERM_WAIT;
    loop {
        if let Some(c) = child.as_mut() {
            // Reap without blocking: poll try_wait.
            match c.try_wait() {
                Ok(Some(_)) => return Ok(()),
                Ok(None) => {}
                Err(e) => return Err(format!("wait failed: {e}")),
            }
        } else if let Some(pid) = pid
            && !alive(pid)
        {
            return Ok(());
        }

        if Instant::now() >= deadline {
            // Escalate to SIGKILL.
            if let Some(c) = child.as_mut() {
                c.kill().await.map_err(|e| format!("SIGKILL failed: {e}"))?;
                let _ = c.wait().await;
            } else if let Some(pid) = pid
                && unsafe { libc::kill(pid as i32, libc::SIGKILL) } != 0
            {
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() != Some(libc::ESRCH) {
                    return Err(format!("SIGKILL failed: {err}"));
                }
            }
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

/// Resolve the sing-box binary: `sing-box` on PATH when reachable, else the
/// known Homebrew locations (sudo's secure_path does not include
/// /opt/homebrew/bin, research.md environment).
pub fn find_singbox() -> PathBuf {
    if let Ok(found) = which_on_path("sing-box") {
        return found;
    }
    for candidate in [
        "/opt/homebrew/bin/sing-box",
        "/usr/local/bin/sing-box",
        "/usr/local/sbin/sing-box",
        "/usr/bin/sing-box",
    ] {
        let p = PathBuf::from(candidate);
        if p.is_file() {
            return p;
        }
    }
    PathBuf::from("sing-box")
}

fn which_on_path(bin: &str) -> Result<PathBuf, String> {
    let out = std::process::Command::new("/usr/bin/which")
        .arg(bin)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| e.to_string())?;
    if !out.status.success() {
        return Err("not on PATH".into());
    }
    let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if path.is_empty() {
        Err("empty which output".into())
    } else {
        Ok(PathBuf::from(path))
    }
}
