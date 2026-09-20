//! Deepgram provider implementation.
//!
//! Fetches Management API usage breakdowns for one or all Deepgram projects.

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, UsageSnapshot,
};

const DEEPGRAM_API_BASE: &str = "https://api.deepgram.com/v1";
const DEEPGRAM_CREDENTIAL_TARGET: &str = "codexbar-deepgram";

#[derive(Debug, Deserialize)]
struct ProjectsResponse {
    projects: Vec<Project>,
}

#[derive(Debug, Clone, Deserialize)]
struct Project {
    #[serde(rename = "project_id")]
    project_id: String,
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct UsageResponse {
    start: Option<String>,
    end: Option<String>,
    #[serde(default)]
    results: Vec<UsageResult>,
}

#[derive(Debug, Deserialize)]
struct UsageResult {
    hours: Option<f64>,
    #[serde(rename = "total_hours")]
    total_hours: Option<f64>,
    #[serde(rename = "agent_hours")]
    agent_hours: Option<f64>,
    #[serde(rename = "tokens_in")]
    tokens_in: Option<u64>,
    #[serde(rename = "tokens_out")]
    tokens_out: Option<u64>,
    #[serde(rename = "tts_characters")]
    tts_characters: Option<u64>,
    requests: Option<u64>,
}

#[derive(Debug, Clone)]
struct DeepgramUsageSummary {
    project_id: String,
    project_name: Option<String>,
    project_count: usize,
    start: Option<String>,
    end: Option<String>,
    hours: f64,
    total_hours: f64,
    agent_hours: f64,
    tokens_in: u64,
    tokens_out: u64,
    tts_characters: u64,
    requests: u64,
}

pub struct DeepgramProvider {
    metadata: ProviderMetadata,
    client: Client,
}

impl DeepgramProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Deepgram,
                display_name: "Deepgram",
                session_label: "Requests",
                weekly_label: "Usage",
                supports_opus: false,
                supports_credits: true,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://console.deepgram.com/usage"),
                status_page_url: Some("https://status.deepgram.com"),
                tertiary_label_key: None,
            },
            client: crate::core::credentialed_http_client_builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    async fn fetch_api(&self, api_key: &str) -> Result<UsageSnapshot, ProviderError> {
        let projects = self.list_projects(api_key).await?;
        if projects.is_empty() {
            return Err(ProviderError::Other(
                "Deepgram API returned no projects for this key.".to_string(),
            ));
        }

        let mut summaries = Vec::with_capacity(projects.len());
        for project in projects {
            summaries.push(self.fetch_project_usage(api_key, &project).await?);
        }

        Ok(snapshot_from_summary(&aggregate_summaries(&summaries)?))
    }

    async fn list_projects(&self, api_key: &str) -> Result<Vec<Project>, ProviderError> {
        let response = self
            .client
            .get(format!("{DEEPGRAM_API_BASE}/projects"))
            .header("Authorization", format!("Token {api_key}"))
            .header("Accept", "application/json")
            .send()
            .await?;

        match response.status() {
            reqwest::StatusCode::UNAUTHORIZED => return Err(ProviderError::AuthRequired),
            reqwest::StatusCode::FORBIDDEN => {
                return Err(ProviderError::Other(
                    "Deepgram API key does not have Management API access.".to_string(),
                ));
            }
            status if !status.is_success() => {
                return Err(ProviderError::Other(format!(
                    "Deepgram projects API returned status {status}"
                )));
            }
            _ => {}
        }

        let body: ProjectsResponse = response
            .json()
            .await
            .map_err(|e| ProviderError::Parse(format!("Failed to parse Deepgram projects: {e}")))?;
        Ok(body.projects)
    }

    async fn fetch_project_usage(
        &self,
        api_key: &str,
        project: &Project,
    ) -> Result<DeepgramUsageSummary, ProviderError> {
        let response = self
            .client
            .get(format!(
                "{DEEPGRAM_API_BASE}/projects/{}/usage/breakdown",
                project.project_id
            ))
            .header("Authorization", format!("Token {api_key}"))
            .header("Accept", "application/json")
            .send()
            .await?;

        match response.status() {
            reqwest::StatusCode::UNAUTHORIZED => return Err(ProviderError::AuthRequired),
            reqwest::StatusCode::FORBIDDEN => {
                return Err(ProviderError::Other(format!(
                    "Deepgram API key cannot read usage for project {}.",
                    project.project_id
                )));
            }
            status if !status.is_success() => {
                return Err(ProviderError::Other(format!(
                    "Deepgram usage API returned status {status}"
                )));
            }
            _ => {}
        }

        let usage: UsageResponse = response
            .json()
            .await
            .map_err(|e| ProviderError::Parse(format!("Failed to parse Deepgram usage: {e}")))?;
        Ok(summary_from_usage(project, &usage))
    }
}

impl Default for DeepgramProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for DeepgramProvider {
    fn id(&self) -> ProviderId {
        ProviderId::Deepgram
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::OAuth => {
                let api_key = resolve_api_key(
                    ctx.api_key.as_deref(),
                    DEEPGRAM_CREDENTIAL_TARGET,
                    &["DEEPGRAM_API_KEY"],
                )?;
                Ok(ProviderFetchResult::new(
                    self.fetch_api(&api_key).await?,
                    "api",
                ))
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

fn summary_from_usage(project: &Project, usage: &UsageResponse) -> DeepgramUsageSummary {
    DeepgramUsageSummary {
        project_id: project.project_id.clone(),
        project_name: project.name.clone(),
        project_count: 1,
        start: usage.start.clone(),
        end: usage.end.clone(),
        hours: usage.results.iter().map(|r| r.hours.unwrap_or(0.0)).sum(),
        total_hours: usage
            .results
            .iter()
            .map(|r| r.total_hours.unwrap_or(0.0))
            .sum(),
        agent_hours: usage
            .results
            .iter()
            .map(|r| r.agent_hours.unwrap_or(0.0))
            .sum(),
        tokens_in: usage.results.iter().map(|r| r.tokens_in.unwrap_or(0)).sum(),
        tokens_out: usage
            .results
            .iter()
            .map(|r| r.tokens_out.unwrap_or(0))
            .sum(),
        tts_characters: usage
            .results
            .iter()
            .map(|r| r.tts_characters.unwrap_or(0))
            .sum(),
        requests: usage.results.iter().map(|r| r.requests.unwrap_or(0)).sum(),
    }
}

fn aggregate_summaries(
    summaries: &[DeepgramUsageSummary],
) -> Result<DeepgramUsageSummary, ProviderError> {
    let Some(first) = summaries.first() else {
        return Err(ProviderError::Other(
            "Deepgram API returned no usage summaries.".to_string(),
        ));
    };
    if summaries.len() == 1 {
        return Ok(first.clone());
    }

    Ok(DeepgramUsageSummary {
        project_id: "all".to_string(),
        project_name: None,
        project_count: summaries.len(),
        start: summaries.iter().filter_map(|s| s.start.clone()).min(),
        end: summaries.iter().filter_map(|s| s.end.clone()).max(),
        hours: summaries.iter().map(|s| s.hours).sum(),
        total_hours: summaries.iter().map(|s| s.total_hours).sum(),
        agent_hours: summaries.iter().map(|s| s.agent_hours).sum(),
        tokens_in: summaries.iter().map(|s| s.tokens_in).sum(),
        tokens_out: summaries.iter().map(|s| s.tokens_out).sum(),
        tts_characters: summaries.iter().map(|s| s.tts_characters).sum(),
        requests: summaries.iter().map(|s| s.requests).sum(),
    })
}

fn snapshot_from_summary(summary: &DeepgramUsageSummary) -> UsageSnapshot {
    let mut primary = RateWindow::new(0.0);
    primary.reset_description = Some(format!("{} requests", format_count(summary.requests)));

    let mut secondary = RateWindow::new(0.0);
    secondary.reset_description = Some(format!(
        "{} audio hours / {} billable hours",
        format_decimal(summary.hours),
        format_decimal(summary.total_hours)
    ));

    let mut tertiary = RateWindow::new(0.0);
    tertiary.reset_description = Some(format!(
        "{} tokens / {} TTS chars",
        format_count(summary.tokens_in + summary.tokens_out),
        format_count(summary.tts_characters)
    ));

    let identity = if summary.project_count > 1 {
        format!("{} projects", summary.project_count)
    } else if let Some(name) = summary
        .project_name
        .as_deref()
        .filter(|name| !name.is_empty())
    {
        format!("Project: {name}")
    } else {
        format!("Project: {}", summary.project_id)
    };

    UsageSnapshot::new(primary)
        .with_secondary(secondary)
        .with_tertiary(tertiary)
        .with_login_method(identity)
}

fn format_count(value: u64) -> String {
    let raw = value.to_string();
    let mut out = String::with_capacity(raw.len() + raw.len() / 3);
    for (idx, ch) in raw.chars().rev().enumerate() {
        if idx > 0 && idx % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.chars().rev().collect()
}

fn format_decimal(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    }
}

fn resolve_api_key(
    explicit: Option<&str>,
    credential_target: &str,
    env_names: &[&str],
) -> Result<String, ProviderError> {
    if let Some(key) = explicit
        && !key.trim().is_empty()
    {
        return Ok(key.trim().to_string());
    }
    if let Ok(entry) = keyring::Entry::new(credential_target, "api_key")
        && let Ok(key) = entry.get_password()
        && !key.trim().is_empty()
    {
        return Ok(key);
    }
    for env in env_names {
        if let Ok(key) = std::env::var(env)
            && !key.trim().is_empty()
        {
            return Ok(key);
        }
    }
    Err(ProviderError::NotInstalled(format!(
        "API key not found. Set {} in Preferences or environment.",
        env_names.join(" / ")
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aggregates_project_usage() {
        let project = Project {
            project_id: "project-1".into(),
            name: Some("Prod".into()),
        };
        let summary = summary_from_usage(
            &project,
            &UsageResponse {
                start: Some("2026-05-01".into()),
                end: Some("2026-05-19".into()),
                results: vec![
                    UsageResult {
                        hours: Some(1.5),
                        total_hours: Some(2.0),
                        agent_hours: Some(0.25),
                        tokens_in: Some(1000),
                        tokens_out: Some(2000),
                        tts_characters: Some(3000),
                        requests: Some(4),
                    },
                    UsageResult {
                        hours: Some(0.5),
                        total_hours: Some(1.0),
                        agent_hours: None,
                        tokens_in: Some(500),
                        tokens_out: None,
                        tts_characters: None,
                        requests: Some(6),
                    },
                ],
            },
        );

        assert_eq!(summary.requests, 10);
        assert_eq!(summary.hours, 2.0);
        assert_eq!(summary.total_hours, 3.0);
        assert_eq!(summary.tokens_in, 1500);

        let snapshot = snapshot_from_summary(&summary);
        assert_eq!(
            snapshot.primary.reset_description.as_deref(),
            Some("10 requests")
        );
        assert_eq!(snapshot.login_method.as_deref(), Some("Project: Prod"));
    }
}
