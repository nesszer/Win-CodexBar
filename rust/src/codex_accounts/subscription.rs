use std::path::Path;

use crate::providers::openai::{
    OpenAISubscriptionFetchResult, account_identity_matches, parse_subscription_http_response,
};

use super::{
    AuthCredentials, CodexAccountApi, identity_from_credentials, normalize_string,
    resolve_usage_url,
};
use crate::codex_accounts::models::AccountUsageSnapshot;

const SUBSCRIPTION_PATH: &str = "/subscriptions";

pub(super) async fn enrich_subscription_metadata(
    api: &CodexAccountApi,
    codex_home_path: &Path,
    credentials: &AuthCredentials,
    email_hint: Option<&str>,
    workspace_account_id: Option<&str>,
    snapshot: AccountUsageSnapshot,
) -> AccountUsageSnapshot {
    if !crate::settings::Settings::load().codex_openai_web_extras() {
        return snapshot;
    }
    let identity = identity_from_credentials(credentials);
    if !account_identity_matches(email_hint, identity.email.as_deref()) {
        // A managed account hint and the current credential identity
        // disagree; retaining quota while dropping optional dates avoids
        // attaching a dashboard answer to the wrong lane.
        return snapshot;
    }
    let account_id = workspace_account_id
        .and_then(|id| normalize_string(Some(id)))
        .or(identity.provider_account_id);
    match api
        .fetch_subscription_metadata(codex_home_path, credentials, account_id.as_deref())
        .await
    {
        OpenAISubscriptionFetchResult::Success(metadata) => AccountUsageSnapshot {
            subscription: metadata,
            ..snapshot
        },
        OpenAISubscriptionFetchResult::Unavailable => snapshot,
    }
}

pub(super) async fn fetch_subscription_metadata(
    api: &CodexAccountApi,
    codex_home_path: &Path,
    credentials: &AuthCredentials,
    account_id: Option<&str>,
) -> OpenAISubscriptionFetchResult {
    let Some(url) = resolve_subscription_url(codex_home_path) else {
        return OpenAISubscriptionFetchResult::Unavailable;
    };
    let mut request = api
        .client
        .get(url)
        .header(
            "Authorization",
            format!("Bearer {}", credentials.access_token),
        )
        .header("User-Agent", "codex-cli")
        .header("Accept", "application/json")
        .header("Cache-Control", "no-cache, no-store, max-age=0")
        .header("Pragma", "no-cache")
        .timeout(std::time::Duration::from_secs(8));
    if let Some(account_id) = account_id.filter(|id| !id.is_empty()) {
        request = request.header("ChatGPT-Account-Id", account_id);
    }
    let Ok(response) = request.send().await else {
        return OpenAISubscriptionFetchResult::Unavailable;
    };
    let status = response.status().as_u16();
    let Ok(body) = response.bytes().await else {
        return OpenAISubscriptionFetchResult::Unavailable;
    };
    let Ok(body) = std::str::from_utf8(&body) else {
        return OpenAISubscriptionFetchResult::Unavailable;
    };
    parse_subscription_http_response(status, body)
}

pub(super) fn resolve_subscription_url(codex_home_path: &Path) -> Option<String> {
    let usage_url = resolve_usage_url(codex_home_path);
    let mut url = reqwest::Url::parse(&usage_url).ok()?;
    if !matches!(
        url.host_str()
            .map(|host| host.to_ascii_lowercase())
            .as_deref(),
        Some("chatgpt.com") | Some("chat.openai.com")
    ) || !url.path().ends_with("/wham/usage")
    {
        return None;
    }
    let base_path = url.path().trim_end_matches("/wham/usage");
    url.set_path(&format!("{base_path}{SUBSCRIPTION_PATH}"));
    Some(url.to_string())
}

#[cfg(test)]
mod tests {
    use super::resolve_subscription_url;

    #[test]
    fn resolve_subscription_url_default() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            resolve_subscription_url(dir.path()).as_deref(),
            Some("https://chatgpt.com/backend-api/subscriptions")
        );
    }

    #[test]
    fn custom_backend_never_becomes_subscription_authority() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("config.toml"),
            "chatgpt_base_url = \"https://gateway.example/backend-api\"\n",
        )
        .unwrap();
        assert_eq!(resolve_subscription_url(dir.path()), None);
    }
}
