//! Service lifecycle management (T011).
//!
//! Provides:
//! - `ServiceInstance` — lifecycle state machine Stopped → Starting → Running
//!   → Stopping → Stopped with the 10 s "not responding" rule (FR-002a)
//! - `derive_health` — health badge derivation (data-model.md HealthStatus)
//! - `CheckStatus` / `FailureReason` / `ConnectivityCheck` — US2 result model

pub mod probe;
pub mod process;
pub mod tunnel;

use std::time::{Duration, Instant, SystemTime};

/// Lifecycle states (exactly four, per data-model.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LifecycleState {
    #[default]
    Stopped,
    Starting,
    Running,
    Stopping,
}

impl LifecycleState {
    pub fn as_str(&self) -> &'static str {
        match self {
            LifecycleState::Stopped => "Stopped",
            LifecycleState::Starting => "Starting",
            LifecycleState::Running => "Running",
            LifecycleState::Stopping => "Stopping",
        }
    }
}

/// Health badge values (data-model.md HealthStatus).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HealthStatus {
    #[default]
    Down,
    Healthy,
    Degraded,
}

impl HealthStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            HealthStatus::Healthy => "Healthy",
            HealthStatus::Degraded => "Degraded",
            HealthStatus::Down => "Down",
        }
    }
}

/// Derive health from lifecycle state, tunnel presence, and connectivity results.
///
/// Rules (data-model.md, quoted verbatim in tests/health_rules.rs):
/// - `Down` when state is Stopped
/// - `Degraded` when state is Running but tunnel is missing or has no address
/// - `Degraded` when any connectivity check has failed or been blocked
/// - `Healthy` only when state is Running, tunnel is present with address,
///   and no connectivity check has failed
pub fn derive_health(
    state: LifecycleState,
    tunnel_present: bool,
    tunnel_has_address: bool,
    any_check_failed: bool,
) -> HealthStatus {
    if state == LifecycleState::Stopped {
        return HealthStatus::Down;
    }
    // "Healthy ONLY when state is Running" — transitional states are Degraded.
    if state != LifecycleState::Running {
        return HealthStatus::Degraded;
    }
    if !tunnel_present || !tunnel_has_address {
        return HealthStatus::Degraded;
    }
    if any_check_failed {
        return HealthStatus::Degraded;
    }
    HealthStatus::Healthy
}

/// Connectivity check state (data-model.md CheckStatus).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CheckStatus {
    #[default]
    Pending,
    Running,
    Complete,
    Cancelled,
    Interrupted,
    Failed,
}

/// Failure reasons (FR-011, data-model.md FailureReason values).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FailureReason {
    #[default]
    NoReply,
    NameNotResolved,
    BlockedOrUnsupported,
    Interrupted,
    /// The echo reference itself was unreachable — never reported as the
    /// tunnel failing (FR-010a).
    ReferenceUnavailable,
}

impl FailureReason {
    pub fn as_str(&self) -> &'static str {
        match self {
            FailureReason::NoReply => "no reply",
            FailureReason::NameNotResolved => "name not resolved",
            FailureReason::BlockedOrUnsupported => "blocked or unsupported",
            FailureReason::Interrupted => "interrupted",
            FailureReason::ReferenceUnavailable => "reference unavailable",
        }
    }
}

/// A single connectivity check result (data-model.md ConnectivityCheck).
#[derive(Debug, Clone, Default)]
pub struct ConnectivityCheck {
    pub target: String,
    pub status: CheckStatus,
    pub inferred_route: String,
    pub sent: u32,
    pub received: u32,
    pub loss: f64,
    pub latency_min_ms: u64,
    pub latency_avg_ms: f64,
    pub latency_max_ms: u64,
    pub first_reply_latency_ms: Option<u64>,
    pub failure_reason: Option<FailureReason>,
}

impl ConnectivityCheck {
    /// Whether this check counts as a failure for health derivation:
    /// failed or blocked checks do (FR-011); cancelled/interrupted retain
    /// partial results and do not (FR-012).
    pub fn failed_for_health(&self) -> bool {
        self.status == CheckStatus::Failed
            || self.failure_reason == Some(FailureReason::BlockedOrUnsupported)
    }

    /// Data-model invariants: `received ≤ sent`, `loss` ∈ [0, 100],
    /// `latencyMin ≤ latencyAvg ≤ latencyMax` (min/max are whole-ms while
    /// avg keeps fractions, so allow half a ms of rounding skew).
    pub fn invariants_hold(&self) -> bool {
        self.received <= self.sent
            && (0.0..=100.0).contains(&self.loss)
            && (self.latency_min_ms as f64) <= self.latency_avg_ms + 0.5
            && self.latency_avg_ms <= (self.latency_max_ms as f64) + 0.5
    }
}

/// A transition stuck longer than this keeps showing the transition state and
/// is marked "not responding" (FR-002a).
pub const NOT_RESPONDING_AFTER: Duration = Duration::from_secs(10);

/// Service instance state (data-model.md ServiceInstance).
#[derive(Debug, Clone, Default)]
pub struct ServiceInstance {
    pub state: LifecycleState,
    pub pid: Option<u32>,
    pub start_time: Option<SystemTime>,
    pub version: Option<String>,
    pub config_path: Option<String>,
    pub tunnel: Option<tunnel::TunnelInfo>,
    pub active_outbound: Option<String>,
    pub health: HealthStatus,
    pub health_reason: Option<String>,
    /// True when the instance was started by this application (FR-019a).
    pub owned_by_app: bool,
    /// When the current transition began (Starting/Stopping watchdog).
    pub transition_since: Option<Instant>,
    /// Set when a transition exceeded `NOT_RESPONDING_AFTER` (FR-002a).
    pub not_responding: bool,
    /// Last known uptime, kept after stop and flagged stale (US1 scenario 6).
    pub uptime_stale: bool,
    /// When the process was last confirmed alive (for external instances).
    last_alive: Option<Instant>,
}

impl ServiceInstance {
    /// Uptime in seconds (None when never seen running).
    pub fn uptime_secs(&self) -> Option<u64> {
        self.start_time
            .and_then(|st| SystemTime::now().duration_since(st).ok())
            .map(|d| d.as_secs())
    }

    /// Stopped → Starting. Records the pid of the process being started.
    pub fn begin_starting(&mut self, pid: Option<u32>) -> Result<(), &'static str> {
        match self.state {
            LifecycleState::Stopped => {
                self.state = LifecycleState::Starting;
                self.pid = pid;
                self.owned_by_app = true;
                self.transition_since = Some(Instant::now());
                self.not_responding = false;
                self.health_reason = None;
                Ok(())
            }
            _ => Err("start is only allowed from Stopped"),
        }
    }

    /// Starting → Running.
    pub fn confirm_running(&mut self) -> Result<(), &'static str> {
        match self.state {
            LifecycleState::Starting | LifecycleState::Running => {
                if self.start_time.is_none() {
                    self.start_time = Some(SystemTime::now());
                }
                self.state = LifecycleState::Running;
                self.transition_since = None;
                self.not_responding = false;
                self.last_alive = Some(Instant::now());
                Ok(())
            }
            _ => Err("confirm_running is only allowed from Starting"),
        }
    }

    /// Running → Stopping.
    pub fn begin_stopping(&mut self) -> Result<(), &'static str> {
        match self.state {
            LifecycleState::Running | LifecycleState::Stopping => {
                // Freeze the last known uptime before it goes stale.
                let _ = self.uptime_secs();
                self.state = LifecycleState::Stopping;
                self.transition_since = Some(Instant::now());
                self.not_responding = false;
                Ok(())
            }
            _ => Err("stop is only allowed from Running"),
        }
    }

    /// Stopping → Stopped. Uptime is retained but marked no longer current.
    pub fn confirm_stopped(&mut self) {
        self.state = LifecycleState::Stopped;
        self.pid = None;
        self.tunnel = None;
        self.transition_since = None;
        self.not_responding = false;
        self.uptime_stale = self.start_time.is_some();
    }

    /// Mark a transition stuck beyond `limit` as "not responding" (FR-002a).
    pub fn check_not_responding(&mut self, limit: Duration) {
        self.not_responding = match (self.state, self.transition_since) {
            (LifecycleState::Starting, Some(since)) | (LifecycleState::Stopping, Some(since)) => {
                since.elapsed() > limit
            }
            _ => false,
        };
    }

    /// Fold an observed probe into the state machine. The observable truth
    /// (process alive or not) reconciles the model with reality, including
    /// instances started outside the app.
    pub fn apply_probe(&mut self, probe: &probe::ProbedInstance) {
        if probe.alive {
            if self.state == LifecycleState::Stopped || self.state == LifecycleState::Stopping {
                // External (re)start, or our stop completed and someone started
                // it again: observable truth wins.
                if self.state == LifecycleState::Stopped {
                    self.owned_by_app = false;
                }
                self.state = LifecycleState::Running;
                self.start_time = probe.start_time.or(Some(SystemTime::now()));
            }
            if self.state == LifecycleState::Starting {
                self.start_time = probe
                    .start_time
                    .or(self.start_time)
                    .or(Some(SystemTime::now()));
                self.state = LifecycleState::Running;
                self.transition_since = None;
                self.not_responding = false;
                self.uptime_stale = false;
            }
            self.pid = probe.pid;
            if let Some(p) = &probe.config_path {
                self.config_path = Some(p.clone());
            }
            self.last_alive = Some(Instant::now());
        } else {
            // No process observed.
            match self.state {
                LifecycleState::Running | LifecycleState::Stopping | LifecycleState::Starting => {
                    self.confirm_stopped();
                }
                LifecycleState::Stopped => {}
            }
            self.owned_by_app = false;
        }
    }

    /// Recompute the health badge from current state, tunnel, and checks.
    pub fn recompute_health(&mut self, checks: &[ConnectivityCheck]) {
        let tunnel_present = self.tunnel.is_some();
        let tunnel_has_address = self
            .tunnel
            .as_ref()
            .is_some_and(|t| t.address.as_deref().is_some_and(|a| !a.is_empty()));
        let any_failed = checks.iter().any(ConnectivityCheck::failed_for_health);
        self.health = derive_health(self.state, tunnel_present, tunnel_has_address, any_failed);

        self.health_reason = match self.health {
            HealthStatus::Healthy => None,
            HealthStatus::Down => Some("service stopped".to_string()),
            HealthStatus::Degraded => {
                if self.state != LifecycleState::Running {
                    Some(format!("service is {}", self.state.as_str().to_lowercase()))
                } else if !tunnel_present {
                    Some("tunnel interface not found".to_string())
                } else if !tunnel_has_address {
                    Some("tunnel interface has no address".to_string())
                } else {
                    Some("a connectivity check failed or was blocked".to_string())
                }
            }
        };
    }
}
