//! Fetch-context routing for providers that pick their own credential lane
//! (Command Code, upstream #4462): Auto and the API source keep their usage
//! source, a configured key rides along, and `off` never hands over a cookie.

use std::collections::HashMap;

use codexbar::core::{FetchContext, ProviderId, SourceMode};
use codexbar::settings::{ApiKeys, ManualCookies, Settings};

const STORED_COOKIE: &str = "__Secure-commandcode_prod_.session_token=stored";

fn command_code_context(
    cookie_source: &str,
    usage_source: &str,
    cookie: Option<&str>,
    api_key: Option<&str>,
) -> FetchContext {
    let id = ProviderId::CommandCode;
    let mut settings = Settings::default();
    settings.set_cookie_source(id, cookie_source);
    settings.set_usage_source(id, usage_source);
    let mut cookies = ManualCookies::default();
    if let Some(cookie) = cookie {
        cookies.set(id.cli_name(), cookie);
    }
    let mut api_keys = ApiKeys::default();
    if let Some(key) = api_key {
        api_keys.set(id.cli_name(), key, None);
    }
    super::build_fetch_context(id, &settings, &cookies, &api_keys, &HashMap::new())
}

#[test]
fn command_code_auto_with_a_key_and_no_cookie_stays_on_auto() {
    let ctx = command_code_context("auto", "auto", None, Some("fixture-key"));
    assert_eq!(ctx.source_mode, SourceMode::Auto);
    assert!(ctx.manual_cookie_header.is_none());
    assert!(!ctx.manual_cookie_missing);
    assert_eq!(ctx.api_key.as_deref(), Some("fixture-key"));
}

#[test]
fn command_code_api_source_keeps_the_key_whatever_the_cookie_source() {
    for cookie_source in ["auto", "manual", "off"] {
        let ctx = command_code_context(
            cookie_source,
            "oauth",
            Some(STORED_COOKIE),
            Some("fixture-key"),
        );
        assert_eq!(ctx.source_mode, SourceMode::OAuth, "{cookie_source}");
        assert_eq!(ctx.api_key.as_deref(), Some("fixture-key"));
    }
}

#[test]
fn command_code_off_drops_the_stored_cookie_and_keeps_the_key() {
    let ctx = command_code_context("off", "auto", Some(STORED_COOKIE), Some("fixture-key"));
    assert_eq!(ctx.source_mode, SourceMode::Auto);
    assert!(ctx.manual_cookie_header.is_none());
    assert!(ctx.manual_cookie_missing);
    assert_eq!(ctx.api_key.as_deref(), Some("fixture-key"));
}

#[test]
fn command_code_auto_with_a_stored_cookie_passes_it_to_the_provider() {
    let ctx = command_code_context("manual", "auto", Some(STORED_COOKIE), None);
    assert_eq!(ctx.source_mode, SourceMode::Auto);
    assert_eq!(ctx.manual_cookie_header.as_deref(), Some(STORED_COOKIE));
    assert!(!ctx.manual_cookie_missing);
}

#[test]
fn command_code_web_source_still_routes_to_web_with_the_stored_cookie() {
    let ctx = command_code_context("manual", "web", Some(STORED_COOKIE), Some("fixture-key"));
    assert_eq!(ctx.source_mode, SourceMode::Web);
    assert_eq!(ctx.manual_cookie_header.as_deref(), Some(STORED_COOKIE));
}
