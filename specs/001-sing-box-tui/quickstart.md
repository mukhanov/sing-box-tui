# Quickstart: sing-box TUI

**Created**: 2026-10-05
**Feature**: sing-box TUI (specs/001-sing-box-tui)

## Prerequisites

- macOS (Apple Silicon or Intel)
- sing-box 1.14.x installed and on PATH
- Configuration file at `~/.config/sing-box/config.json`
- Terminal with at least 80×24 size

## Setup

### 1. Verify sing-box Installation

```bash
# Check sing-box is available
which sing-box

# Check version
sing-box version

# Validate configuration
sing-box check -c ~/.config/sing-box/config.json
```

**Expected Output**:
- `which sing-box` → `/opt/homebrew/bin/sing-box` (or similar)
- `sing-box version` → `sing-box version 1.14.1`
- `sing-box check` → Exit code 0 (no errors)

### 2. Build the Application

```bash
# Clone or navigate to project
cd /Users/mukhanov/Projects/MINE/sing-box-tui

# Build release binary
cargo build --release

# Binary location
ls -lh target/release/sing-box-tui
```

**Expected Output**:
- Build completes without errors
- Binary at `target/release/sing-box-tui`

### 3. Install (Optional)

```bash
# Install to local bin
sudo cp target/release/sing-box-tui /usr/local/bin/
sudo chmod +x /usr/local/bin/sing-box-tui

# Verify installation
which sing-box-tui
```

## Validation Scenarios

### Scenario 1: Service Status Display

**Goal**: Verify the application shows service status correctly.

**Steps**:
1. Start sing-box manually: `sudo sing-box run -c ~/.config/sing-box/config.json &`
2. Note the PID from the output
3. Launch TUI: `sudo sing-box-tui`
4. Observe Status panel

**Expected Results**:
- Status panel shows "Running"
- Process ID matches the one from step 2
- Tunnel interface name, address, and MTU are displayed
- Active outbound is shown
- Health badge shows "Healthy" (if all checks pass)

**Cleanup**: Stop sing-box: `kill <PID>`

---

### Scenario 2: Service Control

**Goal**: Verify start/stop/restart functionality.

**Steps**:
1. Start with service stopped: `sudo sing-box-tui`
2. Press `s` to start service
3. Observe status transition: Stopped → Starting → Running
4. Press `x` to stop service
5. Observe status transition: Running → Stopping → Stopped

**Expected Results**:
- Start completes within 15 seconds
- Status updates automatically
- Stop completes cleanly (SIGTERM, then exit)
- Tunnel interface disappears after stop

---

### Scenario 3: Connectivity Testing

**Goal**: Verify ping functionality and route attribution.

**Steps**:
1. Ensure service is running
2. Navigate to Ping panel (Tab)
3. Press `c` to run connectivity test
4. Observe results for default targets

**Expected Results**:
- First latency appears within 2 seconds
- Full summary within 10 seconds
- Each target shows:
  - Sent/received counts
  - Loss percentage
  - Min/avg/max latency
  - Inferred route (proxy/direct)
- The measured address check (through tunnel vs. direct, side by side) is available on demand (`m` key) and answers within 5 seconds

**Test Targets** (default set):
- Tunnel gateway (172.18.0.1)
- DNS resolver (8.8.8.8)
- Proxy server (i.warmine.tech)
- Public host (example.com)

---

### Scenario 4: Configuration Display

**Goal**: Verify configuration viewer and credential masking.

**Steps**:
1. Navigate to Config panel (Tab)
2. Observe configuration sections
3. Press `v` to toggle credential visibility

**Expected Results**:
- Sections displayed: log, dns, inbounds, outbounds, route
- Credential fields masked by default (e.g., `****`)
- Toggle with `v` reveals actual values
- Routing rules shown in evaluation order

---

### Scenario 5: Log Viewer

**Goal**: Verify log output display.

**Steps**:
1. Ensure service is running
2. Navigate to Log panel (Tab)
3. Observe log lines with levels

**Expected Results**:
- Recent log lines displayed
- Log level visible (info, warn, error)
- New lines appear as produced
- Press `p` to pause output

---

### Scenario 6: Terminal Resize

**Goal**: Verify graceful handling of terminal resize.

**Steps**:
1. Launch application
2. Resize terminal (Cmd+ in Terminal, or Ctrl+Shift+/-)
3. Observe layout adaptation

**Expected Results**:
- All four panels remain visible
- No crash or garbled output
- Status panel remains fully visible
- Config and Log panels scroll internally

---

### Scenario 7: Quit Behavior

**Goal**: Verify clean exit and service handling.

**Steps**:
1. Start application with service stopped
2. Start service from application (press `s`)
3. Press `q` to quit
4. Verify service is stopped

**Expected Results**:
- Service stops on quit
- Final screen states "Service stopped"
- Terminal left in normal state
- Shell prompt intact, no leftover content

---

### Scenario 8: Privilege Handling

**Goal**: Verify graceful degradation without elevated privileges.

**Steps**:
1. Launch without sudo: `sing-box-tui`
2. Observe behavior

**Expected Results**:
- Application opens
- Status panel shows "unavailable" for privileged fields
- Start/Stop/Restart actions unavailable (not failing)
- Message tells user to relaunch with sudo

---

### Scenario 9: Config Change Detection

**Goal**: Verify config change detection and reload.

**Steps**:
1. Open Config panel
2. In another terminal, modify config file
3. Return to TUI, observe notification
4. Press `r` to reload

**Expected Results**:
- Config panel indicates file changed
- Reload updates displayed configuration

---

### Scenario 10: Invalid Config Handling

**Goal**: Verify error handling for invalid configuration.

**Steps**:
1. Introduce syntax error in config
2. Try to start service from TUI

**Expected Results**:
- Service does not start
- Exact error message from `sing-box check` shown
- TUI remains usable

## Success Criteria

All scenarios must pass for the feature to be considered complete:

| Scenario | Pass Criteria |
|----------|---------------|
| 1. Service Status | Status panel shows correct state within 3 seconds |
| 2. Service Control | Start/stop completes within 15 seconds |
| 3. Connectivity | First latency within 2s, full summary within 10s |
| 4. Config Display | All sections visible, credentials masked by default |
| 5. Log Viewer | Recent logs visible, new lines appended |
| 6. Terminal Resize | No crash, all panels visible at 80×24 |
| 7. Quit Behavior | Service stopped, terminal cleaned up |
| 8. Privilege Handling | Graceful degradation, clear message |
| 9. Config Change | Change detected, reload works |
| 10. Invalid Config | Error shown, TUI remains usable |

## Next Steps

After validation passes:

1. **Integration Testing**: Run all scenarios in sequence
2. **Performance Testing**: Verify refresh intervals and resource usage
3. **User Acceptance**: Test with actual sing-box configuration
4. **Deployment**: Package for distribution (Homebrew tap or direct download)

## References

- [Feature Specification](../spec.md)
- [Data Model](../data-model.md)
- [UI Contract](contracts/ui.md)
- [Research](../research.md)
