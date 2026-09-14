use regex_lite::Regex;
use serde::Deserialize;
use std::sync::OnceLock;

use crate::core::{NamedRateWindow, ProviderError, RateWindow, UsageSnapshot};

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UserStatusResponse {
    user_status: Option<UserStatus>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UserStatus {
    #[allow(
        dead_code,
        reason = "field mirrors the Antigravity API user payload; deserialized for round-trip fidelity but not read yet"
    )]
    email: Option<String>,
    plan_status: Option<PlanStatus>,
    user_tier: Option<UserTier>,
    cascade_model_config_data: Option<ModelConfigData>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct UserTier {
    #[allow(
        dead_code,
        reason = "mirrors the Antigravity API user tier payload; deserialized for round-trip fidelity"
    )]
    id: Option<String>,
    name: Option<String>,
    description: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlanStatus {
    plan_info: Option<PlanInfo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PlanInfo {
    plan_name: Option<String>,
    plan_display_name: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelConfigData {
    client_model_configs: Option<Vec<ModelConfig>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ModelConfig {
    #[serde(default)]
    label: String,
    #[serde(default)]
    model_id: Option<String>,
    #[serde(default)]
    id: Option<String>,
    quota_info: Option<QuotaInfo>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct QuotaInfo {
    remaining_fraction: Option<f64>,
    reset_time: Option<String>,
}

// ── Model-family classification ──────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum ModelFamily {
    Claude,
    ClaudeThinking,
    GeminiPro,
    GeminiFlash,
    Other,
}

pub(crate) fn classify_model(label: &str) -> ModelFamily {
    let lower = label.to_lowercase();
    if lower.contains("claude") {
        if lower.contains("thinking") {
            ModelFamily::ClaudeThinking
        } else {
            ModelFamily::Claude
        }
    } else if lower.contains("gemini") && lower.contains("pro") {
        ModelFamily::GeminiPro
    } else if lower.contains("gemini") && lower.contains("flash") {
        ModelFamily::GeminiFlash
    } else if lower.contains("pro") && !is_noisy_summary_model(&lower) {
        ModelFamily::GeminiPro
    } else if lower.contains("flash") {
        ModelFamily::GeminiFlash
    } else {
        ModelFamily::Other
    }
}

fn best_summary_model<'a>(
    candidates: &[&'a ModelConfig],
    family: ModelFamily,
) -> Option<&'a ModelConfig> {
    candidates
        .iter()
        .copied()
        .filter(|config| classify_model(model_label(config)) == family)
        .min_by(|a, b| {
            let a_label = model_label(a);
            let b_label = model_label(b);
            let a_priority = selection_priority(a_label, family);
            let b_priority = selection_priority(b_label, family);
            a_priority
                .cmp(&b_priority)
                .then_with(|| compare_model_configs(a, b))
        })
}

fn selection_priority(label: &str, family: ModelFamily) -> u8 {
    let lower = label.to_lowercase();
    match family {
        ModelFamily::GeminiPro if lower.contains("low") => 0,
        ModelFamily::GeminiPro => 1,
        _ => 0,
    }
}

fn compare_model_configs(a: &ModelConfig, b: &ModelConfig) -> std::cmp::Ordering {
    let a_label = model_label(a);
    let b_label = model_label(b);
    family_rank(classify_model(a_label))
        .cmp(&family_rank(classify_model(b_label)))
        .then_with(|| parse_model_version(b_label).cmp(&parse_model_version(a_label)))
        .then_with(|| tier_rank(a_label).cmp(&tier_rank(b_label)))
        .then_with(|| clean_model_label(a_label).cmp(&clean_model_label(b_label)))
}

fn family_rank(family: ModelFamily) -> u8 {
    match family {
        ModelFamily::Claude => 0,
        ModelFamily::GeminiPro => 1,
        ModelFamily::GeminiFlash => 2,
        ModelFamily::ClaudeThinking => 3,
        ModelFamily::Other => 4,
    }
}

fn tier_rank(label: &str) -> u8 {
    let lower = label.to_lowercase();
    if lower.contains("high") {
        0
    } else if lower.contains("medium") {
        1
    } else if lower.contains("low") {
        2
    } else {
        3
    }
}

fn parse_model_version(label: &str) -> (u16, u16) {
    static VERSION_RE: OnceLock<Regex> = OnceLock::new();
    let regex =
        VERSION_RE.get_or_init(|| Regex::new(r"(?i)(\d+)(?:[.-](\d+))?").expect("valid regex"));
    let Some(caps) = regex.captures(label) else {
        return (0, 0);
    };
    let major = caps
        .get(1)
        .and_then(|m| m.as_str().parse::<u16>().ok())
        .unwrap_or(0);
    let minor = caps
        .get(2)
        .and_then(|m| m.as_str().parse::<u16>().ok())
        .unwrap_or(0);
    (major, minor)
}

fn is_noisy_summary_model(label: &str) -> bool {
    let lower = label.to_lowercase();
    lower.contains("image")
        || lower.contains("lite")
        || lower.contains("autocomplete")
        || lower.contains("completion")
        || lower.contains("internal")
}

fn model_label(config: &ModelConfig) -> &str {
    if !config.label.trim().is_empty() {
        &config.label
    } else if let Some(model_id) = config.model_id.as_deref() {
        model_id
    } else {
        config.id.as_deref().unwrap_or_default()
    }
}

pub(crate) fn canonical_model_id(raw: &str) -> &str {
    match raw.trim().to_ascii_lowercase().as_str() {
        "gemini-3.6-flash"
        | "gemini-3.6-flash-low"
        | "gemini-3.6-flash-medium"
        | "gemini-3.6-flash-high"
        | "gemini-3.5-flash-extra-low"
        | "gemini-3.5-flash-low"
        | "gemini-3.5-flash-mid"
        | "gemini-3.5-flash-high"
        | "gemini-3-flash-agent" => "gemini-3.7-flash",
        _ => raw,
    }
}

fn model_window_id(config: &ModelConfig) -> String {
    let raw = config
        .model_id
        .as_deref()
        .or(config.id.as_deref())
        .unwrap_or_else(|| model_label(config));
    let raw = canonical_model_id(raw);
    let slug = raw
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() {
                ch.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    format!("model-{}", if slug.is_empty() { "unknown" } else { &slug })
}

fn rate_window_from_quota(quota: &QuotaInfo) -> RateWindow {
    let remaining = quota.remaining_fraction.unwrap_or(1.0);
    let used_percent = (1.0 - remaining) * 100.0;
    RateWindow::with_details(used_percent, None, None, quota.reset_time.clone())
}

fn clean_model_label(label: &str) -> String {
    let mut out = label.trim().replace('_', " ");
    while out.contains("  ") {
        out = out.replace("  ", " ");
    }
    out
}

pub(super) fn resolve_plan_name(status: &UserStatus) -> Option<String> {
    status
        .user_tier
        .as_ref()
        .and_then(|tier| first_non_empty([tier.name.as_deref(), tier.description.as_deref()]))
        .or_else(|| {
            status
                .plan_status
                .as_ref()
                .and_then(|plan_status| plan_status.plan_info.as_ref())
                .and_then(|plan| {
                    first_non_empty([plan.plan_display_name.as_deref(), plan.plan_name.as_deref()])
                })
        })
}

fn first_non_empty<'a>(values: impl IntoIterator<Item = Option<&'a str>>) -> Option<String> {
    values
        .into_iter()
        .flatten()
        .map(str::trim)
        .find(|value| !value.is_empty())
        .map(str::to_string)
}

pub(super) fn apply_user_identity(snapshot: &mut UsageSnapshot, response: &UserStatusResponse) {
    let Some(status) = response.user_status.as_ref() else {
        return;
    };
    snapshot.account_email = status
        .email
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    snapshot.login_method = resolve_plan_name(status);
}

pub(super) fn parse_user_status(
    response: UserStatusResponse,
) -> Result<UsageSnapshot, ProviderError> {
    let user_status = response
        .user_status
        .ok_or_else(|| ProviderError::Other("Missing userStatus".to_string()))?;

    let plan_name = resolve_plan_name(&user_status);
    let model_configs = user_status
        .cascade_model_config_data
        .and_then(|d| d.client_model_configs)
        .unwrap_or_default();

    let mut quota_configs = model_configs
        .iter()
        .filter(|config| config.quota_info.is_some())
        .filter(|config| !model_label(config).is_empty())
        .collect::<Vec<_>>();
    quota_configs.sort_by(|a, b| compare_model_configs(a, b));

    let summary_candidates = quota_configs
        .iter()
        .copied()
        .filter(|config| !is_noisy_summary_model(model_label(config)))
        .collect::<Vec<_>>();

    let primary_config = best_summary_model(&summary_candidates, ModelFamily::Claude)
        .or_else(|| summary_candidates.first().copied())
        .or_else(|| quota_configs.first().copied());
    let secondary_config = best_summary_model(&summary_candidates, ModelFamily::GeminiPro);
    let tertiary_config = best_summary_model(&summary_candidates, ModelFamily::GeminiFlash);

    let primary = primary_config
        .and_then(|config| config.quota_info.as_ref())
        .map(rate_window_from_quota);
    let secondary = secondary_config
        .and_then(|config| config.quota_info.as_ref())
        .map(rate_window_from_quota);
    let tertiary = tertiary_config
        .and_then(|config| config.quota_info.as_ref())
        .map(rate_window_from_quota);

    let primary = primary.unwrap_or_else(|| RateWindow::new(0.0));
    let mut snapshot = UsageSnapshot::new(primary);

    if let Some(sec) = secondary {
        snapshot = snapshot.with_secondary(sec);
    }
    if let Some(ter) = tertiary {
        snapshot = snapshot.with_model_specific(ter);
    }

    // Upstream 0.50.1 #2963: selected configs occupy the canonical slots.
    // Exclude them by config identity. The response has no authoritative pool
    // identity, so retain every unselected config rather than merging equal
    // readings that may belong to distinct pools.
    let selected_configs = [primary_config, secondary_config, tertiary_config];
    for config in quota_configs {
        if selected_configs
            .iter()
            .flatten()
            .any(|selected| std::ptr::eq(*selected, config))
        {
            continue;
        }
        let Some(quota) = &config.quota_info else {
            continue;
        };
        let title = clean_model_label(model_label(config));
        if title.is_empty() {
            continue;
        }
        snapshot.extra_rate_windows.push(
            NamedRateWindow::new(
                model_window_id(config),
                title,
                rate_window_from_quota(quota),
            )
            .with_usage_known(quota.remaining_fraction.is_some()),
        );
    }

    if let Some(email) = first_non_empty([user_status.email.as_deref()]) {
        snapshot = snapshot.with_email(email);
    }

    // Add plan information from the authenticated user-status response.
    if let Some(plan) = plan_name {
        snapshot = snapshot.with_login_method(plan);
    }

    Ok(snapshot)
}
