//! ClinePass usage provider (upstream 0.44 #2219, session reuse 0.68.0).
//!
//! `GET https://api.cline.bot/api/v1/users/me/plan/usage-limits`
//!
//! Credentials: explicit or stored API key, then `CLINE_API_KEY` /
//! `CLINEPASS_API_KEY`, then a read-only reuse of the `cline auth` session file
//! (see [`session`]).

mod session;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::Deserialize;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderStateKind,
    RateWindow, SourceMode, UsageSnapshot,
};
use session::FileCredential;

const USAGE_URL: &str = "https://api.cline.bot/api/v1/users/me/plan/usage-limits";
const CREDENTIAL_TARGET: &str = "codexbar-clinepass";
const ENV_KEYS: &[&str] = &["CLINE_API_KEY", "CLINEPASS_API_KEY"];
const API_KEY_LOGIN_METHOD: &str = "API key";
const MISSING_CREDENTIALS: &str =
    "ClinePass credentials not found. Add an API key or run cline auth to sign in.";
const REJECTED_CREDENTIALS: &str = "ClinePass credentials were rejected. Check your API key or run `cline auth` to refresh your browser session.";

/// A bearer token and how it was obtained, for the snapshot's login method.
struct Credential {
    token: String,
    login_method: &'static str,
}

/// An explicit, stored or environment API key wins; only when none exists is
/// the Cline session file consulted (lazily, so it is never read otherwise).
fn resolve_credential(
    api_key: Result<String, ProviderError>,
    file_credential: impl FnOnce() -> Option<FileCredential>,
) -> Result<Credential, ProviderError> {
    match api_key {
        Ok(token) => Ok(Credential {
            token,
            login_method: API_KEY_LOGIN_METHOD,
        }),
        Err(ProviderError::NotInstalled(_)) => file_credential()
            .map(|file| Credential {
                login_method: file.login_method(),
                token: file.token,
            })
            .ok_or_else(|| ProviderError::Other(MISSING_CREDENTIALS.into())),
        Err(other) => Err(other),
    }
}

#[derive(Debug, Deserialize)]
struct LimitsResponse {
    success: bool,
    data: LimitsData,
}

#[derive(Debug, Deserialize)]
struct LimitsData {
    limits: Vec<LimitEntry>,
}

#[derive(Debug, Deserialize)]
struct LimitEntry {
    #[serde(rename = "type")]
    limit_type: String,
    #[serde(rename = "percentUsed")]
    percent_used: f64,
    #[serde(rename = "resetsAt")]
    resets_at: Option<String>,
}

pub struct ClinePassProvider {
    client: Client,
}

impl ClinePassProvider {
    pub fn new() -> Self {
        Self {
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }
}

impl Default for ClinePassProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for ClinePassProvider {
    fn id(&self) -> ProviderId {
        ProviderId::ClinePass
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => {
                let credential = resolve_credential(
                    crate::providers::resolve_api_key(
                        ctx.api_key.as_deref(),
                        CREDENTIAL_TARGET,
                        ENV_KEYS,
                    ),
                    session::read_file_credential,
                )?;
                let resp = self
                    .client
                    .get(USAGE_URL)
                    .bearer_auth(&credential.token)
                    .header("Accept", "application/json")
                    .send()
                    .await?;
                let status = resp.status();
                if status == reqwest::StatusCode::UNAUTHORIZED
                    || status == reqwest::StatusCode::FORBIDDEN
                {
                    return Err(ProviderError::Other(REJECTED_CREDENTIALS.into()));
                }
                if !status.is_success() {
                    return Err(ProviderError::Other(format!(
                        "ClinePass API error: HTTP {status}"
                    )));
                }
                let body: LimitsResponse = resp.json().await.map_err(|e| {
                    ProviderError::Parse(format!("Failed to parse ClinePass usage: {e}"))
                })?;
                let snap = snapshot_from_limits(&body, credential.login_method)?;
                Ok(ProviderFetchResult::new(snap, "api"))
            }
            SourceMode::Web | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }

    /// Missing or rejected credentials are sign-in gates, not unknown failures.
    fn error_state_kind(&self, error: &ProviderError) -> ProviderStateKind {
        match error {
            ProviderError::Other(message)
                if message == MISSING_CREDENTIALS || message == REJECTED_CREDENTIALS =>
            {
                ProviderStateKind::NeedsAuthentication
            }
            _ => error.state_kind(),
        }
    }
}

fn parse_iso(raw: Option<&str>) -> Option<DateTime<Utc>> {
    let raw = raw?.trim();
    if raw.is_empty() {
        return None;
    }
    DateTime::parse_from_rfc3339(raw)
        .ok()
        .map(|d| d.with_timezone(&Utc))
}

fn window_for(entry: &LimitEntry) -> Option<(RateWindow, &'static str)> {
    let minutes = match entry.limit_type.as_str() {
        "five_hour" => Some(5 * 60),
        "weekly" => Some(7 * 24 * 60),
        "monthly" => Some(30 * 24 * 60),
        _ => None,
    }?;
    let mut w = RateWindow::new(entry.percent_used.clamp(0.0, 100.0));
    w.window_minutes = Some(minutes);
    w.resets_at = parse_iso(entry.resets_at.as_deref());
    let slot = match entry.limit_type.as_str() {
        "five_hour" => "primary",
        "weekly" => "secondary",
        "monthly" => "tertiary",
        _ => return None,
    };
    Some((w, slot))
}

fn snapshot_from_limits(
    body: &LimitsResponse,
    login_method: &str,
) -> Result<UsageSnapshot, ProviderError> {
    if !body.success {
        return Err(ProviderError::Parse(
            "ClinePass response success was false".into(),
        ));
    }
    let mut primary = None;
    let mut secondary = None;
    let mut tertiary = None;
    for limit in &body.data.limits {
        if let Some((w, slot)) = window_for(limit) {
            match slot {
                "primary" => primary = Some(w),
                "secondary" => secondary = Some(w),
                "tertiary" => tertiary = Some(w),
                _ => {}
            }
        }
    }
    let primary = primary.ok_or_else(|| {
        ProviderError::Parse("ClinePass response missing five_hour window".into())
    })?;
    let mut snap = UsageSnapshot::new(primary).with_login_method(login_method);
    if let Some(s) = secondary {
        snap = snap.with_secondary(s);
    }
    if let Some(t) = tertiary {
        snap = snap.with_tertiary(t);
    }
    Ok(snap)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignores_unknown_limit_types() {
        let body: LimitsResponse = serde_json::from_str(
            r#"{
          "success": true,
          "data": {
            "limits": [
              { "type": "five_hour", "percentUsed": 12.5, "resetsAt": "2026-07-16T15:00:00Z" },
              { "type": "experimental_pool", "percentUsed": 77, "resetsAt": "2026-07-16T15:00:00Z" },
              { "type": "weekly", "percentUsed": 25, "resetsAt": "2026-07-20T00:00:00Z" },
              { "type": "monthly", "percentUsed": 40, "resetsAt": null }
            ]
          }
        }"#,
        )
        .unwrap();
        let snap = snapshot_from_limits(&body, "API key").unwrap();
        assert!((snap.primary.used_percent - 12.5).abs() < 0.01);
        assert_eq!(snap.primary.window_minutes, Some(300));
        assert!((snap.secondary.as_ref().unwrap().used_percent - 25.0).abs() < 0.01);
        assert!((snap.tertiary.as_ref().unwrap().used_percent - 40.0).abs() < 0.01);
    }

    fn file_credential(is_oauth: bool) -> FileCredential {
        FileCredential {
            token: if is_oauth {
                "workos:session"
            } else {
                "file-key"
            }
            .into(),
            is_oauth,
        }
    }

    fn missing() -> ProviderError {
        ProviderError::NotInstalled("API key not found".into())
    }

    #[test]
    fn api_key_wins_and_never_reads_the_session_file() {
        let credential = resolve_credential(Ok("explicit".into()), || {
            panic!("session file must not be read when an API key exists")
        })
        .unwrap();
        assert_eq!(credential.token, "explicit");
        assert_eq!(credential.login_method, "API key");
    }

    #[test]
    fn session_file_is_the_fallback_with_matching_login_method() {
        let browser = resolve_credential(Err(missing()), || Some(file_credential(true))).unwrap();
        assert_eq!(browser.token, "workos:session");
        assert_eq!(browser.login_method, "Browser");
        let key = resolve_credential(Err(missing()), || Some(file_credential(false))).unwrap();
        assert_eq!(key.token, "file-key");
        assert_eq!(key.login_method, "API key");
    }

    #[test]
    fn missing_everything_reports_the_friendly_message() {
        let error = resolve_credential(Err(missing()), || None)
            .err()
            .expect("no credential");
        assert_eq!(error.to_string(), MISSING_CREDENTIALS);
        assert_eq!(
            ClinePassProvider::new().error_state_kind(&error),
            ProviderStateKind::NeedsAuthentication
        );
    }

    #[test]
    fn env_keys_prefer_the_primary_name() {
        assert_eq!(ENV_KEYS, ["CLINE_API_KEY", "CLINEPASS_API_KEY"]);
    }

    #[test]
    fn rejected_credentials_are_a_sign_in_gate() {
        let provider = ClinePassProvider::new();
        let error = ProviderError::Other(REJECTED_CREDENTIALS.into());
        assert_eq!(
            provider.error_state_kind(&error),
            ProviderStateKind::NeedsAuthentication
        );
        assert_eq!(
            provider.error_state_kind(&ProviderError::Other("boom".into())),
            ProviderStateKind::Unknown
        );
    }

    #[test]
    fn usage_snapshot_reports_the_credential_login_method() {
        let body: LimitsResponse = serde_json::from_str(
            r#"{"success":true,"data":{"limits":[{"type":"five_hour","percentUsed":10},{"type":"weekly","percentUsed":25}]}}"#,
        )
        .unwrap();
        let snap = snapshot_from_limits(&body, "Browser").unwrap();
        assert_eq!(snap.login_method.as_deref(), Some("Browser"));
        assert!((snap.secondary.unwrap().used_percent - 25.0).abs() < 0.01);
    }
}
