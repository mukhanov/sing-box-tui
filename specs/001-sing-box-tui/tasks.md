---
description: "Task list for sing-box TUI implementation"
---

# Tasks: sing-box TUI

**Input**: Design documents from `/specs/001-sing-box-tui/`

**Prerequisites**: plan.md (required), spec.md (required for user stories), research.md, data-model.md, contracts/ui.md, quickstart.md

**Tests**: Included — the spec's "User Scenarios & Testing" section is marked *(mandatory)* and plan.md declares `cargo test` with `tests/health_rules.rs`, `tests/route_inference.rs`, `tests/config_parse.rs`. Tests cover pure logic only (derivation, inference, parsing/masking); UI and service-control behavior is validated via the quickstart.md scenarios at each checkpoint. Write test tasks first and confirm they fail before implementation.

**Organization**: Tasks grouped by user story (US1 Status P1, US2 Ping P2, US3 Config P3, US4 Control+Log P4) so each story is independently implementable and testable.

## Format: `[ID] [P?] [Story] Description`

- **[P]**: Can run in parallel (different files, no dependencies on incomplete tasks)
- **[Story]**: Which user story this task belongs to (US1–US4)
- File paths are relative to repository root, matching plan.md Project Structure

## Path Conventions

- **Single project**: `src/` and `tests/` at repository root (per plan.md)

---

## Phase 1: Setup (Shared Infrastructure)

**Purpose**: Repository and Rust project initialization

- [X] T001 Initialize git repository and create branch `001-sing-box-tui` (this directory is not yet a git repo; plan.md Structure Decision requires it before any commit)
- [X] T002 Initialize Rust project: `cargo init --name sing-box-tui`, then add dependencies to `Cargo.toml`: `ratatui`, `crossterm`, `tokio` (full features), `serde` (derive), `serde_json`; verify with `cargo build`
- [X] T003 [P] Configure formatting/lint: create `rustfmt.toml` (default rules) and add `[lints.clippy]` warnings-as-notes config in `Cargo.toml`; verify with `cargo fmt --check` and `cargo clippy`

---

## Phase 2: Foundational (Blocking Prerequisites)

**Purpose**: Terminal shell, layout skeleton, and read-only config model that EVERY user story depends on

**⚠️ CRITICAL**: No user story work can begin until this phase is complete

- [X] T004 Implement terminal lifecycle in `src/main.rs`: enter raw mode + alternate screen, install panic hook that restores the terminal, and a teardown path that always leaves the shell prompt intact with no leftover screen content (FR-021, SC-006)
- [X] T005 Implement application core in `src/app.rs`: `App` state struct with `focusedPanel` (Status, Ping, Config, Log), input-mode and dialog routing (while target input or a dialog is open, all other keys are ignored — contracts/ui.md), and a tokio event loop mixing crossterm key events, resize events, and a 3-second status tick (SC-010)
- [X] T006 Implement dashboard layout in `src/ui/layout.rs` + `src/ui/mod.rs`: Status fixed-height top row, remaining space split into three columns (Ping, Config, Log) per contracts/ui.md; minimum 80×24 with all four panels visible and Status fully visible; focused panel marked with a bold border that does not depend on color (FR-013, FR-022); panels render placeholder content; resize redraws without crash
- [X] T007 Implement config path resolution in `src/config/mod.rs`: resolve `~/.config/sing-box/config.json` against the invoking user's home via `SUDO_USER` (falling back to `$HOME` only when not elevated), display the resolved absolute path, and never touch the elevated account's home (FR-020b, FR-025)
- [X] T008 Implement read-only config model in `src/config/parse.rs`: serde-deserialize `config.json` into permissive typed structs — `Outbound` (tag, type, server, port), `DNSConfig` (servers[], strategy), `RoutingRule` (conditions, outbound, order), tun inbound (interface_name, address, mtu) — where unknown sing-box options are preserved as raw JSON for display instead of failing parse (FR-028; data-model.md "Configuration File Structure")
- [X] T009 [P] Implement privilege detection in `src/main.rs`: detect at launch whether the process has the privileges needed to control the service (euid); expose the result as app state for the US4 banner and action gating (FR-020a)

**Checkpoint**: Foundation ready — `cargo run` shows four placeholder panels, survives resize, exits cleanly. User story implementation can now begin.

---

## Phase 3: User Story 1 — See at a glance whether the VPN is working (Priority: P1) 🎯 MVP

**Goal**: Status panel reports the service lifecycle, pid, uptime, version, config path, tunnel details, outbounds, DNS, and a health badge — refreshed in place every ≤3 s, derived entirely from observable state (no management API).

**Independent Test**: quickstart.md Scenario 1 — start the service manually, launch the TUI, confirm Running + pid + uptime + tunnel + active outbound within 3 seconds; stop the service, relaunch, confirm Stopped with no false "healthy".

### Tests for User Story 1

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [X] T010 [P] [US1] Write `tests/health_rules.rs`: table-driven tests of health derivation. Quote data-model.md HealthStatus rules verbatim as the spec: `Down` when state is Stopped; `Degraded` when state is Running but tunnel is missing or has no address; `Degraded` when any connectivity check has failed or been blocked; `Healthy` only when state is Running, tunnel is present with address, and no connectivity check has failed

### Implementation for User Story 1

- [X] T011 [US1] Implement `ServiceInstance` + lifecycle state machine in `src/service/mod.rs`: states exactly Stopped, Starting, Running, Stopping with transitions `Stopped → Starting → Running → Stopping → Stopped` (data-model.md); a transition older than 10 s keeps showing the transition state and marks it "not responding" with the reason where known (FR-002a); track `pid`, `startTime`, `version`, `configPath`, `activeOutbound`, `health`, `healthReason`
- [X] T012 [P] [US1] Implement process probing in `src/service/probe.rs`: find a running instance via `pgrep`/`ps` (match `sing-box run` command line), read pid and start time for uptime, run `sing-box version` once and cache it; derive config-in-use from the process command line (FR-002, FR-025)
- [X] T013 [P] [US1] Implement tunnel discovery in `src/service/tunnel.rs`: prefer the config's tun `interface_name`, else the newest `utun*` interface; read its address and MTU via `ifconfig`; absent interface or missing address must be reportable as a distinct condition (FR-003, spec Edge Cases)
- [X] T014 [US1] Implement active-outbound derivation in `src/config/mod.rs`: resolve the outbound currently handling traffic from `route.final` and selector defaults, and mark the remaining defined outbounds idle, listing each with type and destination (FR-004); surface DNS servers and strategy from `DNSConfig` (FR-005)
- [X] T015 [P] [US1] Implement the Status panel in `src/ui/status.rs`: lifecycle state, pid, uptime, version, config path, tunnel name/address/MTU, outbounds (active vs idle), DNS servers + strategy, and the health badge with reason for anything not Healthy; render into a fixed region so refresh never scrolls or moves the information being read (FR-002–FR-007)
- [X] T016 [US1] Wire status polling into the app tick in `src/app.rs`: auto-refresh ≤ every 3 s plus manual `r` (FR-006); when the service stops while open, state flips to Stopped within one refresh and the last known uptime is marked no longer current (spec US1 scenario 6)
- [X] T017 [US1] Implement stopped/degraded presentation in `src/ui/status.rs`: when Stopped, connectivity results show "not measured" instead of failures or stale numbers (spec US1 scenario 5); when Running with tunnel absent/addressless, lifecycle reads Running, badge reads Degraded, reason named on the same screen (spec US1 scenario 3a)

**Checkpoint**: User Story 1 fully functional — run quickstart.md Scenario 1 and the 80×24 check from Scenario 6; `cargo test --test health_rules` passes.

---

## Phase 4: User Story 2 — Test connectivity without leaving the TUI (Priority: P2)

**Goal**: Ping panel runs latency tests against default + custom targets with per-target stats, the config-selected route for every target, failure-reason classification, cancellation with partial results, and an on-demand measured address check.

**Independent Test**: quickstart.md Scenario 3 — with the service running, run the default target set, add a custom hostname, and confirm per-target latency/loss/route plus an unreachable target reported (not hung).

### Tests for User Story 2

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [X] T018 [P] [US2] Write `tests/route_inference.rs`: fixture-based tests of rule evaluation in order — first match wins, conditions from the user's config shape (domain_suffix, ip_cidr, port, protocol), final/default outcome applies when nothing matches (FR-010, FR-014)

### Implementation for User Story 2

- [X] T019 [US2] Implement route inference in `src/net/route.rs`: given a target, evaluate the parsed routing rules in order and return the selected outbound (proxy vs direct vs reject) — computed from the config alone, with no outside dependency (FR-010, SC-015)
- [X] T020 [US2] Implement default target derivation in `src/net/mod.rs`: tunnel gateway, configured DNS resolver, proxy server(s), and one public host — derived from the config (spec Confirmed Decisions)
- [X] T021 [US2] Implement the ping driver in `src/net/ping.rs`: drive `/sbin/ping` per target, stream-parse output so the first reply's latency is displayed within 2 s of test start (SC-002), accumulate `sent`, `received`, `loss`, `latencyMin/Avg/Max`, `firstReplyLatency` with constraints `received ≤ sent`, `loss` in [0, 100], `latencyMin ≤ latencyAvg ≤ latencyMax` (data-model.md); cancellation stops the test within 1 s and retains partial results (FR-008, FR-012)
- [X] T022 [US2] Implement custom target input in `src/app.rs` + `src/ui/ping.rs`: `t` opens the input, `Enter` submits, `Esc` closes; while open, all other keys route to the input (contracts/ui.md); the result appears in the same list as default targets (FR-009)
- [X] T023 [US2] Implement failure classification in `src/net/ping.rs`: distinguish exactly `NoReply`, `NameNotResolved`, `BlockedOrUnsupported` (data-model.md FailureReason values); a blocked test is never presented as a healthy result (FR-011, SC-009)
- [X] T024 [US2] Implement the measured address check in `src/net/address.rs`: on demand (`m`), fetch the apparent source address through the tunnel and without it via `/usr/bin/curl`, display side by side so a difference is obvious, answer within 5 s (SC-015); "reference unavailable" is distinguished from a tunnel failure and never reported as the tunnel failing (FR-010a, FR-011, spec Edge Cases)
- [X] T025 [US2] Implement the Ping panel in `src/ui/ping.rs`: per-target rows with stats + inferred route, measured-check result area, cancel key `C`; when the service is stopped, results are still produced but the panel clearly indicates the tunnel is down so the numbers must not be read as VPN results (spec US2 scenario 9)

**Checkpoint**: User Stories 1 and 2 both work independently — run quickstart.md Scenario 3; `cargo test --test route_inference` passes.

---

## Phase 5: User Story 3 — Read configuration options in plain terms (Priority: P3)

**Goal**: Config panel shows the config as grouped name/value sections (general/log, inbounds, outbounds, DNS, routing rules in evaluation order), credentials masked by default with a deliberate reveal, change detection with reload, and verbatim validation output.

**Independent Test**: quickstart.md Scenario 4 — with the service stopped, open the Config panel, confirm every section is represented, credentials masked by default, reveal requires `v`.

### Tests for User Story 3

> **NOTE: Write these tests FIRST, ensure they FAIL before implementation**

- [X] T026 [P] [US3] Write `tests/config_parse.rs`: parsing the fixture config preserves unknown options; credential masking by key-name pattern (uuid, password, secret, key, token) masks 100% of credential-bearing values on first display (SC-004); `sing-box check` output is passed through verbatim, warnings included (FR-017)

### Implementation for User Story 3

- [X] T027 [P] [US3] Implement credential masking in `src/config/mask.rs`: detect credential-bearing fields by key-name pattern (uuid, password, secret, key, token, short_id, public_key) and replace values with a mask character by default; revealing is only possible through the explicit `v` toggle (FR-015, SC-004, FR-027 — masked values are also what any logging must see)
- [X] T028 [P] [US3] Implement validation in `src/config/validate.rs`: run `sing-box check -c <path>` and capture stdout/stderr verbatim, separating errors from warnings; warnings are visible without blocking (spec Edge Cases); expose a validate-before-start hook for US4 (FR-017, SC-008)
- [X] T029 [US3] Implement the Config panel in `src/ui/config_view.rs`: grouped, labelled name/value entries covering general/log, inbounds, outbounds, DNS, routing rules; internal scrolling only — the panel must never push other panels off screen, even with many rules (FR-013a, spec Edge Cases)
- [X] T030 [US3] Implement routing-rule rendering in `src/ui/config_view.rs`: rules in evaluation order, each showing its conditions and its outcome (selected outbound or reject), including the final/default outcome (FR-014)
- [X] T031 [US3] Implement change detection in `src/config/watch.rs`: compare mtime (+size) each tick; when the file changed on disk, indicate it and offer reload via `l` with a y/n/Esc dialog per contracts/ui.md (FR-016)
- [X] T032 [US3] Implement missing/unreadable config handling in `src/ui/config_view.rs`: show a clear message naming the path that was tried and the error, never an empty panel or a crash, and never present the service as healthy on an invalid config (spec Edge Cases)

**Checkpoint**: User Stories 1–3 all work independently — run quickstart.md Scenarios 4, 9, 10; `cargo test --test config_parse` passes.

---

## Phase 6: User Story 4 — Control the service from the TUI (Priority: P4)

**Goal**: Start/stop/restart from the app with launch-time elevation (no password prompts), owned-vs-external instance tracking with confirmations, quit-stops-owned-service semantics, and the Log panel streaming the service's own output.

**Independent Test**: quickstart.md Scenarios 2, 5, 7, 8 — start/stop/restart cycle under 15 s; invalid config shows the exact error and leaves the TUI usable; quit stops an owned instance and states it; without sudo the app degrades but stays usable.

### Implementation for User Story 4

- [X] T033 [US4] Implement start in `src/service/process.rs`: validate first (T028 hook — SC-008), then spawn `sing-box run -c <resolved-path>` with stdout/stderr piped, record the child pid, mark the instance owned-by-app (FR-019, FR-019a); state transitions Starting → Running with the 10 s not-responding rule (FR-002a)
- [X] T034 [US4] Implement stop/restart in `src/service/process.rs`: SIGTERM, wait 5 s, then SIGKILL (research.md Task 2); an instance not started by the app is never stopped without a y/n/Esc confirmation dialog — declining leaves it running and says so (FR-019a, SC-012); restart is a full stop→start cycle completing within 15 s from key press (SC-011)
- [X] T035 [US4] Implement privilege gating in `src/app.rs` + `src/ui/mod.rs`: without elevation, name the action needed (relaunch with sudo) within 1 second, keep every unprivileged view working, present Start/Stop/Restart as unavailable rather than failing when pressed, and degrade unreadable privileged fields to "unavailable" with a hint (FR-020a, FR-024, SC-013); with elevation, no password prompt ever appears (SC-014)
- [X] T036 [US4] Implement action-outcome reporting in `src/ui/status.rs`: a failed start shows the exact error text on screen (from T028 passthrough) and the app stays usable after any outcome; no half-started state is left behind (FR-020, spec US4 scenario 4)
- [X] T037 [P] [US4] Implement log capture in `src/logging.rs`: read the spawned process's stdout/stderr into `LogEntry` lines (timestamp, level, message) kept in a bounded ring buffer (FR-018)
- [X] T038 [P] [US4] Implement the Log panel in `src/ui/log_view.rs`: recent lines with the level visible, new lines appended as produced, `p` pauses that panel only; when the instance was started externally (no captured stream), the panel says so instead of showing nothing (FR-018, FR-024)
- [X] T039 [US4] Implement the quit flow in `src/app.rs` + `src/main.rs`: quitting always stops an app-owned service and the final screen states it was stopped (FR-019b, SC-012a); an externally started instance triggers the ask-first dialog and the final screen states it was left running if declined; if the stop itself fails, the final screen states the service is still running and names the pid; terminal is always restored (spec Edge Cases)
- [X] T040 [US4] Implement in-flight test interruption in `src/app.rs`: when the service stops or restarts while a ping test is running, the test ends cleanly and is labelled Interrupted (spec Edge Cases, data-model.md `CheckStatus`)

**Checkpoint**: All four user stories functional — run quickstart.md Scenarios 2, 5, 7, 8.

---

## Phase 7: Polish & Cross-Cutting Concerns

**Purpose**: Robustness edges, terminal compatibility, and end-to-end validation

- [X] T041 [P] Harden quit-during-action paths: quitting while a test runs or while the service is starting/stopping ends the test and pending action cleanly, stops the service, and states the resulting state on the final screen (spec Edge Cases)
- [X] T042 [P] Verify monochrome and encoding robustness: run with no color support (e.g. `TERM=vt100`) and confirm all information stays readable and focus marking stays visible (FR-023); confirm Cyrillic and `.xn--p1ai` IDN content renders correctly (spec Edge Cases)
- [X] T043 Verify resize robustness: 20 consecutive resizes including down to 80×24 — all four panels stay visible, Status fully visible, no crash, no garbled output (FR-022, SC-005); verify 10 consecutive quits leave a clean terminal with the shell prompt intact (SC-006)
- [X] T044 [P] Write `README.md`: what it is, the `sudo sing-box-tui` launch model, key bindings table from contracts/ui.md, and the privilege/home-resolution note (FR-020b)
- [X] T045 Run the full quickstart.md validation (all 10 scenarios) and record pass/fail per scenario against its pass criteria

---

## Dependencies & Execution Order

### Phase Dependencies

- **Setup (Phase 1)**: No dependencies — start immediately
- **Foundational (Phase 2)**: Depends on Phase 1 — BLOCKS all user stories
- **User Stories (Phases 3–6)**: All depend on Phase 2 completion; otherwise independent of each other
- **Polish (Phase 7)**: Depends on the stories it hardens (T041 needs US2+US4; T043/T045 need all)

### User Story Dependencies

- **US1 Status (P1)**: After Phase 2 — no story dependencies (MVP)
- **US2 Ping (P2)**: After Phase 2 — uses the Phase 2 config model for route inference; independently testable
- **US3 Config (P3)**: After Phase 2 — independently testable with the service stopped
- **US4 Control+Log (P4)**: After Phase 2 — uses T028 (US3 validation) as its validate-before-start hook; otherwise independent

### Within Each User Story

- Tests first, failing, then implementation
- Models/services before UI panels
- Each story completes and validates at its checkpoint before the next priority begins

### Parallel Opportunities

- Phase 1: T003 alongside T001/T002 (after T002 for Cargo.toml, different files)
- Phase 2: T012 ∥ T013 ∥ T009 (different files); T015 ∥ T010 in Phase 3; T018 ∥ T019-adjacent work in Phase 4; T026 ∥ T027 ∥ T028 in Phase 5; T037 ∥ T038 in Phase 6
- After Phase 2, stories could proceed in parallel if separately staffed — for a single implementer, sequential P1 → P2 → P3 → P4

---

## Parallel Example: User Story 1

```bash
# Write tests and independent probes in parallel (different files):
Task: "T010 [P] [US1] tests/health_rules.rs"
Task: "T012 [P] [US1] src/service/probe.rs"
Task: "T013 [P] [US1] src/service/tunnel.rs"
# Then sequential integration:
Task: "T011 [US1] src/service/mod.rs state machine"
Task: "T015 [P] [US1] src/ui/status.rs"
Task: "T016 [US1] wire polling into src/app.rs"
```

---

## Implementation Strategy

### MVP First (User Story 1 Only)

1. Complete Phase 1: Setup
2. Complete Phase 2: Foundational (CRITICAL — blocks all stories)
3. Complete Phase 3: User Story 1 (status dashboard)
4. **STOP and VALIDATE**: quickstart.md Scenario 1 + Scenario 6 layout check
5. The MVP already replaces the "privileged terminal + second terminal" workflow for status (SC-007)

### Incremental Delivery

1. Setup + Foundational → four-panel skeleton that opens and exits cleanly
2. + US1 → live status MVP
3. + US2 → connectivity testing
4. + US3 → config reading
5. + US4 → service control and logs
6. Polish → edge hardening, full validation run

---

## Notes

- [P] tasks touch different files with no dependency on incomplete tasks
- Commit after each task or logical group, on branch `001-sing-box-tui`
- The config file is strictly read-only (FR-026); no task may write to it
- No credential values in any log, cache, or file (FR-027) — masking (T027) is the single source of truth
- External commands only: `sing-box`, `/sbin/ping`, `/usr/bin/curl`, `/sbin/ifconfig`, `pgrep`/`ps` — no `jq`, no other dependencies (research.md)
