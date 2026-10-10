//! llmman provider: memory and model usage of a local (or LAN) llmman daemon.
//!
//! Ported from upstream CodexBar 0.66.0 (`llmman` plugin). The daemon reports
//! its memory budget plus the loaded and stored models at `GET /llmman/node`;
//! the version comes best-effort from `GET /api/version`. The API key is
//! optional because a daemon without configured keys is open.

mod endpoint;
#[cfg(test)]
mod tests;

use std::collections::BTreeMap;
use std::time::Duration;

use async_trait::async_trait;
use reqwest::{Client, Response, StatusCode};
use serde::Deserialize;

use crate::core::{
    FetchContext, Provider, ProviderDisplayDetail, ProviderError, ProviderFetchResult, ProviderId,
    ProviderStateKind, RateWindow, SourceMode, UsageSnapshot,
};
use crate::providers::{BoundedBodyError, read_bounded_response};

pub(crate) use endpoint::DEFAULT_BASE_URL;
use endpoint::request_base;
pub(crate) use endpoint::validated_llmman_base_url;

const CREDENTIAL_TARGET: &str = "codexbar-llmman";
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_BODY_BYTES: usize = 1024 * 1024;
const MAX_LOADED_MODEL_ROWS: usize = 24;
/// JavaScript's `Number.isSafeInteger` bound, which the upstream plugin uses.
const MAX_SAFE_BYTES: u64 = (1 << 53) - 1;
const NOT_REACHABLE_PREFIX: &str = "llmman is not reachable at ";

pub struct LLMManProvider {
    client: Client,
}

impl LLMManProvider {
    pub fn new() -> Self {
        Self {
            client: crate::core::credentialed_http_client_builder()
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }
}

impl Default for LLMManProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for LLMManProvider {
    fn id(&self) -> ProviderId {
        ProviderId::LLMMan
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => {
                let base = resolve_base_url(ctx.workspace_id.as_deref())?;
                let key = crate::providers::resolve_api_key(
                    ctx.api_key.as_deref(),
                    CREDENTIAL_TARGET,
                    &["LLMMAN_API_KEY"],
                )
                .ok();
                fetch_daemon(&self.client, &base, key.as_deref()).await
            }
            SourceMode::Web | SourceMode::Cli => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::OAuth]
    }

    /// An unreachable daemon is a local runtime that is not running, not a
    /// credential problem.
    fn error_state_kind(&self, error: &ProviderError) -> ProviderStateKind {
        match error {
            ProviderError::Other(message) if message.starts_with(NOT_REACHABLE_PREFIX) => {
                ProviderStateKind::LocalRuntimeOffline
            }
            _ => error.state_kind(),
        }
    }
}

/// Saved base URL, else `LLMMAN_HOST`, else the default local daemon.
fn resolve_base_url(saved: Option<&str>) -> Result<String, ProviderError> {
    let configured = saved
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
        .or_else(|| std::env::var("LLMMAN_HOST").ok())
        .unwrap_or_default();
    validated_llmman_base_url(&configured).map_err(ProviderError::Other)
}

/// The dashboard is the daemon itself, so it follows the configured base URL
/// (saved value, else `LLMMAN_HOST`) and falls back to the default local
/// daemon when that value is invalid.
pub fn dashboard_url(saved: Option<&str>) -> String {
    let base = resolve_base_url(saved).unwrap_or_else(|_| DEFAULT_BASE_URL.to_string());
    request_base(&base).to_string()
}

async fn fetch_daemon(
    client: &Client,
    base: &str,
    key: Option<&str>,
) -> Result<ProviderFetchResult, ProviderError> {
    let base = request_base(base);
    let response = get(client, base, "/llmman/node", key).await?;
    check_status(response.status(), base, key.is_some())?;
    let node = parse_node(&read_body(response).await?)?;
    let version = fetch_version(client, base, key).await;
    Ok(build_result(&node, version, key.is_some()))
}

async fn get(
    client: &Client,
    base: &str,
    path: &str,
    key: Option<&str>,
) -> Result<Response, ProviderError> {
    let mut request = client
        .get(format!("{base}{path}"))
        .header("Accept", "application/json")
        .timeout(REQUEST_TIMEOUT);
    if let Some(key) = key {
        request = request.bearer_auth(key);
    }
    request.send().await.map_err(|_| not_reachable(base))
}

fn not_reachable(base: &str) -> ProviderError {
    ProviderError::Other(format!(
        "{NOT_REACHABLE_PREFIX}{base}. Start it with llmman serve."
    ))
}

fn check_status(status: StatusCode, base: &str, has_key: bool) -> Result<(), ProviderError> {
    let code = status.as_u16();
    match code {
        200 => Ok(()),
        401 | 403 if !has_key => Err(ProviderError::NotInstalled(
            "llmman requires an API key. Set one in Settings or LLMMAN_API_KEY.".into(),
        )),
        401 | 403 => Err(ProviderError::OAuthExpired(format!(
            "llmman rejected the API key (HTTP {code})."
        ))),
        429 => Err(ProviderError::Other("llmman is busy.".into())),
        500.. => Err(ProviderError::Other(format!("llmman error: HTTP {code}"))),
        _ => Err(ProviderError::Other(format!(
            "{base} is not an llmman daemon (HTTP {code})."
        ))),
    }
}

async fn read_body(response: Response) -> Result<Vec<u8>, ProviderError> {
    read_bounded_response(response, MAX_BODY_BYTES)
        .await
        .map_err(|error| match error {
            BoundedBodyError::TooLarge => unrecognized_node(),
            BoundedBodyError::Read(_) => ProviderError::Other(
                "llmman closed the connection before sending the full response.".into(),
            ),
        })
}

fn unrecognized_node() -> ProviderError {
    ProviderError::Parse("llmman returned an unrecognized /llmman/node response.".into())
}

/// Best effort: the version is display-only and older daemons may answer
/// differently, so every failure is ignored.
async fn fetch_version(client: &Client, base: &str, key: Option<&str>) -> Option<String> {
    #[derive(Deserialize)]
    struct VersionReply {
        version: String,
    }

    let response = get(client, base, "/api/version", key).await.ok()?;
    if response.status() != StatusCode::OK {
        return None;
    }
    let body = read_bounded_response(response, MAX_BODY_BYTES).await.ok()?;
    serde_json::from_slice::<VersionReply>(&body)
        .ok()
        .map(|reply| reply.version)
}

/// A byte count that must be a non-negative safe integer.
#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(try_from = "u64")]
struct ByteCount(u64);

impl TryFrom<u64> for ByteCount {
    type Error = String;

    fn try_from(value: u64) -> Result<Self, Self::Error> {
        if value <= MAX_SAFE_BYTES {
            Ok(Self(value))
        } else {
            Err(format!("byte count {value} is not a safe integer"))
        }
    }
}

#[derive(Debug, Deserialize)]
struct NodeBody {
    memory: ByteCount,
    loaded: BTreeMap<String, ByteCount>,
    stored: BTreeMap<String, ByteCount>,
}

/// A validated `/llmman/node` reply: sizes in bytes, models largest first
/// with ties broken by name.
#[derive(Debug)]
struct Node {
    memory: u64,
    loaded: Vec<(String, u64)>,
    stored: Vec<(String, u64)>,
}

fn parse_node(body: &[u8]) -> Result<Node, ProviderError> {
    let value: serde_json::Value = serde_json::from_slice(body).map_err(|_| unrecognized_node())?;
    // A derived struct would also accept a positional JSON array.
    if !value.is_object() {
        return Err(unrecognized_node());
    }
    let node = NodeBody::deserialize(value).map_err(|_| unrecognized_node())?;
    Ok(Node {
        memory: node.memory.0,
        loaded: largest_first(node.loaded),
        stored: largest_first(node.stored),
    })
}

fn largest_first(models: BTreeMap<String, ByteCount>) -> Vec<(String, u64)> {
    // BTreeMap iterates by name, and the sort is stable, so ties stay by name.
    let mut models: Vec<_> = models
        .into_iter()
        .map(|(name, size)| (name, size.0))
        .collect();
    models.sort_by_key(|model| std::cmp::Reverse(model.1));
    models
}

fn total(models: &[(String, u64)]) -> u64 {
    models.iter().map(|(_, size)| size).sum()
}

/// Decimal units, as llmman's own list/ps print them.
fn format_size(bytes: u64) -> String {
    if bytes < 1_000 {
        return format!("{bytes} B");
    }
    let (unit, scale) = match bytes {
        1_000_000_000.. => ("GB", 1e9),
        1_000_000.. => ("MB", 1e6),
        _ => ("kB", 1e3),
    };
    format!("{:.1} {unit}", bytes as f64 / scale)
}

fn count_and_size(models: &[(String, u64)]) -> String {
    format!("{} · {}", models.len(), format_size(total(models)))
}

fn build_result(node: &Node, version: Option<String>, has_key: bool) -> ProviderFetchResult {
    let in_use = total(&node.loaded);
    let primary = if node.memory > 0 {
        let mut window = RateWindow::new((in_use as f64 / node.memory as f64 * 100.0).min(100.0));
        window.reset_description = Some(format!(
            "{} of {}",
            format_size(in_use),
            format_size(node.memory)
        ));
        window.with_description_as_detail()
    } else {
        RateWindow::informational("Memory limit not reported")
    };
    let login_method = if has_key { "API key" } else { "Local daemon" };
    let usage = UsageSnapshot::new(primary).with_login_method(login_method);

    let mut result = ProviderFetchResult::new(usage, "api")
        .with_display_detail(ProviderDisplayDetail::new(
            "loaded",
            "Loaded",
            count_and_size(&node.loaded),
        ))
        .with_display_detail(ProviderDisplayDetail::new(
            "stored",
            "Stored",
            count_and_size(&node.stored),
        ))
        .with_display_detail(
            version.and_then(|version| ProviderDisplayDetail::new("version", "Version", version)),
        );
    for row in loaded_model_rows(node) {
        result = result.with_display_detail(Some(row));
    }
    result
}

/// Up to 24 loaded models, largest first; names that cannot be shown are
/// skipped before the cap is applied.
fn loaded_model_rows(node: &Node) -> Vec<ProviderDisplayDetail> {
    let mut rows = Vec::new();
    for (name, weight) in &node.loaded {
        if rows.len() == MAX_LOADED_MODEL_ROWS {
            break;
        }
        let id = format!("loaded-model-{}", rows.len());
        let Some(row) = ProviderDisplayDetail::new(id, name, format_size(*weight)) else {
            continue;
        };
        let row = match node.memory {
            0 => Some(row),
            memory => row.with_progress((*weight).min(memory) as f64, memory as f64),
        };
        rows.extend(row);
    }
    rows
}
