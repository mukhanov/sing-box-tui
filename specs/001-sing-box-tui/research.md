# Research: sing-box TUI

**Created**: 2026-10-05
**Feature**: sing-box TUI (specs/001-sing-box-tui)

## Technical Context

| Category | Status | Notes |
|----------|--------|-------|
| Target platform | Clear | macOS (Apple Silicon, sing-box 1.14.x) |
| Privilege model | Clear | Launch-time elevation (sudo), no in-app password prompt |
| Configuration | Clear | `~/.config/sing-box/config.json`, read-only |
| Service control | Clear | Start/stop/restart via `sudo sing-box run` |
| API availability | Clear | No management API enabled; status from observable state |
| Terminal requirements | Clear | 80x24 minimum, color optional, keyboard-only |
| Language/framework | Clear | Rust + Ratatui (Task 1) |
| Build system | Clear | cargo, release build (Task 6) |
| Distribution | Clear | Direct binary; Homebrew tap optional (Task 6) |
| Environment verified | Clear | sing-box 1.14.1 at `/opt/homebrew/bin/sing-box`, darwin/arm64, go1.26.8 runtime |

## Research Tasks

### Task 1: TUI Framework Selection

**Context**: The application must be a terminal UI with four panels (Status, Ping, Config, Log), keyboard navigation, and graceful degradation for small terminals and no-color output.

**Decision**: **Ratatui (Rust)**

**Rationale**:
- Mature, actively maintained TUI library for Rust
- Built-in support for terminal resize, color detection, and keyboard input
- Cross-platform (macOS, Linux, Windows)
- Strong ecosystem for async operations (ping tests, service monitoring)
- Single binary deployment (no runtime dependencies)
- Performance: minimal overhead for polling service state

**Alternatives considered**:
- **Bubble Tea (Go)**: Good framework, but Go TUI ecosystem less mature than Rust's
- **Python (curses/asciimatics)**: Requires Python runtime, harder to distribute as single binary
- **Node.js (blessed/blessed-contrib)**: Runtime dependency, larger footprint

---

### Task 2: Process Control & Privilege Handling

**Context**: The application must start/stop/restart sing-box with elevated privileges, detect externally started instances, and handle privilege failures gracefully.

**Decision**: **Use `sudo` wrapper with process tracking**

**Rationale**:
- macOS `sudo` provides standard privilege escalation
- Track PID via `pgrep sing-box` to detect externally started instances
- Use `launchctl` to query service state if available
- Graceful degradation: show "unavailable" for fields requiring root

**Implementation notes**:
- Store PID file in `/tmp/sing-box-tui.<user>.pid` for session tracking
- Use `ps -o pid,command` to verify process ownership
- On quit, send SIGTERM with 5-second timeout, then SIGKILL

---

### Task 3: Connectivity Testing Strategy

**Context**: The application must show inferred route (from config) and measured address (from actual traffic) for ping targets.

**Decision**: **Hybrid approach with external reference service**

**Rationale**:
- Inferred route: parse config to determine which outbound handles each target
- Measured address: use public IP echo service (e.g., `https://api.ipify.org`)
- Run test through tunnel vs. direct to compare addresses
- Graceful fallback: if reference unavailable, show inferred route only

**Implementation notes**:
- Use `curl` or `wget` for HTTP-based IP echo (available on macOS)
- Alternative: DNS-based resolution to different endpoints
- Cache results to avoid excessive external requests

---

### Task 4: Configuration Parsing

**Context**: The application must read and display sing-box JSON configuration without modifying it.

**Decision**: **Parse JSON natively with `serde_json`** (revised 2026-10-05; earlier draft chose `jq`)

**Rationale**:
- `jq` is a Homebrew package on macOS (`/opt/homebrew/bin/jq`), not a system standard — depending on it violates the "works with the existing installation" constraint (FR-028) for anyone without it
- `serde`/`serde_json` is already a project dependency (config model, IPC-free typed access); a second JSON parser adds a failure mode for no benefit
- Typed deserialization into the config model gives compile-time structure where `jq` gives stringly-typed extraction

**Implementation notes**:
- Validate config with `sing-box check -c <path>` before displaying (verbatim error/warning passthrough, FR-017)
- Unknown sing-box options must not fail parsing: deserialize into a permissive model (known fields typed, unknowns kept as raw JSON for display)
- Mask credential fields by key-name pattern (uuid, key, secret, password, token) at the model layer
- Detect file changes via mtime comparison

---

### Task 5: Terminal Rendering

**Context**: The application must display four panels simultaneously, with focus indication and internal scrolling.

**Decision**: **Grid-based layout with ratatui**

**Rationale**:
- Ratatui supports flexible layout (Constraint, Direction)
- Built-in widgets for lists, tables, paragraphs
- Focus management via key events
- Color-independent focus indication (bold/reverse)

**Layout strategy**:
- Status panel: top row (fixed height, always visible)
- Remaining space split into three equal columns (Ping, Config, Log)
- Focus indicator: bold border around active panel
- Scroll internally within focused panel

---

### Task 6: Build & Distribution

**Context**: The application must be distributable as a single binary to a macOS user.

**Decision**: **Cross-compile with cargo, distribute via Homebrew tap or direct download**

**Rationale**:
- Rust's `cargo` produces static binaries by default
- Homebrew tap provides versioned updates
- Direct download for quick installation

**Implementation notes**:
- Target: `aarch64-apple-darwin` (Apple Silicon) and `x86_64-apple-darwin` (Intel)
- Use `cargo cross` for cross-compilation
- Sign binary with developer certificate for notarization

---

## Consolidated Decisions

| Area | Decision | Rationale |
|------|----------|-----------|
| Language | Rust | Single binary, performance, mature TUI ecosystem |
| TUI Framework | Ratatui | Active maintenance, cross-platform, keyboard-first |
| Process Control | sudo wrapper + PID tracking | Standard macOS privilege model, graceful degradation |
| Connectivity | HTTP IP echo + config parsing | Works without API, shows inferred + measured |
| Config Parsing | serde_json + sing-box check | No external JSON dependency; typed model |
| Layout | Grid with ratatui | Flexible, focus indication, internal scrolling |
| Distribution | Homebrew tap + direct download | Easy updates, single binary |

## Next Steps

1. **Initialize Rust project** with `cargo init --name sing-box-tui`
2. **Add dependencies**: ratatui, crossterm, serde, serde_json, jq (via shell)
3. **Implement core loop**: status polling, key handling, panel rendering
4. **Implement process control**: start/stop/restart with PID tracking
5. **Implement connectivity**: IP echo service + config-based routing
6. **Implement config viewer**: jq parsing + credential masking
7. **Implement log viewer**: tail sing-box output
8. **Add terminal resize handling**
9. **Add color detection + graceful degradation**
10. **Build and test on target macOS**

## Open Questions

- [ ] Should the application support both Apple Silicon and Intel macOS? (Recommended: yes, cross-compile)
- [ ] Should the application include a built-in config editor? (Answer: No, read-only per spec)
- [ ] Should the application persist any state between sessions? (Answer: No, session-only per spec)
