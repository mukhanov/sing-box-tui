//! Log capture (T037, FR-018): the spawned process's stdout/stderr lines are
//! read into `LogEntry` values kept in a bounded ring buffer. LogEntry lives
//! here (the capture module), not in the app, so the app stays UI state.

use std::collections::VecDeque;
use std::time::SystemTime;

/// One line of service output (data-model.md LogEntry).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    pub timestamp: SystemTime,
    pub level: String,
    pub message: String,
}

/// Bounded ring buffer of log lines.
#[derive(Debug, Clone)]
pub struct LogBuffer {
    entries: VecDeque<LogEntry>,
    cap: usize,
    /// Set when the output stream ended (process exited) — pause UI churn.
    pub total_dropped: u64,
}

impl Default for LogBuffer {
    fn default() -> Self {
        LogBuffer::new(1000)
    }
}

impl LogBuffer {
    pub fn new(cap: usize) -> Self {
        LogBuffer {
            entries: VecDeque::new(),
            cap,
            total_dropped: 0,
        }
    }

    /// Append an entry, evicting the oldest beyond capacity.
    pub fn push(&mut self, entry: LogEntry) {
        if self.entries.len() >= self.cap {
            self.entries.pop_front();
            self.total_dropped += 1;
        }
        self.entries.push_back(entry);
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl DoubleEndedIterator<Item = &LogEntry> {
        self.entries.iter()
    }

    /// The newest `n` entries, oldest first.
    pub fn tail(&self, n: usize) -> Vec<&LogEntry> {
        let skip = self.entries.len().saturating_sub(n);
        self.entries.iter().skip(skip).collect()
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }
}

/// Parse one raw output line into a LogEntry (pure).
///
/// sing-box console output looks like `INFO[0000] start initial services`
/// (level token, optional bracketed counter, message). JSON-formatted log
/// lines carry `level`/`message` fields. Anything else keeps the whole line
/// with an unknown level (`-`).
pub fn parse_line(raw: &str) -> LogEntry {
    let line = raw.trim_end_matches('\r');
    let timestamp = SystemTime::now();

    // JSON format: {"level":"info","message":"...","timestamp":"..."}
    if line.starts_with('{')
        && let Ok(v) = serde_json::from_str::<serde_json::Value>(line)
    {
        let level = v
            .get("level")
            .and_then(|l| l.as_str())
            .unwrap_or("-")
            .to_string();
        let message = v
            .get("message")
            .or_else(|| v.get("msg"))
            .and_then(|m| m.as_str())
            .unwrap_or(line)
            .to_string();
        return LogEntry {
            timestamp,
            level,
            message,
        };
    }

    // Console format: `INFO[0000] message` / `WARN message`.
    let first: &str = line.split_whitespace().next().unwrap_or("");
    let known = ["TRACE", "DEBUG", "INFO", "WARN", "ERROR", "FATAL", "PANIC"];
    let upper = first.trim_end_matches(|c: char| c == '[' || c == ']' || c.is_ascii_digit());
    if known.contains(&upper) && line.len() >= first.len() {
        // Strip the level token (and any bracketed counter) from the message.
        let rest = &line[first.len()..];
        let rest = rest
            .strip_prefix(|c: char| c == '[' || c.is_ascii_digit() || c == ']')
            .unwrap_or(rest);
        return LogEntry {
            timestamp,
            level: upper.to_ascii_lowercase(),
            message: rest.trim().to_string(),
        };
    }

    LogEntry {
        timestamp,
        level: "-".to_string(),
        message: line.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_console_format() {
        let e = parse_line("INFO[0000] start initial services");
        assert_eq!(e.level, "info");
        assert_eq!(e.message, "start initial services");

        let e = parse_line("ERROR failed to start inbound/tun-in");
        assert_eq!(e.level, "error");
        assert_eq!(e.message, "failed to start inbound/tun-in");

        let e = parse_line("WARN[0012] deprecated option");
        assert_eq!(e.level, "warn");
        assert_eq!(e.message, "deprecated option");
    }

    #[test]
    fn parses_json_format() {
        let e = parse_line(
            r#"{"level":"debug","message":"outbound/proxy resolved","timestamp":"2026-10-05T14:00:00Z"}"#,
        );
        assert_eq!(e.level, "debug");
        assert_eq!(e.message, "outbound/proxy resolved");
    }

    #[test]
    fn unknown_lines_kept_verbatim() {
        let e = parse_line("plain text without level");
        assert_eq!(e.level, "-");
        assert_eq!(e.message, "plain text without level");
    }

    #[test]
    fn ring_buffer_bounds() {
        let mut buf = LogBuffer::new(3);
        for i in 0..5 {
            buf.push(LogEntry {
                timestamp: SystemTime::now(),
                level: "info".into(),
                message: format!("m{i}"),
            });
        }
        assert_eq!(buf.len(), 3);
        assert_eq!(buf.total_dropped, 2);
        let tail: Vec<String> = buf.tail(10).iter().map(|e| e.message.clone()).collect();
        assert_eq!(tail, vec!["m2", "m3", "m4"]);
    }
}
