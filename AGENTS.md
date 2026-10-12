# AGENTS.md — project context for AI agents (opencode et al.)

Terminal dashboard (TUI) for the sing-box VPN on macOS: one screen, four
panels — Status, Ping, Config, Log. Rust + ratatui/crossterm + tokio.
Repo: https://github.com/mukhanov/sing-box-tui (branch `main`).

## Non-negotiable design rules

- **Observable state only.** No sing-box management/clash API is enabled and
  the config file is NEVER modified (spec FR-026). Everything is derived by
  shelling out: `pgrep`/`ps` (process), `ifconfig` (tunnel), `netstat`
  (traffic counters), `lsof` (connections), `/sbin/ping` (checks),
  `curl` (exit IP), `sing-box check` (validation). The whole observation
  layer is macOS-bound — see "Windows port" below.
- **Spec is the source of truth**: `specs/001-sing-box-tui/` (spec.md,
  plan.md, data-model.md, contracts/ui.md, tasks.md). Behavior changes get
  an inline dated amendment in the spec (examples already there: FR-007
  server-host exemption, FR-004 urltest resolution), not a silent code
  drift. Test headers quote data-model rules verbatim — keep them in sync.
- Pure logic is unit-tested (parsers are table-driven, tests quote real
  command output); UI/service control is validated by running the TUI.
  Integration tests: `tests/health_rules.rs`, `tests/route_inference.rs`,
  `tests/config_parse.rs`.

## Module map

- `src/app.rs` — `App` state + `apply(AppEvent)` + event loop `run()`;
  window title (`title_text`), sticky urltest-member logic (`observe_member`)
- `src/ui/` — layout (Status top row + Ping/Config/Log columns),
  status/ping/config_view/log_view panels
- `src/config/` — permissive serde model (`parse.rs`, unknown keys kept),
  credential masking, `active_outbound()` / `urltest_group()` /
  `group_members()` (mod.rs), `sing-box check` wrapper (validate.rs),
  mtime watch, vless:// link → config generator (vless.rs)
- `src/service/` — lifecycle + health derivation (mod.rs:
  `derive_health`, `recompute_health(checks, server_hosts)`), process
  probe/start/stop (probe.rs, process.rs), tunnel discovery (tunnel.rs),
  urltest member observation (conns.rs)
- `src/net/` — default ping targets + `server_hosts()` (mod.rs), `/sbin/ping`
  driver with streaming parse (ping.rs), route inference from config rules
  (route.rs), traffic counters, exit-IP via api.ipify.org only (address.rs)

## Decisions made from live use (don't regress them)

1. **Health exemption** (2026-10-09): ping targets include proxy servers'
   own hosts; those hosts commonly drop ICMP (naive on :8443 does), which
   permanently read Degraded while the tunnel was fine. `recompute_health`
   takes `server_hosts` (from `net::server_hosts`) and excludes those
   targets from the verdict. Their ✗ rows stay in the Ping panel as
   diagnostics. Any other failing target still degrades (FR-007).
2. **urltest is technical** (2026-10-11): when `route.final` is a urltest
   group, the title and status panel must show the *member* actually
   carrying traffic, never the group. Resolution: `service/conns.rs`
   observes the sing-box process's established TCP connections (`lsof -nP
   -i TCP -a -p PID`), matches remote ip:port against member servers
   (DNS-resolved). Sticky pick; a switch needs TWO consecutive
   observations (`observe_member`) because urltest probes open short-lived
   connections to every member every 2 min. Fallback before first
   observation: first member in config order. Reset on stop/config reload.
3. **Window title format** (2026-10-11, user request): state glyph +
   outbound NAME only — `🟢 naive-a · <ip> · ↓x ↑y`. No protocol prefix
   (`protocol_label()` was deleted). Protocol/server live in the status
   panel.
4. Exit IP: api.ipify.org only (ifconfig.me resets TLS from datacenter
   IPs). Refreshed every 60 s while running; dropped (never stale) on
   failure or stop.

## Build / run / test

```bash
cargo test                      # 77 tests across lib + 3 integration files
cargo clippy --all-targets     # must be clean (pedantic allowed)
cargo fmt
cargo build --release
./install.sh                    # installs to /usr/local/bin (sudo; needs password)
sudo sing-box-tui               # normal launch; autostarts the service when down
./target/release/sing-box-tui -c ~/.config/sing-box/config-<profile>.json
```

- Configs: `~/.config/sing-box/config*.json`; profiles selectable in the
  Config panel with `[` `]` + Enter. Under `sudo`, home resolves via
  `SUDO_USER` + `dscl` (never /var/root).
- Keys: `c` test, `C` cancel, `t` target input, `m` measured address,
  `v` unmask, `r` refresh, `l` reload config, `s` start, `x` stop,
  `R` restart, `q` quit (stops owned service; external one asks).

## Live-verification gotchas (macOS)

- sing-box runs as root → an **unprivileged** TUI cannot `lsof` its
  sockets: urltest member observation returns nothing and the first-member
  fallback shows. Verify the observation path under `sudo`.
- No passwordless sudo on the dev machine: elevated smoke must be run by
  the user. Binary in `/usr/local/bin` goes stale — after changes remind
  the user to `sudo ./install.sh`.
- ICMP through the naive TUN is often answered locally by sing-box's stack
  (sub-ms "replies" like example.com 0.22 ms are an artifact, not a real
  RTT). Real member RTT is better judged by the measured address check.
- TUI smoke without a real tty (e.g. `script`) dies on terminal-size query;
  use a pty. Window-title OSC is not visible in pty captures — judge the
  title via unit tests + `app.title_text()`.

## Windows port (assessed 2026-10-12, NOT done)

`cargo check --target x86_64-pc-windows-msvc` fails only on
`libc::geteuid`/`kill` (main.rs:192, process.rs:88–133) — trivial cfg fix.
The real port is the observation layer: pgrep/ps → `Get-CimInstance
Win32_Process`; ifconfig → wintun adapter via `Get-NetAdapter`/`netsh`;
netstat -ibdn → `Get-NetAdapterStatistics`; lsof →
`Get-NetTCPConnection -OwningProcess`; `/sbin/ping` → IcmpSendEcho (ping.exe
output is localized); curl `--interface` by name doesn't bind on Windows
(bind by adapter IP); SIGTERM → TerminateProcess; sudo → UAC. ~1–2 days,
needs a real Windows box or CI runner for runtime verification.
