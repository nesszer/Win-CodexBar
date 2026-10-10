//! Ollama session cookie normalization, recognition, and browser import.
//!
//! Extracted from `mod.rs`. Owns the cookie source enum, header normalization
//! (cURL/Cookie: label stripping), session-cookie name recognition (AuthKit,
//! NextAuth chunked), browser import + validated-cache reuse, and sign-in
//! redirect detection.

use reqwest::Url;

use crate::browser::cookies::{Cookie, CookieExtractor};
use crate::core::{FetchContext, ProviderError, ProviderId};
use crate::providers::strip_cookie_prefix;

pub(super) const OLLAMA_COOKIE_DOMAIN: &str = "ollama.com";
pub(super) const OLLAMA_SESSION_COOKIE_NAME: &str = "__Secure-session";
pub(super) const OLLAMA_SESSION_COOKIE_NAMES: &[&str] = &[
    "session",
    OLLAMA_SESSION_COOKIE_NAME,
    "ollama_session",
    "__Host-ollama_session",
    "wos-session",
    "__Secure-next-auth.session-token",
    "next-auth.session-token",
];

pub(super) enum OllamaCookieSource {
    Manual(String),
    Browser(Vec<Cookie>),
}

impl OllamaCookieSource {
    pub(super) fn header_for_url(&self, url: &Url) -> Option<String> {
        match self {
            Self::Manual(header) => should_attach_ollama_cookie(url).then(|| header.clone()),
            Self::Browser(cookies) => ollama_cookie_header_for_url(cookies, url),
        }
    }
}

/// Normalize a raw cookie header input — strip cURL wrappers and `Cookie:`
/// labels, and prefix bare values with the session cookie name.
pub(super) fn normalize_cookie_header(input: &str) -> Option<String> {
    let mut header = strip_curl_cookie_wrapper(input);
    if header.is_empty() {
        return None;
    }

    header = strip_cookie_prefix(header);

    if header.is_empty() {
        return None;
    }

    if header.contains('=') {
        // Upstream 0.50.1 #2949: a copied `Cookie:` label can appear
        // mid-string when another cookie comes first — drop the label
        // from every `;`-separated segment before sending.
        let cleaned = header
            .split(';')
            .map(str::trim)
            .map(|segment| strip_cookie_prefix(segment).to_string())
            .filter(|segment| !segment.is_empty())
            .collect::<Vec<_>>()
            .join("; ");
        (!cleaned.is_empty()).then_some(cleaned)
    } else {
        Some(format!("{OLLAMA_SESSION_COOKIE_NAME}={header}"))
    }
}

/// Resolve cookies from manual cookies, validated cache, or browser import.
///
/// Upstream #2404: reuse the last validated browser session cookie header
/// across refreshes until auth fails, then re-import.
pub(super) fn resolve_cookie_source(
    ctx: &FetchContext,
) -> Result<OllamaCookieSource, ProviderError> {
    // Check manual cookie header first
    if let Some(cookie) = &ctx.manual_cookie_header
        && let Some(header) = normalize_cookie_header(cookie)
    {
        return has_recognized_ollama_session_cookie(&header)
            .then_some(OllamaCookieSource::Manual(header))
            .ok_or(ProviderError::NoCookies);
    }

    // Manual source with nothing pasted: fail closed before any browser import
    // (upstream `OllamaUsageError.manualCookieHeaderEmpty`, #3891).
    if ctx.manual_cookie_missing {
        return Err(manual_cookie_header_empty_error());
    }

    match resolve_browser_cookie_header(false)? {
        Some(header) => Ok(OllamaCookieSource::Manual(header)),
        None => Err(ProviderError::NoCookies),
    }
}

pub(super) fn manual_cookie_header_empty_error() -> ProviderError {
    ProviderError::Other(
        concat!(
            "Ollama cookie source is Manual, but no cookie header is configured. ",
            "Paste a Cookie header from https://ollama.com/settings, or set Cookie source to Auto."
        )
        .to_string(),
    )
}

/// After a successful web fetch, cache the validated browser/manual session header.
pub(super) fn cache_validated_session_cookie(source: &OllamaCookieSource) {
    use crate::browser::cookie_cache::CookieHeaderCache;
    if let Some(header) =
        source.header_for_url(&Url::parse("https://ollama.com/settings").expect("static url"))
    {
        let label = match source {
            OllamaCookieSource::Manual(_) => "validated",
            OllamaCookieSource::Browser(_) => "browser",
        };
        // Cache store is best-effort: a failed write just means the next fetch re-validates.
        let _stored = CookieHeaderCache::store(ProviderId::Ollama, &header, label);
    }
}

/// Clear cached session after auth failure so the next refresh re-imports.
pub(super) fn invalidate_cached_session_cookie() {
    use crate::browser::cookie_cache::CookieHeaderCache;
    CookieHeaderCache::clear(ProviderId::Ollama);
}

/// Strip copied cURL cookie syntax (`-b …`, `--cookie …`, `-H …`) and the
/// surrounding quotes before normalizing the header value (upstream 0.50.1
/// #2949).
pub(super) fn strip_curl_cookie_wrapper(raw: &str) -> &str {
    let mut header = raw.trim();
    for prefix in ["-b ", "--cookie ", "-H "] {
        if let Some(rest) = header.strip_prefix(prefix) {
            header = rest.trim();
        }
    }
    header.trim_matches('\'').trim_matches('"').trim()
}

/// Resolve a browser/session cookie header for Ollama.
///
/// When `force_reimport` is false, prefers the last validated cached header
/// (upstream #2404). On force or cache miss, imports from the browser.
pub(super) fn resolve_browser_cookie_header(
    force_reimport: bool,
) -> Result<Option<String>, ProviderError> {
    use crate::browser::cookie_cache::CookieHeaderCache;

    if !force_reimport
        && let Some(cached) = CookieHeaderCache::load(ProviderId::Ollama)
        && has_recognized_ollama_session_cookie(&cached.cookie_header)
    {
        return Ok(Some(cached.cookie_header));
    }

    let url = Url::parse("https://ollama.com/settings")
        .map_err(|e| ProviderError::Other(e.to_string()))?;

    // Upstream Win-CodexBar #426: the generic `browser_cookies_for_domain` helper
    // stops at the FIRST installed browser that has *any* cookies for the domain,
    // even if those cookies are stale/irrelevant (e.g. a consent or CDN cookie left
    // behind in Chrome/Edge from a one-off visit) and don't include a recognized
    // Ollama session cookie. That starves out a later browser (often Brave) that
    // actually holds the logged-in session. Walk every detected browser ourselves
    // and keep going until one yields a header with a recognized session cookie.
    use crate::browser::cookies::CookieExtractor;
    use crate::browser::detection::BrowserDetector;

    let mut first_error = None;
    for browser in BrowserDetector::detect_all() {
        match CookieExtractor::extract_for_domain(&browser, OLLAMA_COOKIE_DOMAIN) {
            Ok(cookies) => {
                if let Some(header) = ollama_cookie_header_for_url(&cookies, &url) {
                    return Ok(Some(header));
                }
            }
            Err(error) => {
                let error = crate::providers::map_browser_cookie_error(error);
                if !matches!(error, ProviderError::NoCookies) && first_error.is_none() {
                    first_error = Some(error);
                }
            }
        }
    }
    if let Some(error) = first_error {
        return Err(error);
    }
    Ok(None)
}

/// Return the header for the first cookie set (in order) that contains a
/// recognized Ollama session cookie for `url`, skipping sets that decrypt
/// fine but carry no usable session (see `resolve_browser_cookie_header`).
fn first_recognized_cookie_header(
    mut cookie_sets: impl Iterator<Item = Vec<Cookie>>,
    url: &Url,
) -> Option<String> {
    cookie_sets.find_map(|cookies| ollama_cookie_header_for_url(&cookies, url))
}

pub(super) fn should_attach_ollama_cookie(url: &Url) -> bool {
    url.scheme() == "https"
        && url
            .host_str()
            .is_some_and(|host| host.eq_ignore_ascii_case(OLLAMA_COOKIE_DOMAIN))
}

pub(super) fn has_recognized_ollama_session_cookie(header: &str) -> bool {
    header.split(';').any(|pair| {
        let name = pair.trim().split_once('=').map(|(name, _)| name.trim());
        name.is_some_and(is_recognized_ollama_session_cookie_name)
    })
}

pub(super) fn ollama_cookie_header_for_url(cookies: &[Cookie], url: &Url) -> Option<String> {
    let cookies: Vec<_> = cookies
        .iter()
        .filter(|cookie| cookie_applies_to_ollama_url(cookie, url))
        .cloned()
        .collect();
    let header = CookieExtractor::build_cookie_header(&cookies);
    has_recognized_ollama_session_cookie(&header).then_some(header)
}

pub(super) fn cookie_applies_to_ollama_url(cookie: &Cookie, url: &Url) -> bool {
    let domain = cookie
        .domain
        .trim()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    let path = if cookie.path.is_empty() {
        "/"
    } else {
        cookie.path.as_str()
    };
    let request_path = url.path();
    should_attach_ollama_cookie(url)
        && (domain == OLLAMA_COOKIE_DOMAIN
            || domain.strip_prefix('.') == Some(OLLAMA_COOKIE_DOMAIN))
        && (path == "/"
            || request_path == path
            || (request_path.starts_with(path)
                && (path.ends_with('/') || request_path.as_bytes().get(path.len()) == Some(&b'/'))))
}

pub(super) fn is_recognized_ollama_session_cookie_name(name: &str) -> bool {
    OLLAMA_SESSION_COOKIE_NAMES.contains(&name)
        || is_chunked_nextauth_cookie_name(name, "__Secure-next-auth.session-token")
        || is_chunked_nextauth_cookie_name(name, "next-auth.session-token")
}

pub(super) fn is_chunked_nextauth_cookie_name(name: &str, base_name: &str) -> bool {
    name.strip_prefix(base_name)
        .and_then(|suffix| suffix.strip_prefix('.'))
        .is_some_and(|suffix| {
            !suffix.is_empty() && suffix.bytes().all(|byte| byte.is_ascii_digit())
        })
}

pub(super) fn is_ollama_sign_in_redirect(url: &Url) -> bool {
    if url.scheme() != "https" {
        return false;
    }
    let Some(host) = url.host_str().map(str::to_ascii_lowercase) else {
        return false;
    };
    let path = url.path().to_ascii_lowercase();
    if host == OLLAMA_COOKIE_DOMAIN || host == "www.ollama.com" {
        return path == "/signin" || path.starts_with("/signin/") || path.contains("/login");
    }
    host == "signin.ollama.com"
        || (host.ends_with(".workos.com") && path.starts_with("/user_management/authorize"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_manual_cookie_source_fails_closed_with_upstream_message() {
        // Upstream #3891 `OllamaUsageError.manualCookieHeaderEmpty`. The shell
        // sets `manual_cookie_missing`; no browser import may be attempted.
        let ctx = FetchContext {
            manual_cookie_missing: true,
            ..FetchContext::default()
        };
        let Err(error) = resolve_cookie_source(&ctx) else {
            panic!("empty Manual source must not resolve a cookie");
        };
        assert_eq!(
            error.to_string(),
            "Ollama cookie source is Manual, but no cookie header is configured. Paste a Cookie header from https://ollama.com/settings, or set Cookie source to Auto."
        );
    }

    #[test]
    fn blank_manual_header_with_missing_flag_fails_closed() {
        let ctx = FetchContext {
            manual_cookie_header: Some("  ".to_string()),
            manual_cookie_missing: true,
            ..FetchContext::default()
        };
        assert!(matches!(
            resolve_cookie_source(&ctx),
            Err(ProviderError::Other(message)) if message.contains("no cookie header is configured")
        ));
    }

    #[test]
    fn non_empty_header_without_session_cookie_keeps_existing_error() {
        let ctx = FetchContext {
            manual_cookie_header: Some("theme=dark".to_string()),
            manual_cookie_missing: false,
            ..FetchContext::default()
        };
        assert!(matches!(
            resolve_cookie_source(&ctx),
            Err(ProviderError::NoCookies)
        ));
    }

    #[test]
    fn normalizes_raw_ollama_session_cookie_value() {
        assert_eq!(
            normalize_cookie_header("abc123"),
            Some("__Secure-session=abc123".to_string())
        );
    }

    #[test]
    fn preserves_full_cookie_header() {
        assert_eq!(
            normalize_cookie_header("__Secure-session=abc123; aid=device"),
            Some("__Secure-session=abc123; aid=device".to_string())
        );
    }

    #[test]
    fn strips_cookie_header_prefix() {
        assert_eq!(
            normalize_cookie_header("Cookie: __Secure-session=abc123"),
            Some("__Secure-session=abc123".to_string())
        );
    }

    #[test]
    fn strips_mid_string_cookie_label_and_curl_syntax() {
        // Upstream 0.50.1 #2949: a copied `Cookie:` label after another
        // cookie, and cURL `-H`/`-b` wrappers with quotes.
        assert_eq!(
            normalize_cookie_header("aid=device; Cookie: __Secure-session=abc123"),
            Some("aid=device; __Secure-session=abc123".to_string())
        );
        assert_eq!(
            normalize_cookie_header("-H 'Cookie: __Secure-session=abc123'"),
            Some("__Secure-session=abc123".to_string())
        );
        assert_eq!(
            normalize_cookie_header("-b \"__Secure-session=abc123\""),
            Some("__Secure-session=abc123".to_string())
        );
    }

    #[test]
    fn first_recognized_cookie_header_skips_browsers_without_a_session_cookie() {
        // Regression for #426: an earlier-priority browser (e.g. Chrome/Edge)
        // may hold only stale/irrelevant ollama.com cookies (analytics,
        // consent) with no session cookie at all. The old
        // `browser_cookies_for_domain` helper stopped at that first non-empty
        // result and never reached a later browser (e.g. Brave) that actually
        // holds the logged-in session.
        let irrelevant_only = vec![Cookie {
            name: "aid".to_string(),
            value: "device-id".to_string(),
            domain: "ollama.com".to_string(),
            path: "/".to_string(),
            expires: None,
            is_secure: true,
            is_http_only: false,
        }];
        let real_session = vec![Cookie {
            name: OLLAMA_SESSION_COOKIE_NAME.to_string(),
            value: "abc123".to_string(),
            domain: "ollama.com".to_string(),
            path: "/".to_string(),
            expires: None,
            is_secure: true,
            is_http_only: true,
        }];

        let url = Url::parse("https://ollama.com/settings").unwrap();
        let header =
            first_recognized_cookie_header(vec![irrelevant_only, real_session].into_iter(), &url);

        assert_eq!(
            header.as_deref(),
            Some("__Secure-session=abc123"),
            "should skip the first (irrelevant) cookie set and use the second (real session)"
        );
    }

    #[test]
    fn first_recognized_cookie_header_none_when_no_set_has_a_session_cookie() {
        let only_irrelevant = vec![Cookie {
            name: "aid".to_string(),
            value: "device-id".to_string(),
            domain: "ollama.com".to_string(),
            path: "/".to_string(),
            expires: None,
            is_secure: true,
            is_http_only: false,
        }];

        let url = Url::parse("https://ollama.com/settings").unwrap();
        let header = first_recognized_cookie_header(vec![only_irrelevant].into_iter(), &url);

        assert_eq!(header, None);
    }

    #[test]
    fn ignores_empty_cookie_input() {
        assert_eq!(normalize_cookie_header("   "), None);
        assert_eq!(normalize_cookie_header("Cookie:   "), None);
    }

    #[test]
    fn recognizes_exact_authkit_and_nextauth_session_cookie_names() {
        assert!(has_recognized_ollama_session_cookie(
            "wos-session=auth; theme=dark"
        ));
        assert!(has_recognized_ollama_session_cookie(
            "__Secure-next-auth.session-token.0=auth"
        ));
        assert!(!has_recognized_ollama_session_cookie(
            "notwos-session=auth; theme=dark"
        ));
        assert!(!has_recognized_ollama_session_cookie(
            "next-auth.session-token.evil=auth"
        ));
        assert!(!has_recognized_ollama_session_cookie("theme=dark"));
    }

    #[test]
    fn limits_browser_cookie_headers_to_ollama_settings_scope() {
        use crate::browser::cookies::Cookie;

        let cookie = |name: &str, domain: &str, path: &str| Cookie {
            name: name.to_string(),
            value: "test".to_string(),
            domain: domain.to_string(),
            path: path.to_string(),
            expires: None,
            is_secure: true,
            is_http_only: true,
        };
        let cookies = [
            cookie("wos-session", ".ollama.com", "/"),
            cookie("wos-session", "signin.ollama.com", "/"),
            cookie("__Secure-session", "ollama.com", "/signin"),
        ];

        assert_eq!(
            ollama_cookie_header_for_url(
                &cookies,
                &Url::parse("https://ollama.com/settings").unwrap()
            )
            .as_deref(),
            Some("wos-session=test")
        );
        assert_eq!(
            ollama_cookie_header_for_url(
                &[cookie("__Secure-session", "ollama.com", "/settings")],
                &Url::parse("https://ollama.com/api/tags").unwrap()
            ),
            None
        );
        assert_eq!(
            ollama_cookie_header_for_url(
                &[cookie("__Secure-session", "ollama.com", "/settings")],
                &Url::parse("https://ollama.com/settings/account").unwrap()
            )
            .as_deref(),
            Some("__Secure-session=test")
        );
        let source = OllamaCookieSource::Browser(vec![
            cookie("__Secure-session", "ollama.com", "/settings"),
            cookie("wos-session", "ollama.com", "/api"),
        ]);
        assert_eq!(
            source
                .header_for_url(&Url::parse("https://ollama.com/settings").unwrap())
                .as_deref(),
            Some("__Secure-session=test")
        );
        assert_eq!(
            source
                .header_for_url(&Url::parse("https://ollama.com/api/models").unwrap())
                .as_deref(),
            Some("wos-session=test")
        );
    }

    #[test]
    fn only_attaches_web_cookie_to_https_ollama_urls() {
        assert!(should_attach_ollama_cookie(
            &Url::parse("https://ollama.com/settings").unwrap()
        ));
        assert!(!should_attach_ollama_cookie(
            &Url::parse("http://ollama.com/settings").unwrap()
        ));
        assert!(!should_attach_ollama_cookie(
            &Url::parse("https://example.com/settings").unwrap()
        ));
    }

    #[test]
    fn recognizes_workos_signin_redirects_as_expired_sessions() {
        assert!(is_ollama_sign_in_redirect(
            &Url::parse("https://signin.ollama.com/?client_id=test").unwrap()
        ));
        assert!(is_ollama_sign_in_redirect(
            &Url::parse("https://auth.workos.com/user_management/authorize?client_id=test")
                .unwrap()
        ));
        assert!(!is_ollama_sign_in_redirect(
            &Url::parse("https://auth.workos.com/other").unwrap()
        ));
        assert!(!is_ollama_sign_in_redirect(
            &Url::parse("http://signin.ollama.com/").unwrap()
        ));
    }

    #[test]
    fn recognized_session_cookie_required_for_cache_reuse() {
        assert!(has_recognized_ollama_session_cookie(
            "__Secure-session=abc123; path=/"
        ));
        assert!(!has_recognized_ollama_session_cookie("foo=bar; baz=qux"));
    }
}
