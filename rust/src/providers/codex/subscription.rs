use std::time::Duration;

use crate::core::UsageSnapshot;
use crate::providers::openai::{OpenAISubscriptionFetchResult, parse_subscription_http_response};

use super::CodexApi;

const SUBSCRIPTION_PATH: &str = "/subscriptions";

pub(super) async fn enrich_subscription_metadata(
    api: &CodexApi,
    base_url: &str,
    access_token: &str,
    account_id: Option<&str>,
    usage: UsageSnapshot,
) -> UsageSnapshot {
    if !crate::settings::Settings::load().codex_openai_web_extras() {
        return usage;
    }
    match api
        .fetch_subscription_metadata(base_url, access_token, account_id)
        .await
    {
        OpenAISubscriptionFetchResult::Success(metadata) => usage.with_subscription(metadata),
        OpenAISubscriptionFetchResult::Unavailable => usage,
    }
}

pub(super) async fn fetch_subscription_metadata(
    api: &CodexApi,
    base_url: &str,
    access_token: &str,
    account_id: Option<&str>,
) -> OpenAISubscriptionFetchResult {
    let Some(host) = reqwest::Url::parse(base_url)
        .ok()
        .and_then(|url| url.host_str().map(str::to_ascii_lowercase))
    else {
        return OpenAISubscriptionFetchResult::Unavailable;
    };
    if !matches!(host.as_str(), "chatgpt.com" | "chat.openai.com") {
        return OpenAISubscriptionFetchResult::Unavailable;
    }

    let mut request = api
        .client
        .get(format!(
            "{}{}",
            base_url.trim_end_matches('/'),
            SUBSCRIPTION_PATH
        ))
        .header("Authorization", format!("Bearer {access_token}"))
        .header("User-Agent", "CodexBar")
        .header("Accept", "application/json")
        .header("Cache-Control", "no-cache, no-store, max-age=0")
        .header("Pragma", "no-cache")
        .timeout(Duration::from_secs(8));
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
