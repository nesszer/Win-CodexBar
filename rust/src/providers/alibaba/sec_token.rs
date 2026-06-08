use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::{OnceLock, RwLock};
use std::time::{Duration, Instant};

/// How long a scraped SEC_TOKEN stays valid in the in-memory cache.
const SEC_TOKEN_TTL: Duration = Duration::from_secs(25 * 60);

#[derive(Clone)]
struct CachedSecToken {
    token: String,
    fetched_at: Instant,
}

fn token_cache() -> &'static RwLock<HashMap<String, CachedSecToken>> {
    static CACHE: OnceLock<RwLock<HashMap<String, CachedSecToken>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Cache key bound to the real auth boundary: the region code plus a full
/// SHA-256 hash of the cookie header.
pub(crate) fn sec_token_cache_key(region_code: &str, cookies: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(cookies.as_bytes());
    let digest = hasher.finalize();
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    format!("{region_code}:{hex}")
}

pub(crate) fn cached_sec_token(key: &str) -> Option<String> {
    let cache = token_cache().read().ok()?;
    let entry = cache.get(key)?;
    (entry.fetched_at.elapsed() < SEC_TOKEN_TTL).then(|| entry.token.clone())
}

pub(crate) fn store_sec_token(key: &str, token: &str) {
    if let Ok(mut cache) = token_cache().write() {
        cache.retain(|_, v| v.fetched_at.elapsed() < SEC_TOKEN_TTL);
        cache.insert(
            key.to_string(),
            CachedSecToken {
                token: token.to_string(),
                fetched_at: Instant::now(),
            },
        );
    }
}

pub(crate) fn invalidate_sec_token(key: &str) {
    if let Ok(mut cache) = token_cache().write() {
        cache.remove(key);
    }
}

/// Scan the dashboard HTML for a `SEC_TOKEN` assignment.
pub(crate) fn extract_sec_token(html: &str) -> Option<String> {
    const NEEDLE: &str = "SEC_TOKEN";
    let mut search_from = 0;
    while let Some(rel) = html[search_from..].find(NEEDLE) {
        let pos = search_from + rel;
        let after = &html[pos + NEEDLE.len()..];
        if let Some(token) = parse_sec_token_value(after) {
            return Some(token);
        }
        search_from = pos + NEEDLE.len();
    }
    None
}

fn parse_sec_token_value(after_key: &str) -> Option<String> {
    let mut chars = after_key.char_indices().peekable();
    if matches!(chars.peek(), Some(&(_, '"' | '\''))) {
        chars.next();
    }
    while let Some(&(_, c)) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else {
            break;
        }
    }
    let assign = chars.next()?;
    if assign.1 != ':' && assign.1 != '=' {
        return None;
    }
    while let Some(&(_, c)) = chars.peek() {
        if c.is_whitespace() {
            chars.next();
        } else {
            break;
        }
    }
    let (open_idx, open_quote) = chars.next()?;
    if open_quote != '"' && open_quote != '\'' {
        return None;
    }
    let value_start = open_idx + open_quote.len_utf8();
    let value = &after_key[value_start..];
    let end = value.find(open_quote)?;
    let token = &value[..end];
    if token.is_empty() || !is_valid_sec_token(token) {
        return None;
    }
    Some(token.to_string())
}

fn is_valid_sec_token(token: &str) -> bool {
    token.len() >= 8
        && token.len() <= 256
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-' | b'+' | b'/' | b'='))
}

/// Look up a cookie value by name from a `Cookie:` header.
pub(crate) fn extract_cookie_value(name: &str, cookie_header: &str) -> Option<String> {
    cookie_header.split(';').find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        (key.trim() == name)
            .then(|| value.trim().to_string())
            .filter(|v| !v.is_empty())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sec_token_cache_key_distinguishes_region_and_cookies() {
        let a = sec_token_cache_key("ap-southeast-1", "cookie=one");
        let b = sec_token_cache_key("us-east-1", "cookie=one");
        let c = sec_token_cache_key("ap-southeast-1", "cookie=two");
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_eq!(a, sec_token_cache_key("ap-southeast-1", "cookie=one"));
    }

    #[test]
    fn sec_token_cache_key_uses_full_sha256_hex() {
        let key = sec_token_cache_key("ap-southeast-1", "cookie=value");
        let (_region, hex) = key.split_once(':').expect("key must contain ':'");
        assert_eq!(hex.len(), 64, "full SHA-256 hex is 64 characters");
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn extract_cookie_value_is_case_sensitive() {
        assert_eq!(
            extract_cookie_value("sec_token", "sec_token=lower").as_deref(),
            Some("lower"),
        );
        assert_eq!(extract_cookie_value("sec_token", "SEC_TOKEN=upper"), None);
        assert_eq!(
            extract_cookie_value("sec_token", "SEC_TOKEN=upper; sec_token=lower").as_deref(),
            Some("lower"),
        );
    }

    #[test]
    fn extract_sec_token_from_html() {
        let html = r#"var config = { SEC_TOKEN: "AvLZTKds7DW5utd3p5xm48", OTHER: "x" };"#;
        assert_eq!(
            extract_sec_token(html).as_deref(),
            Some("AvLZTKds7DW5utd3p5xm48")
        );
    }

    #[test]
    fn extract_sec_token_handles_json_quoted_key() {
        let html = r#"{"SEC_TOKEN":"AbcDefGhi123","other":1}"#;
        assert_eq!(extract_sec_token(html).as_deref(), Some("AbcDefGhi123"));
    }

    #[test]
    fn extract_sec_token_handles_single_quotes() {
        let html = r#"window.SEC_TOKEN = 'AvLZTKds7DW5utd3p5xm48';"#;
        assert_eq!(
            extract_sec_token(html).as_deref(),
            Some("AvLZTKds7DW5utd3p5xm48"),
        );
    }

    #[test]
    fn extract_sec_token_skips_decoy_occurrences() {
        let html = r#"<!-- SEC_TOKEN missing on "deadbeef" -->
            <script>var config = { SEC_TOKEN: "RealTokenValue1234" };</script>"#;
        assert_eq!(
            extract_sec_token(html).as_deref(),
            Some("RealTokenValue1234"),
        );
    }

    #[test]
    fn extract_sec_token_rejects_invalid_charset() {
        let html = r#"var x = { SEC_TOKEN: "bad token with spaces" };"#;
        assert_eq!(extract_sec_token(html), None);
    }

    #[test]
    fn extract_sec_token_rejects_too_short_value() {
        let html = r#"var x = { SEC_TOKEN: "abc" };"#;
        assert_eq!(extract_sec_token(html), None);
    }
}
