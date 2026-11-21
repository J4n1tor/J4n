//! What a fetch may reach. An address is `http` or `https`, names a host, and carries no
//! login; the host is not a machine of the operator's own — loopback, a private or link-local range (the cloud's metadata
//! address among them), a name that leads there — unless the operator said `web.local`. The check is made on the name as it
//! resolves *now*, and again at every hop of a redirect.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, ToSocketAddrs};
use url::Url;

/// Whether an address is one of this machine or its network.
pub fn private(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v) => private_v4(v),
        IpAddr::V6(v) => private_v6(v),
    }
}

fn private_v4(v: Ipv4Addr) -> bool {
    let o = v.octets();
    v.is_loopback() || v.is_private() || v.is_link_local() || v.is_unspecified() || v.is_broadcast() || v.is_multicast() || (o[0] == 100 && (64..128).contains(&o[1])) || o[0] == 0
}

fn private_v6(v: Ipv6Addr) -> bool {
    if let Some(m) = v.to_ipv4_mapped() {
        return private_v4(m);
    }
    let s = v.segments();
    v.is_loopback() || v.is_unspecified() || v.is_multicast() || (s[0] & 0xfe00) == 0xfc00 || (s[0] & 0xffc0) == 0xfe80
}

/// The address, parsed, or why it is not one a fetch may take. `local` allows the operator's own machine and network.
pub fn vet(address: &str, local: bool) -> Result<Url, String> {
    let url = Url::parse(address.trim()).map_err(|e| format!("`{address}` is not an address: {e}"))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(format!("only http and https addresses are fetched, not `{}`", url.scheme()));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("an address with a login in it is not fetched: the login would be written down with the address".into());
    }
    let host = url.host_str().ok_or("the address names no host")?;
    if !local {
        let port = url.port_or_known_default().unwrap_or(80);
        // a literal address needs no lookup; a name is looked up, and every address it has must be a public one
        let found: Vec<IpAddr> = match host.parse::<IpAddr>() {
            Ok(ip) => vec![ip],
            Err(_) => (host, port).to_socket_addrs().map_err(|e| format!("`{host}` cannot be found: {e}"))?.map(|a| a.ip()).collect(),
        };
        if let Some(ip) = found.iter().find(|ip| private(**ip)) {
            return Err(format!("`{host}` is {ip}, a machine of your own or of your network: set `web.local` to fetch from there"));
        }
        if found.is_empty() {
            return Err(format!("`{host}` has no address"));
        }
    }
    Ok(url)
}

/// The address a redirect leads to, from the one it came from.
pub fn follow(from: &Url, location: &str) -> Result<String, String> {
    from.join(location.trim()).map(|u| u.to_string()).map_err(|e| format!("the redirect to `{location}` is not an address: {e}"))
}
