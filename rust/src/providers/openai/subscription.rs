//! Authenticated OpenAI subscription metadata parsing and page-capture support.
//!
//! The dashboard is the authority for these dates. The parser accepts only
//! explicit, typed values from the subscription response and never derives a
//! subscription boundary from a quota reset, plan name, or current time.

use chrono::{DateTime, Utc};
use serde_json::Value;

use crate::core::SubscriptionMetadata;

/// Result of a subscription request. `Success(None)` is distinct from an
/// unavailable/invalid response so callers may clear dates only after a
/// valid, authoritative response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OpenAISubscriptionFetchResult {
    Unavailable,
    Success(Option<SubscriptionMetadata>),
}

impl OpenAISubscriptionFetchResult {
    pub fn metadata(&self) -> Option<&SubscriptionMetadata> {
        match self {
            Self::Success(metadata) => metadata.as_ref(),
            Self::Unavailable => None,
        }
    }

    pub fn cloned_metadata(&self) -> Option<SubscriptionMetadata> {
        self.metadata().cloned()
    }

    pub const fn succeeded(&self) -> bool {
        matches!(self, Self::Success(_))
    }
}

/// Page-side capture hook for WebView2/Tauri dashboard sessions.
///
/// It is intentionally limited to same-origin `/backend-api/subscriptions`
/// responses. Generation and request guards prevent a late response from a
/// previous navigation or an older concurrent request from being published.
pub const OPENAI_SUBSCRIPTION_CAPTURE_SCRIPT: &str = r#"
(() => {
  if (window.__codexbarSubscriptionCaptureInstalled) return;
  window.__codexbarSubscriptionCaptureInstalled = true;
  window.__codexbarSubscriptionCaptureGeneration = 0;
  window.__codexbarSubscriptionResponse = null;
  let latestRequest = 0;
  const originalFetch = window.fetch.bind(window);
  window.fetch = async (...args) => {
    const generation = Number(window.__codexbarSubscriptionCaptureGeneration || 0);
    let request = null;
    try {
      const input = args[0];
      const rawUrl = input && input.url ? input.url : input;
      const url = new URL(String(rawUrl), window.location.href);
      if (url.origin === window.location.origin &&
          url.pathname === '/backend-api/subscriptions') {
        request = ++latestRequest;
      }
    } catch (_) {}
    const response = await originalFetch(...args);
    if (request !== null) {
      try {
        const payload = await response.clone().json();
        if (generation !== Number(window.__codexbarSubscriptionCaptureGeneration || 0) ||
            request !== latestRequest) return response;
        window.__codexbarSubscriptionResponse = {
          status: response.status,
          payload: payload
        };
      } catch (_) {
        if (generation === Number(window.__codexbarSubscriptionCaptureGeneration || 0) &&
            request === latestRequest) {
          window.__codexbarSubscriptionResponse = {
            status: response.status,
            payload: null
          };
        }
      }
    }
    return response;
  };
})();
"#;

/// Bump the capture generation before a dashboard navigation or account
/// change. A caller should evaluate this in the same WebView2 page context.
pub const OPENAI_SUBSCRIPTION_RESET_SCRIPT: &str = r#"
(() => {
  window.__codexbarSubscriptionCaptureGeneration =
    Number(window.__codexbarSubscriptionCaptureGeneration || 0) + 1;
  window.__codexbarSubscriptionResponse = null;
  return window.__codexbarSubscriptionCaptureGeneration;
})();
"#;

/// Read the latest same-origin response captured by
/// [`OPENAI_SUBSCRIPTION_CAPTURE_SCRIPT`].
pub const OPENAI_SUBSCRIPTION_READ_SCRIPT: &str = r#"
(() => window.__codexbarSubscriptionResponse || null)();
"#;

/// Compare a managed-account hint with the identity observed from the same
/// authenticated credentials. A supplied hint without a matching observed
/// email is ambiguous and cannot authorize subscription metadata.
pub fn account_identity_matches(
    expected_email: Option<&str>,
    observed_email: Option<&str>,
) -> bool {
    let expected = expected_email
        .map(str::trim)
        .filter(|email| !email.is_empty())
        .map(str::to_ascii_lowercase);
    let observed = observed_email
        .map(str::trim)
        .filter(|email| !email.is_empty())
        .map(str::to_ascii_lowercase);
    match (expected.as_deref(), observed.as_deref()) {
        (Some(expected), Some(observed)) => expected == observed,
        (Some(_), None) => false,
        (None, _) => true,
    }
}

pub fn parse_subscription_json(json: &str) -> OpenAISubscriptionFetchResult {
    let Ok(value) = serde_json::from_str::<Value>(json) else {
        return OpenAISubscriptionFetchResult::Unavailable;
    };
    parse_subscription_value(&value)
}

/// Parse a response captured by a dashboard page or fetched directly from the
/// authenticated API. Non-success responses are never interpreted as an
/// empty subscription.
pub fn parse_subscription_http_response(status: u16, body: &str) -> OpenAISubscriptionFetchResult {
    if !(200..300).contains(&status) {
        return OpenAISubscriptionFetchResult::Unavailable;
    }
    parse_subscription_json(body)
}

/// Parse a successful JSON response. Both lifecycle fields must be present and
/// correctly typed; otherwise the result is unavailable and the caller must
/// retain any prior dates.
pub fn parse_subscription_value(value: &Value) -> OpenAISubscriptionFetchResult {
    let Some(object) = value.as_object() else {
        return OpenAISubscriptionFetchResult::Unavailable;
    };

    let active_until = object
        .get("active_until")
        .or_else(|| object.get("activeUntil"));
    let will_renew = object.get("will_renew").or_else(|| object.get("willRenew"));
    let (Some(active_until), Some(will_renew)) = (active_until, will_renew) else {
        return OpenAISubscriptionFetchResult::Unavailable;
    };

    let active_until = match active_until {
        Value::Null => None,
        Value::String(value) => Some(value.as_str()),
        _ => return OpenAISubscriptionFetchResult::Unavailable,
    };
    let will_renew = match will_renew {
        Value::Null => None,
        Value::Bool(value) => Some(*value),
        _ => return OpenAISubscriptionFetchResult::Unavailable,
    };

    let starts_at = parse_optional_date(
        object,
        &["starts_at", "startsAt", "active_from", "activeFrom"],
    );
    if has_any(
        object,
        &["starts_at", "startsAt", "active_from", "activeFrom"],
    ) && starts_at.is_err()
    {
        return OpenAISubscriptionFetchResult::Unavailable;
    }
    let starts_at = starts_at.ok().flatten();

    match (active_until, will_renew) {
        (Some(active_until), Some(true)) => {
            let Some(renews_at) = parse_date(active_until) else {
                return OpenAISubscriptionFetchResult::Unavailable;
            };
            OpenAISubscriptionFetchResult::Success(Some(SubscriptionMetadata::new(
                starts_at,
                None,
                Some(renews_at),
            )))
        }
        (Some(active_until), Some(false)) => {
            let Some(expires_at) = parse_date(active_until) else {
                return OpenAISubscriptionFetchResult::Unavailable;
            };
            OpenAISubscriptionFetchResult::Success(Some(SubscriptionMetadata::new(
                starts_at,
                Some(expires_at),
                None,
            )))
        }
        (None, Some(false)) => OpenAISubscriptionFetchResult::Success(
            (!starts_at.is_none()).then(|| SubscriptionMetadata::new(starts_at, None, None)),
        ),
        // A renewal without an explicit active-until date is not safe to
        // represent as a date, even if a plan is known.
        (None, Some(true)) | (_, None) => OpenAISubscriptionFetchResult::Unavailable,
    }
}

fn has_any(object: &serde_json::Map<String, Value>, keys: &[&str]) -> bool {
    keys.iter().any(|key| object.contains_key(*key))
}

fn parse_optional_date(
    object: &serde_json::Map<String, Value>,
    keys: &[&str],
) -> Result<Option<DateTime<Utc>>, ()> {
    let Some(value) = keys.iter().find_map(|key| object.get(*key)) else {
        return Ok(None);
    };
    match value {
        Value::Null => Ok(None),
        Value::String(value) => parse_date(value).ok_or(()).map(Some),
        _ => Err(()),
    }
}

fn parse_date(value: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_renewal_and_preserves_explicit_start() {
        let result = parse_subscription_json(
            r#"{"active_until":"2026-09-20T14:30:07.123Z","will_renew":true,"active_from":"2026-08-20T14:30:07Z"}"#,
        );
        let OpenAISubscriptionFetchResult::Success(Some(metadata)) = result else {
            panic!("expected subscription metadata")
        };
        assert_eq!(
            metadata.starts_at.unwrap().to_rfc3339(),
            "2026-08-20T14:30:07+00:00"
        );
        assert_eq!(metadata.expires_at, None);
        assert_eq!(
            metadata
                .renews_at
                .unwrap()
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            "2026-09-20T14:30:07.123Z"
        );
    }

    #[test]
    fn cancellation_maps_active_until_to_expiration() {
        let result = parse_subscription_json(
            r#"{"active_until":"2026-09-20T14:30:07Z","will_renew":false}"#,
        );
        let metadata = result.metadata().expect("metadata");
        assert!(metadata.renews_at.is_none());
        assert_eq!(
            metadata.expires_at.unwrap().to_rfc3339(),
            "2026-09-20T14:30:07+00:00"
        );
    }

    #[test]
    fn rejects_missing_or_malformed_dates_and_flags_without_fallback() {
        for json in [
            r#"{"active_until":null,"will_renew":true}"#,
            r#"{"active_until":"not-a-date","will_renew":false}"#,
            r#"{"active_until":null,"will_renew":0}"#,
            r#"{"active_until":"2026-09-20T14:30:07Z"}"#,
        ] {
            assert_eq!(
                parse_subscription_json(json),
                OpenAISubscriptionFetchResult::Unavailable
            );
        }
    }

    #[test]
    fn valid_empty_cancellation_is_success_without_inventing_dates() {
        assert_eq!(
            parse_subscription_json(r#"{"active_until":null,"will_renew":false}"#),
            OpenAISubscriptionFetchResult::Success(None)
        );
    }

    #[test]
    fn non_success_dashboard_response_cannot_clear_dates() {
        assert_eq!(
            parse_subscription_http_response(403, r#"{"active_until":null,"will_renew":false}"#),
            OpenAISubscriptionFetchResult::Unavailable
        );
    }

    #[test]
    fn managed_identity_mismatch_fails_closed() {
        assert!(account_identity_matches(
            Some("A@EXAMPLE.COM"),
            Some("a@example.com")
        ));
        assert!(!account_identity_matches(
            Some("old@example.com"),
            Some("new@example.com")
        ));
        assert!(!account_identity_matches(
            Some("expected@example.com"),
            None
        ));
        assert!(account_identity_matches(None, Some("observed@example.com")));
    }
}
