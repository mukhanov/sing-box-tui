//! Import a `vless://` sharing link and generate a sing-box config from it.
//!
//! The generated config mirrors the user's working structure (tun inbound,
//! direct DNS, hijack-dns, private/direct rules, final → proxy) so an
//! imported link produces a runnable profile. Import always writes a NEW
//! file — the existing config is never touched (FR-026).
//!
//! vless URL shape:
//! `vless://<uuid>@<host>:<port>?<query>#<label>`
//! Recognized query keys: type/network, security, encryption, flow, sni,
//! fp, pbk, sid, alpn, path, host, headerType, allowInsecure/insecure,
//! serviceName.
use serde_json::{Value, json};
use std::collections::HashMap;

use crate::config::parse::SingBoxConfig;

/// Parsed vless link parameters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VlessParams {
    pub uuid: String,
    pub server: String,
    pub port: u16,
    pub label: String,
    /// network/type: tcp (default), ws, grpc, http, quic.
    pub network: String,
    /// none | tls | reality.
    pub security: String,
    pub flow: Option<String>,
    pub sni: Option<String>,
    pub fingerprint: Option<String>,
    pub public_key: Option<String>,
    pub short_id: Option<String>,
    pub alpn: Option<Vec<String>>,
    pub path: Option<String>,
    pub host_header: Option<String>,
    pub service_name: Option<String>,
    pub allow_insecure: bool,
}

/// Percent-decode a URL component (pure; tested).
pub fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                let hex = bytes
                    .get(i + 1..i + 3)
                    .and_then(|h| std::str::from_utf8(h).ok())
                    .and_then(|h| u8::from_str_radix(h, 16).ok());
                match hex {
                    Some(b) => {
                        out.push(b);
                        i += 3;
                    }
                    None => {
                        out.push(bytes[i]);
                        i += 1;
                    }
                }
            }
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            b => {
                out.push(b);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).to_string()
}

/// Parse a `vless://` URL into parameters (pure; tested).
pub fn parse_vless_url(url: &str) -> Result<VlessParams, String> {
    let rest = url
        .strip_prefix("vless://")
        .ok_or_else(|| "not a vless:// URL".to_string())?;

    // Split off the label fragment first.
    let (rest, label_raw) = match rest.split_once('#') {
        Some((r, l)) => (r, l),
        None => (rest, ""),
    };
    let label = percent_decode(label_raw);

    // authority[?query]
    let (authority, query) = match rest.split_once('?') {
        Some((a, q)) => (a, q),
        None => (rest, ""),
    };

    // uuid@host:port — uuid first, then split host:port from the right
    // (IPv6 hosts keep their colons).
    let (uuid, hostport) = authority
        .split_once('@')
        .ok_or_else(|| "missing <uuid>@<host>".to_string())?;
    if uuid.is_empty() {
        return Err("empty uuid".to_string());
    }
    let (host, port_str) = hostport
        .rsplit_once(':')
        .ok_or_else(|| "missing :port".to_string())?;
    if host.is_empty() {
        return Err("empty server".to_string());
    }
    let port: u16 = port_str
        .parse()
        .map_err(|_| format!("invalid port {port_str:?}"))?;
    if port == 0 {
        return Err("port must be non-zero".to_string());
    }

    // Query parameters (last occurrence wins).
    let mut q: HashMap<String, String> = HashMap::new();
    for pair in query.split('&') {
        if pair.is_empty() {
            continue;
        }
        let (k, v) = pair.split_once('=').unwrap_or((pair, ""));
        q.insert(percent_decode(k).to_ascii_lowercase(), percent_decode(v));
    }
    let get = |k: &str| q.get(k).cloned().filter(|v| !v.is_empty());

    let security = get("security").unwrap_or_else(|| "none".to_string());
    if !matches!(security.as_str(), "none" | "tls" | "reality") {
        return Err(format!("unsupported security {security:?}"));
    }
    let network = get("type")
        .or_else(|| get("network"))
        .unwrap_or_else(|| "tcp".to_string());
    if !matches!(network.as_str(), "tcp" | "ws" | "grpc" | "http" | "quic") {
        return Err(format!("unsupported transport type {network:?}"));
    }
    let public_key = get("pbk");
    let short_id = get("sid");
    if security == "reality" && (public_key.is_none() || short_id.is_none()) {
        return Err("reality requires pbk and sid parameters".to_string());
    }

    Ok(VlessParams {
        uuid: uuid.to_string(),
        server: host.to_string(),
        port,
        label,
        network,
        security,
        flow: get("flow"),
        sni: get("sni").or_else(|| get("peer")),
        fingerprint: get("fp"),
        public_key,
        short_id,
        alpn: get("alpn").map(|a| {
            a.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        }),
        path: get("path"),
        host_header: get("host"),
        service_name: get("servicename").or_else(|| get("service_name")),
        allow_insecure: get("allowinsecure")
            .or_else(|| get("insecure"))
            .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true")),
    })
}

/// Build the outbound JSON for parsed parameters.
pub fn build_outbound(p: &VlessParams) -> Value {
    let mut out = json!({
        "type": "vless",
        "tag": "proxy",
        "server": p.server,
        "server_port": p.port,
        "uuid": p.uuid,
    });
    let obj = out.as_object_mut().expect("fresh object");

    if let Some(flow) = &p.flow {
        obj.insert("flow".into(), json!(flow));
    }

    if p.security != "none" {
        let mut tls = json!({ "enabled": true });
        let tls_obj = tls.as_object_mut().expect("fresh object");
        if let Some(sni) = &p.sni {
            tls_obj.insert("server_name".into(), json!(sni));
        }
        if p.allow_insecure {
            tls_obj.insert("insecure".into(), json!(true));
        }
        if let Some(alpn) = &p.alpn {
            tls_obj.insert("alpn".into(), json!(alpn));
        }
        if let Some(fp) = &p.fingerprint {
            tls_obj.insert("utls".into(), json!({ "enabled": true, "fingerprint": fp }));
        }
        if p.security == "reality" {
            tls_obj.insert(
                "reality".into(),
                json!({
                    "enabled": true,
                    "public_key": p.public_key.clone().unwrap_or_default(),
                    "short_id": p.short_id.clone().unwrap_or_default(),
                }),
            );
        }
        obj.insert("tls".into(), tls);
    }

    let transport = match p.network.as_str() {
        "ws" => {
            let mut t = json!({ "type": "ws" });
            let to = t.as_object_mut().expect("fresh object");
            if let Some(path) = &p.path {
                to.insert("path".into(), json!(path));
            }
            if let Some(host) = &p.host_header {
                to.insert("headers".into(), json!({ "Host": host }));
            }
            Some(t)
        }
        "grpc" => {
            let mut t = json!({ "type": "grpc" });
            if let Some(sn) = &p.service_name {
                t.as_object_mut()
                    .expect("fresh object")
                    .insert("service_name".into(), json!(sn));
            }
            Some(t)
        }
        "http" => {
            let mut t = json!({ "type": "http" });
            let to = t.as_object_mut().expect("fresh object");
            if let Some(host) = &p.host_header {
                to.insert("host".into(), json!([host]));
            }
            if let Some(path) = &p.path {
                to.insert("path".into(), json!(path));
            }
            Some(t)
        }
        _ => None, // tcp/quic carry no transport block
    };
    if let Some(t) = transport {
        obj.insert("transport".into(), t);
    }

    out
}

/// Build a complete sing-box config for the link: tun inbound, direct DNS,
/// hijack-dns + private/direct rules, final → proxy (mirrors the user's
/// working profile shape).
pub fn build_config_value(p: &VlessParams) -> Value {
    let raw = json!({
        "log": { "level": "info", "timestamp": true },
        "dns": {
            "servers": [
                { "type": "udp", "tag": "dns-direct", "server": "8.8.8.8", "server_port": 53 }
            ],
            "final": "dns-direct",
            "strategy": "ipv4_only"
        },
        "inbounds": [
            {
                "type": "tun",
                "tag": "tun-in",
                "address": ["172.18.0.1/30"],
                "mtu": 1400,
                "auto_route": true,
                "strict_route": true
            }
        ],
        "outbounds": [build_outbound(p), { "type": "direct", "tag": "direct" }],
        "route": {
            "auto_detect_interface": true,
            "default_domain_resolver": "dns-direct",
            "rules": [
                { "protocol": "dns", "action": "hijack-dns" },
                { "port": 53, "action": "route", "outbound": "direct" },
                { "domain": [p.server.clone()], "action": "route", "outbound": "direct" },
                { "ip_is_private": true, "action": "route", "outbound": "direct" }
            ],
            "final": "proxy"
        }
    });
    raw
}

/// The same config as the typed model (for the reader path and tests).
pub fn build_config(p: &VlessParams) -> SingBoxConfig {
    serde_json::from_value(build_config_value(p)).expect("generated config must deserialize")
}

/// File name for an imported link: `config-<label|host>.json`, label
/// sanitized to filename-safe characters (Unicode letters kept; pure; tested).
pub fn config_file_name(p: &VlessParams) -> String {
    let base = if p.label.is_empty() {
        p.server.clone()
    } else {
        p.label.clone()
    };
    let safe: String = base
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = safe.trim_matches('-').to_string();
    let stem = if trimmed.is_empty() {
        "imported".to_string()
    } else {
        trimmed
    };
    format!("config-{stem}.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    const REALITY_URL: &str = "vless://3f1b2c4d-0000-4abc-9def-a1b2c3d4e5f6@i.warmine.tech:443?encryption=none&flow=xtls-rprx-vision&security=reality&sni=yandex.tm&fp=chrome&pbk=PUBKEY1234567890123456789012345678901234567890123&sid=0123abcd&type=tcp#%D0%9C%D0%BE%D0%B9%20%D0%BF%D1%80%D0%BE%D0%BA%D1%81%D0%B8";

    #[test]
    fn percent_decodes() {
        assert_eq!(
            percent_decode("%D0%9F%D1%80%D0%B8%D0%B2%D0%B5%D1%82"),
            "Привет"
        );
        assert_eq!(percent_decode("a%20b+c"), "a b c");
        assert_eq!(percent_decode("%zz"), "%zz");
        assert_eq!(percent_decode("plain"), "plain");
    }

    #[test]
    fn parses_reality_link() {
        let p = parse_vless_url(REALITY_URL).unwrap();
        assert_eq!(p.uuid, "3f1b2c4d-0000-4abc-9def-a1b2c3d4e5f6");
        assert_eq!(p.server, "i.warmine.tech");
        assert_eq!(p.port, 443);
        assert_eq!(p.security, "reality");
        assert_eq!(p.sni.as_deref(), Some("yandex.tm"));
        assert_eq!(p.fingerprint.as_deref(), Some("chrome"));
        assert_eq!(p.flow.as_deref(), Some("xtls-rprx-vision"));
        assert_eq!(
            p.public_key.as_deref(),
            Some("PUBKEY1234567890123456789012345678901234567890123")
        );
        assert_eq!(p.short_id.as_deref(), Some("0123abcd"));
        assert_eq!(p.network, "tcp");
        assert_eq!(p.label, "Мой прокси", "fragment percent-decoded");
    }

    #[test]
    fn rejects_bad_links() {
        assert!(parse_vless_url("ss://x").is_err());
        assert!(parse_vless_url("vless://onlyhost:443").is_err());
        assert!(parse_vless_url("vless://u@host:notaport").is_err());
        assert!(parse_vless_url("vless://u@host:0").is_err());
        // reality without pbk/sid is rejected, not silently degraded.
        assert!(parse_vless_url("vless://u@h:443?security=reality").is_err());
        assert!(parse_vless_url("vless://u@h:443?type=smtp").is_err());
    }

    #[test]
    fn generated_config_parses_into_model() {
        let p = parse_vless_url(REALITY_URL).unwrap();
        let cfg = build_config(&p);
        // Outbound carries the link's fields.
        let proxy = cfg.outbounds.iter().find(|o| o.tag == "proxy").unwrap();
        assert_eq!(proxy.ty, "vless");
        assert_eq!(proxy.server, "i.warmine.tech");
        assert_eq!(proxy.server_port, Some(443));
        assert_eq!(
            proxy.extra.get("uuid"),
            Some(&json!("3f1b2c4d-0000-4abc-9def-a1b2c3d4e5f6"))
        );
        assert_eq!(proxy.extra.get("flow"), Some(&json!("xtls-rprx-vision")));
        let tls = proxy.extra.get("tls").unwrap();
        assert_eq!(
            tls["reality"]["public_key"],
            json!("PUBKEY1234567890123456789012345678901234567890123")
        );
        assert_eq!(tls["utls"]["fingerprint"], json!("chrome"));
        // Profile shape: tun inbound, DNS, rules, final proxy.
        assert!(cfg.tun_inbound().is_some());
        assert_eq!(cfg.dns.servers.len(), 1);
        assert_eq!(cfg.route.final_, "proxy");
        // Server domain routed direct so the tunnel can reach it.
        assert!(cfg.route.rules.iter().any(|r| r.domain.contains(&p.server)));
        // The generated document round-trips through the reader model.
        let re: SingBoxConfig = serde_json::from_value(build_config_value(&p)).unwrap();
        assert_eq!(re.route.final_, "proxy");
    }

    #[test]
    fn ws_transport_and_tls() {
        let p = parse_vless_url(
            "vless://u@h.example:8443?security=tls&sni=h.example&type=ws&path=%2Fray&host=cdn.example#edge",
        )
        .unwrap();
        let cfg = build_config(&p);
        let proxy = cfg.outbounds.iter().find(|o| o.tag == "proxy").unwrap();
        let transport = proxy.extra.get("transport").unwrap();
        assert_eq!(transport["type"], "ws");
        assert_eq!(transport["path"], "/ray");
        assert_eq!(transport["headers"]["Host"], "cdn.example");
        let tls = proxy.extra.get("tls").unwrap();
        assert_eq!(tls["enabled"], json!(true));
        assert_eq!(tls["server_name"], "h.example");
        assert!(tls.get("reality").is_none());
    }

    #[test]
    fn file_names_sanitized() {
        let mut p = parse_vless_url(REALITY_URL).unwrap();
        assert_eq!(config_file_name(&p), "config-Мой-прокси.json");
        p.label = "a/b\\c d:1".into();
        assert_eq!(config_file_name(&p), "config-a-b-c-d-1.json");
        p.label.clear();
        assert_eq!(
            config_file_name(&p),
            "config-i-warmine-tech.json",
            "dots are not filename-safe here"
        );
    }
}
