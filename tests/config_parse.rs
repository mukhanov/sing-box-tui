//! Fixture-based tests for config parsing, credential masking, and
//! verbatim `sing-box check` passthrough (FR-015, FR-017, FR-028, SC-004).

use sing_box_tui::config::mask::{MASK, is_credential_key, mask_json};
use sing_box_tui::config::parse::parse;
use sing_box_tui::config::validate::classify_output;

/// The user's real config shape (credential values replaced; structure identical).
const FIXTURE: &str = r#"{
  "log": { "level": "info", "timestamp": true },
  "dns": {
    "servers": [
      { "type": "udp", "tag": "dns-direct", "server": "8.8.8.8", "server_port": 53 }
    ],
    "final": "dns-direct",
    "strategy": "ipv4_only",
    "independent_cache": true
  },
  "inbounds": [
    { "type": "tun", "tag": "tun-in", "address": ["172.18.0.1/30"], "mtu": 1400,
      "auto_route": true, "strict_route": true }
  ],
  "outbounds": [
    { "type": "vless", "tag": "proxy", "server": "i.warmine.tech", "server_port": 443,
      "uuid": "SECRET-UUID-VALUE", "flow": "xtls-rprx-vision",
      "tls": { "enabled": true, "server_name": "yandex.tm",
               "reality": { "enabled": true, "public_key": "SECRET-PUBKEY", "short_id": "SECRET-SHORT" } } },
    { "type": "direct", "tag": "direct" }
  ],
  "route": { "final": "proxy", "rules": [] },
  "experimental": { "unknown_future_option": { "nested": [1, 2] } }
}"#;

/// Unknown sing-box options must be preserved, not rejected (FR-028):
/// parsing keeps them as raw JSON for display.
#[test]
fn unknown_options_preserved() {
    let cfg = parse(FIXTURE).expect("fixture must parse");
    // Unknown top-level key survives.
    assert!(
        cfg.extra.contains_key("experimental"),
        "unknown top-level key must be preserved for display"
    );
    // Unknown outbound option survives (flow is not part of the typed model).
    let proxy = cfg
        .outbounds
        .iter()
        .find(|o| o.tag == "proxy")
        .expect("proxy outbound");
    assert_eq!(
        proxy.extra.get("flow").and_then(|v| v.as_str()),
        Some("xtls-rprx-vision")
    );
    // Unknown dns option survives.
    assert!(cfg.dns.extra.contains_key("independent_cache"));
    // Unknown tun inbound option survives.
    let tun = cfg.tun_inbound().expect("tun inbound");
    assert_eq!(tun.extra.get("auto_route"), Some(&serde_json::json!(true)));
    // Typed fields parsed correctly.
    assert_eq!(cfg.dns.strategy, "ipv4_only");
    assert_eq!(cfg.route.final_, "proxy");
    assert_eq!(tun.address, vec!["172.18.0.1/30".to_string()]);
    assert_eq!(tun.mtu, Some(1400));
    assert_eq!(tun.interface_name, None);
}

/// Credential masking by key-name pattern masks 100% of credential-bearing
/// values on first display (SC-004). The credential keys present in the
/// user's real config: uuid, tls.reality.public_key, tls.reality.short_id —
/// plus the common secret-shaped names from the spec's masking rule.
#[test]
fn masking_covers_one_hundred_percent_of_credentials() {
    let value: serde_json::Value = serde_json::from_str(FIXTURE).unwrap();
    let masked = mask_json(&value);
    let masked_str = serde_json::to_string(&masked).unwrap();

    // Every planted secret value is gone, replaced by the fixed mask.
    for secret in ["SECRET-UUID-VALUE", "SECRET-PUBKEY", "SECRET-SHORT"] {
        assert!(
            !masked_str.contains(secret),
            "secret {secret} leaked through masking"
        );
    }
    assert_eq!(masked["outbounds"][0]["uuid"], serde_json::json!(MASK));
    assert_eq!(
        masked["outbounds"][0]["tls"]["reality"]["public_key"],
        serde_json::json!(MASK)
    );
    assert_eq!(
        masked["outbounds"][0]["tls"]["reality"]["short_id"],
        serde_json::json!(MASK)
    );

    // Non-credential values are untouched.
    assert_eq!(
        masked["outbounds"][0]["server"],
        serde_json::json!("i.warmine.tech")
    );
    assert_eq!(
        masked["dns"]["servers"][0]["server"],
        serde_json::json!("8.8.8.8")
    );
    assert_eq!(
        masked["outbounds"][0]["tls"]["server_name"],
        serde_json::json!("yandex.tm")
    );

    // The full credential key-name pattern from the spec masks everything.
    let probe = serde_json::json!({
        "uuid": "a", "password": "b", "secret": "c", "key": "d", "token": "e",
        "short_id": "f", "public_key": "g", "private_key": "h",
        "auth": "i", "psk": "j", "client_secret": "k",
        "nested": { "password": "l", "list": [{ "token": "m" }] }
    });
    let masked_probe = mask_json(&probe);
    let flat = serde_json::to_string(&masked_probe).unwrap();
    for secret in [
        "\"a\"", "\"b\"", "\"c\"", "\"d\"", "\"e\"", "\"f\"", "\"g\"", "\"h\"", "\"i\"", "\"j\"",
        "\"k\"", "\"l\"", "\"m\"",
    ] {
        assert!(
            !flat.contains(secret),
            "credential value {secret} not masked"
        );
    }
    // And non-credentials near them survive.
    assert_eq!(
        masked_probe["nested"]["list"][0]["token"],
        serde_json::json!(MASK)
    );
}

/// Key-name detection: exactly the credential-shaped names (SC-004 / FR-027).
#[test]
fn credential_key_detection() {
    for k in [
        "uuid",
        "password",
        "secret",
        "key",
        "token",
        "short_id",
        "public_key",
        "private_key",
        "UUID",
        "ServerPassword",
        "auth",
        "psk",
    ] {
        assert!(
            is_credential_key(k),
            "{k} must be detected as a credential key"
        );
    }
    for k in [
        "type",
        "tag",
        "server",
        "server_port",
        "enabled",
        "mtu",
        "address",
        "level",
        "strategy",
        "fingerprint",
        "flow",
        "network",
    ] {
        assert!(
            !is_credential_key(k),
            "{k} must NOT be treated as a credential key"
        );
    }
}

/// `sing-box check` output is passed through verbatim, warnings included (FR-017):
/// classification separates errors from warnings without rewriting lines.
#[test]
fn validation_output_verbatim() {
    let stdout = "";
    let stderr = "ERROR: decode config at /Users/x/.config/sing-box/config.json: missing server\nWARN: deprecated option outbound.flow\n";
    let res = classify_output(false, stdout, stderr);
    assert!(!res.ok);
    assert_eq!(
        res.errors,
        vec!["ERROR: decode config at /Users/x/.config/sing-box/config.json: missing server"]
    );
    assert_eq!(res.warnings, vec!["WARN: deprecated option outbound.flow"]);
    // Verbatim: the raw combined output is preserved exactly, warnings included.
    assert_eq!(res.raw, stderr.trim_end());
    assert!(res.raw.contains("WARN: deprecated option outbound.flow"));
    // Non-blocking summary for warnings-only result.
    let ok_with_warnings = classify_output(true, "", stderr);
    assert!(ok_with_warnings.ok);
    assert_eq!(ok_with_warnings.summary(), "valid, 1 warning(s)");

    let clean = classify_output(true, "", "");
    assert_eq!(clean.summary(), "valid");
    assert!(clean.errors.is_empty() && clean.warnings.is_empty());
}
