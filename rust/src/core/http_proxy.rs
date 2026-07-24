//! Global HTTP(S) proxy configuration for provider / app network traffic.
//!
//! Issue #235 — opt-in settings proxy (not CLIProxyAPI, not LLM Proxy provider).
//! Password may live in local settings.json (same trust boundary as other local secrets).

use reqwest::{ClientBuilder, Proxy, Url};

/// Snapshot of proxy-related settings used when building HTTP clients.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HttpProxySettings {
    pub enabled: bool,
    pub url: String,
    pub username: String,
    pub password: String,
}

impl HttpProxySettings {
    pub fn from_parts(
        enabled: bool,
        url: impl Into<String>,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        Self {
            enabled,
            url: url.into(),
            username: username.into(),
            password: password.into(),
        }
    }

    /// Load from persisted app settings (no-op if settings fail to load).
    pub fn from_app_settings() -> Self {
        let settings = crate::settings::Settings::load();
        Self {
            enabled: settings.http_proxy_enabled,
            url: settings.http_proxy_url.clone(),
            username: settings.http_proxy_username.clone(),
            password: settings.http_proxy_password.clone(),
        }
    }
}

/// Resolve a reqwest [`Proxy`] from settings.
///
/// - Disabled or empty URL → `Ok(None)` (direct / default).
/// - Invalid URL when enabled → `Err(...)`.
/// - Supports `http` and `https` proxy schemes only (MVP).
pub fn resolve_proxy(settings: &HttpProxySettings) -> Result<Option<Proxy>, String> {
    if !settings.enabled {
        return Ok(None);
    }
    let raw = settings.url.trim();
    if raw.is_empty() {
        return Err("Proxy URL is required when the proxy is enabled.".into());
    }

    let parsed = Url::parse(raw).map_err(|e| format!("Invalid proxy URL: {e}"))?;
    let scheme = parsed.scheme().to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        return Err(format!(
            "Unsupported proxy scheme '{scheme}'. Use http:// or https://."
        ));
    }
    if parsed.host_str().is_none() {
        return Err("Proxy URL must include a host.".into());
    }

    let mut proxy = Proxy::all(raw).map_err(|e| format!("Invalid proxy URL: {e}"))?;

    // Prefer explicit username/password fields. If the URL already embeds
    // credentials, do not call basic_auth again (would double-apply).
    let url_has_user = !parsed.username().is_empty();
    let user = settings.username.trim();
    let pass = settings.password.as_str();
    if !user.is_empty() && !url_has_user {
        proxy = proxy.basic_auth(user, pass);
    }

    Ok(Some(proxy))
}

/// Apply resolved proxy to a client builder.
///
/// On misconfiguration, returns the builder unchanged so callers can still
/// connect direct (caller should log the error).
pub fn apply_proxy_to_builder(
    builder: ClientBuilder,
    settings: &HttpProxySettings,
) -> ClientBuilder {
    match resolve_proxy(settings) {
        Ok(Some(proxy)) => builder.proxy(proxy),
        Ok(None) => builder,
        Err(err) => {
            tracing::warn!(error = %err, "http proxy config ignored; using direct connection");
            builder
        }
    }
}

/// Convenience: apply the current app settings proxy to a builder.
pub fn apply_app_proxy(builder: ClientBuilder) -> ClientBuilder {
    apply_proxy_to_builder(builder, &HttpProxySettings::from_app_settings())
}

/// Validate proxy settings for UI (returns `None` when OK / disabled).
pub fn validation_error(settings: &HttpProxySettings) -> Option<String> {
    if !settings.enabled {
        return None;
    }
    resolve_proxy(settings).err()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_returns_none() {
        let s = HttpProxySettings::from_parts(false, "http://127.0.0.1:7890", "", "");
        assert!(resolve_proxy(&s).unwrap().is_none());
    }

    #[test]
    fn enabled_empty_url_errors() {
        let s = HttpProxySettings::from_parts(true, "  ", "", "");
        assert!(resolve_proxy(&s).unwrap_err().contains("required"));
    }

    #[test]
    fn valid_http_url_ok() {
        let s = HttpProxySettings::from_parts(true, "http://127.0.0.1:7890", "", "");
        assert!(resolve_proxy(&s).unwrap().is_some());
    }

    #[test]
    fn rejects_socks_scheme() {
        let s = HttpProxySettings::from_parts(true, "socks5://127.0.0.1:1080", "", "");
        let err = resolve_proxy(&s).unwrap_err();
        assert!(err.contains("Unsupported"), "{err}");
    }

    #[test]
    fn rejects_garbage_url() {
        let s = HttpProxySettings::from_parts(true, "not a url", "", "");
        assert!(resolve_proxy(&s).is_err());
    }

    #[test]
    fn accepts_userinfo_in_url() {
        let s = HttpProxySettings::from_parts(true, "http://alice:secret@proxy.local:8080", "", "");
        assert!(resolve_proxy(&s).unwrap().is_some());
    }

    #[test]
    fn accepts_explicit_basic_auth_fields() {
        let s = HttpProxySettings::from_parts(true, "http://proxy.local:8080", "alice", "secret");
        assert!(resolve_proxy(&s).unwrap().is_some());
    }

    #[test]
    fn validation_error_none_when_disabled() {
        let s = HttpProxySettings::from_parts(false, "bad", "", "");
        assert!(validation_error(&s).is_none());
    }

    #[test]
    fn validation_error_when_enabled_and_bad() {
        let s = HttpProxySettings::from_parts(true, "ftp://x", "", "");
        assert!(validation_error(&s).is_some());
    }
}
