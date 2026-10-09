//! Connectivity testing and route inference (US2).

pub mod address;
pub mod ping;
pub mod route;
pub mod traffic;

use crate::config::parse::SingBoxConfig;

/// Default connectivity test targets derived from the config (T020, spec
/// Confirmed Decisions): tunnel gateway, configured DNS resolver, proxy
/// server(s), and one public host.
pub fn default_targets(config: &SingBoxConfig) -> Vec<String> {
    let mut targets: Vec<String> = Vec::new();
    let push = |t: String, targets: &mut Vec<String>| {
        if !t.is_empty() && !targets.contains(&t) {
            targets.push(t);
        }
    };

    // Tunnel gateway: the tun address (172.18.0.1/30 → 172.18.0.1).
    if let Some(tun) = config.tun_inbound()
        && let Some(addr) = tun.address.first().and_then(|a| a.split('/').next())
    {
        push(addr.to_string(), &mut targets);
    }

    // Configured DNS resolver (host only — ping takes no port).
    for server in &config.dns.servers {
        push(server.server.clone(), &mut targets);
    }

    // Proxy server(s).
    for host in server_hosts(config) {
        push(host, &mut targets);
    }

    // One public host.
    push("example.com".to_string(), &mut targets);

    targets
}

/// Hosts of the configured proxy outbounds (T020). Checks against them are
/// diagnostic only: proxy servers commonly drop ICMP echo by policy, so
/// their silence says nothing about the tunnel (see `recompute_health`).
pub fn server_hosts(config: &SingBoxConfig) -> Vec<String> {
    config
        .outbounds
        .iter()
        .filter(|o| !o.server.is_empty() && o.ty != "direct")
        .map(|o| o.server.clone())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targets_from_fixture_shape() {
        let cfg: SingBoxConfig = serde_json::from_str(
            r#"{
          "dns": {"servers": [{"type":"udp","tag":"dns-direct","server":"8.8.8.8","server_port":53}]},
          "inbounds": [{"type":"tun","tag":"tun-in","address":["172.18.0.1/30"],"mtu":1400}],
          "outbounds": [
            {"type":"vless","tag":"proxy","server":"i.warmine.tech","server_port":443},
            {"type":"direct","tag":"direct"}
          ]
        }"#,
        )
        .unwrap();
        assert_eq!(
            default_targets(&cfg),
            vec!["172.18.0.1", "8.8.8.8", "i.warmine.tech", "example.com"]
        );
    }

    #[test]
    fn empty_config_still_yields_public_host() {
        assert_eq!(
            default_targets(&SingBoxConfig::default()),
            vec!["example.com"]
        );
    }
}
