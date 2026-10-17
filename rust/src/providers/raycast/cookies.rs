//! Raycast website session cookie selection.
//!
//! Only `__raycast_session` (required) and `csrf_token` (optional) are ever
//! forwarded to Raycast; every other cookie in a pasted header or a browser
//! profile is dropped before a request is built.

use crate::browser::cookies::Cookie;
use crate::browser::detection::BrowserType;

pub(super) const SESSION_COOKIE: &str = "__raycast_session";
const CSRF_COOKIE: &str = "csrf_token";

/// Domain handed to the browser extractor. It matches `www.raycast.com`,
/// `raycast.com`, and other subdomains; [`host_rank`] then keeps only the two
/// hosts the credits route accepts.
pub(super) const EXTRACT_DOMAIN: &str = "raycast.com";

/// Lower is more specific. `None` excludes hosts such as `backend.raycast.com`
/// whose cookies must never reach the website credits route.
fn host_rank(cookie_domain: &str) -> Option<u8> {
    let host = cookie_domain
        .trim()
        .trim_start_matches('.')
        .trim_end_matches('.')
        .to_ascii_lowercase();
    match host.as_str() {
        "www.raycast.com" => Some(0),
        "raycast.com" => Some(1),
        _ => None,
    }
}

fn usable_value(value: &str) -> Option<&str> {
    let value = value.trim();
    (!value.is_empty() && !value.chars().any(|ch| ch.is_control() || ch == ';')).then_some(value)
}

fn cookie_header(session: &str, csrf: Option<&str>) -> String {
    match csrf {
        Some(csrf) => format!("{SESSION_COOKIE}={session}; {CSRF_COOKIE}={csrf}"),
        None => format!("{SESSION_COOKIE}={session}"),
    }
}

/// Cookie headers to try, most specific host first, one per distinct session
/// value. An exact-host `www.raycast.com` cookie always outranks a same-name
/// parent-domain cookie, and the CSRF cookie follows the same precedence.
pub(super) fn browser_candidates(cookies: &[Cookie]) -> Vec<String> {
    let ranked = |name: &str| {
        let mut found: Vec<(u8, &str)> = cookies
            .iter()
            .filter(|cookie| cookie.name == name)
            .filter_map(|cookie| Some((host_rank(&cookie.domain)?, usable_value(&cookie.value)?)))
            .collect();
        found.sort_by_key(|(rank, _)| *rank);
        found
    };
    let csrf = ranked(CSRF_COOKIE).first().map(|(_, value)| *value);
    let mut seen: Vec<&str> = Vec::new();
    let mut headers = Vec::new();
    for (_, session) in ranked(SESSION_COOKIE) {
        if !seen.contains(&session) {
            seen.push(session);
            headers.push(cookie_header(session, csrf));
        }
    }
    headers
}

/// Candidate headers for each scanned browser, in the scan's browser order and
/// labelled with the browser's display name. Browsers without a usable session
/// keep an empty list so callers can skip them.
pub(super) fn browser_groups(
    scan: &[(BrowserType, Vec<Cookie>)],
) -> Vec<(&'static str, Vec<String>)> {
    scan.iter()
        .map(|(browser, cookies)| (browser.display_name(), browser_candidates(cookies)))
        .collect()
}

/// Reduce a pasted header to the two forwarded cookies. Returns `None` without
/// a non-empty `__raycast_session`. The first occurrence of a name wins, which
/// matches the most specific cookie a browser sends.
pub(super) fn manual_header(raw: &str) -> Option<String> {
    let normalized = crate::providers::normalize_cookie_header(raw)?;
    let first = |name: &str| {
        crate::providers::cookie_values(&normalized, name)
            .into_iter()
            .next()
    };
    let session = first(SESSION_COOKIE)?;
    Some(cookie_header(session, first(CSRF_COOKIE)))
}
