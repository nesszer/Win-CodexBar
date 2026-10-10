//! llmman daemon endpoint normalization and policy.
//!
//! Upstream ships the `https-or-private-network-http` endpoint policy: HTTPS
//! to any host, plain HTTP only to loopback, private-network, and `.local`
//! hosts, so an API key never crosses the public internet unencrypted.

use std::net::{Ipv4Addr, Ipv6Addr};

use reqwest::Url;

pub(crate) const DEFAULT_BASE_URL: &str = "http://127.0.0.1:17434";
const DEFAULT_PORT: u16 = 17434;

const POLICY_MESSAGE: &str = "llmman base URL must use HTTPS, or HTTP for localhost and private-network hosts, without embedded credentials, query, or fragment.";

/// Normalize a user-supplied llmman base URL, or fail with a friendly message.
///
/// Empty input resolves to the default local daemon. A scheme-less input is
/// HTTP and gets the daemon port unless one is given; `0.0.0.0` (a listen
/// address, not a destination) is rewritten to loopback. The result has no
/// trailing slash; a trailing `/v1` is kept here and dropped per request.
pub(crate) fn validated_llmman_base_url(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim().trim_matches(|c| c == '"' || c == '\'').trim();
    if trimmed.is_empty() {
        return Ok(DEFAULT_BASE_URL.to_string());
    }
    if trimmed.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(POLICY_MESSAGE.to_string());
    }

    let has_scheme = trimmed.contains("://");
    let candidate = if has_scheme {
        trimmed.to_string()
    } else {
        format!("http://{trimmed}")
    };
    let url = Url::parse(&candidate).map_err(|_| POLICY_MESSAGE.to_string())?;

    let scheme = url.scheme();
    if !matches!(scheme, "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(POLICY_MESSAGE.to_string());
    }
    let host = url
        .host_str()
        .map(Host::parse)
        .ok_or_else(|| POLICY_MESSAGE.to_string())?;
    if scheme == "http" && !is_private_network_host(&host) {
        return Err(POLICY_MESSAGE.to_string());
    }

    let host_text = match host {
        Host::Ipv4(ip) if ip.is_unspecified() => Ipv4Addr::LOCALHOST.to_string(),
        Host::Ipv4(ip) => ip.to_string(),
        Host::Ipv6(ip) => format!("[{ip}]"),
        Host::Domain(domain) => domain,
    };
    let port = if !has_scheme && !has_explicit_port(&candidate) {
        Some(DEFAULT_PORT)
    } else {
        url.port()
    };
    let port_text = port.map(|port| format!(":{port}")).unwrap_or_default();
    let path = url.path().trim_end_matches('/');
    Ok(format!("{scheme}://{host_text}{port_text}{path}"))
}

/// The base URL requests are built from: `/v1` (the OpenAI-style base agents
/// are given) is not part of the daemon's own routes.
pub(super) fn request_base(base: &str) -> &str {
    base.strip_suffix("/v1").unwrap_or(base)
}

/// Whether the authority of `http://...` text names a port. `Url::port` cannot
/// tell, because it drops the scheme's default port.
fn has_explicit_port(candidate: &str) -> bool {
    let authority = candidate
        .trim_start_matches("http://")
        .split(['/', '?', '#'])
        .next()
        .unwrap_or_default();
    let host_and_port = authority
        .rsplit_once('@')
        .map_or(authority, |(_, rest)| rest);
    match host_and_port.rsplit_once(']') {
        Some((_, after_bracket)) => after_bracket.starts_with(':'),
        None => host_and_port.contains(':'),
    }
}

fn is_private_network_host(host: &Host) -> bool {
    match host {
        Host::Ipv4(ip) => is_private_ipv4(*ip),
        Host::Ipv6(ip) => is_private_ipv6(*ip),
        Host::Domain(domain) => {
            let domain = domain.trim_end_matches('.');
            domain == "localhost"
                || domain
                    .strip_suffix(".local")
                    .is_some_and(|label| !label.is_empty())
        }
    }
}

enum Host {
    Ipv4(Ipv4Addr),
    Ipv6(Ipv6Addr),
    Domain(String),
}

impl Host {
    /// Classify the serialized host of a parsed URL (IPv6 keeps its brackets).
    fn parse(host: &str) -> Self {
        if let Some(inner) = host.strip_prefix('[').and_then(|h| h.strip_suffix(']'))
            && let Ok(ip) = inner.parse()
        {
            return Self::Ipv6(ip);
        }
        match host.parse() {
            Ok(ip) => Self::Ipv4(ip),
            Err(_) => Self::Domain(host.to_string()),
        }
    }
}

fn is_private_ipv4(ip: Ipv4Addr) -> bool {
    // Loopback 127/8, 10/8, 172.16/12, 192.168/16, link-local 169.254/16, and
    // 0.0.0.0 (rewritten to loopback by the caller).
    ip.is_loopback() || ip.is_private() || ip.is_link_local() || ip.is_unspecified()
}

fn is_private_ipv6(ip: Ipv6Addr) -> bool {
    let first = ip.segments()[0];
    // ::1, unique local fc00::/7, link-local fe80::/10.
    ip.is_loopback() || first & 0xfe00 == 0xfc00 || first & 0xffc0 == 0xfe80
}
