//! Bearer-authenticated MiniMax coding-plan quota fetch.
//!
//! Kept separate from the legacy billing client so the client-rendered console
//! workaround (#425) does not further grow the already-large provider module.

use chrono::Utc;

use crate::core::{FetchContext, ProviderError, ProviderFetchResult};

use super::{JSON_ACCEPT, MiniMaxRegion, check_status, coding_plan, coding_plan_html, http_client};

fn resolve_plain_api_key(explicit: Option<&str>, environment: Option<&str>) -> Option<String> {
    explicit
        .map(str::trim)
        .filter(|key| !key.is_empty())
        .or_else(|| environment.map(str::trim).filter(|key| !key.is_empty()))
        .map(str::to_string)
}

pub(super) fn read_plain_api_key(ctx: &FetchContext) -> Option<String> {
    let environment = std::env::var("MINIMAX_API_KEY").ok();
    resolve_plain_api_key(ctx.api_key.as_deref(), environment.as_deref())
}

pub(super) async fn fetch_remains_via_api_key(
    api_key: &str,
    region: MiniMaxRegion,
) -> Result<ProviderFetchResult, ProviderError> {
    let now = Utc::now();
    let urls = [region.coding_plan_remains_url(), region.www_remains_url()];
    let mut last_err: Option<ProviderError> = None;
    for url in urls {
        match fetch_remains_once_via_api_key(api_key, &url).await {
            Ok(snapshot) => {
                let usage = coding_plan_html::to_usage_snapshot(&snapshot, now)?;
                return Ok(ProviderFetchResult::new(usage, "api"));
            }
            Err(err @ ProviderError::Parse(_)) => last_err = Some(err),
            Err(err) => return Err(err),
        }
    }
    Err(last_err.unwrap_or_else(|| ProviderError::Parse("Missing MiniMax remains URL.".into())))
}

async fn fetch_remains_once_via_api_key(
    api_key: &str,
    url: &str,
) -> Result<coding_plan::MiniMaxCodingPlanSnapshot, ProviderError> {
    let response = http_client()?
        .get(url)
        .header("Authorization", format!("Bearer {api_key}"))
        .header("Accept", JSON_ACCEPT)
        .send()
        .await?;
    check_status(response.status(), "remains (api key)", true)?;

    let json: serde_json::Value = response
        .json()
        .await
        .map_err(|e| ProviderError::Parse(format!("Failed to parse remains JSON: {e}")))?;
    coding_plan::parse_coding_plan_value(&json, Utc::now())
}

#[cfg(test)]
mod tests {
    use super::super::http_tests::{JSON_ACCEPT, assert_status_error};
    use super::{fetch_remains_once_via_api_key, resolve_plain_api_key};

    #[tokio::test]
    async fn api_key_remains_request_maps_statuses() {
        let cases: [(usize, Option<&str>); 4] = [
            (401, None),
            (403, None),
            (
                404,
                Some("Parse:MiniMax remains (api key) returned status 404 Not Found"),
            ),
            (
                503,
                Some("Other:MiniMax remains (api key) returned status 503 Service Unavailable"),
            ),
        ];
        for (status, expected) in cases {
            let mut server = mockito::Server::new_async().await;
            let mock = server
                .mock("GET", "/remains")
                .match_header("authorization", "Bearer fixture-key")
                .match_header("accept", JSON_ACCEPT)
                .with_status(status)
                .expect(1)
                .create_async()
                .await;
            let result =
                fetch_remains_once_via_api_key("fixture-key", &format!("{}/remains", server.url()))
                    .await;
            mock.assert_async().await;
            assert_status_error(result.map(|_| ()), expected, status);
        }
    }

    #[test]
    fn plain_api_key_prefers_explicit_then_environment_without_mutating_process_env() {
        assert_eq!(
            resolve_plain_api_key(Some("  ctx-key  "), Some("env-key")).as_deref(),
            Some("ctx-key")
        );
        assert_eq!(
            resolve_plain_api_key(Some("   "), Some("  env-key  ")).as_deref(),
            Some("env-key")
        );
        assert_eq!(resolve_plain_api_key(Some("   "), Some("   ")), None);
        assert_eq!(resolve_plain_api_key(None, None), None);
    }
}
