//! Application state and event loop (T005, T016, T022, T035, T039–T041).
//!
//! `App` is the pure UI/state model; `Ctx` owns the async plumbing (channels,
//! child process, cancel signal). The loop mixes crossterm events, a 3-second
//! status tick (SC-010), and background-task results via one event channel.

use std::path::PathBuf;
use std::time::Duration;

use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use tokio::sync::{mpsc, watch};
use tokio::time::interval;

use crate::config;
use crate::config::parse::SingBoxConfig;
use crate::config::validate::ValidationResult;
use crate::logging::{LogBuffer, LogEntry};
use crate::net;
use crate::net::address::MeasuredAddress;
use crate::net::ping::{Cancel, CheckOutcome, CheckProgress};
use crate::service::probe::ProbedInstance;
use crate::service::tunnel::TunnelInfo;
use crate::service::{LifecycleState, ServiceInstance, process};
use crate::ui;

/// Status refresh period (SC-010: ≤ every 3 s).
const STATUS_TICK: Duration = Duration::from_secs(3);

/// Focusable panels (contracts/ui.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Panel {
    #[default]
    Status,
    Ping,
    Config,
    Log,
}

impl Panel {
    pub fn next(self) -> Panel {
        match self {
            Panel::Status => Panel::Ping,
            Panel::Ping => Panel::Config,
            Panel::Config => Panel::Log,
            Panel::Log => Panel::Status,
        }
    }

    pub fn prev(self) -> Panel {
        match self {
            Panel::Status => Panel::Log,
            Panel::Ping => Panel::Status,
            Panel::Config => Panel::Ping,
            Panel::Log => Panel::Config,
        }
    }
}

/// Input mode: while the target input is open, all other keys route to the
/// input (no panel actions, no quit) until Enter or Esc (contracts/ui.md).
#[derive(Debug, Clone, Default)]
pub struct TargetInput {
    pub buffer: String,
}

/// Confirmation dialogs (y / n / Esc; other keys ignored).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dialog {
    /// Stop an instance not started by this app (FR-019a).
    StopExternal { pid: u32, then_start: bool },
    /// Same decision on quit (FR-019b).
    QuitExternal { pid: u32 },
    /// Reload a config that changed on disk (FR-016).
    ReloadConfig,
    /// A profile was selected while the service runs: restart applies it.
    SwitchProfile { name: String, pid: Option<u32> },
}

impl Dialog {
    pub fn message(&self) -> String {
        match self {
            Dialog::StopExternal { pid, then_start } => format!(
                "sing-box (pid {pid}) was not started by this app.\n{}",
                if *then_start {
                    "Stop it to restart?"
                } else {
                    "Stop it?"
                }
            ),
            Dialog::QuitExternal { pid } => {
                format!("sing-box (pid {pid}) was not started by this app.\nStop it on quit?")
            }
            Dialog::ReloadConfig => "config.json changed on disk.\nReload it?".to_string(),
            Dialog::SwitchProfile { name, .. } => {
                format!("service is running.\nRestart it with {name}?")
            }
        }
    }
}

/// Background-task results delivered into the loop.
pub enum AppEvent {
    Probe {
        probe: Option<ProbedInstance>,
        tunnel: Option<TunnelInfo>,
        /// Cumulative interface counters, for the throughput diff.
        traffic: Option<(u64, u64)>,
        changed: bool,
    },
    Version(String),
    /// Resulting public IP through the tunnel (for the window title).
    PublicIp(Option<String>),
    StartDone(Result<process::StartedService, process::StartError>),
    /// Ok on clean stop; Err carries the reason. `pid` names the survivor on
    /// failure so the final screen can state it (FR-020).
    StopDone {
        result: Result<(), String>,
        pid: Option<u32>,
        then_start: bool,
        was_quit: bool,
    },
    CheckUpdate {
        target: String,
        progress: CheckProgress,
    },
    CheckDone {
        target: String,
        outcome: CheckOutcome,
    },
    MeasuredDone(MeasuredAddress),
    LogLine(LogEntry),
    ValidateDone(ValidationResult),
}

/// Central application state (all of it Debug/plain — no handles).
#[derive(Debug, Default)]
pub struct App {
    pub elevated: bool,
    pub config_path: Option<PathBuf>,
    pub config: Option<SingBoxConfig>,
    /// Read/parse error, naming the path tried (spec Edge Cases).
    pub config_error: Option<String>,
    pub config_changed: bool,
    /// Config files selectable in the Config panel (`[` / `]` select, Enter apply).
    pub profiles: Vec<PathBuf>,
    /// Selection cursor into `profiles`.
    pub profile_cursor: usize,
    pub credentials_visible: bool,
    pub validation: Option<ValidationResult>,
    pub validating: bool,

    pub service: ServiceInstance,
    pub version: Option<String>,

    pub checks: Vec<crate::service::ConnectivityCheck>,
    pub custom_targets: Vec<String>,
    pub test_running: bool,
    pub measured: Option<MeasuredAddress>,
    pub measuring: bool,

    /// Current tunnel throughput (down, up) in bytes/s, and the sample it
    /// was diffed from.
    pub traffic_rate: Option<(f64, f64)>,
    last_traffic: Option<((u64, u64), std::time::Instant)>,
    /// Resulting public IP through the tunnel, for the window title.
    pub vpn_ip: Option<String>,
    ip_fetch_at: Option<std::time::Instant>,

    /// One-shot: attempt to start the service at launch when elevated and
    /// the service is down (user request 2026-10-05).
    autostart_tried: bool,

    pub logs: LogBuffer,
    pub log_paused: bool,
    /// Log view offset from the newest line (0 = follow, SC-005).
    pub log_offset_from_end: u16,

    pub focused: Panel,
    pub scroll_ping: u16,
    pub scroll_config: u16,
    pending_g: bool,

    pub input: Option<TargetInput>,
    pub dialog: Option<Dialog>,

    /// Verbatim error from the last failed action (FR-020, SC-008).
    pub action_error: Option<String>,
    pub action_note: Option<String>,

    pub quitting: bool,
    pub quit_outcome: Option<String>,
    pub refresh_in_flight: bool,
    starting: bool,
    stopping: bool,
    stop_after_start: bool,
}

impl App {
    pub fn new(elevated: bool, config_path: PathBuf) -> Self {
        let mut app = App {
            elevated,
            config_path: Some(config_path),
            focused: Panel::Status,
            ..App::default()
        };
        app.rescan_profiles();
        app
    }

    /// Load (or reload) the config; errors are recorded, never fatal (T032).
    pub fn load_config(&mut self) {
        self.config_error = None;
        let Some(path) = self.config_path.clone() else {
            self.config_error = Some("no config path resolved".to_string());
            return;
        };
        match std::fs::read_to_string(&path) {
            Ok(content) => match config::parse::parse(&content) {
                Ok(cfg) => {
                    self.config = Some(cfg);
                    self.config_changed = false;
                }
                Err(e) => {
                    self.config_error = Some(format!("{}: invalid JSON: {e}", path.display()));
                }
            },
            Err(e) => {
                self.config_error = Some(format!("{}: {e}", path.display()));
            }
        }
    }

    /// Rescan the profile directory (json files next to the current config)
    /// and park the selection cursor on the active profile.
    pub fn rescan_profiles(&mut self) {
        let Some(dir) = self.config_path.as_ref().and_then(|p| p.parent()) else {
            return;
        };
        self.profiles = scan_profiles(dir);
        // The current file always stays selectable even if the scan misses it.
        if let Some(cur) = &self.config_path
            && !self.profiles.contains(cur)
        {
            self.profiles.push(cur.clone());
        }
        self.profile_cursor = self
            .profiles
            .iter()
            .position(|p| Some(p) == self.config_path.as_ref())
            .unwrap_or(0);
    }

    /// Move the profile selection cursor (`[` previous, `]` next). Selection
    /// only; Enter applies it.
    pub fn select_profile_step(&mut self, forward: bool) {
        if self.profiles.len() < 2 {
            self.note("no other profiles found next to the config");
            return;
        }
        let len = self.profiles.len();
        self.profile_cursor = if forward {
            (self.profile_cursor + 1) % len
        } else {
            (self.profile_cursor + len - 1) % len
        };
    }

    /// Apply the selected profile (Enter): reload the view and, when the
    /// service runs, offer a restart to apply it for real.
    pub fn apply_selected_profile(&mut self, ctx: &mut Ctx) {
        if self.profiles.is_empty() {
            return;
        }
        let cursor = self.profile_cursor.min(self.profiles.len() - 1);
        let path = self.profiles[cursor].clone();
        if Some(&path) == self.config_path.as_ref() {
            self.note("selected profile is already active");
            return;
        }
        self.config_path = Some(path);
        self.scroll_config = 0;
        self.load_config();
        self.rescan_profiles();
        ctx.watch.reset();
        if let Some(p) = self.config_path.clone() {
            ctx.spawn_validate(p);
            self.validating = true;
        }
        let name = self
            .config_path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if self.service.state == LifecycleState::Running {
            self.dialog = Some(Dialog::SwitchProfile {
                name,
                pid: self.service.pid,
            });
        } else {
            self.note(format!("switched to {name} (service stopped)"));
        }
        self.recompute();
    }

    pub fn recompute(&mut self) {
        self.service
            .check_not_responding(crate::service::NOT_RESPONDING_AFTER);
        self.service.recompute_health(&self.checks);
        if let Some(cfg) = &self.config {
            self.service.active_outbound = config::active_outbound(cfg).map(|o| o.tag.clone());
        }
        // A stopped service has no resulting address; the next running
        // session refetches on its first tick.
        if self.service.state == LifecycleState::Stopped {
            self.vpn_ip = None;
            self.ip_fetch_at = None;
        }
    }

    /// Diff consecutive interface counter samples into a rate; no tunnel or
    /// a single sample leaves the rate unset.
    fn update_traffic(&mut self, sample: Option<(u64, u64)>) {
        match (sample, self.last_traffic.take()) {
            (Some(now), Some(((in_b, out_b), at))) => {
                let dt = at.elapsed().as_secs_f64();
                if dt >= 1.0 {
                    let down = now.0.saturating_sub(in_b) as f64 / dt;
                    let up = now.1.saturating_sub(out_b) as f64 / dt;
                    self.traffic_rate = Some((down, up));
                }
                self.last_traffic = Some((now, std::time::Instant::now()));
            }
            (Some(now), None) => {
                self.last_traffic = Some((now, std::time::Instant::now()));
            }
            (None, _) => {
                self.traffic_rate = None;
            }
        }
    }

    /// Terminal window title: state + current throughput (user request
    /// 2026-10-05). Pure — unit-tested.
    pub fn title_text(&self) -> String {
        let mut title = format!("sing-box: {}", self.service.state.as_str());
        if let Some((down, up)) = self.traffic_rate
            && self.service.state == LifecycleState::Running
        {
            title.push_str(&format!(
                " · ↓{} ↑{}",
                crate::net::traffic::fmt_rate(down),
                crate::net::traffic::fmt_rate(up)
            ));
        }
        if let Some(ip) = &self.vpn_ip
            && self.service.state == LifecycleState::Running
        {
            title.push_str(&format!(" · {ip}"));
        }
        title
    }
    /// Check lookup by target for live updates.
    fn check_mut(&mut self, target: &str) -> Option<&mut crate::service::ConnectivityCheck> {
        self.checks.iter_mut().find(|c| c.target == target)
    }

    pub fn take_exit(&mut self) -> Option<String> {
        if let Some(outcome) = self.quit_outcome.clone()
            && !self.stopping
            && !self.starting
        {
            return Some(outcome);
        }
        None
    }

    fn note(&mut self, msg: impl Into<String>) {
        self.action_note = Some(msg.into());
        self.action_error = None;
    }

    fn error(&mut self, msg: impl Into<String>) {
        self.action_error = Some(msg.into());
        self.action_note = None;
    }

    /// Gating for control actions (FR-020a, SC-013): without elevation the
    /// action names what's needed within 1 s instead of failing.
    fn gate_control(&mut self, action: &str) -> bool {
        if !self.elevated {
            self.error(format!(
                "{action} requires elevated privileges — relaunch with: sudo sing-box-tui"
            ));
            return false;
        }
        true
    }

    // ---- Event application -------------------------------------------------

    pub fn apply(&mut self, ev: AppEvent, ctx: &mut Ctx) {
        match ev {
            AppEvent::Probe {
                probe,
                tunnel,
                traffic,
                changed,
            } => {
                let was = self.service.state;
                match probe {
                    Some(p) => self.service.apply_probe(&p),
                    None => self.service.apply_probe(&ProbedInstance::default()),
                }
                if self.service.state == LifecycleState::Running {
                    self.service.tunnel = tunnel;
                }
                if changed {
                    self.config_changed = true;
                }
                // A stop completed underneath us: interrupt in-flight tests (T040).
                if matches!(was, LifecycleState::Running | LifecycleState::Starting)
                    && self.service.state == LifecycleState::Stopped
                {
                    self.cancel_checks(ctx, Cancel::Service);
                    self.stopping = false;
                    if self.quitting {
                        self.quit_outcome = Some("service stopped".to_string());
                    }
                }
                self.refresh_in_flight = false;
                // Throughput: diff consecutive interface counters.
                self.update_traffic(traffic);
                // Autostart once: elevated + stopped + readable config.
                if !self.autostart_tried {
                    self.autostart_tried = true;
                    if self.elevated
                        && self.service.state == LifecycleState::Stopped
                        && self.config.is_some()
                        && !self.quitting
                    {
                        self.start_service(ctx);
                    }
                }
                self.recompute();
            }
            AppEvent::Version(v) => {
                self.version = Some(v);
                self.service.version = self.version.clone();
            }
            AppEvent::PublicIp(ip) => {
                // A failed refresh means traffic no longer demonstrably
                // exits at the old address (broken tunnel): show no IP
                // rather than a stale one.
                self.vpn_ip = ip;
            }
            AppEvent::StartDone(Ok(started)) => {
                let pid = started.pid;
                let _ = self.service.begin_starting(Some(pid));
                // Spawned process is alive: Running (tunnel may still be
                // coming up; health reads Degraded until it appears).
                let _ = self.service.confirm_running();
                self.starting = false;
                let stop_now = self.stop_after_start || self.quitting;
                self.stop_after_start = false;
                self.note(format!(
                    "started sing-box (pid {pid}){}",
                    if stop_now { " — stopping" } else { "" }
                ));
                ctx.adopt_child(started);
                self.logs.clear();
                if stop_now {
                    self.initiate_stop(ctx, false, self.quitting);
                }
                self.recompute();
            }
            AppEvent::StartDone(Err(e)) => {
                self.starting = false;
                // Verbatim validation error, no half-started state (T036, SC-008).
                self.error(e.to_string());
                if self.quitting {
                    self.quit_outcome = Some("service not started".to_string());
                }
                self.recompute();
            }
            AppEvent::StopDone {
                result,
                pid,
                then_start,
                was_quit,
            } => {
                self.stopping = false;
                match result {
                    Ok(()) => {
                        self.service.confirm_stopped();
                        if was_quit {
                            self.quit_outcome = Some("service stopped".to_string());
                        } else if then_start {
                            self.start_service(ctx);
                        } else {
                            self.note("service stopped");
                        }
                    }
                    Err(e) => {
                        // Failed stop: state what survived (FR-020).
                        self.error(format!("stop failed ({e}) — service still running"));
                        if was_quit {
                            self.quit_outcome = Some(format!(
                                "stop failed — service still running (pid {})",
                                pid.unwrap_or(0)
                            ));
                        }
                    }
                }
                self.cancel_checks(ctx, Cancel::Service);
                self.recompute();
            }
            AppEvent::CheckUpdate { target, progress } => {
                if let Some(c) = self.check_mut(&target) {
                    c.status = crate::service::CheckStatus::Running;
                    c.sent = progress.sent;
                    c.received = progress.received;
                    c.first_reply_latency_ms = progress.first_reply_latency_ms;
                }
            }
            AppEvent::CheckDone { target, outcome } => {
                if let Some(c) = self.check_mut(&target) {
                    c.status = outcome.status;
                    c.sent = outcome.sent;
                    c.received = outcome.received;
                    c.loss = outcome.loss;
                    c.latency_min_ms = outcome.latency_min_ms;
                    c.latency_avg_ms = outcome.latency_avg_ms;
                    c.latency_max_ms = outcome.latency_max_ms;
                    c.first_reply_latency_ms = outcome.first_reply_latency_ms;
                    c.failure_reason = outcome.failure_reason;
                } else {
                    self.checks.push(crate::service::ConnectivityCheck {
                        target: outcome.target.clone(),
                        status: outcome.status,
                        sent: outcome.sent,
                        received: outcome.received,
                        loss: outcome.loss,
                        latency_min_ms: outcome.latency_min_ms,
                        latency_avg_ms: outcome.latency_avg_ms,
                        latency_max_ms: outcome.latency_max_ms,
                        first_reply_latency_ms: outcome.first_reply_latency_ms,
                        failure_reason: outcome.failure_reason,
                        ..Default::default()
                    });
                }
                // Data-model invariants hold for every recorded outcome.
                if let Some(c) = self.check_mut(&target) {
                    debug_assert!(c.invariants_hold() || (c.sent == 0 && c.received == 0));
                }
                if !self
                    .checks
                    .iter()
                    .any(|c| c.status == crate::service::CheckStatus::Running)
                {
                    self.test_running = false;
                    self.action_note = None;
                }
                self.recompute();
            }
            AppEvent::MeasuredDone(m) => {
                self.measured = Some(m);
                self.measuring = false;
                self.action_note = None;
            }
            AppEvent::LogLine(entry) => {
                // Pause anchors the view: lines keep buffering, and the
                // offset from the end grows so the paused view stays put.
                if self.log_paused {
                    self.log_offset_from_end = self.log_offset_from_end.saturating_add(1);
                }
                self.logs.push(entry);
            }
            AppEvent::ValidateDone(v) => {
                self.validating = false;
                self.validation = Some(v);
            }
        }
    }

    // ---- Actions ------------------------------------------------------------

    pub fn refresh_now(&mut self, ctx: &Ctx) {
        if !self.refresh_in_flight {
            ctx.spawn_refresh();
            self.refresh_in_flight = true;
        }
    }

    pub fn start_service(&mut self, ctx: &Ctx) {
        if !self.gate_control("start") || self.starting {
            return;
        }
        if self.service.state != LifecycleState::Stopped {
            self.note("service is already running");
            return;
        }
        let Some(path) = self.config_path.clone() else {
            self.error("no config path resolved");
            return;
        };
        self.starting = true;
        self.note("validating config…");
        ctx.spawn_start(path);
    }

    /// Stop the service. External instances require confirmation (FR-019a).
    pub fn stop_service(&mut self, ctx: &mut Ctx, then_start: bool) {
        if !self.gate_control(if then_start { "restart" } else { "stop" }) || self.stopping {
            return;
        }
        match self.service.state {
            LifecycleState::Running | LifecycleState::Stopping => {}
            _ => {
                self.note("service is not running");
                return;
            }
        }
        if self.service.owned_by_app {
            self.initiate_stop(ctx, then_start, false);
        } else if let Some(pid) = self.service.pid {
            self.dialog = Some(Dialog::StopExternal { pid, then_start });
        } else {
            self.error("cannot stop: unknown pid");
        }
    }

    fn initiate_stop(&mut self, ctx: &mut Ctx, then_start: bool, was_quit: bool) {
        if self.service.begin_stopping().is_ok() || self.service.state == LifecycleState::Stopping {
            self.service.state = LifecycleState::Stopping;
        }
        self.stopping = true;
        self.cancel_checks(ctx, Cancel::Service);
        self.note("stopping service…");
        ctx.spawn_stop(then_start, was_quit);
        self.recompute();
    }

    pub fn restart_service(&mut self, ctx: &mut Ctx) {
        if !self.gate_control("restart") || self.stopping || self.starting {
            return;
        }
        if self.service.state != LifecycleState::Running {
            self.note("restart needs a running service");
            return;
        }
        self.stop_service(ctx, true);
    }

    pub fn run_test(&mut self, ctx: &mut Ctx) {
        let Some(cfg) = &self.config else {
            self.error("config not loaded — cannot derive targets");
            return;
        };
        let mut targets = net::default_targets(cfg);
        for t in &self.custom_targets {
            if !targets.contains(t) {
                targets.push(t.clone());
            }
        }
        self.spawn_tests(ctx, targets);
    }

    fn spawn_tests(&mut self, ctx: &mut Ctx, targets: Vec<String>) {
        let Some(cfg) = &self.config else { return };
        let paired: Vec<(String, String)> = targets
            .into_iter()
            .map(|t| {
                let route = net::route::infer_route(
                    &t,
                    &cfg.route.rules,
                    &cfg.route.final_,
                    &cfg.outbounds,
                )
                .as_str()
                .to_string();
                (t, route)
            })
            .collect();
        for (target, route) in paired {
            // Replace any previous result for the same target.
            self.checks.retain(|c| c.target != target);
            self.checks.push(crate::service::ConnectivityCheck {
                target: target.clone(),
                status: crate::service::CheckStatus::Running,
                inferred_route: route,
                ..Default::default()
            });
            ctx.spawn_check(target);
        }
        self.test_running = true;
        self.action_error = None;
        self.note(format!("testing {} target(s)…", self.checks.len()));
    }

    fn cancel_checks(&mut self, ctx: &Ctx, reason: Cancel) {
        if self.test_running {
            ctx.cancel_checks(reason);
        }
    }

    pub fn run_measured_check(&mut self, ctx: &Ctx) {
        if self.measuring {
            return;
        }
        self.measuring = true;
        let tunnel = if self.service.state == LifecycleState::Running {
            self.service.tunnel.as_ref().map(|t| t.name.clone())
        } else {
            None
        };
        ctx.spawn_measure(tunnel);
        self.note("measuring addresses…");
    }

    pub fn toggle_credentials(&mut self) {
        self.credentials_visible = !self.credentials_visible;
    }

    pub fn reload_config_confirmed(&mut self, ctx: &mut Ctx) {
        self.load_config();
        self.note(if self.config_error.is_none() {
            "config reloaded"
        } else {
            "config reload failed"
        });
        if let Some(path) = self.config_path.clone() {
            ctx.spawn_validate(path);
            self.validating = true;
        }
        ctx.watch.reset();
        self.recompute();
    }

    // ---- Quit flow (T039, T041) ----------------------------------------------

    pub fn request_quit(&mut self, ctx: &mut Ctx) {
        // End in-flight tests cleanly regardless of what follows (T041).
        self.cancel_checks(ctx, Cancel::User);
        match self.service.state {
            LifecycleState::Running | LifecycleState::Starting | LifecycleState::Stopping => {
                if self.service.owned_by_app || self.starting {
                    self.quitting = true;
                    if self.service.state == LifecycleState::Running {
                        self.initiate_stop(ctx, false, true);
                    } else if self.service.state == LifecycleState::Starting {
                        // Child not spawned yet: stop it the moment it lands
                        // (StartDone drives the rest).
                        self.stop_after_start = true;
                    } else {
                        // Stopping already in flight — its StopDone completes the quit.
                    }
                    self.note("quitting: stopping service…");
                } else if let Some(pid) = self.service.pid {
                    self.dialog = Some(Dialog::QuitExternal { pid });
                }
            }
            _ => {
                self.quit_outcome = Some(String::new());
            }
        }
    }

    // ---- Key routing ----------------------------------------------------------

    pub fn on_key(&mut self, key: KeyEvent, ctx: &mut Ctx) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        // Input mode: everything routes to the input (contracts/ui.md).
        if let Some(input) = &mut self.input {
            match key.code {
                KeyCode::Enter => {
                    let submitted = std::mem::take(&mut input.buffer);
                    self.input = None;
                    if !submitted.is_empty() && !self.custom_targets.contains(&submitted) {
                        self.custom_targets.push(submitted.clone());
                    }
                    if !submitted.is_empty() {
                        self.spawn_tests(ctx, vec![submitted]);
                    }
                }
                KeyCode::Esc => self.input = None,
                KeyCode::Backspace => {
                    input.buffer.pop();
                }
                KeyCode::Char(c) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                    input.buffer.push(c);
                }
                _ => {}
            }
            return;
        }
        // Dialog mode: y / n / Esc only.
        if self.dialog.is_some() {
            self.on_dialog_key(key, ctx);
            return;
        }

        // Two-key sequence: gg (top).
        if self.pending_g {
            self.pending_g = false;
            if let KeyCode::Char('g') = key.code {
                self.scroll_top();
                return;
            }
        }

        match (key.code, key.modifiers) {
            (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => self.request_quit(ctx),
            (KeyCode::Tab, _) => self.focused = self.focused.next(),
            (KeyCode::BackTab, _) => self.focused = self.focused.prev(),
            (KeyCode::Char('j'), _) | (KeyCode::Down, _) => self.scroll_down(),
            (KeyCode::Char('k'), _) | (KeyCode::Up, _) => self.scroll_up(),
            (KeyCode::Char('g'), _) => self.pending_g = true,
            (KeyCode::Char('G'), _) => self.scroll_bottom(),
            (KeyCode::Char('r'), _) => self.refresh_now(ctx),
            (KeyCode::Char('q'), _) => self.request_quit(ctx),
            (KeyCode::Char('s'), _) => self.start_service(ctx),
            (KeyCode::Char('x'), _) => self.stop_service(ctx, false),
            (KeyCode::Char('R'), _) => self.restart_service(ctx),
            (KeyCode::Char('c'), _) => self.run_test(ctx),
            (KeyCode::Char('C'), _) => {
                if self.test_running {
                    ctx.cancel_checks(Cancel::User);
                    self.note("cancelling test…");
                } else {
                    self.note("no test running");
                }
            }
            (KeyCode::Char('t'), _) => self.input = Some(TargetInput::default()),
            (KeyCode::Char('m'), _) => self.run_measured_check(ctx),
            (KeyCode::Char('v'), _) => self.toggle_credentials(),
            (KeyCode::Char('l'), _) => {
                if self.config_changed {
                    self.dialog = Some(Dialog::ReloadConfig);
                } else {
                    self.note("no config change detected");
                }
            }
            (KeyCode::Char('p'), _) => self.log_paused = !self.log_paused,
            (KeyCode::Char(']'), _) => self.select_profile_step(true),
            (KeyCode::Char('['), _) => self.select_profile_step(false),
            (KeyCode::Enter, _) => self.apply_selected_profile(ctx),
            _ => {}
        }
    }

    fn on_dialog_key(&mut self, key: KeyEvent, ctx: &mut Ctx) {
        let Some(dialog) = self.dialog.take() else {
            return;
        };
        let confirmed = matches!(
            key.code,
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter
        );
        let declined = matches!(
            key.code,
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc
        );
        if !confirmed && !declined {
            self.dialog = Some(dialog);
            return;
        }
        match dialog {
            Dialog::StopExternal { pid, then_start } => {
                if confirmed {
                    self.initiate_external_stop(ctx, pid, then_start, false);
                } else {
                    self.note("declined — service left running");
                }
            }
            Dialog::QuitExternal { pid } => {
                if confirmed {
                    self.quitting = true;
                    self.initiate_external_stop(ctx, pid, false, true);
                } else {
                    self.quit_outcome = Some(format!("service left running (pid {pid})"));
                }
            }
            Dialog::ReloadConfig => {
                if confirmed {
                    self.reload_config_confirmed(ctx);
                } else {
                    self.note("reload declined");
                }
            }
            Dialog::SwitchProfile { name, pid } => {
                if confirmed {
                    if !self.gate_control("restart") {
                        // Message already set; profile stays switched for view.
                    } else if self.service.owned_by_app {
                        self.initiate_stop(ctx, true, false);
                    } else if let Some(pid) = pid {
                        self.initiate_external_stop(ctx, pid, true, false);
                    }
                } else {
                    self.note(format!("switched to {name} (view) — 'R' restarts with it"));
                }
            }
        }
    }

    fn initiate_external_stop(
        &mut self,
        ctx: &mut Ctx,
        pid: u32,
        then_start: bool,
        was_quit: bool,
    ) {
        if self.service.begin_stopping().is_ok() || self.service.state == LifecycleState::Stopping {
            self.service.state = LifecycleState::Stopping;
        }
        self.stopping = true;
        self.cancel_checks(ctx, Cancel::Service);
        self.note("stopping service…");
        ctx.spawn_stop_pid(pid, then_start, was_quit);
    }

    // ---- Scrolling ------------------------------------------------------------

    fn scroll_down(&mut self) {
        match self.focused {
            Panel::Status => {}
            Panel::Ping => self.scroll_ping = self.scroll_ping.saturating_add(1),
            Panel::Config => self.scroll_config = self.scroll_config.saturating_add(1),
            Panel::Log => self.log_offset_from_end = self.log_offset_from_end.saturating_sub(1),
        }
    }

    fn scroll_up(&mut self) {
        match self.focused {
            Panel::Status => {}
            Panel::Ping => self.scroll_ping = self.scroll_ping.saturating_sub(1),
            Panel::Config => self.scroll_config = self.scroll_config.saturating_sub(1),
            Panel::Log => self.log_offset_from_end = self.log_offset_from_end.saturating_add(1),
        }
    }

    fn scroll_top(&mut self) {
        match self.focused {
            Panel::Status => {}
            Panel::Ping => self.scroll_ping = 0,
            Panel::Config => self.scroll_config = 0,
            Panel::Log => self.log_offset_from_end = u16::MAX,
        }
    }

    fn scroll_bottom(&mut self) {
        match self.focused {
            Panel::Status => {}
            Panel::Ping => self.scroll_ping = u16::MAX,
            Panel::Config => self.scroll_config = u16::MAX,
            Panel::Log => self.log_offset_from_end = 0,
        }
    }
}

/// Async plumbing owned by the event loop (not part of the debuggable state).
pub struct Ctx {
    evt_tx: mpsc::Sender<AppEvent>,
    bin: PathBuf,
    child: Option<tokio::process::Child>,
    cancel_tx: watch::Sender<Cancel>,
    pub watch: config::watch::ConfigWatch,
}

impl Ctx {
    fn new(evt_tx: mpsc::Sender<AppEvent>, bin: PathBuf) -> Self {
        let (cancel_tx, _) = watch::channel(Cancel::No);
        Ctx {
            evt_tx,
            bin,
            child: None,
            cancel_tx,
            watch: config::watch::ConfigWatch::default(),
        }
    }

    /// Take ownership of a started child and stream its output into the log
    /// panel (FR-018).
    fn adopt_child(&mut self, started: process::StartedService) {
        let mut child = started.child;
        if let Some(stdout) = child.stdout.take() {
            let tx = self.evt_tx.clone();
            tokio::spawn(stream_lines(stdout, tx));
        }
        if let Some(stderr) = child.stderr.take() {
            let tx = self.evt_tx.clone();
            tokio::spawn(stream_lines(stderr, tx));
        }
        self.child = Some(child);
    }

    fn spawn_refresh(&self) {
        let tx = self.evt_tx.clone();
        let known_pid = self.child.as_ref().and_then(|c| c.id());
        tokio::spawn(async move {
            // A probe error must not read as "service stopped": skip the
            // update entirely and let the next tick retry.
            let Ok(probe) = crate::service::probe::probe(known_pid).await else {
                return;
            };
            let tunnel = if probe.as_ref().is_some_and(|p| p.alive) {
                crate::service::tunnel::discover(None).await
            } else {
                None
            };
            let traffic = match &tunnel {
                Some(t) => crate::net::traffic::interface_bytes(&t.name).await,
                None => None,
            };
            let _ = tx
                .send(AppEvent::Probe {
                    probe,
                    tunnel,
                    traffic,
                    changed: false,
                })
                .await;
        });
    }

    fn spawn_start(&self, config_path: PathBuf) {
        let tx = self.evt_tx.clone();
        let bin = self.bin.clone();
        tokio::spawn(async move {
            let res = process::start(&bin, &config_path).await;
            let _ = tx.send(AppEvent::StartDone(res)).await;
        });
    }

    fn spawn_stop(&mut self, then_start: bool, was_quit: bool) {
        let tx = self.evt_tx.clone();
        if let Some(child) = self.child.take() {
            let pid = child.id();
            tokio::spawn(async move {
                let result = process::stop_child(child).await.map_err(|e| e.to_string());
                let _ = tx
                    .send(AppEvent::StopDone {
                        result,
                        pid,
                        then_start,
                        was_quit,
                    })
                    .await;
            });
        } else {
            // Nothing owned to stop: report completion immediately so a
            // pending quit never hangs waiting for a StopDone.
            tokio::spawn(async move {
                let _ = tx
                    .send(AppEvent::StopDone {
                        result: Ok(()),
                        pid: None,
                        then_start,
                        was_quit,
                    })
                    .await;
            });
        }
    }

    fn spawn_stop_pid(&mut self, pid: u32, then_start: bool, was_quit: bool) {
        let tx = self.evt_tx.clone();
        tokio::spawn(async move {
            let result = process::stop_pid(pid).await.map_err(|e| e.to_string());
            let _ = tx
                .send(AppEvent::StopDone {
                    result,
                    pid: Some(pid),
                    then_start,
                    was_quit,
                })
                .await;
        });
    }

    fn spawn_check(&self, target: String) {
        // Bridge the ping module's channel into the app event channel.
        let (check_tx, mut check_rx) = mpsc::channel(32);
        let cancel = self.cancel_tx.subscribe();
        net::ping::spawn_check(target, cancel, check_tx);
        let evt_tx = self.evt_tx.clone();
        tokio::spawn(async move {
            while let Some(ev) = check_rx.recv().await {
                let ev = match ev {
                    net::ping::CheckEvent::Progress(p) => AppEvent::CheckUpdate {
                        target: p.target.clone(),
                        progress: p,
                    },
                    net::ping::CheckEvent::Done(o) => AppEvent::CheckDone {
                        target: o.target.clone(),
                        outcome: o,
                    },
                };
                if evt_tx.send(ev).await.is_err() {
                    break;
                }
            }
        });
    }

    fn cancel_checks(&self, reason: Cancel) {
        self.cancel_tx.send_replace(reason);
    }

    fn spawn_measure(&self, tunnel: Option<String>) {
        let tx = self.evt_tx.clone();
        tokio::spawn(async move {
            let m = net::address::measure(tunnel.as_deref()).await;
            let _ = tx.send(AppEvent::MeasuredDone(m)).await;
        });
    }

    fn spawn_validate(&self, path: PathBuf) {
        let tx = self.evt_tx.clone();
        let bin = self.bin.clone();
        tokio::spawn(async move {
            let v = config::validate::check(&bin, &path).await;
            let _ = tx.send(AppEvent::ValidateDone(v)).await;
        });
    }

    fn spawn_version(&self) {
        let tx = self.evt_tx.clone();
        let bin = self.bin.clone();
        tokio::spawn(async move {
            if let Some(v) = crate::service::probe::singbox_version(&bin.to_string_lossy()).await {
                let _ = tx.send(AppEvent::Version(v)).await;
            }
        });
    }

    fn spawn_public_ip(&self, tunnel: Option<String>) {
        let tx = self.evt_tx.clone();
        tokio::spawn(async move {
            let ip = crate::net::address::public_ip(tunnel.as_deref()).await;
            let _ = tx.send(AppEvent::PublicIp(ip)).await;
        });
    }
}

/// Stream a child's pipe into LogLine events.
async fn stream_lines<R: tokio::io::AsyncRead + Unpin>(pipe: R, tx: mpsc::Sender<AppEvent>) {
    use tokio::io::{AsyncBufReadExt, BufReader};
    let mut lines = BufReader::new(pipe).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let entry = crate::logging::parse_line(&line);
        if tx.send(AppEvent::LogLine(entry)).await.is_err() {
            break;
        }
    }
}

/// List selectable `*.json` config files in a directory, sorted by name.
/// Hidden files and non-JSON files (e.g. `config.json.bak`) are skipped.
pub fn scan_profiles(dir: &std::path::Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.extension().is_some_and(|e| e == "json")
                && p.file_name()
                    .is_some_and(|n| !n.to_string_lossy().starts_with('.'))
        })
        .collect();
    paths.sort();
    paths
}

/// Main event loop. Returns the final-screen message for main to print after
/// restoring the terminal (T039).
pub async fn run(
    terminal: &mut ratatui::Terminal<ratatui::backend::CrosstermBackend<std::io::Stdout>>,
    elevated: bool,
    config_path: PathBuf,
) -> anyhow::Result<Option<String>> {
    let bin = process::find_singbox();
    let mut app = App::new(elevated, config_path.clone());
    app.load_config();

    let (evt_tx, mut evt_rx) = mpsc::channel::<AppEvent>(256);
    let mut ctx = Ctx::new(evt_tx, bin);
    let mut tick = interval(STATUS_TICK);
    let mut events = crossterm::event::EventStream::new();
    use futures::StreamExt;

    // Initial probe + validation + version.
    ctx.spawn_refresh();
    app.refresh_in_flight = true;
    ctx.spawn_validate(config_path);
    app.validating = true;
    ctx.spawn_version();

    let mut last_title = String::new();
    loop {
        terminal.draw(|f| ui::draw(f, &app))?;

        // Window title: state + throughput, written only on change.
        let title = app.title_text();
        if title != last_title {
            crossterm::execute!(
                terminal.backend_mut(),
                crossterm::terminal::SetTitle(title.clone())
            )?;
            last_title = title;
        }

        tokio::select! {
            ev = evt_rx.recv() => {
                match ev {
                    Some(ev) => app.apply(ev, &mut ctx),
                    None => break,
                }
            }
            maybe = events.next() => {
                match maybe {
                    Some(Ok(Event::Key(key))) => app.on_key(key, &mut ctx),
                    Some(Ok(Event::Resize(_, _))) => {} // redraw happens next loop
                    Some(Ok(_)) => {}
                    Some(Err(_)) | None => break,
                }
            }
            _ = tick.tick() => {
                if !app.refresh_in_flight {
                    ctx.spawn_refresh();
                    app.refresh_in_flight = true;
                }
                if app.version.is_none() {
                    ctx.spawn_version();
                }
                // Resulting public IP for the title: refresh every 30 s
                // while the tunnel is up (an echo request per tick would be
                // chatty).
                const IP_REFRESH: std::time::Duration = std::time::Duration::from_secs(30);
                let ip_due = app
                    .ip_fetch_at
                    .is_none_or(|at| at.elapsed() >= IP_REFRESH);
                if ip_due
                    && app.service.state == LifecycleState::Running
                    && let Some(tunnel) = app.service.tunnel.as_ref().map(|t| t.name.clone())
                {
                    app.ip_fetch_at = Some(std::time::Instant::now());
                    ctx.spawn_public_ip(Some(tunnel));
                }
                // Config change detection (FR-016).
                if let Some(path) = &app.config_path
                    && app.config_error.is_none()
                        && let Ok(changed) = ctx.watch.changed(path)
                            && changed && !app.config_changed {
                                app.config_changed = true;
                                app.note("config changed on disk — 'l' to reload");
                            }
            }
        }

        if let Some(outcome) = app.take_exit() {
            return Ok(if outcome.is_empty() {
                None
            } else {
                Some(outcome)
            });
        }
    }

    Ok(app.quit_outcome.take().filter(|s| !s.is_empty()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::service::LifecycleState;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::empty(),
        }
    }

    fn app_external_running() -> (App, Ctx) {
        let (tx, _rx) = mpsc::channel(8);
        let mut app = App::new(false, PathBuf::from("/tmp/config.json"));
        app.service.state = LifecycleState::Running;
        app.service.pid = Some(4242);
        app.service.owned_by_app = false;
        (app, Ctx::new(tx, PathBuf::from("/bin/true")))
    }

    /// Declining the quit dialog leaves the external instance running and the
    /// final screen states it (FR-019b, T039).
    #[tokio::test]
    async fn quit_external_declined_leaves_running() {
        let (mut app, mut ctx) = app_external_running();
        app.on_key(key(KeyCode::Char('q')), &mut ctx);
        assert_eq!(app.dialog, Some(Dialog::QuitExternal { pid: 4242 }));
        app.on_key(key(KeyCode::Char('n')), &mut ctx);
        assert_eq!(
            app.quit_outcome.as_deref(),
            Some("service left running (pid 4242)")
        );
    }

    /// Only y/n/Esc affect an open dialog; other keys are ignored.
    #[tokio::test]
    async fn dialog_ignores_other_keys() {
        let (mut app, mut ctx) = app_external_running();
        app.on_key(key(KeyCode::Char('q')), &mut ctx);
        app.on_key(key(KeyCode::Char('x')), &mut ctx);
        assert_eq!(app.dialog, Some(Dialog::QuitExternal { pid: 4242 }));
    }

    /// While the target input is open, all other keys route to the input —
    /// no panel actions, no quit (contracts/ui.md, T022).
    #[tokio::test]
    async fn target_input_captures_all_keys() {
        let (mut app, mut ctx) = app_external_running();
        app.on_key(key(KeyCode::Char('t')), &mut ctx);
        assert!(app.input.is_some());
        for c in ['q', 'x', 's', 'c'] {
            app.on_key(key(KeyCode::Char(c)), &mut ctx);
        }
        assert_eq!(app.input.as_ref().unwrap().buffer, "qxsc");
        assert_eq!(
            app.service.state,
            LifecycleState::Running,
            "no action taken"
        );
        app.on_key(key(KeyCode::Enter), &mut ctx);
        assert!(app.input.is_none());
        assert_eq!(app.custom_targets, vec!["qxsc".to_string()]);
    }

    /// Unprivileged start is gated with a message naming the needed action
    /// and never starts anything (FR-020a, SC-013).
    #[tokio::test]
    async fn unprivileged_start_gated() {
        let (mut app, mut ctx) = app_external_running();
        app.service.state = LifecycleState::Stopped;
        app.service.pid = None;
        app.on_key(key(KeyCode::Char('s')), &mut ctx);
        assert!(
            app.action_error
                .as_deref()
                .unwrap()
                .contains("sudo sing-box-tui")
        );
        assert_eq!(app.service.state, LifecycleState::Stopped);
        assert!(!app.starting);
    }

    /// Quit with no service exits silently with an empty outcome.
    #[tokio::test]
    async fn quit_without_service_is_silent() {
        let (tx, _rx) = mpsc::channel(8);
        let mut app = App::new(true, PathBuf::from("/tmp/config.json"));
        let mut ctx = Ctx::new(tx, PathBuf::from("/bin/true"));
        app.on_key(key(KeyCode::Char('q')), &mut ctx);
        assert_eq!(app.quit_outcome.as_deref(), Some(""));
        assert!(!app.quitting);
    }

    /// Stopped presentation: health is Down and the reason names it; ping
    /// results read as not measured while stopped (T017).
    #[test]
    fn stopped_presentation() {
        let mut app = App::new(false, PathBuf::from("/tmp/config.json"));
        app.checks.push(crate::service::ConnectivityCheck {
            target: "example.com".into(),
            status: crate::service::CheckStatus::Complete,
            sent: 4,
            received: 4,
            loss: 0.0,
            ..Default::default()
        });
        app.recompute();
        assert_eq!(app.service.health, crate::service::HealthStatus::Down);
        assert_eq!(
            app.service.health_reason.as_deref(),
            Some("service stopped")
        );
    }

    /// The title ends with the resulting public IP while running.
    #[test]
    fn title_includes_vpn_ip() {
        let mut app = App::new(true, PathBuf::from("/tmp/config.json"));
        app.service.state = LifecycleState::Running;
        app.traffic_rate = Some((1_258_291.2, 348_160.0));
        assert_eq!(app.title_text(), "sing-box: Running · ↓1.2 MB/s ↑340 KB/s");
        app.vpn_ip = Some("195.201.136.249".into());
        assert_eq!(
            app.title_text(),
            "sing-box: Running · ↓1.2 MB/s ↑340 KB/s · 195.201.136.249"
        );
        // Stopped never shows rates or the (stale) IP.
        app.service.state = LifecycleState::Stopped;
        app.recompute();
        assert_eq!(app.title_text(), "sing-box: Stopped");
        assert!(app.vpn_ip.is_none(), "stop clears the address");
    }

    /// A failed echo refresh (broken tunnel) drops the address instead of
    /// showing a stale exit IP.
    #[tokio::test]
    async fn failed_ip_refresh_clears_address() {
        let (mut app, mut ctx) = app_external_running();
        app.vpn_ip = Some("195.201.136.249".into());
        app.apply(AppEvent::PublicIp(None), &mut ctx);
        assert!(app.vpn_ip.is_none());
        assert_eq!(app.title_text(), "sing-box: Running");

        app.apply(AppEvent::PublicIp(Some("178.176.79.178".into())), &mut ctx);
        assert_eq!(app.vpn_ip.as_deref(), Some("178.176.79.178"));
    }

    /// Window title carries the state and, only while running with a rate,
    /// the current throughput.
    #[test]
    fn title_tracks_state_and_rate() {
        let mut app = App::new(true, PathBuf::from("/tmp/config.json"));
        assert_eq!(app.title_text(), "sing-box: Stopped");

        app.service.state = LifecycleState::Running;
        assert_eq!(app.title_text(), "sing-box: Running");

        app.traffic_rate = Some((1_258_291.2, 348_160.0));
        assert_eq!(app.title_text(), "sing-box: Running · ↓1.2 MB/s ↑340 KB/s");

        // Rate never shows while stopped (stale counters are not traffic).
        app.service.state = LifecycleState::Stopped;
        assert_eq!(app.title_text(), "sing-box: Stopped");
    }

    /// Two samples diff into a rate; one sample does not; losing the tunnel
    /// clears it.
    #[test]
    fn traffic_diffing() {
        let mut app = App::new(true, PathBuf::from("/tmp/config.json"));
        app.update_traffic(Some((1000, 500)));
        assert!(app.traffic_rate.is_none(), "first sample only baselines");

        // Simulate an aged sample: rewind the stored timestamp.
        if let Some((_bytes, at)) = app.last_traffic.as_mut() {
            *at = std::time::Instant::now() - std::time::Duration::from_secs(2);
        }
        app.update_traffic(Some((3000, 1500)));
        if let Some((down, up)) = app.traffic_rate {
            assert!((down - 1000.0).abs() < 5.0, "down {down}");
            assert!((up - 500.0).abs() < 5.0, "up {up}");
        } else {
            panic!("rate missing after second sample");
        }

        app.update_traffic(None);
        assert!(app.traffic_rate.is_none());
    }
}

#[cfg(test)]
mod profile_tests {
    use super::*;
    use std::fs;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("sb-tui-profile-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn scan_lists_sorted_json_only() {
        let dir = temp_dir("scan");
        fs::write(dir.join("config-ya.json"), "{}").unwrap();
        fs::write(dir.join("config.json"), "{}").unwrap();
        fs::write(dir.join("config.json.bak"), "{}").unwrap();
        fs::write(dir.join("notes.txt"), "x").unwrap();
        fs::write(dir.join(".hidden.json"), "{}").unwrap();

        let names: Vec<String> = scan_profiles(&dir)
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(
            names,
            vec!["config-ya.json", "config.json"],
            "sorted, no bak/txt/hidden"
        );
        assert!(scan_profiles(&dir.join("missing")).is_empty());
        fs::remove_dir_all(&dir).ok();
    }

    /// Selecting with [ / ] only moves the cursor; Enter applies the
    /// selection, reloads the config, and asks about a restart when the
    /// service runs.
    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::empty(),
        }
    }

    #[tokio::test]
    async fn profile_select_then_enter_applies() {
        let dir = temp_dir("switch");
        std::fs::write(dir.join("a.json"), r#"{"route":{"final":"direct"}}"#).unwrap();
        std::fs::write(dir.join("b.json"), r#"{"route":{"final":"proxy"}}"#).unwrap();

        let (tx, _rx) = mpsc::channel(8);
        let mut app = App::new(true, dir.join("a.json"));
        let mut ctx = Ctx::new(tx, PathBuf::from("/bin/true"));
        app.load_config();
        assert_eq!(app.profiles.len(), 2);
        assert_eq!(app.profile_cursor, 0, "cursor starts on the active profile");

        // Selection moves the cursor only — nothing reloads.
        app.on_key(key(KeyCode::Char(']')), &mut ctx);
        assert_eq!(app.profile_cursor, 1, "moved onto b.json");
        assert_eq!(app.config_path, Some(dir.join("a.json")), "no switch yet");

        // Enter applies (service stopped → silent switch).
        app.on_key(key(KeyCode::Enter), &mut ctx);
        assert_eq!(app.config_path, Some(dir.join("b.json")));
        assert_eq!(app.config.as_ref().unwrap().route.final_, "proxy");
        assert!(app.dialog.is_none());

        // Running service: applying a different profile asks about restart.
        app.service.state = LifecycleState::Running;
        app.service.pid = Some(999);
        app.on_key(key(KeyCode::Char('[')), &mut ctx);
        app.on_key(key(KeyCode::Enter), &mut ctx);
        assert!(matches!(app.dialog, Some(Dialog::SwitchProfile { .. })));
        // Decline keeps the switch view-only.
        app.on_key(key(KeyCode::Char('n')), &mut ctx);
        assert_eq!(app.config_path, Some(dir.join("a.json")));
        assert!(app.dialog.is_none());
        assert_eq!(
            app.service.state,
            LifecycleState::Running,
            "decline never stops anything"
        );

        // Enter on the already-active profile is a no-op.
        app.on_key(key(KeyCode::Enter), &mut ctx);
        assert_eq!(app.config_path, Some(dir.join("a.json")));

        std::fs::remove_dir_all(&dir).ok();
    }
}
