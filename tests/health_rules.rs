//! Table-driven tests of health derivation.
//!
//! The rules below quote data-model.md "HealthStatus" verbatim:
//! - `Down` when state is Stopped
//! - `Degraded` when state is Running but tunnel is missing or has no address
//! - `Degraded` when any connectivity check has failed or been blocked
//!   (checks against a proxy server's own host are exempt — such hosts
//!   commonly drop ICMP echo; the tunnel carrying traffic is the evidence
//!   they are reachable)
//! - `Healthy` only when state is Running, tunnel is present with address,
//!   and no connectivity check has failed

use sing_box_tui::service::{
    CheckStatus, ConnectivityCheck, FailureReason, HealthStatus, LifecycleState, ServiceInstance,
    derive_health,
};

#[test]
fn health_rules_table() {
    // (name, state, tunnel_present, tunnel_has_address, any_check_failed, expected)
    let cases: &[(&str, LifecycleState, bool, bool, bool, HealthStatus)] = &[
        // `Down` when state is Stopped — regardless of tunnel or checks.
        (
            "stopped → Down",
            LifecycleState::Stopped,
            false,
            false,
            false,
            HealthStatus::Down,
        ),
        (
            "stopped + stale tunnel info → Down",
            LifecycleState::Stopped,
            true,
            true,
            true,
            HealthStatus::Down,
        ),
        // `Degraded` when state is Running but tunnel is missing or has no address.
        (
            "running, no tunnel → Degraded",
            LifecycleState::Running,
            false,
            false,
            false,
            HealthStatus::Degraded,
        ),
        (
            "running, tunnel without address → Degraded",
            LifecycleState::Running,
            true,
            false,
            false,
            HealthStatus::Degraded,
        ),
        // `Degraded` when any connectivity check has failed or been blocked.
        (
            "running, healthy tunnel, failed check → Degraded",
            LifecycleState::Running,
            true,
            true,
            true,
            HealthStatus::Degraded,
        ),
        // `Healthy` only when state is Running, tunnel present with address,
        // and no connectivity check has failed.
        (
            "running, tunnel + address, no failures → Healthy",
            LifecycleState::Running,
            true,
            true,
            false,
            HealthStatus::Healthy,
        ),
        // Not Stopped and not Running: transitional states are never Healthy
        // ("Healthy ONLY when state is Running").
        (
            "starting → not Healthy",
            LifecycleState::Starting,
            true,
            true,
            false,
            HealthStatus::Degraded,
        ),
        (
            "stopping → not Healthy",
            LifecycleState::Stopping,
            true,
            true,
            false,
            HealthStatus::Degraded,
        ),
    ];

    for (name, state, tunnel_present, tunnel_has_address, any_check_failed, expected) in cases {
        let got = derive_health(
            *state,
            *tunnel_present,
            *tunnel_has_address,
            *any_check_failed,
        );
        assert_eq!(
            got, *expected,
            "case {name}: derived {got:?}, expected {expected:?}"
        );
    }
}

/// A failed or blocked check must count as a failure for badge derivation
/// (FR-011: a blocked test is never presented as a healthy result).
#[test]
fn blocked_check_counts_as_failure() {
    let blocked = ConnectivityCheck {
        target: "example.com".into(),
        status: CheckStatus::Failed,
        inferred_route: "proxy".into(),
        sent: 4,
        received: 0,
        loss: 100.0,
        failure_reason: Some(FailureReason::BlockedOrUnsupported),
        ..ConnectivityCheck::default()
    };
    assert!(blocked.failed_for_health());

    let ok = ConnectivityCheck {
        target: "example.com".into(),
        status: CheckStatus::Complete,
        inferred_route: "proxy".into(),
        sent: 4,
        received: 4,
        loss: 0.0,
        ..ConnectivityCheck::default()
    };
    assert!(!ok.failed_for_health());

    // Cancelled/Interrupted checks are not failures (partial results retained, FR-012).
    let cancelled = ConnectivityCheck {
        target: "example.com".into(),
        status: CheckStatus::Cancelled,
        sent: 2,
        received: 1,
        loss: 50.0,
        ..ConnectivityCheck::default()
    };
    assert!(!cancelled.failed_for_health());
}

/// Data-model invariants on ConnectivityCheck (received ≤ sent, loss ∈ [0,100],
/// latencyMin ≤ latencyAvg ≤ latencyMax) must hold for finished checks.
#[test]
fn check_invariants() {
    let ok = ConnectivityCheck {
        sent: 4,
        received: 3,
        loss: 25.0,
        latency_min_ms: 10,
        latency_avg_ms: 15.0,
        latency_max_ms: 20,
        ..ConnectivityCheck::default()
    };
    assert!(ok.invariants_hold());

    assert!(
        !ConnectivityCheck {
            received: 5,
            sent: 4,
            ..Default::default()
        }
        .invariants_hold()
    );
    assert!(
        !ConnectivityCheck {
            loss: 101.0,
            ..Default::default()
        }
        .invariants_hold()
    );
    assert!(
        !ConnectivityCheck {
            latency_min_ms: 30,
            latency_avg_ms: 20.0,
            latency_max_ms: 10,
            ..Default::default()
        }
        .invariants_hold()
    );
}

/// A single reply at 77.6 ms rounds min/max to 78 while the average keeps
/// its fraction — the ordering invariant must tolerate display rounding.
#[test]
fn check_invariants_tolerate_rounding() {
    let rounded = ConnectivityCheck {
        target: "example.com".into(),
        status: CheckStatus::Complete,
        sent: 4,
        received: 1,
        loss: 75.0,
        latency_min_ms: 78,
        latency_avg_ms: 77.6,
        latency_max_ms: 78,
        ..ConnectivityCheck::default()
    };
    assert!(rounded.invariants_hold());
}

/// Lifecycle state machine: Stopped → Starting → Running → Stopping → Stopped.
#[test]
fn lifecycle_transitions() {
    let mut s = ServiceInstance::default();
    assert_eq!(s.state, LifecycleState::Stopped);
    assert_eq!(s.health, HealthStatus::Down);

    assert!(s.begin_starting(Some(4242)).is_ok());
    assert_eq!(s.state, LifecycleState::Starting);
    assert_eq!(s.pid, Some(4242));

    assert!(s.confirm_running().is_ok());
    assert_eq!(s.state, LifecycleState::Running);
    assert!(
        s.start_time.is_some(),
        "running instance must expose a start time"
    );
    assert_eq!(s.pid, Some(4242), "pid must be non-zero while not Stopped");

    assert!(s.begin_stopping().is_ok());
    assert_eq!(s.state, LifecycleState::Stopping);
    assert_eq!(s.pid, Some(4242), "pid stays known while stopping");

    s.confirm_stopped();
    assert_eq!(s.state, LifecycleState::Stopped);
    assert_eq!(s.pid, None, "pid must be zero/None when Stopped");
    assert_eq!(
        s.health,
        HealthStatus::Down,
        "health must be Down when Stopped"
    );
}

/// Illegal transitions are rejected (the state machine never skips states).
#[test]
fn illegal_transitions_rejected() {
    let mut s = ServiceInstance::default();
    // Cannot run/stop from Stopped.
    assert!(s.confirm_running().is_err());
    assert!(s.begin_stopping().is_err());

    let mut s = ServiceInstance::default();
    assert!(s.begin_starting(Some(1)).is_ok());
    // Cannot stop what has not started running.
    assert!(s.begin_stopping().is_err());
    // Double start rejected.
    assert!(s.begin_starting(Some(2)).is_err());

    let mut s = ServiceInstance::default();
    assert!(s.begin_starting(Some(1)).is_ok());
    assert!(s.confirm_running().is_ok());
    assert!(s.begin_stopping().is_ok());
    // Cannot start while stopping.
    assert!(s.begin_starting(Some(3)).is_err());
}

/// A transition stuck for more than 10 s keeps showing the transition state
/// and is marked "not responding" (FR-002a).
#[test]
fn stuck_transition_marks_not_responding() {
    let mut s = ServiceInstance::default();
    assert!(s.begin_starting(Some(9)).is_ok());
    // Simulate the transition having started 11 s ago.
    s.transition_since = Some(std::time::Instant::now() - std::time::Duration::from_secs(11));
    s.check_not_responding(std::time::Duration::from_secs(10));
    assert!(
        s.not_responding,
        "stuck Starting must be marked not responding"
    );
    assert_eq!(s.state, LifecycleState::Starting, "state stays Starting");

    // A fresh transition is fine.
    let mut s = ServiceInstance::default();
    assert!(s.begin_starting(Some(9)).is_ok());
    s.check_not_responding(std::time::Duration::from_secs(10));
    assert!(!s.not_responding);
}

/// When the service stops while open, the last known uptime is retained but
/// flagged as no longer current (spec US1 scenario 6).
#[test]
fn uptime_goes_stale_on_stop() {
    let mut s = ServiceInstance::default();
    assert!(s.begin_starting(Some(7)).is_ok());
    assert!(s.confirm_running().is_ok());
    assert!(s.uptime_secs().is_some());
    assert!(!s.uptime_stale);

    assert!(s.begin_stopping().is_ok());
    s.confirm_stopped();
    assert!(s.uptime_secs().is_some(), "last known uptime is retained");
    assert!(s.uptime_stale, "uptime must be marked no longer current");
}

/// Applying probe results drives the machine from observable state:
/// discovering an external instance flips Stopped → Running (external, not owned).
#[test]
fn probe_application_tracks_external_instance() {
    let mut s = ServiceInstance::default();
    let probe = sing_box_tui::service::probe::ProbedInstance {
        pid: Some(555),
        alive: true,
        start_time: Some(std::time::SystemTime::now() - std::time::Duration::from_secs(30)),
        config_path: Some("/home/u/.config/sing-box/config.json".into()),
    };
    s.apply_probe(&probe);
    assert_eq!(s.state, LifecycleState::Running);
    assert_eq!(s.pid, Some(555));
    assert!(
        !s.owned_by_app,
        "instance not started by the app is external"
    );

    // Probe sees it gone → Stopped.
    let gone = sing_box_tui::service::probe::ProbedInstance {
        pid: None,
        alive: false,
        ..probe
    };
    s.apply_probe(&gone);
    assert_eq!(s.state, LifecycleState::Stopped);
}

/// Regression: a proxy server host that drops ICMP echo (naive endpoints
/// commonly do) must not degrade the badge while every other check passes —
/// the tunnel carrying traffic is itself the evidence the server is up.
/// The lost-ping row stays in the Ping panel as a diagnostic.
#[test]
fn server_host_icmp_loss_does_not_degrade_health() {
    let mut s = ServiceInstance::default();
    s.state = LifecycleState::Running;
    s.tunnel = Some(sing_box_tui::service::tunnel::TunnelInfo {
        name: "utun6".into(),
        address: Some("172.18.0.1".into()),
        mtu: Some(1400),
    });
    let checks = vec![
        ConnectivityCheck {
            target: "ivmvs.wlld.dev".into(),
            status: CheckStatus::Failed,
            sent: 4,
            loss: 100.0,
            failure_reason: Some(FailureReason::NoReply),
            ..Default::default()
        },
        ConnectivityCheck {
            target: "77.88.8.8".into(),
            status: CheckStatus::Complete,
            sent: 4,
            received: 4,
            ..Default::default()
        },
    ];
    s.recompute_health(&checks, &["ivmvs.wlld.dev".to_string()]);
    assert_eq!(s.health, HealthStatus::Healthy);
    assert!(s.health_reason.is_none());
}

/// FR-007 keeps its teeth: a failing target that is not the proxy server's
/// own host still degrades the badge with the named reason.
#[test]
fn other_target_failure_still_degrades() {
    let mut s = ServiceInstance::default();
    s.state = LifecycleState::Running;
    s.tunnel = Some(sing_box_tui::service::tunnel::TunnelInfo {
        name: "utun6".into(),
        address: Some("172.18.0.1".into()),
        mtu: Some(1400),
    });
    let checks = vec![ConnectivityCheck {
        target: "77.88.8.8".into(),
        status: CheckStatus::Failed,
        sent: 4,
        loss: 100.0,
        failure_reason: Some(FailureReason::NoReply),
        ..Default::default()
    }];
    s.recompute_health(&checks, &["ivmvs.wlld.dev".to_string()]);
    assert_eq!(s.health, HealthStatus::Degraded);
    assert_eq!(
        s.health_reason.as_deref(),
        Some("a connectivity check failed or was blocked")
    );
}
