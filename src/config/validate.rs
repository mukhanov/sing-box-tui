//! Configuration validation via `sing-box check`, with verbatim output
//! passthrough (T028, FR-017, SC-008).
//!
//! Errors and warnings are surfaced exactly as the tool reports them;
//! warnings never block the user (spec Edge Cases).

use std::path::Path;
use std::process::Stdio;

/// Result of a validation run.
#[derive(Debug, Clone, Default)]
pub struct ValidationResult {
    /// True when `sing-box check` exited 0.
    pub ok: bool,
    pub errors: Vec<String>,
    pub warnings: Vec<String>,
    /// Full combined output, shown verbatim.
    pub raw: String,
}

impl ValidationResult {
    /// A short single-line summary for the Config/Status panels.
    pub fn summary(&self) -> String {
        if self.ok && self.warnings.is_empty() {
            "valid".to_string()
        } else if self.ok {
            format!("valid, {} warning(s)", self.warnings.len())
        } else {
            format!("invalid, {} error(s)", self.errors.len())
        }
    }
}

/// Classify raw tool output into errors/warnings without rewriting any line
/// (pure — tested verbatim in tests/config_parse.rs).
///
/// `ok` is the tool's exit status. Classification is additive: lines matching
/// error/fatal go to `errors`, warn/deprecated to `warnings`, and every line
/// is preserved in `raw`.
pub fn classify_output(ok: bool, stdout: &str, stderr: &str) -> ValidationResult {
    let raw = match (stdout.trim().is_empty(), stderr.trim().is_empty()) {
        (true, true) => String::new(),
        (true, false) => stderr.trim_end().to_string(),
        (false, true) => stdout.trim_end().to_string(),
        (false, false) => format!("{}\n{}", stdout.trim_end(), stderr.trim_end()),
    };

    let mut errors = Vec::new();
    let mut warnings = Vec::new();
    for line in raw.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let lower = t.to_ascii_lowercase();
        if lower.contains("error") || lower.contains("fatal") {
            errors.push(t.to_string());
        } else if lower.contains("warn") || lower.contains("deprecated") {
            warnings.push(t.to_string());
        }
    }

    ValidationResult {
        ok,
        errors,
        warnings,
        raw,
    }
}

/// Run `sing-box check -c <path>` (FR-017). Never fails: a spawn failure is
/// reported as an invalid result with the reason, so the app stays usable.
pub async fn check(bin: &Path, path: &Path) -> ValidationResult {
    let output = tokio::process::Command::new(bin)
        .args(["check", "-c"])
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .output()
        .await;

    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            classify_output(out.status.success(), &stdout, &stderr)
        }
        Err(e) => ValidationResult {
            ok: false,
            errors: vec![format!("sing-box check could not run: {e}")],
            warnings: Vec::new(),
            raw: format!("sing-box check could not run: {e}"),
        },
    }
}
