//! A `kimi.com` web token together with where it came from (upstream `KimiWebCredential`,
//! #4416 / #3536).
//!
//! A token discovered from the current Kimi Desktop session keeps a revalidation callback until
//! each request is dispatched: the Desktop session is read again right before the request is
//! sent, and a missing, replaced, expired or unreadable session rejects it. Tokens from any other
//! source keep their existing behavior.

use std::sync::{Arc, Mutex};

use reqwest::header::{AUTHORIZATION, COOKIE};

use super::KimiRegion;
use crate::core::ProviderError;

pub(super) const DESKTOP_SESSION_CHANGED_MESSAGE: &str = "Kimi Desktop session changed before the request was sent. \
     Confirm the intended account is signed in to Kimi Desktop, then refresh CodexBar.";

#[derive(Clone)]
pub(super) struct KimiWebCredential {
    token: String,
    desktop_check: Option<Arc<DesktopDispatchValidation>>,
}

impl KimiWebCredential {
    pub(super) fn plain(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
            desktop_check: None,
        }
    }

    /// A Desktop session token; `is_current` must re-read the session and report whether it
    /// still holds exactly this token.
    pub(super) fn desktop(
        token: impl Into<String>,
        is_current: impl Fn() -> bool + Send + Sync + 'static,
    ) -> Self {
        Self {
            token: token.into(),
            desktop_check: Some(Arc::new(DesktopDispatchValidation {
                valid: Mutex::new(true),
                is_current: Box::new(is_current),
            })),
        }
    }

    pub(super) fn token(&self) -> &str {
        &self.token
    }

    /// Gate `request` before it is handed to the HTTP client. Only Desktop credentials are
    /// checked: the request must be the China web API request carrying this token, and the
    /// Desktop session must still hold it. This narrows the window between discovery and
    /// dispatch; it cannot recall a request that was already sent.
    pub(super) fn check_dispatch(
        &self,
        request: &reqwest::Request,
        region: KimiRegion,
    ) -> Result<(), ProviderError> {
        let Some(validation) = &self.desktop_check else {
            return Ok(());
        };
        if region == KimiRegion::China
            && self.is_bound_request(request, region)
            && validation.check()
        {
            Ok(())
        } else {
            Err(ProviderError::Other(DESKTOP_SESSION_CHANGED_MESSAGE.into()))
        }
    }

    fn is_bound_request(&self, request: &reqwest::Request, region: KimiRegion) -> bool {
        let url = request.url();
        let expected_host = region
            .web_base_url()
            .strip_prefix("https://")
            .unwrap_or_default();
        let header = |name| {
            request
                .headers()
                .get(name)
                .and_then(|value| value.to_str().ok())
        };
        url.scheme() == "https"
            && url.host_str() == Some(expected_host)
            && url.port().is_none_or(|port| port == 443)
            && header(AUTHORIZATION) == Some(format!("Bearer {}", self.token).as_str())
            && header(COOKIE) == Some(format!("kimi-auth={}", self.token).as_str())
    }
}

impl PartialEq for KimiWebCredential {
    fn eq(&self, other: &Self) -> bool {
        self.token == other.token && self.desktop_check.is_some() == other.desktop_check.is_some()
    }
}

impl Eq for KimiWebCredential {}

impl std::fmt::Debug for KimiWebCredential {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("KimiWebCredential")
            .field("token", &"[REDACTED]")
            .field("desktop", &self.desktop_check.is_some())
            .finish()
    }
}

/// Shared by every clone of one acquired credential, so concurrent requests share invalidation:
/// once a check fails, restoring the old session value cannot revive this credential.
struct DesktopDispatchValidation {
    valid: Mutex<bool>,
    is_current: Box<dyn Fn() -> bool + Send + Sync>,
}

impl DesktopDispatchValidation {
    fn check(&self) -> bool {
        let mut valid = self
            .valid
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if *valid {
            *valid = (self.is_current)();
        }
        *valid
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    fn request(client: &reqwest::Client, url: &str, token: &str) -> reqwest::Request {
        super::super::kimi_web_request(client, url, KimiRegion::China, token, serde_json::json!({}))
            .unwrap()
    }

    fn china_url() -> String {
        KimiRegion::China.web_api_url("svc")
    }

    #[test]
    fn plain_credentials_are_never_gated() {
        let client = reqwest::Client::new();
        let credential = KimiWebCredential::plain("browser-token");
        let other = request(&client, "https://example.com/x", "something-else");
        assert!(
            credential
                .check_dispatch(&other, KimiRegion::International)
                .is_ok()
        );
    }

    #[test]
    fn current_desktop_session_is_rechecked_before_every_request() {
        let client = reqwest::Client::new();
        let checks = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&checks);
        let credential = KimiWebCredential::desktop("desktop-token", move || {
            counter.fetch_add(1, Ordering::SeqCst);
            true
        });
        let request = request(&client, &china_url(), "desktop-token");
        for _ in 0..3 {
            credential
                .check_dispatch(&request, KimiRegion::China)
                .unwrap();
        }
        assert_eq!(checks.load(Ordering::SeqCst), 3);
    }

    #[test]
    fn changed_session_rejects_the_request_and_stays_rejected() {
        let client = reqwest::Client::new();
        let current = Arc::new(AtomicBool::new(true));
        let session = Arc::clone(&current);
        let credential =
            KimiWebCredential::desktop("desktop-token", move || session.load(Ordering::SeqCst));
        let concurrent = credential.clone();
        let request = request(&client, &china_url(), "desktop-token");

        assert!(
            credential
                .check_dispatch(&request, KimiRegion::China)
                .is_ok()
        );
        current.store(false, Ordering::SeqCst);
        let error = credential
            .check_dispatch(&request, KimiRegion::China)
            .unwrap_err();
        assert_eq!(error.to_string(), DESKTOP_SESSION_CHANGED_MESSAGE);
        assert!(!error.to_string().contains("desktop-token"));

        // Restoring the old value cannot revive this credential or any clone of it.
        current.store(true, Ordering::SeqCst);
        assert!(
            credential
                .check_dispatch(&request, KimiRegion::China)
                .is_err()
        );
        assert!(
            concurrent
                .check_dispatch(&request, KimiRegion::China)
                .is_err()
        );
    }

    #[test]
    fn desktop_token_is_bound_to_the_china_web_api_request() {
        let client = reqwest::Client::new();
        let credential = KimiWebCredential::desktop("desktop-token", || true);
        let cases = [
            (
                request(&client, &china_url(), "other-token"),
                KimiRegion::China,
            ),
            (
                request(&client, "https://evil.example/apiv2/svc", "desktop-token"),
                KimiRegion::China,
            ),
            (
                request(&client, "http://www.kimi.com/apiv2/svc", "desktop-token"),
                KimiRegion::China,
            ),
            (
                request(
                    &client,
                    "https://www.kimi.com:8443/apiv2/svc",
                    "desktop-token",
                ),
                KimiRegion::China,
            ),
            (
                request(&client, &china_url(), "desktop-token"),
                KimiRegion::International,
            ),
        ];
        for (request, region) in cases {
            assert!(credential.check_dispatch(&request, region).is_err());
        }
        // A rejected binding is a session error too; it does not consult or poison the session.
        let bound = request(&client, &china_url(), "desktop-token");
        assert!(credential.check_dispatch(&bound, KimiRegion::China).is_ok());
    }

    #[test]
    fn debug_output_redacts_the_token() {
        let rendered = format!("{:?}", KimiWebCredential::desktop("secret-token", || true));
        assert!(!rendered.contains("secret-token"));
    }
}
