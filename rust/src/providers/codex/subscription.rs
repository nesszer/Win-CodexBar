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
    let mut request = api
        .authed_get(
            &format!("{}{}", base_url.trim_end_matches('/'), SUBSCRIPTION_PATH),
            access_token,
            account_id,
        )
        .header("Cache-Control", "no-cache, no-store, max-age=0")
        .header("Pragma", "no-cache")
        .timeout(Duration::from_secs(8));
    // 订阅接口同时要求账号查询参数；由 reqwest 编码，保持与请求头的账号一致。
    if let Some(account_id) = account_id.filter(|id| !id.is_empty()) {
        request = request.query(&[("account_id", account_id)]);
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn subscription_account_query_returns_live_renewal_and_replaces_old_period() {
        // 复现 184 的响应形状：查询参数必填，自动续费日优先于旧登录周期。
        let mut server = mockito::Server::new_async().await;
        let mock = server
            .mock("GET", "/subscriptions")
            .match_query(mockito::Matcher::UrlEncoded(
                "account_id".into(),
                "account-test+id&x=1/?".into(),
            ))
            .match_header("chatgpt-account-id", "account-test+id&x=1/?")
            .match_header("authorization", "Bearer synthetic-token")
            .with_status(200)
            .with_body(r#"{"active_until":"2026-12-01T04:55:35Z","will_renew":true}"#)
            .create_async()
            .await;
        let api = CodexApi {
            client: reqwest::Client::builder().no_proxy().build().unwrap(),
            home_dir: std::path::PathBuf::new(),
            codex_home_override: None,
        };
        let result = fetch_subscription_metadata(
            &api,
            &server.url(),
            "synthetic-token",
            Some("account-test+id&x=1/?"),
        )
        .await;
        let period = CredentialSubscriptionPeriod {
            starts_at: None,
            ends_at: Some("2026-10-01T03:17:52Z".parse().unwrap()),
            recorded_at: None,
        };
        let usage = apply_subscription_result(
            UsageSnapshot::new(crate::core::RateWindow::new(10.0)),
            result,
            Some(&period),
        );
        let subscription = usage.subscription.expect("实时订阅日期");
        assert_eq!(
            subscription.renews_at.unwrap().to_rfc3339(),
            "2026-12-01T04:55:35+00:00"
        );
        assert!(subscription.expires_at.is_none());
        assert!(usage.credential_subscription_period.is_none());
        mock.assert_async().await;
    }
}
