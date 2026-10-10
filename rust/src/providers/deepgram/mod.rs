//! Deepgram provider implementation.
//!
//! Fetches Management API usage breakdowns for one or all Deepgram projects.

use async_trait::async_trait;
use reqwest::Client;
use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, UsageSnapshot,
};
use crate::providers::http_util::{StatusPolicy, send_json};

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
        let policy = StatusPolicy::auth_401("Deepgram projects API")
            .forbidden("Deepgram API key does not have Management API access.".to_string());
        let body: ProjectsResponse = self
            .get_json("/projects", api_key, &policy, "Deepgram projects")
            .await?;
        Ok(body.projects)
    }

    async fn fetch_project_usage(
        &self,
        api_key: &str,
        project: &Project,
    ) -> Result<DeepgramUsageSummary, ProviderError> {
        let policy = StatusPolicy::auth_401("Deepgram usage API").forbidden(format!(
            "Deepgram API key cannot read usage for project {}.",
            project.project_id
        ));
        let path = format!("/projects/{}/usage/breakdown", project.project_id);
        let usage: UsageResponse = self
            .get_json(&path, api_key, &policy, "Deepgram usage")
            .await?;
        Ok(summary_from_usage(project, &usage))
    }

    async fn get_json<T: DeserializeOwned>(
        &self,
        path: &str,
        api_key: &str,
        policy: &StatusPolicy<'_>,
        parse_label: &str,
    ) -> Result<T, ProviderError> {
        let request = self
            .client
            .get(format!("{DEEPGRAM_API_BASE}{path}"))
            .header("Authorization", format!("Token {api_key}"))
            .header("Accept", "application/json");
        send_json(request, policy, parse_label).await
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
                let api_key = crate::providers::resolve_api_key(
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
    let hours = |field: fn(&UsageResult) -> Option<f64>| -> f64 {
        usage.results.iter().map(|r| field(r).unwrap_or(0.0)).sum()
    };
    let count = |field: fn(&UsageResult) -> Option<u64>| -> u64 {
        usage.results.iter().map(|r| field(r).unwrap_or(0)).sum()
    };
    DeepgramUsageSummary {
        project_id: project.project_id.clone(),
        project_name: project.name.clone(),
        project_count: 1,
        start: usage.start.clone(),
        end: usage.end.clone(),
        hours: hours(|r| r.hours),
        total_hours: hours(|r| r.total_hours),
        agent_hours: hours(|r| r.agent_hours),
        tokens_in: count(|r| r.tokens_in),
        tokens_out: count(|r| r.tokens_out),
        tts_characters: count(|r| r.tts_characters),
        requests: count(|r| r.requests),
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
