//! Read-only, permissive deserialization of the sing-box configuration.
//!
//! The model types known fields and preserves everything else as raw JSON
//! (`#[serde(flatten)]`) so that unknown or future sing-box options are shown
//! in the Config panel instead of causing a parse failure (FR-028, FR-013a).
//! The file is never written (FR-026).

use serde::Deserialize;
use serde_json::{Map, Value};

/// The whole configuration document.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct SingBoxConfig {
    #[serde(default)]
    pub log: LogConfig,
    #[serde(default)]
    pub dns: DnsConfig,
    /// Inbounds are kept raw; only the `tun` inbound is decoded on demand.
    #[serde(default)]
    pub inbounds: Vec<Value>,
    #[serde(default)]
    pub outbounds: Vec<Outbound>,
    #[serde(default)]
    pub route: RouteConfig,
    /// Any top-level keys not modelled above (e.g. experimental, ntp).
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl SingBoxConfig {
    /// Find the first `tun` inbound, if any (used for tunnel discovery).
    pub fn tun_inbound(&self) -> Option<TunInbound> {
        self.inbounds.iter().find_map(|v| {
            let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or_default();
            if ty == "tun" {
                serde_json::from_value::<TunInbound>(v.clone()).ok()
            } else {
                None
            }
        })
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct LogConfig {
    #[serde(default)]
    pub level: String,
    #[serde(default)]
    pub timestamp: bool,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DnsConfig {
    #[serde(default)]
    pub servers: Vec<DnsServer>,
    /// `final` is a Rust keyword; renamed.
    #[serde(rename = "final", default)]
    pub final_: String,
    #[serde(default)]
    pub strategy: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct DnsServer {
    #[serde(rename = "type", default)]
    pub ty: String,
    #[serde(default)]
    pub tag: String,
    #[serde(default)]
    pub server: String,
    #[serde(default)]
    pub server_port: Option<u16>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl DnsServer {
    /// "8.8.8.8:53" style display string.
    pub fn display(&self) -> String {
        match self.server_port {
            Some(p) if p != 0 => format!("{}:{}", self.server, p),
            _ => self.server.clone(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct Outbound {
    #[serde(rename = "type", default)]
    pub ty: String,
    #[serde(default)]
    pub tag: String,
    #[serde(default)]
    pub server: String,
    #[serde(default)]
    pub server_port: Option<u16>,
    /// Selector members (selector-type outbounds only).
    #[serde(default)]
    pub outbounds: Vec<String>,
    /// Selector default member.
    #[serde(default)]
    pub default: Option<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

impl Outbound {
    /// "server:port" destination, or empty for `direct`.
    pub fn destination(&self) -> String {
        if self.server.is_empty() {
            String::new()
        } else {
            match self.server_port {
                Some(p) if p != 0 => format!("{}:{}", self.server, p),
                _ => self.server.clone(),
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct RouteConfig {
    #[serde(default)]
    pub rules: Vec<RouteRule>,
    #[serde(rename = "final", default)]
    pub final_: String,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// One routing rule. Conditions are kept as typed fields where inference needs
/// them and raw otherwise; the Config panel renders the raw form.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct RouteRule {
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub outbound: String,
    #[serde(default)]
    pub domain: Vec<String>,
    #[serde(default)]
    pub domain_suffix: Vec<String>,
    #[serde(default)]
    pub domain_keyword: Vec<String>,
    #[serde(default)]
    pub ip_cidr: Vec<String>,
    #[serde(default)]
    pub ip_is_private: Option<bool>,
    #[serde(default)]
    pub port: Option<Value>,
    #[serde(default)]
    pub protocol: String,
    #[serde(default)]
    pub network: String,
    #[serde(default)]
    pub inbound: Vec<String>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct TunInbound {
    #[serde(rename = "type", default)]
    pub ty: String,
    #[serde(default)]
    pub tag: String,
    #[serde(default)]
    pub interface_name: Option<String>,
    #[serde(default)]
    pub address: Vec<String>,
    #[serde(default)]
    pub mtu: Option<u32>,
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Parse a JSON string into the config model.
pub fn parse(json: &str) -> Result<SingBoxConfig, String> {
    serde_json::from_str::<SingBoxConfig>(json).map_err(|e| e.to_string())
}
