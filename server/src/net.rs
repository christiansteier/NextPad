//! Quelladressen: Proxy-Header nur von vertrauenswürdigen Proxys, Adresslisten mit CIDR.
use crate::AppState;
use axum::http::HeaderMap;
use ipnet::IpNet;
use std::net::{IpAddr, SocketAddr};

/// Eintrag einer Adressliste: einzelne Adresse oder Netz (`192.0.2.0/24`), `*` für alle.
pub fn parse_list(s: &str) -> Vec<IpNet> {
    s.split(',').map(str::trim).filter(|x| !x.is_empty()).filter_map(|x| {
        if x == "*" { return Some(vec!["0.0.0.0/0".parse().ok()?, "::/0".parse().ok()?]); }
        if let Ok(n) = x.parse::<IpNet>() { return Some(vec![n]); }
        x.parse::<IpAddr>().ok().map(|a| vec![IpNet::from(a)])
    }).flatten().collect()
}

fn canonical(a: IpAddr) -> IpAddr {
    match a { IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(IpAddr::V6(v6)), v4 => v4 }
}

pub fn in_list(list: &[IpNet], a: IpAddr) -> bool { let a = canonical(a); list.iter().any(|n| n.contains(&a)) }

/// Adresse des Clients. Header nur, wenn die direkte Gegenstelle ein vertrauenswürdiger Proxy ist;
/// dann X-Real-IP, sonst der letzte Eintrag von X-Forwarded-For (den hat der Proxy angehängt).
pub fn client_ip(state: &AppState, headers: &HeaderMap, peer: SocketAddr) -> IpAddr {
    let peer_ip = canonical(peer.ip());
    if state.config.trust_proxy && in_list(&state.config.trusted_proxies, peer_ip) {
        if let Some(ip) = headers.get("x-real-ip").and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse::<IpAddr>().ok()) { return canonical(ip); }
        if let Some(ip) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()).and_then(|v| v.rsplit(',').next()).and_then(|v| v.trim().parse::<IpAddr>().ok()) { return canonical(ip); }
    }
    peer_ip
}

/// Schlüssel für die Ratenbegrenzung: IPv4 je Adresse, IPv6 je /64.
pub fn rate_key(a: IpAddr) -> String {
    match a { IpAddr::V4(v4) => v4.to_string(), IpAddr::V6(v6) => { let s = v6.segments(); format!("{:x}:{:x}:{:x}:{:x}::/64", s[0], s[1], s[2], s[3]) } }
}
