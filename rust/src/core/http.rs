//! HTTP helpers shared by provider fetchers.

use reqwest::Url;

use super::http_proxy::apply_app_proxy;

/// Build a client for requests that may carry cookies, OAuth tokens, API keys,
/// or other provider credentials.
///
/// Credentialed provider requests should not automatically follow redirects to
/// a different origin. Reqwest strips some sensitive headers during redirects,
/// but an explicit same-origin policy keeps the invariant local and testable.
///
/// When Settings → Advanced HTTP proxy is enabled, the global proxy is applied
/// here so provider refreshes pick it up on the next `instantiate_provider`.
pub fn credentialed_http_client_builder() -> reqwest::ClientBuilder {
    let builder =
        reqwest::Client::builder().redirect(reqwest::redirect::Policy::custom(|attempt| {
            let previous = attempt.previous();
            let Some(last_url) = previous.last() else {
                return attempt.follow();
            };

            if is_same_origin(last_url, attempt.url()) {
                attempt.follow()
            } else {
                attempt.stop()
            }
        }));
    apply_app_proxy(builder)
}

pub(crate) fn is_same_origin(from: &Url, to: &Url) -> bool {
    from.scheme() == to.scheme()
        && from.host_str() == to.host_str()
        && from.port_or_known_default() == to.port_or_known_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(input: &str) -> Url {
        Url::parse(input).unwrap()
    }

    #[test]
    fn same_origin_redirect_allows_path_changes() {
        assert!(is_same_origin(
            &url("https://example.com/a"),
            &url("https://example.com/b?x=1"),
        ));
    }

    #[test]
    fn same_origin_redirect_rejects_host_changes() {
        assert!(!is_same_origin(
            &url("https://example.com/a"),
            &url("https://evil.example/b"),
        ));
    }

    #[test]
    fn same_origin_redirect_rejects_scheme_changes() {
        assert!(!is_same_origin(
            &url("https://example.com/a"),
            &url("http://example.com/b"),
        ));
    }
}
