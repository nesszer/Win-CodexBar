use std::time::Duration;

use crate::core::{CredentialSubscriptionPeriod, UsageSnapshot};
use crate::providers::openai::{OpenAISubscriptionFetchResult, parse_subscription_http_response};

use super::CodexApi;

const SUBSCRIPTION_PATH: &str = "/subscriptions";

/// Subscription metadata is optional enrichment. Usage remains usable when
/// the endpoint is unavailable, malformed, unauthorized, or points at a
/// custom backend. A successful empty cancellation response is the only
/// result allowed to clear dates on the fresh snapshot.
pub(super) async fn enrich_subscription_metadata(
    api: &CodexApi,
    base_url: &str,
    access_token: &str,
    account_id: Option<&str>,
    usage: UsageSnapshot,
    credential_period: Option<&CredentialSubscriptionPeriod>,
) -> UsageSnapshot {
    if !crate::settings::Settings::load().codex_openai_web_extras() {
        return usage;
    }
    // 自定义用量后端不能证明这些凭据属于它返回的账号，因此不附加登录周期。
    if !is_subscription_authority(base_url) {
        return usage;
    }
    let result = fetch_subscription_metadata(api, base_url, access_token, account_id).await;
    apply_subscription_result(usage, result, credential_period)
}

pub(super) fn apply_subscription_result(
    mut usage: UsageSnapshot,
    result: OpenAISubscriptionFetchResult,
    credential_period: Option<&CredentialSubscriptionPeriod>,
) -> UsageSnapshot {
    // 成功响应（包括明确的空结果）始终优先；仅不可用时挂载独立的登录周期记录。
    usage.credential_subscription_period = None;
    match result {
        OpenAISubscriptionFetchResult::Success(metadata) => usage.with_subscription(metadata),
        OpenAISubscriptionFetchResult::Unavailable => {
            usage.credential_subscription_period = credential_period.copied();
            usage
        }
    }
}

async fn fetch_subscription_metadata(
    api: &CodexApi,
    base_url: &str,
    access_token: &str,
    account_id: Option<&str>,
) -> OpenAISubscriptionFetchResult {
    let request = api
        .authed_get(
            &format!("{}{}", base_url.trim_end_matches('/'), SUBSCRIPTION_PATH),
            access_token,
            account_id,
        )
        .header("Cache-Control", "no-cache, no-store, max-age=0")
        .header("Pragma", "no-cache")
        .timeout(Duration::from_secs(8));
    let response = match request.send().await {
        Ok(response) => response,
        Err(error) => {
            // 不记录错误原文或响应体，避免泄露凭据和账号；保留可区分的失败类别。
            tracing::debug!(timeout = error.is_timeout(), "Codex 订阅查询不可用");
            return OpenAISubscriptionFetchResult::Unavailable;
        }
    };
    let status = response.status().as_u16();
    if !(200..300).contains(&status) {
        tracing::debug!(http_status = status, "Codex 订阅查询返回非成功状态");
        return OpenAISubscriptionFetchResult::Unavailable;
    }
    let Ok(body) = response.bytes().await else {
        return OpenAISubscriptionFetchResult::Unavailable;
    };
    let Ok(body) = std::str::from_utf8(&body) else {
        return OpenAISubscriptionFetchResult::Unavailable;
    };
    parse_subscription_http_response(status, body)
}

pub(super) fn is_subscription_authority(base_url: &str) -> bool {
    reqwest::Url::parse(base_url).is_ok_and(|url| {
        url.scheme() == "https"
            && url.port_or_known_default() == Some(443)
            && matches!(url.host_str(), Some("chatgpt.com" | "chat.openai.com"))
            && url.username().is_empty()
            && url.password().is_none()
            && url.query().is_none()
            && url.fragment().is_none()
    })
}
