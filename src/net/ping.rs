//! Ping driver (T021, T023): drive `/sbin/ping` per target, stream-parse the
//! output so the first reply's latency is displayed within 2 s of test start
//! (SC-002), accumulate stats with the data-model constraints, and classify
//! failures (FR-011). Cancellation stops the test within 1 s and retains
//! partial results (FR-012).

use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::{mpsc, watch};

use crate::service::{CheckStatus, FailureReason};

/// Packets per check. Four keeps a full run under the 10 s summary budget.
const COUNT: u32 = 4;
/// Interval between packets (seconds as string flag value).
const INTERVAL_SECS: &str = "0.5";

/// Why a running check is being cancelled — decides Cancelled vs Interrupted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Cancel {
    #[default]
    No,
    /// User pressed `C`.
    User,
    /// The service stopped/restarted under the test (spec Edge Cases).
    Service,
}

/// Live progress for one target.
#[derive(Debug, Clone)]
pub struct CheckProgress {
    pub target: String,
    pub first_reply_latency_ms: Option<u64>,
    pub sent: u32,
    pub received: u32,
}

/// Final outcome for one target (fields mirror ConnectivityCheck).
#[derive(Debug, Clone)]
pub struct CheckOutcome {
    pub target: String,
    pub status: CheckStatus,
    pub sent: u32,
    pub received: u32,
    pub loss: f64,
    pub latency_min_ms: u64,
    pub latency_avg_ms: f64,
    pub latency_max_ms: u64,
    pub first_reply_latency_ms: Option<u64>,
    pub failure_reason: Option<FailureReason>,
}

/// Events one running check emits.
#[derive(Debug, Clone)]
pub enum CheckEvent {
    Progress(CheckProgress),
    Done(CheckOutcome),
}

/// Spawn one ping check as its own task. Emits Progress events as replies
/// arrive and exactly one Done at the end (SC-002 streaming).
pub fn spawn_check(
    target: String,
    mut cancel: watch::Receiver<Cancel>,
    evt_tx: mpsc::Sender<CheckEvent>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut child = match Command::new("/sbin/ping")
            .arg("-c")
            .arg(COUNT.to_string())
            .arg("-i")
            .arg(INTERVAL_SECS)
            .arg("-W")
            .arg("2000")
            .arg(&target)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(c) => c,
            Err(e) => {
                let _ = evt_tx
                    .send(CheckEvent::Done(CheckOutcome {
                        target: target.clone(),
                        status: CheckStatus::Failed,
                        failure_reason: Some(classify_spawn_error(&e)),
                        ..zeroed_outcome(&target)
                    }))
                    .await;
                return;
            }
        };

        let stdout = child.stdout.take().expect("stdout piped");
        let stderr = child.stderr.take().expect("stderr piped");
        let mut lines = BufReader::new(stdout).lines();
        let mut err_lines = BufReader::new(stderr).lines();

        let mut sent = 0u32;
        let mut received = 0u32;
        let mut latencies: Vec<f64> = Vec::new();
        let mut first_reply_latency_ms: Option<u64> = None;
        let mut failure_reason: Option<FailureReason> = None;
        let mut summary: Option<(u32, u32, f64)> = None;
        let mut round_trip: Option<(f64, f64, f64)> = None;
        let mut cancelled: Option<Cancel> = None;

        loop {
            tokio::select! {
                line = lines.next_line() => {
                    match line {
                        Ok(Some(line)) => {
                            if let Some((latency, seq)) = parse_reply(&line) {
                                received += 1;
                                latencies.push(latency);
                                if first_reply_latency_ms.is_none() {
                                    first_reply_latency_ms = Some(latency.round() as u64);
                                }
                                sent = sent.max(seq + 1).min(COUNT);
                                // Stream the first reply immediately (SC-002),
                                // then every reply so live stats stay fresh.
                                let _ = evt_tx
                                    .send(CheckEvent::Progress(CheckProgress {
                                        target: target.clone(),
                                        first_reply_latency_ms,
                                        sent: sent.max(received),
                                        received,
                                    }))
                                    .await;
                            } else if let Some(seq) = parse_timeout_seq(&line) {
                                sent = sent.max(seq + 1).min(COUNT);
                            } else if let Some(s) = parse_summary(&line) {
                                summary = Some(s);
                            } else if let Some(rt) = parse_round_trip(&line) {
                                round_trip = Some(rt);
                            }
                        }
                        Ok(None) => break,
                        Err(_) => break,
                    }
                }
                err = err_lines.next_line() => {
                    match err {
                        Ok(Some(line)) => {
                            if let Some(reason) = classify_failure(&line) {
                                failure_reason = Some(reason);
                            }
                        }
                        _ => break,
                    }
                }
                changed = cancel.changed() => {
                    if changed.is_ok() {
                        cancelled = Some(*cancel.borrow_and_update());
                        // Stop within 1 s: kill now; the loop breaks with
                        // partial results retained (FR-012).
                        let _ = child.start_kill();
                        break;
                    }
                }
            }
        }

        // Drain remaining stdout so the summary line is seen for natural exits.
        while let Ok(Some(line)) = lines.next_line().await {
            if let Some(s) = parse_summary(&line) {
                summary = Some(s);
            } else if let Some(rt) = parse_round_trip(&line) {
                round_trip = Some(rt);
            }
        }
        let _ = child.wait().await;

        let outcome = finish_outcome(
            &target,
            cancelled,
            summary,
            round_trip,
            sent,
            received,
            &latencies,
            first_reply_latency_ms,
            failure_reason,
        );
        let _ = evt_tx.send(CheckEvent::Done(outcome)).await;
    })
}

fn zeroed_outcome(target: &str) -> CheckOutcome {
    CheckOutcome {
        target: target.to_string(),
        status: CheckStatus::Failed,
        sent: 0,
        received: 0,
        loss: 0.0,
        latency_min_ms: 0,
        latency_avg_ms: 0.0,
        latency_max_ms: 0,
        first_reply_latency_ms: None,
        failure_reason: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn finish_outcome(
    target: &str,
    cancelled: Option<Cancel>,
    summary: Option<(u32, u32, f64)>,
    round_trip: Option<(f64, f64, f64)>,
    mut sent: u32,
    received: u32,
    latencies: &[f64],
    first_reply_latency_ms: Option<u64>,
    failure_reason: Option<FailureReason>,
) -> CheckOutcome {
    // The summary line is authoritative when present.
    if let Some((s, r, _loss)) = summary {
        sent = sent.max(s);
        let _ = r;
    }
    if sent == 0 && received == 0 && failure_reason.is_none() {
        sent = sent.max(received);
    }

    let (loss, min, avg, max) =
        if let (Some((_, _, loss_pct)), Some((mn, av, mx))) = (summary, round_trip) {
            (loss_pct, mn.round() as u64, av, mx.round() as u64)
        } else {
            let loss = if sent > 0 {
                (sent.saturating_sub(received)) as f64 / sent as f64 * 100.0
            } else {
                0.0
            };
            let min = latencies.iter().copied().fold(f64::INFINITY, f64::min);
            let max = latencies.iter().copied().fold(0.0_f64, f64::max);
            let avg = if latencies.is_empty() {
                0.0
            } else {
                latencies.iter().sum::<f64>() / latencies.len() as f64
            };
            (
                loss,
                if latencies.is_empty() {
                    0
                } else {
                    min.round() as u64
                },
                avg,
                if latencies.is_empty() {
                    0
                } else {
                    max.round() as u64
                },
            )
        };

    let status = match cancelled {
        Some(Cancel::User) => CheckStatus::Cancelled,
        Some(Cancel::Service) => CheckStatus::Interrupted,
        _ if received > 0 => CheckStatus::Complete,
        _ => CheckStatus::Failed,
    };
    let failure_reason = match (status, failure_reason) {
        (CheckStatus::Cancelled, _) => None,
        (CheckStatus::Interrupted, _) => Some(FailureReason::Interrupted),
        (_, Some(r)) => Some(r),
        (CheckStatus::Failed, None) => Some(FailureReason::NoReply),
        _ => None,
    };

    CheckOutcome {
        target: target.to_string(),
        status,
        sent,
        received,
        loss: loss.clamp(0.0, 100.0),
        latency_min_ms: min,
        latency_avg_ms: avg,
        latency_max_ms: max,
        first_reply_latency_ms,
        failure_reason,
    }
}

/// Parse "64 bytes from 8.8.8.8: icmp_seq=0 ttl=118 time=23.435 ms" →
/// (latency ms, seq).
pub fn parse_reply(line: &str) -> Option<(f64, u32)> {
    if !line.contains("time=") {
        return None;
    }
    let latency = line
        .split("time=")
        .nth(1)?
        .split_whitespace()
        .next()?
        .trim_end_matches("ms")
        .parse::<f64>()
        .ok()?;
    let seq = line
        .split("icmp_seq=")
        .nth(1)?
        .split(&[' ', ':'][..])
        .next()?
        .parse::<u32>()
        .ok()?;
    Some((latency, seq))
}

/// Parse "Request timeout for icmp_seq 3" → seq.
pub fn parse_timeout_seq(line: &str) -> Option<u32> {
    if !line.contains("Request timeout") {
        return None;
    }
    // "Request timeout for icmp_seq 3" — the seq follows without '='.
    line.split("icmp_seq").nth(1)?.trim().parse().ok()
}

/// Parse "5 packets transmitted, 5 packets received, 0.0% packet loss" →
/// (sent, received, loss %).
pub fn parse_summary(line: &str) -> Option<(u32, u32, f64)> {
    if !line.contains("packets transmitted") {
        return None;
    }
    let sent: u32 = line.split_whitespace().next()?.parse().ok()?;
    let received: u32 = line
        .split("packets transmitted,")
        .nth(1)?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    let loss: f64 = line
        .split(',')
        .nth(2)?
        .trim()
        .split('%')
        .next()?
        .trim()
        .parse()
        .ok()?;
    Some((sent, received, loss))
}

/// Parse "round-trip min/avg/max/stddev = 14.939/15.281/16.063/0.487 ms".
pub fn parse_round_trip(line: &str) -> Option<(f64, f64, f64)> {
    if !line.contains("round-trip") {
        return None;
    }
    let stats = line.split('=').nth(1)?;
    let mut parts = stats.split('/');
    let min = parts.next()?.trim().parse::<f64>().ok()?;
    let avg = parts.next()?.trim().parse::<f64>().ok()?;
    let max = parts.next()?.trim().parse::<f64>().ok()?;
    Some((min, avg, max))
}

/// Classify a failure from ping's stderr/output text (FR-011).
pub fn classify_failure(text: &str) -> Option<FailureReason> {
    let lower = text.to_ascii_lowercase();
    if lower.contains("cannot resolve")
        || lower.contains("unknown host")
        || lower.contains("nodename nor servname")
        || lower.contains("no address associated")
    {
        Some(FailureReason::NameNotResolved)
    } else if lower.contains("permission denied")
        || lower.contains("operation not permitted")
        || lower.contains("can't assign requested address")
        || lower.contains("socket: operation not supported")
    {
        Some(FailureReason::BlockedOrUnsupported)
    } else {
        None
    }
}

fn classify_spawn_error(e: &std::io::Error) -> FailureReason {
    match e.kind() {
        std::io::ErrorKind::PermissionDenied => FailureReason::BlockedOrUnsupported,
        _ => FailureReason::BlockedOrUnsupported,
    }
}

/// How long a natural full run may take (budget guard, SC-002/SC-003).
pub const FULL_RUN_BUDGET: Duration = Duration::from_secs(10);

impl CheckOutcome {
    /// Data-model latency ordering invariant (min ≤ avg ≤ max).
    pub fn invariant_latencies(&self) -> bool {
        (self.latency_min_ms as f64) <= self.latency_avg_ms + 0.5
            && self.latency_avg_ms <= self.latency_max_ms as f64 + 0.5
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_reply_lines() {
        let (latency, seq) =
            parse_reply("64 bytes from 8.8.8.8: icmp_seq=0 ttl=118 time=23.435 ms").unwrap();
        assert!((latency - 23.435).abs() < 1e-9);
        assert_eq!(seq, 0);
        assert!(parse_reply("Request timeout for icmp_seq 1").is_none());
    }

    #[test]
    fn parses_timeout_seq() {
        assert_eq!(parse_timeout_seq("Request timeout for icmp_seq 3"), Some(3));
        assert_eq!(parse_timeout_seq("64 bytes from x"), None);
    }

    #[test]
    fn parses_summary_and_round_trip() {
        let (sent, recv, loss) =
            parse_summary("5 packets transmitted, 4 packets received, 20.0% packet loss").unwrap();
        assert_eq!((sent, recv, loss), (5, 4, 20.0));
        let (min, avg, max) =
            parse_round_trip("round-trip min/avg/max/stddev = 14.9/15.2/16.0/0.4 ms").unwrap();
        assert!(
            (min - 14.9).abs() < 1e-9 && (avg - 15.2).abs() < 1e-9 && (max - 16.0).abs() < 1e-9
        );
    }

    #[test]
    fn classifies_failures() {
        assert_eq!(
            classify_failure("ping: cannot resolve bad.host.example: Unknown host"),
            Some(FailureReason::NameNotResolved)
        );
        assert_eq!(
            classify_failure("ping: sendto: Permission denied"),
            Some(FailureReason::BlockedOrUnsupported)
        );
        assert_eq!(
            classify_failure("PING example.com (1.2.3.4): 56 data bytes"),
            None
        );
    }

    #[test]
    fn cancelled_outcome_keeps_partial_stats() {
        let out = finish_outcome(
            "example.com",
            Some(Cancel::User),
            None,
            None,
            3,
            2,
            &[10.0, 20.0],
            Some(10),
            None,
        );
        assert_eq!(out.status, CheckStatus::Cancelled);
        assert_eq!(out.received, 2, "partial results retained (FR-012)");
        assert_eq!(out.sent, 3);
        assert!(out.invariant_latencies());
        assert!(out.failure_reason.is_none());
    }

    #[test]
    fn service_cancel_marks_interrupted() {
        let out = finish_outcome(
            "example.com",
            Some(Cancel::Service),
            None,
            None,
            1,
            1,
            &[10.0],
            Some(10),
            None,
        );
        assert_eq!(out.status, CheckStatus::Interrupted);
    }

    #[test]
    fn unresolved_target_fails_with_reason() {
        let out = finish_outcome(
            "no.such.host",
            None,
            None,
            None,
            0,
            0,
            &[],
            None,
            Some(FailureReason::NameNotResolved),
        );
        assert_eq!(out.status, CheckStatus::Failed);
        assert_eq!(out.failure_reason, Some(FailureReason::NameNotResolved));
        assert!(out.received <= out.sent);
    }

    #[test]
    fn summary_is_authoritative() {
        let out = finish_outcome(
            "example.com",
            None,
            Some((4, 4, 0.0)),
            Some((14.9, 15.2, 16.0)),
            4,
            4,
            &[14.9, 15.2, 16.0],
            Some(15),
            None,
        );
        assert_eq!(out.status, CheckStatus::Complete);
        assert_eq!(out.loss, 0.0);
        assert_eq!(out.latency_min_ms, 15);
        assert_eq!(out.latency_max_ms, 16);
        assert!((out.latency_avg_ms - 15.2).abs() < 1e-9);
    }
}
