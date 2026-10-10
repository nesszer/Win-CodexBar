use async_trait::async_trait;
use reqwest::header::ACCEPT;
use reqwest::{Client, StatusCode, Url};
use serde_json::{Map, Value, json};

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, SourceMode,
    UsageSnapshot,
};

mod parse;
#[cfg(test)]
mod tests;

use parse::{ParsedQuotas, parse, text};

const CREDENTIAL_TARGET: &str = "codexbar-chutes";
const DEFAULT_API_URL: &str = "https://api.chutes.ai";

pub struct ChutesProvider {
    client: Client,
}

impl ChutesProvider {
    pub fn new() -> Self {
        Self {
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }
}

impl Default for ChutesProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for ChutesProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Chutes
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => {
                let key = crate::providers::resolve_api_key(
                    ctx.api_key.as_deref(),
                    CREDENTIAL_TARGET,
                    &["CHUTES_API_KEY"],
                )?;
                let base = std::env::var("CHUTES_API_URL")
                    .ok()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| DEFAULT_API_URL.to_string());
                let base = crate::providers::validated_https_url(&base, "Chutes API")?;
                let usage = fetch_usage_snapshot(&self.client, &base, &key).await?;
                Ok(ProviderFetchResult::new(usage, "api"))
            }
            SourceMode::Web | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }
}

/// Read `users/me/subscription_usage`, then enrich it from `quotas` and
/// `quota_usage/{id}` only when the rolling or monthly lane is still missing.
///
/// Enrichment is best effort: a rejected key stays fatal, every other failure
/// leaves the subscription result as it was. A cancelled refresh drops this
/// future, so cancellation needs no separate handling here.
async fn fetch_usage_snapshot(
    client: &Client,
    base: &Url,
    key: &str,
) -> Result<UsageSnapshot, ProviderError> {
    let mut subscription = parse(&get_json(client, base, key, &["subscription_usage"]).await?);
    if !subscription.has_rolling_and_monthly() {
        match fetch_quota_details(client, base, key).await {
            Ok(quotas) if quotas.lanes().has_windows() => subscription.fill_missing_from(quotas),
            Ok(_) => {}
            Err(ProviderError::AuthRequired) => return Err(ProviderError::AuthRequired),
            Err(error) => tracing::debug!(%error, "Chutes quota enrichment failed"),
        }
    }
    Ok(subscription.lanes().into_usage())
}

/// `GET users/me/quotas`, then `GET users/me/quota_usage/{id}` for each quota
/// definition, merged with the usage fields taking precedence.
async fn fetch_quota_details(
    client: &Client,
    base: &Url,
    key: &str,
) -> Result<ParsedQuotas, ProviderError> {
    let raw = get_json(client, base, key, &["quotas"]).await?;
    let mut quotas = parse(&raw);
    let Some(definitions) =
        quota_definitions(&raw).filter(|list| list.iter().any(Value::is_object))
    else {
        return Ok(quotas);
    };
    let mut enriched = Vec::new();
    for definition in definitions.iter().filter_map(Value::as_object) {
        let mut merged = definition.clone();
        if let Some(id) = quota_id(definition) {
            match get_json(client, base, key, &["quota_usage", &id]).await {
                Ok(usage) => merged.extend(usage_fields(&usage)),
                Err(ProviderError::AuthRequired) => return Err(ProviderError::AuthRequired),
                Err(error) => tracing::debug!(%error, "Chutes quota usage request failed"),
            }
        }
        enriched.push(Value::Object(merged));
    }
    let detailed = parse(&json!({ "quotas": enriched }));
    if detailed.lanes().has_windows() {
        quotas = detailed;
    }
    Ok(quotas)
}

/// The quota definition list: the payload itself, `quotas`, `data`, or
/// `data.quotas`, whichever is an array first.
fn quota_definitions(raw: &Value) -> Option<&Vec<Value>> {
    let root = raw.as_object();
    let data = root.and_then(|root| root.get("data"));
    [
        Some(raw),
        root.and_then(|root| root.get("quotas")),
        data,
        data.and_then(Value::as_object)
            .and_then(|data| data.get("quotas")),
    ]
    .into_iter()
    .flatten()
    .find_map(Value::as_array)
}

fn quota_id(definition: &Map<String, Value>) -> Option<String> {
    ["chute_id", "chuteId", "id"]
        .iter()
        .find_map(|key| text(definition.get(*key)))
}

/// Usage fields of a `quota_usage/{id}` response, unwrapping `data` or `result`.
fn usage_fields(response: &Value) -> Map<String, Value> {
    let Some(result) = response.as_object() else {
        return Map::new();
    };
    ["data", "result"]
        .iter()
        .find_map(|key| result.get(*key).and_then(Value::as_object))
        .unwrap_or(result)
        .clone()
}

/// `{base}/users/me/{segments...}`; the configured path and query are kept.
fn endpoint(base: &Url, segments: &[&str]) -> Result<Url, ProviderError> {
    let mut url = base.clone();
    url.set_fragment(None);
    url.path_segments_mut()
        .map_err(|()| ProviderError::Other("Invalid Chutes API URL: cannot be a base".into()))?
        .pop_if_empty()
        .push("users")
        .push("me")
        .extend(segments);
    Ok(url)
}

async fn get_json(
    client: &Client,
    base: &Url,
    key: &str,
    segments: &[&str],
) -> Result<Value, ProviderError> {
    let response = client
        .get(endpoint(base, segments)?)
        .bearer_auth(key)
        .header(ACCEPT, "application/json")
        .send()
        .await?;
    let status = response.status();
    if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
        return Err(ProviderError::AuthRequired);
    }
    if !status.is_success() {
        return Err(ProviderError::Other(format!(
            "Chutes usage API error: HTTP {}",
            status.as_u16()
        )));
    }
    response
        .json::<Value>()
        .await
        .ok()
        .filter(|value| value.is_object() || value.is_array())
        .ok_or_else(|| ProviderError::Parse("Chutes usage response is not valid JSON".into()))
}
