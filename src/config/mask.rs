//! Credential masking. This module is the single source of truth for what counts
//! as a secret (FR-015, FR-027, SC-004): the same predicate drives on-screen
//! masking and the guarantee that no credential reaches any log/cache/file.

use serde_json::{Map, Value};

/// The mask placeholder shown in place of a secret value.
pub const MASK: &str = "••••••••";

/// Whether a JSON object key names a credential-bearing field.
///
/// Matches the user's real config (uuid, tls.reality.public_key,
/// tls.reality.short_id) plus the common secret-shaped key names.
pub fn is_credential_key(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    // Substring detection covers camelCase compounds (ServerPassword,
    // publicKey) and both snake_case forms; exact matches catch the bare
    // names. No sing-box display field contains these substrings.
    [
        "uuid",
        "password",
        "secret",
        "token",
        "short_id",
        "api_key",
        "private_key",
        "public_key",
    ]
    .iter()
    .any(|t| k.contains(t))
        || k.ends_with("_key")
        || k == "key"
        || k == "auth"
        || k == "psk"
}

/// Recursively mask credential values in a JSON value, returning a new value.
/// Non-credential values are preserved unchanged.
pub fn mask_json(value: &Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut out = Map::new();
            for (k, v) in map {
                let masked = if is_credential_key(k) {
                    Value::String(mask_text(v))
                } else {
                    mask_json(v)
                };
                out.insert(k.clone(), masked);
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.iter().map(mask_json).collect()),
        other => other.clone(),
    }
}

/// A masked representation of a secret value that does not leak its length or
/// content (a fixed bullet run).
fn mask_text(_value: &Value) -> String {
    MASK.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_known_credential_keys() {
        for k in [
            "uuid",
            "public_key",
            "private_key",
            "short_id",
            "password",
            "token",
            "psk",
        ] {
            assert!(is_credential_key(k), "{k} should be a credential");
        }
        for k in [
            "type",
            "tag",
            "server",
            "server_port",
            "enabled",
            "fingerprint",
            "mtu",
        ] {
            assert!(!is_credential_key(k), "{k} should not be a credential");
        }
    }

    #[test]
    fn masks_nested_secrets_only() {
        let v: Value = serde_json::from_str(
            r#"{"uuid":"abc","tls":{"reality":{"public_key":"PUB","short_id":"sid","enabled":true}},"server":"host"}"#,
        )
        .unwrap();
        let masked = mask_json(&v);
        assert_eq!(masked["uuid"], Value::String(MASK.to_string()));
        assert_eq!(
            masked["tls"]["reality"]["public_key"],
            Value::String(MASK.to_string())
        );
        assert_eq!(
            masked["tls"]["reality"]["short_id"],
            Value::String(MASK.to_string())
        );
        assert_eq!(masked["tls"]["reality"]["enabled"], Value::Bool(true));
        assert_eq!(masked["server"], Value::String("host".to_string()));
    }
}
