//! Route inference from routing rules (T019, FR-010, FR-014, SC-015).
//!
//! Given a target (hostname or IP), evaluate the parsed routing rules in
//! order and return the selected outbound (proxy vs direct vs reject).
//! Computed from the config alone, with no outside dependency.
//!
//! Probe semantics: this tool infers the route *for a ping target* (ICMP,
//! no port, no protocol). Conditions that cannot apply to such a probe —
//! `port`, `protocol`, `network`, `inbound` — make a rule unmatchable here,
//! because the tool must not claim a match it cannot observe. Non-routing
//! actions (`sniff`) are skipped: they never decide the outbound.

use std::net::IpAddr;

use crate::config::parse::{Outbound, RouteRule};

/// The route a target takes (display string via `as_str`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InferredRoute {
    /// Traffic goes through a proxy outbound.
    Proxy,
    /// Traffic goes direct (bypass).
    Direct,
    /// Traffic is rejected.
    Reject,
    /// DNS is hijacked.
    HijackDns,
    /// A named outbound that is neither proxy-type nor direct (e.g. a wireguard
    /// endpoint tag); reported truthfully as its tag.
    Other(String),
}

impl InferredRoute {
    pub fn as_str(&self) -> &str {
        match self {
            InferredRoute::Proxy => "proxy",
            InferredRoute::Direct => "direct",
            InferredRoute::Reject => "reject",
            InferredRoute::HijackDns => "hijack-dns",
            InferredRoute::Other(tag) => tag.as_str(),
        }
    }
}

/// Infer the route for a target: first matching rule wins; if nothing
/// matches, the route's `final`/default outbound applies.
pub fn infer_route(
    target: &str,
    rules: &[RouteRule],
    final_outbound: &str,
    outbounds: &[Outbound],
) -> InferredRoute {
    for rule in rules {
        // Non-routing actions never decide the outbound.
        if rule.action.eq_ignore_ascii_case("sniff") {
            continue;
        }
        if rule_matches_probe_target(target, rule) {
            return route_from_action(&rule.action, &rule.outbound, outbounds);
        }
    }
    classify_outbound_tag(final_outbound, outbounds)
}

/// Map a rule's action/outbound pair to a route.
fn route_from_action(action: &str, outbound: &str, outbounds: &[Outbound]) -> InferredRoute {
    match action.to_ascii_lowercase().as_str() {
        "reject" => InferredRoute::Reject,
        "hijack-dns" => InferredRoute::HijackDns,
        // "route" and legacy empty-action rules carry the outbound.
        _ => classify_outbound_tag(outbound, outbounds),
    }
}

/// Resolve an outbound tag against the outbound list: `direct`-type outbounds
/// are direct; everything else is proxied (FR-004).
fn classify_outbound_tag(tag: &str, outbounds: &[Outbound]) -> InferredRoute {
    if tag.is_empty() {
        return InferredRoute::Direct;
    }
    match outbounds.iter().find(|o| o.tag == tag) {
        Some(o) if o.ty == "direct" => InferredRoute::Direct,
        Some(o) if o.ty == "proxy" => InferredRoute::Proxy,
        Some(_) => InferredRoute::Proxy,
        // Unknown tag (e.g. built-in "direct" not listed): fall back to name.
        None if tag == "direct" => InferredRoute::Direct,
        None => InferredRoute::Other(tag.to_string()),
    }
}

/// Does a rule match the target for an ICMP probe?
///
/// Domain conditions only apply to hostname targets; IP conditions only to
/// literal IP targets; port/protocol/network/inbound conditions can never be
/// satisfied by an ICMP probe.
fn rule_matches_probe_target(target: &str, rule: &RouteRule) -> bool {
    // Any condition the probe cannot satisfy disqualifies the rule outright —
    // a rule is a conjunction of its conditions.
    if is_set(&rule.port)
        || !rule.protocol.is_empty()
        || !rule.network.is_empty()
        || !rule.inbound.is_empty()
    {
        return false;
    }

    let target_ip: Option<IpAddr> = target.trim().parse().ok();
    let matched_any_domain_cond = !rule.domain.is_empty()
        || !rule.domain_suffix.is_empty()
        || !rule.domain_keyword.is_empty();
    let _matched_any_ip_cond = !rule.ip_cidr.is_empty() || rule.ip_is_private.is_some();

    // Domain conditions: target must be a hostname.
    if matched_any_domain_cond && target_ip.is_none() {
        let lower = target.to_ascii_lowercase();
        if rule.domain.iter().any(|d| d.eq_ignore_ascii_case(target)) {
            return true;
        }
        if rule.domain_suffix.iter().any(|s| {
            let s = s.to_ascii_lowercase();
            lower == s || lower.ends_with(&format!(".{s}"))
        }) {
            return true;
        }
        if rule
            .domain_keyword
            .iter()
            .any(|k| lower.contains(&k.to_ascii_lowercase()))
        {
            return true;
        }
    }

    // IP conditions: target must be a literal address.
    if let Some(ip) = target_ip {
        if rule.ip_cidr.iter().any(|cidr| cidr_contains(cidr, ip)) {
            return true;
        }
        if rule.ip_is_private == Some(true) && is_private_ip(ip) {
            return true;
        }
    }

    false
}

fn is_set(port: &Option<serde_json::Value>) -> bool {
    match port {
        None => false,
        Some(serde_json::Value::Null) => false,
        Some(_) => true,
    }
}

/// Real CIDR containment (v4-in-v4, v6-in-v6), not string prefix matching.
pub fn cidr_contains(cidr: &str, ip: IpAddr) -> bool {
    let Some((net_str, prefix_str)) = cidr.split_once('/') else {
        // Bare address: exact match.
        return cidr.trim() == ip.to_string();
    };
    let Ok(prefix) = prefix_str.trim().parse::<u32>() else {
        return false;
    };
    match (net_str.trim().parse::<IpAddr>(), ip) {
        (Ok(IpAddr::V4(net)), IpAddr::V4(addr)) => {
            if prefix > 32 {
                return false;
            }
            let shift = 32 - prefix;
            let net = u32::from(net) >> shift;
            let addr = u32::from(addr) >> shift;
            net == addr
        }
        (Ok(IpAddr::V6(net)), IpAddr::V6(addr)) => {
            if prefix > 128 {
                return false;
            }
            let shift = 128 - prefix;
            let net = u128::from(net) >> shift;
            let addr = u128::from(addr) >> shift;
            net == addr
        }
        _ => false,
    }
}

/// Private/loopback/link-local address check for IPv4 and IPv6.
pub fn is_private_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_private() || v4.is_loopback() || v4.is_link_local(),
        IpAddr::V6(v6) => v6.is_loopback() || v6.is_unique_local() || v6.is_unicast_link_local(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cidr_containment() {
        assert!(cidr_contains(
            "192.168.0.0/16",
            "192.168.255.254".parse().unwrap()
        ));
        assert!(!cidr_contains(
            "192.168.0.0/16",
            "192.169.0.1".parse().unwrap()
        ));
        assert!(cidr_contains("10.8.0.0/24", "10.8.0.7".parse().unwrap()));
        assert!(!cidr_contains("10.8.0.0/24", "10.8.1.1".parse().unwrap()));
        assert!(cidr_contains("fd00::/8", "fd12::1".parse().unwrap()));
        assert!(!cidr_contains("0.0.0.0/0", "::1".parse().unwrap()));
        // Bare address form.
        assert!(cidr_contains("8.8.8.8", "8.8.8.8".parse().unwrap()));
    }

    #[test]
    fn private_ips() {
        for a in ["192.168.1.1", "10.0.0.1", "172.18.0.1", "127.0.0.1"] {
            assert!(is_private_ip(a.parse().unwrap()), "{a} private");
        }
        assert!(!is_private_ip("8.8.8.8".parse().unwrap()));
    }
}
