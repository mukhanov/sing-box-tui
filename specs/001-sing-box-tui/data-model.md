# Data Model: sing-box TUI

**Created**: 2026-10-05
**Feature**: sing-box TUI (specs/001-sing-box-tui)

## Entities

### ServiceInstance

A running occurrence of the sing-box VPN service.

| Field | Type | Description |
|-------|------|-------------|
| `state` | LifecycleState | Current lifecycle state (Stopped, Starting, Running, Stopping) |
| `pid` | int | Process ID of the running service |
| `startTime` | timestamp | When the service was started |
| `version` | string | sing-box version in use |
| `configPath` | string | Path to the configuration file in use |
| `tunnelInterface` | Tunnel | Tunnel interface details (if running) |
| `activeOutbound` | string | Tag of the outbound currently handling traffic |
| `health` | HealthStatus | Overall health (Healthy, Degraded, Down) |
| `healthReason` | string | Reason for non-Healthy status |

**Lifecycle State Transitions**:
```
Stopped → Starting → Running → Stopping → Stopped
```

**Validation Rules**:
- `pid` must be non-zero when state is Starting, Running, or Stopping
- `tunnelInterface` must be present when state is Running
- `health` must be Down when state is Stopped
- `health` must be Degraded when tunnel is missing or has no address

---

### Tunnel

The local network interface created by sing-box.

| Field | Type | Description |
|-------|------|-------------|
| `name` | string | Interface name (e.g., utun10) |
| `address` | string | Assigned IP address (e.g., 172.18.0.1/30) |
| `mtu` | int | Maximum transmission unit |

---

### Outbound

A defined egress path for traffic.

| Field | Type | Description |
|-------|------|-------------|
| `tag` | string | Unique identifier |
| `type` | string | Protocol type (vless, direct, etc.) |
| `server` | string | Destination server (if applicable) |
| `port` | int | Destination port (if applicable) |
| `isActive` | boolean | Whether this outbound is currently handling traffic |

---

### DNSConfig

The overall DNS configuration in effect (FR-005).

| Field | Type | Description |
|-------|------|-------------|
| `servers` | DNSSetting[] | Configured resolvers |
| `strategy` | string | DNS strategy in effect (e.g., prefer_ipv4) |

### DNSSetting

A configured DNS resolver.

| Field | Type | Description |
|-------|------|-------------|
| `tag` | string | Unique identifier |
| `type` | string | Resolver type (udp, tcp, etc.) |
| `server` | string | DNS server address |
| `port` | int | DNS server port |

---

### RoutingRule

An ordered condition-and-outcome pair for traffic routing.

| Field | Type | Description |
|-------|------|-------------|
| `conditions` | object | Rule conditions (domain, port, protocol, etc.) |
| `outbound` | string | Target outbound or action (reject, hijack-dns, etc.) |
| `order` | int | Evaluation order (lower = earlier) |

---

### ConnectivityCheck

A test against one target.

| Field | Type | Description |
|-------|------|-------------|
| `target` | string | Hostname or address being tested |
| `status` | CheckStatus | Pending, Running, Complete, Cancelled, Interrupted, Failed |
| `inferredRoute` | string | Route selected by config (proxy/direct) |
| `sent` | int | Number of packets sent |
| `received` | int | Number of packets received |
| `loss` | float | Packet loss percentage |
| `latencyMin` | int | Minimum latency in ms |
| `latencyAvg` | int | Average latency in ms |
| `latencyMax` | int | Maximum latency in ms |
| `firstReplyLatency` | int | Latency of the first reply (SC-002, shown within 2s) |
| `failureReason` | FailureReason | Why the check failed or was blocked (if applicable) |

**FailureReason values**: `NoReply`, `NameNotResolved`, `BlockedOrUnsupported`, `Interrupted` (FR-011).

**Validation Rules**:
- `received` ≤ `sent`; `loss` in [0, 100]; `latencyMin` ≤ `latencyAvg` ≤ `latencyMax`
- A check with `BlockedOrUnsupported` failure MUST NOT contribute to a Healthy badge (FR-011)
- Cancelled/Interrupted checks retain partial statistics (FR-012)

### ObservedAddress

The address the user's traffic appears from, measured on demand (FR-010a).

| Field | Type | Description |
|-------|------|-------------|
| `throughTunnel` | string | Address traffic appears from when sent through the tunnel |
| `direct` | string | Address traffic appears from when sent without the tunnel |
| `throughTunnelReason` | string | Reason when the tunnel measurement is unavailable |
| `directReason` | string | Reason when the direct measurement is unavailable |

**Validation Rules**:
- The two addresses are displayed side by side so a difference is obvious
- "Reference unavailable" MUST be distinguished from a tunnel failure (FR-011)

---

### LogEntry

One line of service output.

| Field | Type | Description |
|-------|------|-------------|
| `timestamp` | timestamp | When the log entry was produced |
| `level` | string | Log level (info, warn, error, etc.) |
| `message` | string | Log message content |

---

### HealthStatus

Overall verdict derived from service state, tunnel state, and connectivity checks.

**Values**: Healthy, Degraded, Down

**Derivation Rules**:
- `Down` when state is Stopped
- `Degraded` when state is Running but tunnel is missing or has no address
- `Degraded` when any connectivity check has failed or been blocked
  (amendment 2026-10-09: checks against a proxy server's own host are exempt
  — such hosts commonly drop ICMP echo by policy, and the tunnel carrying
  traffic is the evidence they are reachable; the lost-ping row stays in the
  Ping panel as a diagnostic)
- `Healthy` only when state is Running, tunnel is present with address, and no connectivity check has failed

---

## State Diagram

```
┌─────────┐     ┌──────────┐     ┌─────────┐     ┌──────────┐
│ Stopped │────▶│ Starting │────▶│ Running │────▶│ Stopping │
└─────────┘     └──────────┘     └─────────┘     └──────────┘
     ▲                                                    │
     └────────────────────────────────────────────────────┘
```

**Transitions**:
- `Start`: Stopped → Starting → Running
- `Stop`: Running → Stopping → Stopped
- `Restart`: Running → Stopping → Stopped → Starting → Running

---

## Configuration File Structure

The sing-box configuration file (`~/.config/sing-box/config.json`) contains:

```json
{
  "log": { ... },
  "dns": { ... },
  "inbounds": [ ... ],
  "outbounds": [ ... ],
  "route": { ... }
}
```

**Credential Fields (masked by default)**:
- `outbounds[].uuid`
- `outbounds[].tls.reality.publicKey`
- `outbounds[].tls.reality.shortId`

---

## Session State

The application maintains session-only state (no persistence between runs):

| Field | Type | Description |
|-------|------|-------------|
| `focusedPanel` | Panel | Currently focused panel (Status, Ping, Config, Log) |
| `runningInstanceStartedByApp` | boolean | Whether the running instance was started by this application |
| `lastConfigMtime` | timestamp | Last known modification time of config file |
| `connectivityTargets` | string[] | List of test targets (default + user-entered) |

---

## Validation Rules

1. **Service Instance**:
   - `pid` must be valid (non-zero) when state is not Stopped
   - `configPath` must be absolute path
   - `tunnelInterface` must be present when state is Running

2. **Connectivity Check**:
   - `sent` must be non-negative
   - `received` must be ≤ `sent`
   - `loss` must be between 0 and 100
   - `latencyMin` ≤ `latencyAvg` ≤ `latencyMax`

3. **Health Status**:
   - Must be `Down` when state is Stopped
   - Must be `Degraded` when tunnel is missing or has no address
   - Must be `Degraded` when any connectivity check failed (proxy-server-host
     checks exempt — see HealthStatus Derivation Rules)
   - Must be `Healthy` only when all conditions are satisfied

4. **Configuration**:
   - Must be valid JSON
   - Must pass `sing-box check` validation
   - Credential fields must be masked by default
