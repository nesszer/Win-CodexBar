//! AWS Bedrock provider implementation.
//!
//! Fetches current-month Bedrock spend from AWS Cost Explorer using SigV4.

use async_trait::async_trait;
use chrono::{Datelike, Duration, TimeZone, Utc};
use reqwest::Client;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::core::{
    CostSnapshot, FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, RateWindow, SourceMode, UsageSnapshot,
};

const COST_EXPLORER_URL: &str = "https://ce.us-east-1.amazonaws.com";
const COST_EXPLORER_TARGET: &str = "AWSInsightsIndexService.GetCostAndUsage";
const SERVICE: &str = "ce";
const SIGNING_REGION: &str = "us-east-1";

#[derive(Debug, Clone)]
struct AwsCredentials {
    access_key_id: String,
    secret_access_key: String,
    session_token: Option<String>,
}

pub struct BedrockProvider {
    metadata: ProviderMetadata,
    client: Client,
}

impl BedrockProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Bedrock,
                display_name: "AWS Bedrock",
                session_label: "Budget",
                weekly_label: "Cost",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://console.aws.amazon.com/bedrock"),
                status_page_url: Some("https://health.aws.amazon.com/health/status"),
            },
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    fn credentials_from_context(api_key: Option<&str>) -> Option<AwsCredentials> {
        let raw = api_key?.trim();
        if raw.is_empty() {
            return None;
        }

        if let Ok(json) = serde_json::from_str::<Value>(raw) {
            let access_key_id = json
                .get("access_key_id")
                .or_else(|| json.get("accessKeyId"))
                .or_else(|| json.get("AWS_ACCESS_KEY_ID"))
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|v| !v.is_empty())?;
            let secret_access_key = json
                .get("secret_access_key")
                .or_else(|| json.get("secretAccessKey"))
                .or_else(|| json.get("AWS_SECRET_ACCESS_KEY"))
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|v| !v.is_empty())?;
            let session_token = json
                .get("session_token")
                .or_else(|| json.get("sessionToken"))
                .or_else(|| json.get("AWS_SESSION_TOKEN"))
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|v| !v.is_empty())
                .map(str::to_string);

            return Some(AwsCredentials {
                access_key_id: access_key_id.to_string(),
                secret_access_key: secret_access_key.to_string(),
                session_token,
            });
        }

        let parts: Vec<&str> = raw.splitn(3, ':').map(str::trim).collect();
        if parts.len() >= 2 && !parts[0].is_empty() && !parts[1].is_empty() {
            return Some(AwsCredentials {
                access_key_id: parts[0].to_string(),
                secret_access_key: parts[1].to_string(),
                session_token: parts
                    .get(2)
                    .copied()
                    .filter(|value| !value.is_empty())
                    .map(str::to_string),
            });
        }

        None
    }

    fn credentials_from_env() -> Result<AwsCredentials, ProviderError> {
        let access_key_id = cleaned_env("AWS_ACCESS_KEY_ID").ok_or_else(|| {
            ProviderError::NotInstalled(
                "AWS credentials not configured. Set AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY."
                    .to_string(),
            )
        })?;
        let secret_access_key = cleaned_env("AWS_SECRET_ACCESS_KEY").ok_or_else(|| {
            ProviderError::NotInstalled(
                "AWS credentials not configured. Set AWS_ACCESS_KEY_ID and AWS_SECRET_ACCESS_KEY."
                    .to_string(),
            )
        })?;

        Ok(AwsCredentials {
            access_key_id,
            secret_access_key,
            session_token: cleaned_env("AWS_SESSION_TOKEN"),
        })
    }

    fn monthly_budget() -> Option<f64> {
        cleaned_env("CODEXBAR_BEDROCK_BUDGET").and_then(|raw| {
            raw.parse::<f64>()
                .ok()
                .filter(|value| value.is_finite() && *value > 0.0)
        })
    }

    async fn fetch_monthly_spend(
        &self,
        credentials: &AwsCredentials,
    ) -> Result<f64, ProviderError> {
        let (start_date, end_date) = current_month_range();
        let mut total = 0.0;
        let mut next_page_token: Option<String> = None;

        loop {
            let mut body = json!({
                "TimePeriod": {
                    "Start": start_date,
                    "End": end_date,
                },
                "Granularity": "MONTHLY",
                "Metrics": ["UnblendedCost"],
                "GroupBy": [
                    { "Type": "DIMENSION", "Key": "SERVICE" }
                ],
            });
            if let Some(token) = &next_page_token {
                body["NextPageToken"] = Value::String(token.clone());
            }

            let body_bytes = serde_json::to_vec(&body)
                .map_err(|e| ProviderError::Other(format!("Bedrock request build failed: {e}")))?;
            let body_hash = sha256_hex(&body_bytes);
            let now = Utc::now();
            let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
            let date_stamp = now.format("%Y%m%d").to_string();
            let authorization = sign_authorization(
                credentials,
                &date_stamp,
                &amz_date,
                &body_hash,
                COST_EXPLORER_URL,
                &body_bytes,
            )?;

            let mut request = self
                .client
                .post(COST_EXPLORER_URL)
                .header("Content-Type", "application/x-amz-json-1.1")
                .header("Host", "ce.us-east-1.amazonaws.com")
                .header("X-Amz-Target", COST_EXPLORER_TARGET)
                .header("X-Amz-Date", amz_date)
                .header("x-amz-content-sha256", body_hash)
                .header("Authorization", authorization);
            if let Some(token) = &credentials.session_token {
                request = request.header("X-Amz-Security-Token", token);
            }

            let response = request.body(body_bytes).send().await?;
            let status = response.status();
            let text = response.text().await?;

            if status.as_u16() == 401 || status.as_u16() == 403 {
                return Err(ProviderError::AuthRequired);
            }
            if !status.is_success() {
                return Err(ProviderError::Other(format!(
                    "AWS Cost Explorer returned {}: {}",
                    status,
                    sanitized_body(&text)
                )));
            }

            let page: Value = serde_json::from_str(&text).map_err(|e| {
                ProviderError::Parse(format!("Failed to parse AWS Cost Explorer response: {e}"))
            })?;
            total += parse_bedrock_cost(&page);
            next_page_token = page
                .get("NextPageToken")
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .filter(|s| !s.trim().is_empty());

            if next_page_token.is_none() {
                break;
            }
        }

        Ok(total)
    }

    async fn fetch_via_api(
        &self,
        ctx: &FetchContext,
    ) -> Result<ProviderFetchResult, ProviderError> {
        let credentials =
            if let Some(credentials) = Self::credentials_from_context(ctx.api_key.as_deref()) {
                credentials
            } else {
                Self::credentials_from_env()?
            };
        let budget = Self::monthly_budget();
        let spend = self.fetch_monthly_spend(&credentials).await?;
        let resets_at = end_of_current_month();

        let used_percent = budget
            .map(|limit| {
                if limit > 0.0 {
                    (spend / limit) * 100.0
                } else {
                    0.0
                }
            })
            .unwrap_or(0.0);

        let mut primary = RateWindow::with_details(
            used_percent,
            None,
            resets_at,
            budget.map(|_| "Monthly budget".to_string()),
        );
        if budget.is_none() {
            primary.reset_description = Some(format!("Monthly spend ${spend:.2}"));
        }

        let mut cost = CostSnapshot::new(spend, "USD", "Monthly");
        if let Some(limit) = budget {
            cost = cost.with_limit(limit);
        }
        if let Some(reset) = resets_at {
            cost = cost.with_resets_at(reset);
        }

        let mut login_method = format!("Spend: ${spend:.2}");
        if let Some(limit) = budget {
            login_method.push_str(&format!(" - Budget: ${limit:.2}"));
        }

        let usage = UsageSnapshot::new(primary).with_login_method(login_method);
        Ok(ProviderFetchResult::new(usage, "api").with_cost(cost))
    }
}

impl Default for BedrockProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for BedrockProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Bedrock
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => self.fetch_via_api(ctx).await,
            SourceMode::Web | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }
}

fn cleaned_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| {
            value
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .trim()
                .to_string()
        })
        .filter(|value| !value.is_empty())
}

fn current_month_range() -> (String, String) {
    let now = Utc::now();
    let start = Utc
        .with_ymd_and_hms(now.year(), now.month(), 1, 0, 0, 0)
        .single()
        .unwrap_or(now);
    let tomorrow = (now + Duration::days(1)).date_naive();
    (
        start.format("%Y-%m-%d").to_string(),
        tomorrow.format("%Y-%m-%d").to_string(),
    )
}

fn end_of_current_month() -> Option<chrono::DateTime<Utc>> {
    let now = Utc::now();
    let (year, month) = if now.month() == 12 {
        (now.year() + 1, 1)
    } else {
        (now.year(), now.month() + 1)
    };
    Utc.with_ymd_and_hms(year, month, 1, 0, 0, 0).single()
}

fn parse_bedrock_cost(page: &Value) -> f64 {
    page.get("ResultsByTime")
        .and_then(|v| v.as_array())
        .into_iter()
        .flatten()
        .flat_map(|result| {
            result
                .get("Groups")
                .and_then(|v| v.as_array())
                .into_iter()
                .flatten()
        })
        .filter(|group| {
            group
                .get("Keys")
                .and_then(|v| v.as_array())
                .and_then(|keys| keys.first())
                .and_then(|v| v.as_str())
                .is_some_and(|service| service.to_lowercase().contains("bedrock"))
        })
        .filter_map(|group| {
            group
                .get("Metrics")
                .and_then(|v| v.get("UnblendedCost"))
                .and_then(|v| v.get("Amount"))
                .and_then(|v| v.as_str())
                .and_then(|amount| amount.parse::<f64>().ok())
        })
        .sum()
}

fn sign_authorization(
    credentials: &AwsCredentials,
    date_stamp: &str,
    amz_date: &str,
    body_hash: &str,
    url: &str,
    body: &[u8],
) -> Result<String, ProviderError> {
    let parsed = url::Url::parse(url)
        .map_err(|e| ProviderError::Other(format!("Invalid AWS endpoint URL: {e}")))?;
    let host = parsed.host_str().unwrap_or("ce.us-east-1.amazonaws.com");
    let (canonical_headers, signed_headers) = if let Some(session_token) =
        &credentials.session_token
    {
        (
            format!(
                "content-type:application/x-amz-json-1.1\nhost:{host}\nx-amz-content-sha256:{body_hash}\nx-amz-date:{amz_date}\nx-amz-security-token:{session_token}\nx-amz-target:{COST_EXPLORER_TARGET}\n"
            ),
            "content-type;host;x-amz-content-sha256;x-amz-date;x-amz-security-token;x-amz-target",
        )
    } else {
        (
            format!(
                "content-type:application/x-amz-json-1.1\nhost:{host}\nx-amz-content-sha256:{body_hash}\nx-amz-date:{amz_date}\nx-amz-target:{COST_EXPLORER_TARGET}\n"
            ),
            "content-type;host;x-amz-content-sha256;x-amz-date;x-amz-target",
        )
    };
    let canonical_request = [
        "POST",
        "/",
        "",
        canonical_headers.as_str(),
        signed_headers,
        body_hash,
    ]
    .join("\n");
    let credential_scope = format!("{date_stamp}/{SIGNING_REGION}/{SERVICE}/aws4_request");
    let string_to_sign = [
        "AWS4-HMAC-SHA256",
        amz_date,
        credential_scope.as_str(),
        sha256_hex(canonical_request.as_bytes()).as_str(),
    ]
    .join("\n");

    let k_date = hmac_sha256(
        format!("AWS4{}", credentials.secret_access_key).as_bytes(),
        date_stamp.as_bytes(),
    );
    let k_region = hmac_sha256(&k_date, SIGNING_REGION.as_bytes());
    let k_service = hmac_sha256(&k_region, SERVICE.as_bytes());
    let k_signing = hmac_sha256(&k_service, b"aws4_request");
    let signature = hex(&hmac_sha256(&k_signing, string_to_sign.as_bytes()));

    // Keep body in the signature call path so tests catch accidental divergence.
    debug_assert_eq!(sha256_hex(body), body_hash);

    Ok(format!(
        "AWS4-HMAC-SHA256 Credential={}/{}, SignedHeaders={}, Signature={}",
        credentials.access_key_id, credential_scope, signed_headers, signature
    ))
}

fn sha256_hex(data: &[u8]) -> String {
    hex(&Sha256::digest(data))
}

fn hmac_sha256(key: &[u8], data: &[u8]) -> Vec<u8> {
    const BLOCK_SIZE: usize = 64;
    let mut key_block = [0u8; BLOCK_SIZE];
    if key.len() > BLOCK_SIZE {
        key_block[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        key_block[..key.len()].copy_from_slice(key);
    }

    let mut outer = [0x5cu8; BLOCK_SIZE];
    let mut inner = [0x36u8; BLOCK_SIZE];
    for i in 0..BLOCK_SIZE {
        outer[i] ^= key_block[i];
        inner[i] ^= key_block[i];
    }

    let mut inner_hash = Sha256::new();
    inner_hash.update(inner);
    inner_hash.update(data);
    let inner_digest = inner_hash.finalize();

    let mut outer_hash = Sha256::new();
    outer_hash.update(outer);
    outer_hash.update(inner_digest);
    outer_hash.finalize().to_vec()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn sanitized_body(body: &str) -> String {
    let collapsed = body.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() > 240 {
        let preview: String = collapsed.chars().take(240).collect();
        format!("{preview}... [truncated]")
    } else if collapsed.is_empty() {
        "empty body".to_string()
    } else {
        collapsed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_bedrock_cost_only() {
        let page = json!({
            "ResultsByTime": [{
                "Groups": [
                    {
                        "Keys": ["Amazon Bedrock"],
                        "Metrics": { "UnblendedCost": { "Amount": "12.34" } }
                    },
                    {
                        "Keys": ["Amazon S3"],
                        "Metrics": { "UnblendedCost": { "Amount": "99.00" } }
                    }
                ]
            }]
        });
        assert_eq!(parse_bedrock_cost(&page), 12.34);
    }

    #[test]
    fn parses_context_credentials_from_json() {
        let credentials = BedrockProvider::credentials_from_context(Some(
            r#"{
                "access_key_id": "AKIAEXAMPLE",
                "secret_access_key": "secret",
                "session_token": "session"
            }"#,
        ))
        .expect("credentials");

        assert_eq!(credentials.access_key_id, "AKIAEXAMPLE");
        assert_eq!(credentials.secret_access_key, "secret");
        assert_eq!(credentials.session_token.as_deref(), Some("session"));
    }

    #[test]
    fn parses_context_credentials_from_colon_delimited_value() {
        let credentials =
            BedrockProvider::credentials_from_context(Some("AKIAEXAMPLE:secret:session"))
                .expect("credentials");

        assert_eq!(credentials.access_key_id, "AKIAEXAMPLE");
        assert_eq!(credentials.secret_access_key, "secret");
        assert_eq!(credentials.session_token.as_deref(), Some("session"));
    }

    #[test]
    fn hmac_sha256_matches_rfc_4231_case_1() {
        let digest = hmac_sha256(&[0x0b; 20], b"Hi There");
        assert_eq!(
            hex(&digest),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }
}
