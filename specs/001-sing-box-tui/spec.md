# Feature Specification: sing-box TUI

**Feature Branch**: `001-sing-box-tui`

**Created**: 2026-10-05

**Status**: Draft

**Input**: User description: "хочу сделать интерфейс tui для sing-box который я использую для VPN. Мой конфиг лежит тут ~/.config/sing-box/config.json, запускаю его как sudo sing-box run -c ~/.config/sing-box/config.json. Я хочу чтобы у него был понятный гуй со статусом, с возможностью посмотреть опции, с пингом. Запускать планирую так же из терминала."

## Clarifications

### Session 2026-10-05

- Q: How should the application get the elevated privileges it needs to start and stop the service? (FR-019) → A: The user launches the application itself with elevated privileges; the application never prompts for a password itself.
- Q: What should happen to the service when you quit the application? (FR-019b) → A: Quitting always stops the service; the application owns the service process for the whole session.
- Q: What states should the status panel be able to show for the service? (FR-007) → A: Four lifecycle states (Stopped, Starting, Running, Stopping) plus a separate health badge (Healthy, Degraded, Down) derived from tunnel and connectivity state.
- Q: How should you move between the four views — status, ping, config, and log? (FR-006, FR-021) → A: A single dashboard screen showing all four panels at once; keys move focus and scroll inside the focused panel.
- Q: How should the application decide which route a connectivity test actually took — through the proxy or direct? (FR-010) → A: Both. The route the configuration selects is shown for every target, and a measured check that shows the address the traffic actually appears from runs on demand.

## User Scenarios & Testing *(mandatory)*

### User Story 1 — See at a glance whether the VPN is working (Priority: P1)

I open one terminal window and within a couple of seconds I can answer the only question that matters: is my VPN up, and is my traffic actually going through it? I see the service state, how long it has been up, which outbound is currently active, and what the tunnel interface looks like — all on one screen, refreshed continuously, without me having to remember or type any commands.

**Why this priority**: This is the core value of the feature. Today I must keep a privileged terminal open running the service, then open a second terminal to test whether traffic is really being routed. Without a trustworthy live status, the other panels (Config and Ping) have little purpose.

**Independent Test**: Start the service the normal way, then launch the TUI and confirm the status panel reports the service as running with the correct process identifier, uptime, tunnel address and active outbound. Then stop the service and relaunch the TUI and confirm it reports the service as stopped without showing false "healthy" results. This slice delivers value on its own even if no other panel exists.

**Acceptance Scenarios**:

1. **Given** the service is running with the config at `~/.config/sing-box/config.json`, **When** I launch the TUI, **Then** the status panel shows the service as Running together with process identifier, uptime, the sing-box version in use, and the path of the config in use, within 3 seconds of launch.
2. **Given** the service is running, **When** the status panel refreshes, **Then** uptime and connectivity results update without the screen flickering or scrolling away the information I am reading.
3. **Given** the service is running, **When** I look at the status panel, **Then** I can see the tunnel interface name, its assigned address and the configured MTU.
3a. **Given** the service process is running but the tunnel interface is missing or has no address, **When** I look at the status panel, **Then** the lifecycle state reads Running, the health badge reads Degraded, and the reason is named on the same screen.
4. **Given** the service is running, **When** I look at the status panel, **Then** I can see which outbound is currently handling traffic and which outbounds are defined but idle.
5. **Given** the service is not running, **When** I launch the TUI, **Then** the status panel shows Stopped, and connectivity results are shown as "not measured" rather than as failures or stale numbers.
6. **Given** the service stops while the TUI is open, **When** the next refresh occurs, **Then** the status changes to Stopped within one refresh interval and the last known uptime is clearly marked as no longer current.
7. **Given** the TUI is open and the service was started by the application, **When** I press the quit key, **Then** the service is stopped, the final screen states that it was stopped, and the terminal is left in a normal usable state with my shell prompt intact and no leftover screen content.
8. **Given** I choose Start or Stop and the transition does not complete within 10 seconds, **When** the status panel refreshes, **Then** the transition state is still shown and is additionally marked as not responding, with the reason where one is known.
9. **Given** the TUI is open in an 80x24 terminal without color support, **When** I look at the screen, **Then** all four panels are visible, the Status panel is fully visible, and the panel that has focus is marked in a way that does not depend on color.

---

### User Story 2 — Test connectivity without leaving the TUI (Priority: P2)

I press a key to test connectivity and immediately see round-trip times to a set of targets, so I can tell whether the problem — if there is one — is the tunnel, the remote server, or my own network. I can also type my own target and get the same result. For every target I see the route my configuration selects for it, and on demand I can measure the address my traffic actually appears from, through the tunnel and without it.

**Why this priority**: Ping is explicitly requested and is the practical way to prove the VPN is doing its job. It is second only to status because it depends on the status context (is the tunnel even up?) to be interpretable.

**Independent Test**: With the service running, move focus to the Ping panel, run a test against the default targets, then type a custom hostname and run a test against it. Confirm each target shows latency, loss, and the route the configuration selects for it, and that a deliberately unreachable target is reported as unreachable rather than hanging forever.

**Acceptance Scenarios**:

1. **Given** the service is running, **When** I move focus to the Ping panel, **Then** a default set of targets is listed and a test against them starts immediately or on my explicit confirmation.
2. **Given** a ping test is running, **When** the first reply arrives, **Then** the elapsed time for that first reply is displayed within 2 seconds of starting the test.
3. **Given** a ping test is running, **When** the test finishes, **Then** I see sent/received counts, loss percentage, and minimum/average/maximum latency for each target.
4. **Given** a connectivity test completes, **When** I look at the result, **Then** each target shows the route my configuration selects for it (through the proxy, or direct) so I can confirm the routing rules are working.
5. **Given** a connectivity test completes, **When** I ask for the measured check, **Then** I see the address my traffic appears from through the tunnel and the address it appears from without the tunnel, and the two are shown side by side so a difference is obvious.
6. **Given** I type a hostname or address into the target input, **When** I submit it, **Then** a test runs against that target and its result appears in the same list.
7. **Given** a target does not respond, **When** the test finishes, **Then** that target is shown as unreachable with a reason (no reply, name could not be resolved, or blocked) and the rest of the list is still shown.
8. **Given** a ping test is running, **When** I press the cancel key, **Then** the test stops within one second and the partial results are kept on screen.
9. **Given** the service is stopped, **When** I run a ping test, **Then** the results are still produced but the panel clearly indicates that the tunnel is down so the numbers must not be read as VPN results.
10. **Given** the outside reference needed for the measured check is unreachable, **When** I ask for the measured check, **Then** the measured result is shown as unavailable with the reason, and the route selected by the configuration is still shown for every target.

---

### User Story 3 — Read my configuration options in plain terms (Priority: P3)

I move focus to the Config panel and see my configuration as a readable, grouped list — tunnel settings, outbounds, DNS servers, and routing rules — instead of a wall of JSON. I can see the important values (server, port, protocol, TLS settings, routing destinations) without exposing my credentials on screen by default, and I can check that the config the running service is using is the file I think it is.

**Why this priority**: Explicitly requested ("посмотреть опции"). It is lower priority than status and ping because it is consulted occasionally rather than constantly, and it does not depend on the service running.

**Independent Test**: Launch the TUI with the service stopped and move focus to the Config panel. Confirm every section of the current config is represented, that the credential fields are masked by default, and that revealing them requires a deliberate action.

**Acceptance Scenarios**:

1. **Given** the config file exists and is valid, **When** I move focus to the Config panel, **Then** I see the configuration grouped into sections (general/log, inbounds, outbounds, DNS, routing rules) with each option shown as a name and value.
2. **Given** the Config panel has focus, **When** I look at credential-bearing fields, **Then** their values are masked by default and I can reveal them only through an explicit action.
3. **Given** the Config panel has focus, **When** I look at the routing rules, **Then** I can see each rule's conditions and its outcome (which outbound it selects, or reject) in the order they are evaluated.
4. **Given** the Config panel has focus, **When** the config file on disk changes, **Then** the panel indicates that the file has changed since it was last read and offers to reload it.
5. **Given** the config file is missing or cannot be read, **When** I move focus to the Config panel, **Then** I see a clear message naming the path that was tried instead of an empty panel or a crash.
6. **Given** I launched the application with elevated privileges, **When** I look at the config path shown in the Status or Config panel, **Then** it is the absolute path under my own home directory rather than the elevated account's home directory.

---

### User Story 4 — Control the service from the TUI (Priority: P4)

I want to start, stop, and restart the service from the same screen instead of switching to another terminal and typing a privileged command, and I want to see the service's own log output when something goes wrong. I launch the application with elevated privileges, so it never asks me for a password; the application owns the service for as long as it is open, and quitting it stops the service.

**Why this priority**: This is the natural completion of "status": once I can see that it is down, I want to bring it back up without leaving the screen. It is ranked last because the feature still delivers standalone value as a monitor, and this story carries the largest risk (privileged operation, killing the user's existing process). Confirmed in scope by the user.

**Independent Test**: From the TUI, start the service, confirm the status transitions to Running and the tunnel appears; stop the service, confirm the status transitions to Stopped and the tunnel disappears; restart it and confirm the same cycle. Confirm that a start attempt with an invalid config reports the error text and does not leave a half-started state.

**Acceptance Scenarios**:

1. **Given** the service is stopped and I have launched the application with elevated privileges, **When** I choose Start, **Then** the service starts, the status updates to Running, and no password is requested at any point.
2. **Given** I have launched the application without elevated privileges, **When** the application opens, **Then** it names the action I must take to gain the needed privileges, keeps showing everything that does not need them, and Start, Stop, and Restart are unavailable rather than silently failing.
3. **Given** the service is running, **When** I choose Stop, **Then** the service stops cleanly and the status updates to Stopped.
4. **Given** the config has an error, **When** I choose Start, **Then** the service does not start, the exact error message is shown on screen, and the TUI remains usable.
5. **Given** the service is running in another terminal (not started by the TUI), **When** I choose Stop, **Then** the TUI asks me to confirm that I intend to stop that externally started instance, and only stops it after I confirm; if I decline, nothing changes and I am told the instance is still running.
6. **Given** the service is running, **When** I move focus to the Log panel, **Then** I see the service's recent log lines with the message level visible, and new lines appear as they are produced unless I pause that panel.
7. **Given** the service is running and was started by the application, **When** I quit the application, **Then** the service is stopped, the final screen states that it was stopped, and the tunnel goes away.
8. **Given** the service is running but was started elsewhere, **When** I quit the application, **Then** the application asks whether to stop that externally started instance and stops it only if I agree; if I decline, it exits and leaves the instance running and says so.

---

### Edge Cases

- The sing-box binary is not installed or not found — the TUI must say so plainly and still open, rather than crash.
- The config file is absent, unreadable, or contains invalid syntax — the TUI must show the path tried and the exact error, and must not present the service as healthy.
- The config checker reports warnings but no errors (for example a deprecated option) — warnings must be visible without blocking the user.
- More than one config file exists in the config directory — the TUI must state unambiguously which file it is reading and which file the running service is using.
- The service reports itself as running but the tunnel interface is missing or has no address — this must be shown as a degraded state, not as healthy.
- The service is running but no traffic is routed through it (routing rules match nothing) — the Ping panel must distinguish "process up" from "traffic flowing".
- The application is launched without elevated privileges — it must name the action needed to gain them, keep every view that does not need them working, degrade any field it cannot read to "unavailable" with a hint, and present Start, Stop, and Restart as unavailable rather than failing when pressed.
- The service stops or restarts while a ping test is in flight — the test must end cleanly and be labelled as interrupted.
- A ping target's name cannot be resolved, or the network blocks the test entirely — the failure reason must be distinguished from "slow".
- The outside reference used by the measured check is unreachable, rate-limited, or itself routed unexpectedly — the measured result must be shown as unavailable with the reason, the inferred route must still be shown, and the test must not be reported as a failure of the tunnel.
- The terminal is very small (for example 80x24) or is resized while open — all four panels must remain visible, the Status panel must remain fully visible, and the Config and Log panels must scroll internally rather than push other panels off screen; the layout must adapt without crashing or producing garbled output.
- The terminal does not support color, or the app runs over SSH — the information must remain readable without color.
- Cyrillic and internationalised content (for example the `.xn--p1ai` domain suffix in the routing rules) must render correctly.
- The config contains many routing rules — the Config panel must scroll internally within its own region rather than dumping an unscrollable list.
- The user quits while a test is running or while the service is starting or stopping — the test and the pending action must end cleanly, the service must be stopped, the terminal must be cleaned up, and the final screen must state the resulting state.
- Quitting must stop the service, but the stop itself fails (for example the process does not respond) — the final screen must state plainly that the service is still running and name the process identifier so the user can act.
- The application is launched without elevated privileges — it must name the action needed to gain them, keep every unprivileged view working, and present Start, Stop, and Restart as unavailable rather than failing when pressed.
- The application is launched with elevated privileges but cannot control the service anyway (for example the service is owned by another account) — the reason must be shown and the other views must still work.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: The application MUST run as a terminal application launched from the shell by a single command, and MUST NOT require a graphical window or a background daemon of its own.
- **FR-002**: The application MUST present a status panel that shows, at minimum: whether the service is running, its process identifier, how long it has been running, the version of the underlying service in use, and the path of the configuration file in use. The running state MUST be one of exactly four lifecycle states: Stopped, Starting, Running, Stopping.
- **FR-002a**: When a lifecycle transition does not complete within 10 seconds, the application MUST keep showing the transition state and mark it as not responding, with the reason where one is known.
- **FR-003**: The application MUST show the tunnel details that matter to the user: the tunnel interface name, its assigned address, and the configured MTU.
- **FR-004**: The application MUST show which outbound is currently handling traffic and MUST list all defined outbounds with their type and destination, distinguishing active from idle. Amendment (2026-10-11, from live use): when the final outbound is a urltest group, the group is a technical construct — the application MUST resolve to the member actually carrying traffic (observed from the sing-box process's established TCP connections, since no management API is assumed) and MUST name that member as the active outbound in the title and status panel instead of the group (revision the same day, user request: the window title names the outbound alone — no protocol prefix).
- **FR-005**: The application MUST show the configured DNS server(s) and the DNS strategy in effect.
- **FR-006**: The application MUST refresh the status automatically at a regular interval and MUST also offer a manual refresh action. The Status panel MUST refresh in place and MUST never scroll or move the information the user is reading.
- **FR-007**: The application MUST show a health badge that is separate from the lifecycle state and takes exactly one of three values — Healthy, Degraded, or Down — derived from the tunnel state and the connectivity checks. The badge MUST NOT read Healthy when the service is not in the Running state, when the tunnel is absent or has no address, or when a connectivity check has failed or was blocked. Amendment (2026-10-09, from live use): a check against a proxy server's own host is exempt from this rule — such servers commonly drop ICMP echo by policy (observed: naive on :8443), so its loss says nothing about the tunnel; the tunnel carrying traffic and the remaining through-tunnel checks are the evidence the server is reachable. The lost-ping row remains in the Ping panel as a diagnostic.
- **FR-008**: The application MUST run connectivity tests against a set of targets and show, per target, the latency of the first reply, the number sent and received, the loss percentage, and the minimum, average, and maximum latency.
- **FR-009**: The application MUST allow the user to enter an arbitrary hostname or address as a test target and MUST show its result in the same list as the default targets.
- **FR-010**: The application MUST show, per test target, the route the configuration selects for that target (through the proxy, or direct), derived from the routing rules and available without any outside dependency.
- **FR-010a**: The application MUST offer a measured check on demand that shows the address the user's traffic appears from when sent through the tunnel and the address it appears from when sent without the tunnel, displayed side by side.
- **FR-011**: The application MUST distinguish and display these failure reasons for a test target: no reply received, name could not be resolved, and the test was blocked or unsupported — and MUST NOT present a blocked test as a healthy result. For the measured check it MUST additionally distinguish "reference unavailable" from a tunnel failure.
- **FR-012**: The application MUST allow the user to cancel a running test, and MUST retain and display the partial results.
- **FR-013**: The application MUST present all four panels — Status, Ping, Config, and Log — on a single screen at the same time, with keys that move focus between them and keys that scroll inside the focused panel. The focused panel MUST be visibly marked in a way that does not depend on color.
- **FR-013a**: The application MUST present the Config panel as grouped, labelled name-and-value entries covering at least: general/log settings, inbounds, outbounds, DNS settings, and routing rules. The Config panel MUST scroll within its own region and MUST NOT push the other panels off screen.
- **FR-014**: The application MUST render routing rules in evaluation order, showing each rule's conditions and its resulting outcome, including the final/default outcome.
- **FR-015**: The application MUST mask credential-bearing values (identifiers, keys, and similar secrets) by default and MUST require an explicit user action to reveal them.
- **FR-016**: The application MUST detect that the configuration file changed on disk and MUST indicate this and offer to reload it.
- **FR-017**: The application MUST validate the configuration before presenting it as usable and MUST display validation errors and warnings verbatim, including the offending option where the underlying tool reports it.
- **FR-018**: The application MUST show the service's recent output in the Log panel with the message level visible, appending new lines as they are produced, with a pause action that applies to that panel only.
- **FR-019**: The application MUST allow the user to start, stop, and restart the service from the application. The application is launched with elevated privileges by the user and MUST NOT prompt for a password itself.
- **FR-019a**: The application MUST record whether the running instance was started by the application or externally, and MUST require an explicit confirmation before stopping an instance it did not start.
- **FR-019b**: The application MUST stop the service when it exits and MUST state on the final screen that the service was stopped. When the running instance was started outside the application, the application MUST ask before stopping it on exit and MUST state on the final screen that the instance was left running if the user declines.
- **FR-020**: The application MUST report the outcome of a start/stop/restart action, including failure, and MUST leave the application usable after any such outcome.
- **FR-020a**: The application MUST detect at launch that it does not have the privileges needed to control the service and MUST tell the user to relaunch it with elevated privileges, while continuing to show every view that does not need them and presenting the control actions as unavailable.
- **FR-020b**: Because the application runs with elevated privileges, it MUST resolve the configuration path against the invoking user's home directory rather than the elevated account's home directory, and MUST display the resolved absolute path it used.
- **FR-021**: The application MUST be fully operable by keyboard, MUST display the available keys on screen, MUST make the focused panel visibly marked without relying on color, and MUST restore the terminal to a normal usable state on exit.
- **FR-022**: The application MUST remain usable and MUST NOT crash when the terminal is resized, including to a small size such as 80x24, and MUST keep all four panels visible at that size.
- **FR-023**: The application MUST remain readable in terminals without color support and in non-interactive or piped output.
- **FR-024**: The application MUST degrade gracefully when privileged information is unavailable, showing the affected fields as unavailable with a hint instead of failing or showing misleading values.
- **FR-025**: The application MUST state unambiguously which configuration file it is reading and, when the service is running, which file that instance is using.
- **FR-026**: The application MUST NOT modify the user's configuration file.
- **FR-027**: The application MUST NOT write credential values to any log, cache, or file it creates.
- **FR-028**: The application MUST work with the existing installation and existing configuration without requiring the user to change their configuration.

### Key Entities

- **Service Instance**: A running occurrence of the VPN service — state (Stopped, Starting, Running, or Stopping), process identifier, start time, version, and the configuration it is using.
- **Configuration Profile**: The user's configuration file — its path, its sections, its validation state, and whether it changed since it was read.
- **Inbound (Tunnel)**: The local network interface the service creates — name, address, and MTU.
- **Outbound**: A defined egress path — tag, protocol type, destination server and port, and whether it is currently carrying traffic.
- **DNS Setting**: A configured resolver and the overall DNS strategy in effect.
- **Routing Rule**: An ordered condition-and-outcome pair that decides where matching traffic goes, including the final outcome.
- **Connectivity Check**: A test against one target — target, the route the configuration selects, sent/received counts, loss, and latency statistics.
- **Observed Address**: The address the user's traffic appears from, measured through the tunnel and measured without it, shown side by side, with the reason when either cannot be measured.
- **Log Entry**: One line of service output with its level and timestamp.
- **Health Summary**: The overall verdict, separate from the lifecycle state, taking one of three values (Healthy, Degraded, Down) derived from the tunnel state and the connectivity checks, with the reason for anything that is not Healthy.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: A user can determine whether the VPN is working within 3 seconds of launching the application, from a single screen, without running any other command.
- **SC-002**: The first latency figure for a connectivity test appears within 2 seconds of starting the test, and the complete summary for all default targets appears within 10 seconds.
- **SC-003**: Every option present in the user's current configuration appears in the Config panel, and all routing rules are reachable in evaluation order within at most three scrolls of that panel.
- **SC-004**: 100% of credential-bearing values are masked on first display, and revealing them requires a deliberate action by the user.
- **SC-005**: The application is fully usable in an 80x24 terminal with all four panels visible at once, and survives 20 consecutive resizes without crashing or producing unreadable output.
- **SC-006**: In 10 consecutive launches, quitting leaves the terminal with the shell prompt intact and no leftover screen content.
- **SC-007**: The number of commands and windows needed to answer "is my VPN working and how fast is it" drops from the current workflow (a privileged terminal plus a second terminal for manual tests) to a single command in a single window.
- **SC-008**: A configuration error is shown to the user before any start attempt completes, with the exact message, in 100% of cases where the underlying tool reports an error.
- **SC-009**: A stopped service is never displayed as healthy, and a blocked or unsupported connectivity test is never displayed as a successful result, in 100% of observed cases.
- **SC-009a**: In 100% of observed cases the health badge reads Healthy only when the service is in the Running state, the tunnel is present with an address, and no connectivity test has failed or been blocked.
- **SC-010**: Status information updates without the user needing to re-run anything, with a refresh no slower than every 3 seconds while the application is open.
- **SC-011**: A full stop-and-start cycle can be completed from the application in under 15 seconds from key press to the status panel showing the new state, without the user leaving the terminal window.
- **SC-012**: An externally started service is never stopped without a confirmation the user actively gave, in 100% of observed cases.
- **SC-012a**: In 10 consecutive quits while the service is running and was started by the application, the service is stopped every time and the final screen states it.
- **SC-013**: When the application is launched without elevated privileges, the user is told within 1 second what to do, and every view that does not need elevation still works, in 100% of observed cases.
- **SC-014**: No password prompt appears at any point during a session launched with elevated privileges, in 100% of observed cases.
- **SC-015**: The route selected by the configuration is shown for every target of a completed test without any outside dependency, and the measured address check is available on demand within 5 seconds of asking.

## Assumptions

- The environment is the user's own macOS machine with sing-box 1.14.x already installed and already on the shell's command path; the target is this single-user, single-machine personal VPN setup.
- The configuration in scope is `~/.config/sing-box/config.json`. Other files in that directory are the user's alternate profiles and are confirmed out of scope for this feature.
- The service requires elevated privileges because of the tunnel it creates. Confirmed: the user launches the application itself with elevated privileges, so the application never prompts for a password and no system-level approval rule is configured. A consequence recorded as FR-020b: the configuration path must be resolved against the invoking user's home, not the elevated account's home.
- The current configuration does not enable the service's optional management API. The first version is assumed to work without any change to the user's configuration, deriving status from observable process, interface, and connectivity state.
- Some networks block the connectivity test method; a non-blocked alternative is assumed to be available so that a blocked test can still be reported meaningfully.
- The measured address check depends on an outside reference service reachable over the Internet. It is assumed a public address-echo reference is acceptable to the user; the request carries no identifying information beyond the request itself, and when the reference is unavailable the inferred route is still shown.
- The user's connectivity expectations are typical for a personal VPN: latency in the tens to hundreds of milliseconds, and a test completing in a few seconds.
- The application is used interactively in a terminal of at least 80x24; color is desirable but not required.
- Retention of any data is limited to the current session; nothing needs to persist between runs.

### Confirmed Decisions

Resolved with the user on 2026-10-05 during specification:

- **Privilege model is launch-time elevation**: the user starts the application with elevated privileges; the application never prompts for a password and no system approval rule is configured (FR-019, FR-019a, FR-019b, FR-020, FR-020a, FR-020b, SC-013, SC-014).
- **Process control is in scope (full control)**: the application starts, stops, and restarts the service itself; the application owns the service process for the session and quitting the application stops the service. An instance that the user started elsewhere is never stopped without an explicit confirmation (FR-019, FR-019a, FR-019b, FR-020, SC-012, SC-012a).
- **Connectivity testing uses a default target set plus user-entered targets**: the default set covers the tunnel gateway, the configured DNS resolver, the proxy server, and a public host. Tests run on demand, are cancellable, and show the route the configuration selects for every target, with a measured address check available on demand (FR-008 to FR-012, FR-010a, SC-015). Continuous background pinging is out of scope.
- **Layout is a single dashboard**: all four panels (Status, Ping, Config, Log) are visible on one screen at the same time; keys move focus between panels and scroll inside the focused panel; there are no tabs or separate screens (FR-013, FR-013a, FR-018, FR-021, FR-022, SC-003, SC-005).
- **Configuration is read-only**: the Config panel and the validation action are the whole of the configuration feature. Editing values and switching between the other profiles in the config directory are out of scope; the user continues to edit the file in their editor (FR-013a to FR-017, FR-026).
