//! Fixture-based tests of routing-rule evaluation (FR-010, FR-014):
//! rules are evaluated in order — first match wins; conditions come from the
//! user's config shape (domain_suffix, ip_cidr, port, protocol); the
//! final/default outcome applies when nothing matches.

use sing_box_tui::config::parse::{Outbound, parse};
use sing_box_tui::net::route::{InferredRoute, infer_route};

/// The user's real config shape (values replaced; structure identical).
const FIXTURE: &str = r#"{
  "log": { "level": "info", "timestamp": true },
  "dns": {
    "servers": [
      { "type": "udp", "tag": "dns-direct", "server": "8.8.8.8", "server_port": 53 }
    ],
    "final": "dns-direct",
    "strategy": "ipv4_only"
  },
  "inbounds": [
    { "type": "tun", "tag": "tun-in", "address": ["172.18.0.1/30"], "mtu": 1400 }
  ],
  "outbounds": [
    { "type": "vless", "tag": "proxy", "server": "i.warmine.tech", "server_port": 443, "uuid": "x" },
    { "type": "direct", "tag": "direct" }
  ],
  "route": {
    "rules": [
      { "network": "udp", "port": 443, "action": "reject" },
      { "inbound": ["tun-in"], "action": "sniff" },
      { "protocol": "dns", "action": "hijack-dns" },
      { "port": 53, "action": "route", "outbound": "direct" },
      { "domain": ["i.warmine.tech"], "action": "route", "outbound": "direct" },
      { "ip_is_private": true, "action": "route", "outbound": "direct" },
      { "domain_suffix": ["ru", "xn--p1ai"], "action": "route", "outbound": "direct" }
    ],
    "final": "proxy"
  }
}"#;

fn fixture() -> sing_box_tui::config::parse::SingBoxConfig {
    parse(FIXTURE).expect("fixture must parse")
}

#[test]
fn rules_evaluate_in_order_first_match_wins() {
    let cfg = fixture();
    let cases: &[(&str, InferredRoute)] = &[
        // domain_suffix ru → direct (rule 7)
        ("ya.ru", InferredRoute::Direct),
        // IDN TLD in punycode form → direct
        ("мойсайт.xn--p1ai", InferredRoute::Direct),
        // Exact domain rule (rule 5) beats the later suffix rules
        ("i.warmine.tech", InferredRoute::Direct),
        // Private IP → direct (rule 6)
        ("192.168.1.5", InferredRoute::Direct),
        ("10.0.0.1", InferredRoute::Direct),
        ("172.18.0.1", InferredRoute::Direct),
        // Nothing matches → final/default = proxy
        ("example.com", InferredRoute::Proxy),
        ("8.8.8.8", InferredRoute::Proxy),
        ("i.warmine.tech.evil.io", InferredRoute::Proxy),
    ];
    for (target, expected) in cases {
        let got = infer_route(target, &cfg.route.rules, &cfg.route.final_, &cfg.outbounds);
        assert_eq!(
            &got, expected,
            "target {target}: inferred {got:?}, expected {expected:?}"
        );
    }
}

/// Conditions that cannot apply to an ICMP probe (port/protocol/network/inbound)
/// never match: the tool infers the route for ping targets, and e.g. the
/// udp/443 reject rule must not claim `example.com`.
#[test]
fn port_protocol_network_inbound_conditions_do_not_match_probes() {
    let cfg = fixture();
    // Without the domain/suffix rules these targets must fall through to final.
    let mut no_domain_rules = cfg.route.rules.clone();
    no_domain_rules.retain(|r| {
        r.domain.is_empty() && r.domain_suffix.is_empty() && !r.ip_is_private.unwrap_or(false)
    });

    for target in ["i.warmine.tech", "example.com", "ya.ru", "8.8.8.8"] {
        let got = infer_route(target, &no_domain_rules, &cfg.route.final_, &cfg.outbounds);
        assert_eq!(
            got,
            InferredRoute::Proxy,
            "port/protocol/network/inbound-only rules must not match {target}; expected final=proxy"
        );
    }
}

/// The sniff action is non-routing: it is skipped, and a later rule decides.
#[test]
fn sniff_rules_are_skipped() {
    let cfg = fixture();
    // A config whose ONLY rule is sniff → everything falls to final.
    let got = infer_route(
        "ya.ru",
        &cfg.route.rules[..2],
        &cfg.route.final_,
        &cfg.outbounds,
    );
    assert_eq!(got, InferredRoute::Proxy);
}

/// The final/default outcome applies when nothing matches, whatever it is set to.
#[test]
fn final_outbound_applies_when_nothing_matches() {
    let cfg = fixture();
    let rules = &cfg.route.rules[..0];
    assert_eq!(
        infer_route("example.com", rules, "proxy", &cfg.outbounds),
        InferredRoute::Proxy
    );
    assert_eq!(
        infer_route("example.com", rules, "direct", &cfg.outbounds),
        InferredRoute::Direct
    );
    // Unknown final tag is reported as the tag itself, not guessed.
    assert_eq!(
        infer_route("example.com", rules, "wg", &cfg.outbounds),
        InferredRoute::Other("wg".into())
    );
}

/// ip_cidr conditions use real CIDR containment, not prefix string matching.
#[test]
fn ip_cidr_uses_cidr_containment() {
    let cfg = fixture();
    let rules = vec![sing_box_tui::config::parse::RouteRule {
        ip_cidr: vec!["192.168.0.0/16".into(), "10.8.0.0/24".into()],
        action: "route".into(),
        outbound: "direct".into(),
        ..Default::default()
    }];
    for t in ["192.168.1.5", "192.168.255.254", "10.8.0.7"] {
        assert_eq!(
            infer_route(t, &rules, "proxy", &cfg.outbounds),
            InferredRoute::Direct,
            "{t} in CIDR"
        );
    }
    for t in ["192.169.0.1", "10.9.0.1", "10.8.1.1"] {
        assert_eq!(
            infer_route(t, &rules, "proxy", &cfg.outbounds),
            InferredRoute::Proxy,
            "{t} outside CIDR"
        );
    }
}

/// Route classification resolves outbound tags against the outbound list:
/// `direct`-type outbounds are direct, anything else is proxied (FR-004).
#[test]
fn outbound_tags_resolve_against_outbound_types() {
    let outbounds = vec![
        Outbound {
            ty: "vless".into(),
            tag: "proxy".into(),
            ..Default::default()
        },
        Outbound {
            ty: "direct".into(),
            tag: "direct".into(),
            ..Default::default()
        },
    ];
    let rules = vec![sing_box_tui::config::parse::RouteRule {
        domain: ["example.org".into()].into(),
        action: "route".into(),
        outbound: "proxy".into(),
        ..Default::default()
    }];
    assert_eq!(
        infer_route("example.org", &rules, "", &outbounds),
        InferredRoute::Proxy
    );
    assert_eq!(
        infer_route("other.org", &rules, "direct", &outbounds),
        InferredRoute::Direct
    );
}
