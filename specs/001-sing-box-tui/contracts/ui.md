# UI Contract: sing-box TUI

**Created**: 2026-10-05
**Feature**: sing-box TUI (specs/001-sing-box-tui)

## Overview

This document defines the user interface contract for the sing-box TUI application. It specifies the layout, navigation, and interaction patterns that the implementation must follow.

## Layout Contract

### Panel Structure

The application displays four panels simultaneously on a single screen:

```
┌─────────────────────────────────────────────────────────────┐
│ STATUS (always visible, fixed height)                       │
├──────────────┬──────────────┬──────────────┬───────────────┤
│   PING       │    CONFIG    │     LOG      │  (focusable)  │
│  (scroll)    │   (scroll)   │   (scroll)   │               │
└──────────────┴──────────────┴──────────────┴───────────────┘
```

### Panel Responsibilities

| Panel | Content | Scrollable | Focus Indication |
|-------|---------|------------|------------------|
| Status | Service state, tunnel info, active outbound, health badge | No | Bold border |
| Ping | Connectivity test results (latency, loss, route) | Yes | Bold border |
| Config | Configuration options (grouped, credentials masked) | Yes | Bold border |
| Log | Service log output (level, message) | Yes | Bold border |

### Minimum Terminal Size

- **Required**: 80 columns × 24 rows
- **Status panel**: Fixed at 6 lines minimum
- **Remaining panels**: Split equally in remaining space (18 lines / 3 = 6 lines each)

## Navigation Contract

### Key Bindings

| Key | Action |
|-----|--------|
| `Tab` | Move focus to next panel (Status → Ping → Config → Log → Status) |
| `Shift+Tab` | Move focus to previous panel |
| `j` / `↓` | Scroll down in focused panel |
| `k` / `↑` | Scroll up in focused panel |
| `gg` | Scroll to top of focused panel |
| `G` | Scroll to bottom of focused panel |
| `r` | Refresh status (manual) |
| `q` | Quit application |
| `s` | Start service (when stopped) |
| `x` | Stop service (when running) |
| `R` | Restart service |
| `c` | Run connectivity test (Ping panel) |
| `C` | Cancel running test, keep partial results (FR-012) |
| `t` | Open target input in Ping panel (FR-009); `Enter` submits, `Esc` closes |
| `m` | Run measured address check, on demand (FR-010a) |
| `v` | Toggle credential visibility (Config panel) |
| `l` | Reload config when a change is detected (FR-016) |
| `p` | Pause log output (Log panel) |

**Input mode**: while the target input is open, all other keys are routed to the
input (no panel actions, no quit) until `Enter` or `Esc`.

**Confirmation dialogs**: stopping an externally started instance (FR-019a),
stopping such an instance on quit (FR-019b), and reloading a changed config
accept `y` (confirm) / `n` or `Esc` (decline). While a dialog is open, all other
keys are ignored.

### Focus Indication

- **Visual**: Bold border around focused panel
- **Color-independent**: Must be visible in monochrome terminals
- **No focus loss**: Focus must remain on a valid panel at all times

## Interaction Contract

### Service Control

| State | Available Actions | Behavior |
|-------|-------------------|----------|
| Stopped | Start | Launch `sudo sing-box run -c <config>` |
| Running | Stop, Restart | Send SIGTERM, wait 5s, SIGKILL if needed |
| Running (external) | Stop (with confirmation) | Ask before stopping externally started instance |
| Any | Quit | Stop service if started by app, exit |

### Connectivity Testing

| Action | Behavior |
|--------|----------|
| Run test | Test against default targets + user-entered targets |
| Cancel test | Stop within 1 second, retain partial results |
| Measured check | Show address through tunnel vs. without tunnel |
| Target unreachable | Show failure reason (no reply, DNS fail, blocked) |

### Configuration Display

| Action | Behavior |
|--------|----------|
| Open Config panel | Show all sections (log, dns, inbounds, outbounds, route) |
| View credentials | Mask by default, toggle with `v` |
| Config changed | Indicate change, offer reload |
| Config invalid | Show validation error from `sing-box check` |

## Error Handling Contract

### Graceful Degradation

| Condition | Behavior |
|-----------|----------|
| Terminal too small | Show error message, still render basic layout |
| No color support | Use bold/reverse for focus indication |
| Privilege unavailable | Show "unavailable" for privileged fields |
| Config missing | Show path tried and error message |
| Service not running | Show Stopped state, disable control actions |
| Connectivity test blocked | Show failure reason, not "healthy" |

### Terminal Cleanup

On quit, the application MUST:
1. Restore terminal to normal state (no leftover screen content)
2. Leave shell prompt intact
3. Stop service if it was started by the application
4. Report final state on screen before exit

## Accessibility Contract

### Keyboard-Only Operation

- All functionality accessible via keyboard
- No mouse dependency
- Focus indication visible without color

### Internationalization

- Support Cyrillic and internationalized domain names (e.g., `.xn--p1ai`)
- Terminal encoding: UTF-8

## Performance Contract

### Refresh Intervals

| Metric | Target |
|--------|--------|
| Status refresh | Every 3 seconds (automatic) |
| First ping reply | Within 2 seconds of test start |
| Full ping summary | Within 10 seconds |
| Service start/stop cycle | Within 15 seconds |
| Terminal resize handling | Immediate, no crash |

### Resource Usage

- Single binary, no external runtime dependencies
- Minimal memory footprint (< 50 MB typical)
- CPU usage: polling-based, < 5% typical
