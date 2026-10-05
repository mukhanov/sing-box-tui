# sing-box-tui

A keyboard-only terminal dashboard for a personal [sing-box](https://sing-box.sagernet.org/)
VPN on macOS. One screen, four always-visible panels — **Status**, **Ping**,
**Config**, **Log** — refreshed continuously, answering "is my VPN up and is
traffic actually flowing through it?" in a single command.

## Launch

```bash
sudo sing-box-tui                          # default config, autostart
sudo sing-box-tui -c ~/.config/sing-box/config-ya.json
```


All status is derived from observable state — the `sing-box` process
(`pgrep`/`ps`), the tunnel interface (`ifconfig`), the read-only config file,
and on-demand connectivity tests — because the management API is not enabled
and the config is never modified.

**Home resolution**: under `sudo`, the config is resolved against the
*invoking* user's home (`SUDO_USER` → `dscl`), never root's:

```
~/.config/sing-box/config.json
```

## Importing a vless:// link

Convert a `vless://` sharing link into a new sing-box config, validate it
with `sing-box check`, and exit. The generated profile mirrors the working
shape (tun inbound, direct DNS, hijack-dns, private/direct rules, final →
proxy; reality/tls/utls/ws/grpc fields from the link):

```bash
sing-box-tui --import 'vless://uuid@host:443?security=reality&sni=...&fp=chrome&pbk=...&sid=...&flow=xtls-rprx-vision#Label'
    # → ~/.config/sing-box/config-Label.json
sing-box-tui --import '...' -o /path/to/new.json
```

Never overwrites an existing file; written with 0600 (it carries
credentials). See `sing-box-tui --help` for all flags.

## Keys

| Key | Action |
|-----|--------|
| `Tab` / `Shift+Tab` | Move focus to next / previous panel |
| `j` `↓` / `k` `↑` | Scroll focused panel |
| `gg` / `G` | Scroll to top / bottom of focused panel |
| `r` | Refresh status (manual; auto every 3 s) |
| `q` / `Ctrl+C` | Quit (stops an app-owned service; asks for external ones) |
| `s` | Start service |
| `x` | Stop service (asks first for externally started instances) |
| `R` | Restart service |
| `c` | Run connectivity test |
| `C` | Cancel running test, keep partial results |
| `t` | Add a ping target (`Enter` submit, `Esc` cancel) |
| `m` | Measure source address through tunnel vs direct |
| `v` | Toggle credential visibility (Config panel) |
| `l` | Reload config after a change is detected |
| `p` | Pause log output |
| `[` / `]` | Previous / next config profile (asks to restart the service) |

While the target input is open, all other keys route to the input.
Confirmation dialogs accept `y` / `n` / `Esc` only.

## Panels

- **Status** — lifecycle state, pid, uptime, version, config path, tunnel
  name/address/MTU, active vs idle outbounds, DNS servers + strategy, and a
  health badge (`Healthy` / `Degraded` / `Down`) with the reason for anything
  not Healthy.
- **Ping** — per-target latency/loss stats plus the config-inferred route
  (proxy/direct/reject); failure classification (no reply / name not
  resolved / blocked); the measured address check (through tunnel vs direct,
  side by side).
- **Config** — the config as grouped name/value sections (log, inbounds,
  outbounds, DNS, routing rules in evaluation order). Credentials are masked
  by default; `v` reveals deliberately.
- **Log** — the service's own output with levels, streamed live while the
  instance is owned by the app.

## Build

```bash
cargo build --release       # binary at target/release/sing-box-tui
cargo test                  # health rules, route inference, config parsing/masking
```

Requires a terminal ≥ 80×24 (works without color), UTF-8. External tools
used: `sing-box`, `/sbin/ping`, `/usr/bin/curl`, `/sbin/ifconfig`,
`pgrep`/`ps`.
