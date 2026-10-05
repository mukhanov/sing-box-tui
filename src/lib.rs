//! sing-box-tui — a keyboard-only terminal dashboard for the sing-box VPN.
//!
//! One screen, four always-visible panels (Status, Ping, Config, Log), refreshed
//! continuously. All status is derived from observable state (process, tunnel
//! interface, parsed config, connectivity tests) because the user's configuration
//! does not enable sing-box's management API and must not be modified.
//!
//! The library exposes the modules so integration tests can exercise the pure
//! logic (health derivation, route inference, config parsing/masking) without a
//! terminal.

pub mod app;
pub mod config;
pub mod logging;
pub mod net;
pub mod service;
pub mod ui;
