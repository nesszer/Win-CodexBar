//! Provider fetch errors and their transport classification.

use thiserror::Error;

use super::SourceMode;
use crate::core::LastGoodOwner;

/// Errors that can occur when fetching provider data
#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("Provider not installed: {0}")]
    NotInstalled(String),

    #[error("Authentication required")]
    AuthRequired,

    #[error("OAuth error: {0}")]
    OAuth(String),

    #[error("Transient OAuth error: {0}")]
    OAuthTransient(String),

    #[error("OAuth session expired: {0}")]
    OAuthExpired(String),

    #[error("OAuth token revoked: {0}")]
    OAuthRevoked(String),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),

    #[error("Timeout")]
    Timeout,

    #[error("Source mode '{0:?}' not supported for this provider")]
    UnsupportedSource(SourceMode),

    #[error("No cookies available for web API")]
    NoCookies,

    /// Usage needs a provider web session that only a browser sign-in can
    /// restore. `sign_in_url` is the page to open; CLI JSON error rows carry
    /// it as `signInUrl` next to `errorKind: "browserSignInRequired"`.
    #[error("{message}")]
    BrowserSignInRequired {
        message: String,
        sign_in_url: String,
    },

    #[error("{0}")]
    Other(String),

    /// A transport failure tagged with the session that produced it, so the
    /// shell can retain a cached snapshot only for the same session. Build it
    /// with [`ProviderError::with_failure_owner`].
    #[error("{source}")]
    OwnedTransport {
        owner: Option<LastGoodOwner>,
        source: Box<ProviderError>,
    },
}

impl ProviderError {
    /// Return true only for transport failures safe for last-good retention.
    pub fn is_transport_failure(&self) -> bool {
        match self {
            ProviderError::Network(error) => matches!(
                classify_reqwest_error(error),
                ReqwestFailureClass::Timeout | ReqwestFailureClass::Connect
            ),
            ProviderError::Timeout => true,
            ProviderError::OwnedTransport { source, .. } => source.is_transport_failure(),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReqwestFailureClass {
    Timeout,
    Connect,
    Terminal,
}

fn classify_reqwest_error(error: &reqwest::Error) -> ReqwestFailureClass {
    // A response-body failure can also carry the timeout flag when the peer
    // stalls while the body is being read. It is terminal for the snapshot,
    // because retaining last-good data would hide a truncated response.
    if error.is_body() || error.is_decode() {
        return ReqwestFailureClass::Terminal;
    }
    if error.is_timeout() {
        return ReqwestFailureClass::Timeout;
    }
    if !error.is_connect() {
        return ReqwestFailureClass::Terminal;
    }

    // A connect classification alone is too broad: it also covers protocol
    // and TLS-handshake failures. Retain only a typed transient socket error.
    if has_io_error_kind(error, std::io::ErrorKind::ConnectionRefused) {
        return ReqwestFailureClass::Connect;
    }

    ReqwestFailureClass::Terminal
}

fn has_io_error_kind(error: &reqwest::Error, kind: std::io::ErrorKind) -> bool {
    fn contains_kind(
        source: Option<&(dyn std::error::Error + 'static)>,
        kind: std::io::ErrorKind,
    ) -> bool {
        let Some(current) = source else {
            return false;
        };
        if let Some(io_error) = current.downcast_ref::<std::io::Error>() {
            if io_error.kind() == kind {
                return true;
            }
            let mut nested = io_error.get_ref();
            while let Some(inner) = nested {
                let Some(inner_io) = inner.downcast_ref::<std::io::Error>() else {
                    break;
                };
                if inner_io.kind() == kind {
                    return true;
                }
                nested = inner_io.get_ref();
            }
        }
        contains_kind(std::error::Error::source(current), kind)
    }

    contains_kind(std::error::Error::source(error), kind)
}

#[cfg(test)]
#[path = "transport_tests.rs"]
mod transport_tests;
