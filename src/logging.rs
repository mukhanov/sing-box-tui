//! Log capture (T037, FR-018): the spawned process's stdout/stderr lines are
//! read into `LogEntry` values kept in a bounded ring buffer. LogEntry lives
//! here (the capture module), not in the app, so the app stays UI state.

use std::collections::VecDeque;
use std::time::SystemTime;

/// One line of service output (data-model.md LogEntry).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogEntry {
    pub timestamp: SystemTime,
    /// `HH:MM:SS` parsed from the line when sing-box prints timestamps.
    pub time: Option<String>,
    pub level: String,
    pub message: String,
}

/// Bounded ring buffer of log lines.
#[derive(Debug, Clone)]
pub struct LogBuffer {
    entries: VecDeque<LogEntry>,
    cap: usize,
    /// Lines evicted by the ring (diagnostic counter).
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

const LEVELS: [&str; 7] = ["TRACE", "DEBUG", "INFO", "WARN", "ERROR", "FATAL", "PANIC"];

/// Parse one raw output line into a LogEntry (pure).
///
/// Formats handled:
/// - timestamped console (sing-box with `log.timestamp: true`):
///   `+0300 2026-10-05 19:57:45 INFO inbound/tun: accepted connection …`
/// - plain console: `INFO[0000] message` / `WARN message`
/// - JSON: `{"level":"info","message":"…"}`
///
/// Anything else keeps the whole line with an unknown level (`-`).
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
            time: None,
            level,
            message,
        };
    }

    // Timestamped console format: `+0300 2026-10-05 19:57:45 LEVEL message`.
    // The timestamp prefix (~29 chars) must not eat the message — the
    // interesting part (connection addresses) follows the level.
    let tokens: Vec<&str> = line.splitn(5, ' ').filter(|t| !t.is_empty()).collect();
    if tokens.len() >= 4 && is_tz_offset(tokens[0]) {
        let level_token = tokens[3];
        let level = level_token
            .trim_end_matches(|c: char| c == '[' || c == ']' || c.is_ascii_digit())
            .to_ascii_uppercase();
        if LEVELS.contains(&level.as_str()) {
            return LogEntry {
                timestamp,
                time: Some(tokens[2].to_string()),
                level: level.to_ascii_lowercase(),
                message: tokens.get(4).unwrap_or(&"").trim().to_string(),
            };
        }
    }

    // Plain console format: `INFO[0000] message` / `WARN message`.
    let first: &str = line.split_whitespace().next().unwrap_or("");
    let upper = first
        .trim_end_matches(|c: char| c == '[' || c == ']' || c.is_ascii_digit())
        .to_ascii_uppercase();
    if LEVELS.contains(&upper.as_str()) && line.len() >= first.len() {
        // Strip the level token (and any bracketed counter) from the message.
        let rest = &line[first.len()..];
        let rest = rest
            .strip_prefix(|c: char| c == '[' || c.is_ascii_digit() || c == ']')
            .unwrap_or(rest);
        return LogEntry {
            timestamp,
            time: None,
            level: upper.to_ascii_lowercase(),
            message: rest.trim().to_string(),
        };
    }

    LogEntry {
        timestamp,
        time: None,
        level: "-".to_string(),
        message: line.to_string(),
    }
}

/// `+0300` / `-0530` timezone-offset token.
fn is_tz_offset(token: &str) -> bool {
    let bytes = token.as_bytes();
    bytes.len() == 5
        && (bytes[0] == b'+' || bytes[0] == b'-')
        && bytes[1..].iter().all(u8::is_ascii_digit)
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

    /// The real sing-box format with `log.timestamp: true` — the format the
    /// user's service emits. The connection address must survive in the
    /// message, not be truncated away with the timestamp.
    #[test]
    fn parses_timestamped_format() {
        let e = parse_line(
            "+0300 2026-10-05 19:57:45 INFO inbound/tun[tun-in]: accepted connection to tcp:ya.ru:443",
        );
        assert_eq!(e.level, "info");
        assert_eq!(e.time.as_deref(), Some("19:57:45"));
        assert_eq!(
            e.message,
            "inbound/tun[tun-in]: accepted connection to tcp:ya.ru:443"
        );

        let e = parse_line("+0000 2026-10-05 10:00:00 WARN some warning detail");
        assert_eq!(e.level, "warn");
        assert_eq!(e.time.as_deref(), Some("10:00:00"));
        assert_eq!(e.message, "some warning detail");
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
                time: None,
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
