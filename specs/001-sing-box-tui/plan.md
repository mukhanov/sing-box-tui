# Implementation Plan: sing-box TUI

**Branch**: `001-sing-box-tui` | **Date**: 2026-10-05 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/001-sing-box-tui/spec.md`

## Summary

A keyboard-only terminal dashboard for the user's personal sing-box VPN on macOS.
One screen, four always-visible panels — Status, Ping, Config, Log — refreshed
continuously, answering "is my VPN up and is traffic actually flowing through it?"
in a single command (SC-001, SC-007).

Technical approach (from [research.md](./research.md)): a single Rust binary using
**Ratatui + crossterm** for rendering and **tokio** for concurrency. Because the
user's config does not enable sing-box's management API and must not be modified
(FR-026, FR-028), all status is derived from observable state: the sing-box
process (`pgrep`/`ps`), the tunnel interface (`utun*` discovery via `ifconfig`),
the parsed config file (native `serde_json`, never modified), and on-demand
connectivity tests. The app is launched with elevated privileges (`sudo
sing-box-tui`), owns the service process for the session, and stops it on quit
(FR-019b); it never prompts for a password itself (SC-014).

## Technical Context

**Language/Version**: Rust, stable toolchain (edition 2024). No Rust project
exists yet — the repository currently contains only `specs/` and tooling
directories; `cargo init` happens in the first implementation task.

**Primary Dependencies**:
- `ratatui` + `crossterm` — TUI rendering, resize events, key input, color-optional output
- `tokio` — async runtime: status polling, subprocess control, concurrent connectivity tests, log streaming
- `serde` / `serde_json` — read-only parsing of the sing-box config into a permissive typed model (unknown options preserved for display, not rejected)
- External commands already present on the target machine (verified 2026-10-05): `sing-box` 1.14.1 (`check`, `version`, `run`), `/sbin/ping`, `/usr/bin/curl`, `/sbin/ifconfig`
- No `jq` dependency — JSON parsing is native (research.md Task 4, revised)

**Storage**: N/A. Session-only in-memory state; nothing persists between runs
(spec Assumptions). No files written except the session PID file in `/tmp`
(research.md Task 2). Credentials are never written anywhere (FR-027).

**Testing**: `cargo test` — unit tests for config parsing, credential masking,
route inference, health derivation, and state transitions; integration tests
against a config fixture mirroring the user's real config; end-to-end manual
validation via the 10 scenarios in [quickstart.md](./quickstart.md).

**Target Platform**: macOS (darwin/arm64 primary; Intel secondary — universal
build is straightforward). Terminal ≥ 80×24; color optional; UTF-8 required
(Cyrillic / IDN content, spec Edge Cases).

**Project Type**: desktop-app — a single terminal binary launched from the shell
(FR-001), no daemon of its own.

**Performance Goals** (from Success Criteria):
- Status answerable within 3 s of launch (SC-001); auto-refresh ≤ every 3 s (SC-010)
- First ping latency displayed ≤ 2 s after test start; full default-target summary ≤ 10 s (SC-002)
- Full stop-and-start cycle from key press ≤ 15 s (SC-011)
- Measured address check answers ≤ 5 s (SC-015)
- Survives 20 consecutive resizes with all four panels visible (SC-005)

**Constraints**:
- Config file is strictly read-only (FR-026); works against the existing installation unchanged (FR-028)
- No credential values in any log/cache/file (FR-027); masked on screen by default (FR-015, SC-004)
- No password prompts — launch-time elevation only (FR-019, SC-014); config path resolved against the invoking user's home via `SUDO_USER`, not root's home (FR-020b)
- Keyboard-only operation; focused-panel marking must not depend on color (FR-021)
- Graceful degradation everywhere: missing binary, unreadable config, no privileges, no color, tiny terminal (FR-020a, FR-022–FR-024)

**Scale/Scope**: single user, single machine, one dashboard screen, four panels,
one config profile. Personal tool — no multi-user, multi-machine, or persistence concerns.

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

`.specify/memory/constitution.md` is an **unratified placeholder template** —
it contains no project-specific principles or gates. Rather than inventing
binding rules the user never agreed to, the default speckit gates were applied:

| Gate | Status | Notes |
|------|--------|-------|
| All NEEDS CLARIFICATION resolved before Phase 1 | PASS | Language, build, and distribution unknowns resolved in research.md Tasks 1 & 6 (Rust/Ratatui, cargo, direct binary) |
| Design artifacts complete and spec-consistent | PASS | data-model.md covers every Key Entity from the spec; contracts/ui.md covers layout, keys, degradation, performance |
| Simplicity / YAGNI | PASS | Single binary, single crate, no persistence, no config editing, no management API |

**Post-Phase-1 re-check (2026-10-05)**: PASS. During re-check, three
spec-to-artifact gaps were found and fixed rather than waved through:
`jq` dependency replaced with native `serde_json` (FR-028); `ObservedAddress`
extracted into its own entity and `CheckStatus`/`FailureReason` added to
`ConnectivityCheck` (FR-010a, FR-011, FR-012); DNS `strategy` field added
(FR-005); UI contract gained the target-input, cancel, measured-check, and
confirmation-dialog keys (FR-009, FR-012, FR-010a, FR-019a/b).

**Recommendation**: run `/speckit-constitution` before `/speckit-tasks` if the
user wants real project principles recorded; this plan treats the template as inert.

## Project Structure

### Documentation (this feature)

```text
specs/001-sing-box-tui/
├── plan.md              # This file (/speckit.plan command output)
├── research.md          # Phase 0 output (/speckit.plan command)
├── data-model.md        # Phase 1 output (/speckit.plan command)
├── quickstart.md        # Phase 1 output (/speckit.plan command)
├── contracts/
│   └── ui.md            # Phase 1 output: layout, key, degradation, performance contract
└── tasks.md             # Phase 2 output (/speckit.tasks command - NOT created by /speckit.plan)
```

### Source Code (repository root)

```text
src/
├── main.rs               # entry: privilege detection, terminal setup/teardown, final screen
├── app.rs                # App state, event loop, focus management, input mode & dialogs
├── ui/
│   ├── mod.rs            # draw(): grid assembly, focus-aware borders (color-independent)
│   ├── layout.rs         # Constraints for 80×24 minimum; resize adaptation
│   ├── status.rs         # lifecycle state, health badge, tunnel, outbounds summary
│   ├── ping.rs           # target list, per-target stats, inferred route, input field
│   ├── config_view.rs    # grouped name/value entries, masking, change notice
│   └── log_view.rs       # level-coloured log lines, pause
├── service/
│   ├── mod.rs            # ServiceInstance model + lifecycle state machine
│   ├── process.rs        # start/stop/restart, SIGTERM→5s→SIGKILL, owned-vs-external tracking
│   ├── probe.rs          # pgrep/ps polling: pid, uptime, version, config in use
│   └── tunnel.rs         # utun interface discovery, address, MTU
├── config/
│   ├── mod.rs            # read-only config model (permissive serde)
│   ├── parse.rs          # serde_json → model; unknown options preserved
│   ├── validate.rs       # `sing-box check` wrapper, verbatim error/warning passthrough
│   ├── mask.rs           # credential field detection + masking
│   └── watch.rs          # mtime change detection, reload offer
├── net/
│   ├── mod.rs
│   ├── ping.rs           # /sbin/ping driver: streaming first-reply, stats, cancellation
│   ├── route.rs          # inferred route per target from routing rules (no outside dependency)
│   └── address.rs        # measured address check: through tunnel vs direct, side by side
└── logging.rs            # capture service stdout/stderr lines with level

tests/
├── config_parse.rs       # fixture-based parsing, masking, validation passthrough
├── route_inference.rs    # rule evaluation order → expected outbound
└── health_rules.rs       # lifecycle/tunnel/check → badge derivation (SC-009a)
```

**Structure Decision**: single-project layout (template Option 1, Rust flavor).
A `net/` module group is added beyond the generic skeleton because connectivity
testing (inferred route vs measured address) is a distinct concern from service
control and config reading — the spec treats it as its own user story with its
own failure semantics (FR-010, FR-010a, FR-011). Everything stays in one crate:
splitting a ~2k-LOC personal tool into workspace crates would be ceremony, not
separation. Note for implementation: this directory is **not yet a git
repository** — the first task must `git init` and create branch
`001-sing-box-tui` before any commits.

## Complexity Tracking

> **Fill ONLY if Constitution Check has violations that must be justified**

None — no constitution violations; no complexity above the spec's own demands.
