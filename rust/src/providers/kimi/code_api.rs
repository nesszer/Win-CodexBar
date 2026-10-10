//! Kimi Code API path (api-key + Kimi Code CLI credential), including the
//! upstream 0.48.0 enrichment (#2622): Code API / CLI usage snapshots are
//! merged with the monthly membership pool + Code 7-day limit when a
//! `kimi.com` web token is available (manual cookie → Kimi Desktop session →
//! browser import; gated by Cookie Source Off).

use reqwest::Url;
use std::path::{Path, PathBuf};

use super::{
    FetchContext, KimiCodeApiUsageResponse, KimiProvider, KimiRegion, MONTHLY_WINDOW_ID,
    ProviderError, RateWindow, UsageSnapshot, ascii_header_value, cleaned_env, cleaned_owned,
    kimi_window_minutes,
};
use super::{ratio_pool, web};
use crate::providers::json;

const KIMI_CODE_API_KEY_ENV: &str = "KIMI_CODE_API_KEY";
const KIMI_CODE_BASE_URL_ENV: &str = "KIMI_CODE_BASE_URL";
const KIMI_CODE_HOME_ENV: &str = "KIMI_CODE_HOME";
const KIMI_CODE_OAUTH_HOST_ENV: &str = "KIMI_CODE_OAUTH_HOST";
const KIMI_OAUTH_HOST_ENV: &str = "KIMI_OAUTH_HOST";
const KIMI_CODE_CLI_PLATFORM: &str = "kimi_code_cli";
/// CLI access tokens must remain valid for at least this long to be reused.
const KIMI_CODE_CREDENTIAL_MIN_TTL_SECS: f64 = 60.0;
const SESSION_WINDOW_MINUTES: u32 = 5 * 60;
const WEEKLY_WINDOW_MINUTES: u32 = 7 * 24 * 60;
/// Monthly sentinel shared with the web `Total usage` lane.
const MONTHLY_WINDOW_MINUTES: u32 = 30 * 24 * 60;
/// Placeholder text for a Code API response that reports no weekly quota.
pub(super) const MISSING_WEEKLY_DESCRIPTION: &str = "No weekly quota reported";

#[derive(Debug, serde::Deserialize)]
struct KimiCodeCredentialFile {
    #[serde(default, alias = "accessToken")]
    access_token: String,
    /// Read only to tell whether the CLI is still signed in when its access
    /// token is empty (upstream `hasKimiCodeCredential`). Never used to
    /// refresh: the CLI owns and rotates it.
    #[serde(default)]
    refresh_token: Option<String>,
    #[serde(default, alias = "expiresAt")]
    expires_at: Option<serde_json::Value>,
}

/// Fetch usage via the Kimi Code API; optionally enrich the snapshot with the
/// web membership pool (upstream #2622). Enrichment failures degrade silently
/// to the un-enriched snapshot.
pub(crate) async fn fetch_via_code_api(
    ctx: &FetchContext,
    region: KimiRegion,
    api_key_override: Option<&str>,
    identity_headers_override: Option<&[(&str, String)]>,
    login_method: &str,
) -> Result<UsageSnapshot, ProviderError> {
    let api_key = code_api_key(api_key_override.or(ctx.api_key.as_deref()))?;
    let base_url = code_api_base_url(region)?;
    let endpoint = code_api_usage_endpoint(&base_url)?;
    let client = web::client()?;

    let mut request = client
        .get(endpoint)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Accept", "application/json");
    if let Some(headers) = identity_headers_override {
        for (name, value) in headers {
            request = request.header(*name, value);
        }
    }

    let resp = request.send().await?;

    if !resp.status().is_success() {
        return Err(code_api_status_error(resp.status()));
    }

    let json: KimiCodeApiUsageResponse = resp.json().await.map_err(|e| {
        ProviderError::Parse(format!("Failed to parse Kimi Code API response: {e}"))
    })?;
    let plan_name = json.plan_name();
    let has_plan_name = plan_name.is_some();
    let mut snapshot = snapshot_from_code_api_response(json)?;
    snapshot.login_method = Some(plan_name.unwrap_or_else(|| login_method.to_string()));

    // Upstream #2622: enrich Code API + CLI usage with the monthly membership
    // pool from a signed-in Kimi Desktop (or browser/manual) session.
    for web_token in web::web_auth_tokens(ctx.manual_cookie_header.as_deref(), region) {
        match web::fetch_subscription_for_enrichment_result(&client, &web_token, region).await {
            Ok(subscription) => {
                if let Some(subscription) = subscription {
                    snapshot = super::apply_subscription_windows(snapshot, &subscription);
                }
                if !has_plan_name
                    && let Some(plan) =
                        web::fetch_subscription_plan(&client, &web_token, region).await
                {
                    snapshot.login_method = Some(plan);
                }
                break;
            }
            Err(ProviderError::AuthRequired) => continue,
            Err(error) => {
                tracing::debug!(error = %error, "Kimi Code monthly enrichment unavailable");
                break;
            }
        }
    }

    Ok(snapshot)
}

/// Upstream `KimiUsageFetcher.codeAPIError`: only 401 means the API key or
/// CLI token was rejected. A 403 is a permission or quota denial, which
/// signing in again would not fix.
fn code_api_status_error(status: reqwest::StatusCode) -> ProviderError {
    match status {
        reqwest::StatusCode::UNAUTHORIZED => ProviderError::AuthRequired,
        reqwest::StatusCode::FORBIDDEN => ProviderError::Other(format!(
            "Kimi Code API returned status {status} (permission or quota denied)"
        )),
        _ => ProviderError::Other(format!("Kimi Code API returned status {status}")),
    }
}

/// Upstream `KimiUsageSnapshot.toUsageSnapshot` (0.60.5 #3694): weekly is the
/// primary lane and the 5-hour rate limit the secondary, as on the web path.
/// Each ratio pool takes precedence over the legacy counters of its lane; an
/// absent or invalid pool falls back to those counters. The monthly pool is
/// the `Total usage` extra lane. Missing lanes are not invented: an absent
/// weekly quota stays an informational primary, and a response without any
/// supported window is a parse error.
pub(super) fn snapshot_from_code_api_response(
    response: KimiCodeApiUsageResponse,
) -> Result<UsageSnapshot, ProviderError> {
    let pools = response.usages.as_ref();
    let legacy_limit = response.limits.as_ref().and_then(|limits| limits.first());
    let legacy_rate_limit_minutes =
        legacy_limit.and_then(|limit| limit.window.as_ref().and_then(kimi_window_minutes));
    let weekly = pools
        .and_then(|pools| pools.weekly.as_ref())
        .and_then(|pool| {
            ratio_pool::resolved_ratio_window(
                &response,
                pool,
                response.usage.as_ref(),
                WEEKLY_WINDOW_MINUTES,
                Some(WEEKLY_WINDOW_MINUTES),
            )
        })
        .or_else(|| {
            response.usage.as_ref().and_then(|detail| {
                KimiProvider::rate_window_from_usage_detail(detail, Some(WEEKLY_WINDOW_MINUTES))
                    .ok()
            })
        });
    let rate_limit = pools
        .and_then(|pools| pools.session.as_ref())
        .and_then(|pool| {
            ratio_pool::resolved_ratio_window(
                &response,
                pool,
                legacy_limit.map(|limit| &limit.detail),
                SESSION_WINDOW_MINUTES,
                legacy_rate_limit_minutes,
            )
        })
        .or_else(|| {
            legacy_limit.and_then(|limit| {
                KimiProvider::rate_window_from_usage_detail(
                    &limit.detail,
                    legacy_rate_limit_minutes,
                )
                .ok()
            })
        });
    let monthly = pools
        .and_then(|pools| pools.monthly.as_ref())
        .and_then(|pool| pool.rate_window(MONTHLY_WINDOW_MINUTES));
    if weekly.is_none() && rate_limit.is_none() && monthly.is_none() {
        return Err(ProviderError::Parse(
            "No supported quota windows in Code usage response".into(),
        ));
    }

    let primary = weekly.unwrap_or_else(|| RateWindow::informational(MISSING_WEEKLY_DESCRIPTION));
    let mut usage = UsageSnapshot::new(primary).with_login_method(
        response
            .plan_name()
            .unwrap_or_else(|| "Code API".to_string()),
    );
    if let Some(rate_limit) = rate_limit {
        usage = usage.with_secondary(rate_limit);
    }
    if let Some(monthly) = monthly {
        usage = usage.with_extra_rate_window(MONTHLY_WINDOW_ID, "Total usage", monthly);
    }
    Ok(usage)
}

pub(crate) fn code_api_key(explicit: Option<&str>) -> Result<String, ProviderError> {
    if let Some(key) = explicit.map(str::trim).filter(|key| !key.is_empty()) {
        return Ok(key.to_string());
    }
    cleaned_env(KIMI_CODE_API_KEY_ENV).ok_or(ProviderError::AuthRequired)
}

fn code_api_base_url(region: KimiRegion) -> Result<Url, ProviderError> {
    let raw = cleaned_env(KIMI_CODE_BASE_URL_ENV)
        .unwrap_or_else(|| region.code_api_base_url().to_string());
    crate::providers::validated_https_url(&raw, "Kimi Code API base")
}

pub(super) fn code_api_usage_endpoint(base_url: &Url) -> Result<Url, ProviderError> {
    let base = base_url.as_str().trim_end_matches('/');
    let path = base_url.path().trim_matches('/');
    let endpoint = if path == "coding/v1" || path.ends_with("/coding/v1") {
        format!("{base}/usages")
    } else if path == "coding" || path.ends_with("/coding") {
        format!("{base}/v1/usages")
    } else {
        format!("{base}/coding/v1/usages")
    };
    Url::parse(&endpoint)
        .map_err(|_| ProviderError::Other("Kimi Code API usage endpoint is invalid".into()))
}

/// Whether env base/OAuth overrides mean we must not reuse CLI-owned credentials.
fn has_code_endpoint_override() -> bool {
    cleaned_env(KIMI_CODE_BASE_URL_ENV).is_some()
        || cleaned_env(KIMI_CODE_OAUTH_HOST_ENV).is_some()
        || cleaned_env(KIMI_OAUTH_HOST_ENV).is_some()
}

/// Home for Kimi Code CLI state (`%USERPROFILE%\.kimi-code` or `KIMI_CODE_HOME`).
pub(crate) fn kimi_code_home() -> Option<PathBuf> {
    if let Some(override_home) = cleaned_env(KIMI_CODE_HOME_ENV) {
        return Some(PathBuf::from(override_home));
    }
    dirs::home_dir().map(|home| home.join(".kimi-code"))
}

/// State of the Kimi Code CLI credential file, as seen read-only.
#[derive(PartialEq, Eq)]
pub(crate) enum KimiCliCredential {
    /// No CLI credential is usable or eligible (missing or unreadable file,
    /// no access or refresh token, non-default region, or an endpoint
    /// override).
    Unavailable,
    /// The CLI is signed in, but its access token is missing, expired, or
    /// inside the safety margin.
    Stale,
    Fresh(String),
}

impl std::fmt::Debug for KimiCliCredential {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable => formatter.write_str("Unavailable"),
            Self::Stale => formatter.write_str("Stale"),
            Self::Fresh(_) => formatter.write_str("Fresh([REDACTED])"),
        }
    }
}

/// Guidance shown when a stale or rejected CLI credential leaves no working
/// source. Never includes token values.
const KIMI_CLI_CREDENTIAL_GUIDANCE: &str = "Kimi Code CLI credential is invalid or expired. Run kimi to renew it, or add a Kimi Code API key in Settings > Providers > Kimi (KIMI_CODE_API_KEY). CodexBar does not refresh CLI-owned credentials.";

pub(crate) fn kimi_cli_credential_error() -> ProviderError {
    ProviderError::Other(KIMI_CLI_CREDENTIAL_GUIDANCE.into())
}

/// Read-only access to the Kimi Code CLI access token.
///
/// Never refreshes or rewrites CLI-owned `credentials/kimi-code.json`.
/// Skips when `KIMI_CODE_BASE_URL` / OAuth host overrides are set.
pub(crate) fn kimi_code_cli_credential(region: KimiRegion, now_unix: f64) -> KimiCliCredential {
    if region != KimiRegion::China || has_code_endpoint_override() {
        return KimiCliCredential::Unavailable;
    }
    let Some(credential) = kimi_code_home().and_then(|home| read_kimi_code_credential(&home))
    else {
        return KimiCliCredential::Unavailable;
    };
    let has_refresh_token = credential.refresh_token.and_then(cleaned_owned).is_some();
    match cleaned_owned(credential.access_token) {
        Some(token) if is_kimi_code_credential_fresh(credential.expires_at, now_unix) => {
            KimiCliCredential::Fresh(token)
        }
        Some(_) => KimiCliCredential::Stale,
        // Upstream `hasKimiCodeCredential`: a refresh token alone still means
        // the CLI is signed in, so this is stale, not absent.
        None if has_refresh_token => KimiCliCredential::Stale,
        None => KimiCliCredential::Unavailable,
    }
}

pub(crate) fn kimi_code_cli_identity_headers(home: &Path) -> Vec<(&'static str, String)> {
    // Only send device id when the CLI file exists — never mint a fresh UUID
    // per fetch (unstable fingerprinting toward Moonshot).
    let device_id = read_kimi_code_device_id(home);
    let version = env!("CARGO_PKG_VERSION").to_string();
    let os_name = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    let model = format!("{os_name} {arch}");
    let mut headers = vec![
        ("User-Agent", format!("CodexBar/{version}")),
        ("X-Msh-Platform", KIMI_CODE_CLI_PLATFORM.to_string()),
        ("X-Msh-Version", version),
        ("X-Msh-Device-Name", "codexbar".to_string()),
        ("X-Msh-Device-Model", ascii_header_value(&model)),
        ("X-Msh-Os-Version", ascii_header_value(os_name)),
    ];
    if let Some(device_id) = device_id {
        headers.push(("X-Msh-Device-Id", device_id));
    }
    headers
}

fn read_kimi_code_credential(home: &Path) -> Option<KimiCodeCredentialFile> {
    let path = home.join("credentials").join("kimi-code.json");
    let data = std::fs::read(path).ok()?;
    serde_json::from_slice(&data).ok()
}

fn read_kimi_code_device_id(home: &Path) -> Option<String> {
    let path = home.join("device_id");
    let raw = std::fs::read_to_string(path).ok()?;
    cleaned_owned(raw)
}

fn is_kimi_code_credential_fresh(expires_at: Option<serde_json::Value>, now_unix: f64) -> bool {
    let Some(expires) = json::lenient_f64(expires_at.as_ref()) else {
        return false;
    };
    if !expires.is_finite() {
        return false;
    }
    // Support both seconds and millisecond epoch values.
    let expires_secs = if expires > 10_000_000_000.0 {
        expires / 1000.0
    } else {
        expires
    };
    expires_secs > now_unix + KIMI_CODE_CREDENTIAL_MIN_TTL_SECS
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::LazyLock;

    static ENV_LOCK: LazyLock<std::sync::Mutex<()>> = LazyLock::new(|| std::sync::Mutex::new(()));

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Writes `credentials/kimi-code.json` in the official CLI's shape.
    fn write_kimi_code_credential(
        home: &Path,
        access_token: &str,
        refresh_token: &str,
        expires_at: Option<serde_json::Value>,
    ) -> PathBuf {
        let credentials = home.join("credentials");
        std::fs::create_dir_all(&credentials).expect("mkdir credentials");
        let mut payload = serde_json::Map::new();
        payload.insert("access_token".into(), json!(access_token));
        payload.insert("refresh_token".into(), json!(refresh_token));
        payload.insert("expires_in".into(), json!(900));
        payload.insert("scope".into(), json!("synthetic-scope"));
        payload.insert("token_type".into(), json!("Bearer"));
        if let Some(expires) = expires_at {
            payload.insert("expires_at".into(), expires);
        }
        let path = credentials.join("kimi-code.json");
        std::fs::write(
            &path,
            serde_json::to_vec_pretty(&serde_json::Value::Object(payload)).unwrap(),
        )
        .expect("write credentials");
        path
    }

    fn write_temp_kimi_code_home(
        access_token: &str,
        expires_at: Option<serde_json::Value>,
    ) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        write_kimi_code_credential(dir.path(), access_token, "refresh", expires_at);
        dir
    }

    /// Points the CLI credential reader at `home` with no endpoint overrides.
    /// Taking the guard proves the caller holds `env_lock()`.
    fn use_kimi_code_home(_env: &std::sync::MutexGuard<'static, ()>, home: &Path) {
        // SAFETY: the caller holds env_lock(), so no other test thread reads
        // or writes the process environment concurrently.
        unsafe {
            std::env::remove_var(KIMI_CODE_BASE_URL_ENV);
            std::env::remove_var(KIMI_CODE_OAUTH_HOST_ENV);
            std::env::remove_var(KIMI_OAUTH_HOST_ENV);
            std::env::set_var(KIMI_CODE_HOME_ENV, home);
        }
    }

    fn clear_kimi_code_home(_env: &std::sync::MutexGuard<'static, ()>) {
        // SAFETY: the caller holds env_lock() (see `use_kimi_code_home`).
        unsafe {
            std::env::remove_var(KIMI_CODE_HOME_ENV);
        }
    }

    #[test]
    fn code_api_status_errors_follow_upstream_mapping() {
        use reqwest::StatusCode;
        assert!(matches!(
            code_api_status_error(StatusCode::UNAUTHORIZED),
            ProviderError::AuthRequired
        ));
        for (status, message) in [
            (
                StatusCode::FORBIDDEN,
                "Kimi Code API returned status 403 Forbidden (permission or quota denied)",
            ),
            (
                StatusCode::BAD_REQUEST,
                "Kimi Code API returned status 400 Bad Request",
            ),
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Kimi Code API returned status 500 Internal Server Error",
            ),
        ] {
            assert!(matches!(
                code_api_status_error(status),
                ProviderError::Other(actual) if actual == message
            ));
        }
    }

    #[test]
    fn refresh_only_cli_credential_is_stale_not_absent() {
        let env = env_lock();
        let now = 1_800_000_000.0_f64;
        let home = tempfile::tempdir().expect("tempdir");
        use_kimi_code_home(&env, home.path());

        for access_token in ["", "   "] {
            write_kimi_code_credential(
                home.path(),
                access_token,
                "synthetic-rotating-refresh",
                Some(json!(now + 3600.0)),
            );
            assert_eq!(
                kimi_code_cli_credential(KimiRegion::China, now),
                KimiCliCredential::Stale
            );
        }

        write_kimi_code_credential(home.path(), "", " ", Some(json!(now + 3600.0)));
        assert_eq!(
            kimi_code_cli_credential(KimiRegion::China, now),
            KimiCliCredential::Unavailable
        );
        std::fs::write(
            home.path().join("credentials").join("kimi-code.json"),
            b"{}",
        )
        .expect("write empty credential");
        assert_eq!(
            kimi_code_cli_credential(KimiRegion::China, now),
            KimiCliCredential::Unavailable
        );

        clear_kimi_code_home(&env);
    }

    // Upstream `KimiCLICredentialLifecycleTests`: the next fetch recovers once
    // the CLI replaces its rotating credential; CodexBar only rereads it.
    #[test]
    fn next_read_recovers_after_the_cli_replaces_its_credential() {
        let env = env_lock();
        let now = 1_800_000_000.0_f64;
        let home = tempfile::tempdir().expect("tempdir");
        use_kimi_code_home(&env, home.path());

        write_kimi_code_credential(home.path(), "old-access", "refresh", Some(json!(1)));
        assert_eq!(
            kimi_code_cli_credential(KimiRegion::China, now),
            KimiCliCredential::Stale
        );

        let path = write_kimi_code_credential(
            home.path(),
            "cli-ok",
            "rotated-refresh",
            Some(json!(now + 900.0)),
        );
        let renewed = std::fs::read(&path).unwrap();
        assert_eq!(
            kimi_code_cli_credential(KimiRegion::China, now),
            KimiCliCredential::Fresh("cli-ok".into())
        );
        assert_eq!(std::fs::read(&path).unwrap(), renewed);

        clear_kimi_code_home(&env);
    }

    #[test]
    fn code_api_usage_endpoint_normalizes_base_paths() {
        let root = Url::parse("https://api.kimi.com").unwrap();
        assert_eq!(
            code_api_usage_endpoint(&root).unwrap().as_str(),
            "https://api.kimi.com/coding/v1/usages"
        );
        let coding = Url::parse("https://proxy.example/kimi/coding").unwrap();
        assert_eq!(
            code_api_usage_endpoint(&coding).unwrap().as_str(),
            "https://proxy.example/kimi/coding/v1/usages"
        );
        let versioned = Url::parse("https://proxy.example/kimi/coding/v1").unwrap();
        assert_eq!(
            code_api_usage_endpoint(&versioned).unwrap().as_str(),
            "https://proxy.example/kimi/coding/v1/usages"
        );
    }

    #[test]
    fn reuses_fresh_cli_credential_without_rewriting_file() {
        let _guard = env_lock();
        let now = 1_800_000_000.0_f64;
        let home = write_temp_kimi_code_home("oauth-token", Some(json!(now + 3600.0)));
        let cred_path = home.path().join("credentials").join("kimi-code.json");
        let original = std::fs::read(&cred_path).unwrap();
        let original_modified = std::fs::metadata(&cred_path).unwrap().modified().unwrap();

        // SAFETY: guarded by env_lock for process-wide env mutation in tests.
        unsafe {
            std::env::remove_var(KIMI_CODE_BASE_URL_ENV);
            std::env::remove_var(KIMI_CODE_OAUTH_HOST_ENV);
            std::env::remove_var(KIMI_OAUTH_HOST_ENV);
            std::env::set_var(KIMI_CODE_HOME_ENV, home.path());
        }

        assert_eq!(
            kimi_code_cli_credential(KimiRegion::China, now),
            KimiCliCredential::Fresh("oauth-token".into())
        );

        let after = std::fs::read(&cred_path).unwrap();
        let after_modified = std::fs::metadata(&cred_path).unwrap().modified().unwrap();
        assert_eq!(after, original);
        assert_eq!(after_modified, original_modified);

        let headers = kimi_code_cli_identity_headers(home.path());
        assert!(
            headers
                .iter()
                .any(|(k, v)| *k == "X-Msh-Platform" && v == KIMI_CODE_CLI_PLATFORM)
        );
        // No device id is minted or written when the CLI has none.
        assert!(!headers.iter().any(|(k, _)| *k == "X-Msh-Device-Id"));
        assert!(!home.path().join("device_id").exists());

        // SAFETY: this test owns KIMI_CODE_HOME_ENV (set at its start under
        // env_lock); removing it here restores the shared environment.
        unsafe {
            std::env::remove_var(KIMI_CODE_HOME_ENV);
        }
    }

    #[test]
    fn stale_cli_credential_is_reported_without_touching_the_file() {
        let _guard = env_lock();
        // A 15-minute token: the 60 s safety margin makes it stale at 14 min.
        let issued = 1_800_000_000.0_f64;
        let home = write_temp_kimi_code_home("synthetic-stale", Some(json!(issued + 900.0)));
        let cred_path = home.path().join("credentials").join("kimi-code.json");
        let original = std::fs::read(&cred_path).unwrap();

        // SAFETY: guarded by env_lock for process-wide env mutation in tests.
        unsafe {
            std::env::remove_var(KIMI_CODE_BASE_URL_ENV);
            std::env::remove_var(KIMI_CODE_OAUTH_HOST_ENV);
            std::env::remove_var(KIMI_OAUTH_HOST_ENV);
            std::env::set_var(KIMI_CODE_HOME_ENV, home.path());
        }
        assert_eq!(
            kimi_code_cli_credential(KimiRegion::China, issued + 839.0),
            KimiCliCredential::Fresh("synthetic-stale".into())
        );
        for seconds in [840.0, 900.0] {
            assert_eq!(
                kimi_code_cli_credential(KimiRegion::China, issued + seconds),
                KimiCliCredential::Stale
            );
        }
        assert_eq!(std::fs::read(&cred_path).unwrap(), original);
        assert!(!home.path().join("device_id").exists());

        // SAFETY: final cleanup while the env_lock() guard is still alive.
        unsafe {
            std::env::remove_var(KIMI_CODE_HOME_ENV);
        }
    }

    #[test]
    fn missing_cli_credential_is_unavailable_not_stale() {
        let _guard = env_lock();
        let home = tempfile::tempdir().expect("tempdir");
        // SAFETY: guarded by env_lock for process-wide env mutation in tests.
        unsafe {
            std::env::remove_var(KIMI_CODE_BASE_URL_ENV);
            std::env::remove_var(KIMI_CODE_OAUTH_HOST_ENV);
            std::env::remove_var(KIMI_OAUTH_HOST_ENV);
            std::env::set_var(KIMI_CODE_HOME_ENV, home.path());
        }
        assert_eq!(
            kimi_code_cli_credential(KimiRegion::China, 1_800_000_000.0),
            KimiCliCredential::Unavailable
        );
        // SAFETY: final cleanup while the env_lock() guard is still alive.
        unsafe {
            std::env::remove_var(KIMI_CODE_HOME_ENV);
        }
    }

    #[test]
    fn cli_credential_guidance_explains_renewal_and_api_key_setup() {
        assert_eq!(
            kimi_cli_credential_error().to_string(),
            "Kimi Code CLI credential is invalid or expired. Run kimi to renew it, or add a \
             Kimi Code API key in Settings > Providers > Kimi (KIMI_CODE_API_KEY). CodexBar \
             does not refresh CLI-owned credentials."
        );
    }

    #[test]
    fn rejects_expired_or_missing_expiry_cli_credentials() {
        let now = 1_800_000_000.0_f64;
        for expires in [Some(json!(now + 30.0)), None, Some(json!("not-a-time"))] {
            let home = write_temp_kimi_code_home("oauth", expires);
            let cred = read_kimi_code_credential(home.path()).expect("credential present");
            assert!(!is_kimi_code_credential_fresh(cred.expires_at, now));
        }

        let home = write_temp_kimi_code_home("oauth", Some(json!(now + 120.0)));
        let cred = read_kimi_code_credential(home.path()).unwrap();
        assert!(is_kimi_code_credential_fresh(cred.expires_at, now));
    }

    #[test]
    fn skips_cli_credential_when_endpoint_overrides_present() {
        let _guard = env_lock();
        let now = 1_800_000_000.0_f64;
        let home = write_temp_kimi_code_home("oauth-token", Some(json!(now + 3600.0)));

        // SAFETY: env_lock() guard held for the whole test, so these
        // set_var calls cannot race another thread's environment access.
        unsafe {
            std::env::set_var(KIMI_CODE_HOME_ENV, home.path());
            std::env::set_var(KIMI_CODE_BASE_URL_ENV, "https://proxy.example.com/kimi");
        }
        assert!(has_code_endpoint_override());
        assert_eq!(
            kimi_code_cli_credential(KimiRegion::China, now),
            KimiCliCredential::Unavailable
        );

        // SAFETY: still under the same env_lock() guard; swapping which
        // override keys are present between assertions.
        unsafe {
            std::env::remove_var(KIMI_CODE_BASE_URL_ENV);
            std::env::set_var(KIMI_CODE_OAUTH_HOST_ENV, "https://oauth.example.com");
        }
        assert_eq!(
            kimi_code_cli_credential(KimiRegion::China, now),
            KimiCliCredential::Unavailable
        );

        // SAFETY: final cleanup while the env_lock() guard is still alive.
        unsafe {
            std::env::remove_var(KIMI_CODE_OAUTH_HOST_ENV);
            std::env::remove_var(KIMI_CODE_HOME_ENV);
        }
    }

    #[test]
    fn credential_freshness_accepts_millisecond_expiry() {
        let now = 1_800_000_000.0_f64;
        assert!(is_kimi_code_credential_fresh(
            Some(json!((now + 3600.0) * 1000.0)),
            now
        ));
    }

    #[test]
    fn credential_freshness_requires_sixty_second_margin() {
        assert!((KIMI_CODE_CREDENTIAL_MIN_TTL_SECS - 60.0).abs() < f64::EPSILON);
    }

    #[test]
    fn ratio_pools_preserve_unknown_weekly_and_explicit_monthly_zero() {
        let response: KimiCodeApiUsageResponse = serde_json::from_value(json!({
            "usages": {
                "limit_5h": { "used_ratio": 0.25 },
                "limit_7d": null,
                "limit_month_total": { "used_ratio": 0 }
            },
            "limits": [{
                "window": { "duration": 7, "timeUnit": "TIME_UNIT_DAY" },
                "detail": { "limit": "100", "used": "99" }
            }]
        }))
        .expect("ratio-pool fixture parses");
        let snapshot = snapshot_from_code_api_response(response).expect("ratio pools are usable");
        assert!(
            snapshot.primary.is_informational,
            "missing weekly pool stays unknown"
        );
        assert_eq!(
            snapshot.primary.reset_description.as_deref(),
            Some(MISSING_WEEKLY_DESCRIPTION)
        );
        let rate_limit = snapshot
            .secondary
            .expect("session pool is the rate-limit lane");
        assert_eq!(rate_limit.window_minutes, Some(300));
        assert_eq!(rate_limit.used_percent, 25.0);
        assert!(snapshot.tertiary.is_none());
        let [monthly] = snapshot.extra_rate_windows.as_slice() else {
            panic!("explicit monthly zero is the Total usage lane");
        };
        assert_eq!(monthly.id, MONTHLY_WINDOW_ID);
        assert_eq!(monthly.window.window_minutes, Some(43_200));
        assert_eq!(monthly.window.used_percent, 0.0);
        assert!(monthly.window.usage_known);
    }

    #[test]
    fn invalid_ratio_pools_without_counters_fail_to_parse() {
        for fixture in [
            json!({ "usages": { "limit_5h": { "used_ratio": -0.1 } } }),
            json!({
                "usages": {
                    "limit_7d": { "used_ratio": "abc" },
                    "limit_month_total": { "used_ratio": null }
                }
            }),
        ] {
            let response: KimiCodeApiUsageResponse =
                serde_json::from_value(fixture).expect("fixture parses");
            assert!(matches!(
                snapshot_from_code_api_response(response),
                Err(ProviderError::Parse(message))
                    if message == "No supported quota windows in Code usage response"
            ));
        }
    }

    // Upstream 0.60.5 #3694 replaces the earlier rule that an unusable
    // session pool fails the whole response: each lane falls back to its own
    // legacy counters, and lanes without any source stay absent.
    #[test]
    fn invalid_session_pool_keeps_the_legacy_weekly_counters() {
        let response: KimiCodeApiUsageResponse = serde_json::from_value(json!({
            "usages": { "limit_5h": { "used_ratio": -0.1 } },
            "usage": { "limit": "100", "used": "20" }
        }))
        .expect("fixture parses");

        let snapshot = snapshot_from_code_api_response(response).expect("weekly counters");
        assert!(!snapshot.primary.is_informational);
        assert_eq!(snapshot.primary.used_percent, 20.0);
        assert_eq!(snapshot.primary.window_minutes, Some(10_080));
        assert_eq!(
            snapshot.primary.reset_description.as_deref(),
            Some("20/100 credits")
        );
        assert!(snapshot.secondary.is_none());
        assert!(snapshot.extra_rate_windows.is_empty());
    }
}
