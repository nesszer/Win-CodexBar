//! Usage-item visibility descriptors for the provider detail pane.

use super::{ProviderUsageSnapshot, Settings};
use codexbar::core::{PersonalInfoRedactor, ProviderId};
use serde::{Deserialize, Serialize};

/// Presentation descriptor for one quota metric or provider-emitted extra
/// usage row. This intentionally excludes inventory and transient detail
/// sections: Windows only exposes the metric rows already present in the
/// provider snapshot for this visibility lane.
///
/// Descriptor contract: every row the pane displays is either persisted in the
/// provider's hidden-usage-item list or emitted by the current provider
/// snapshot (displayed ⊆ persisted ∪ emitted). Rows the provider stopped
/// emitting stay as `available: false` placeholders so a hidden legacy row can
/// be restored without inventing a new provider detail section.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderUsageItemSnapshot {
    pub id: String,
    pub title: String,
    pub available: bool,
}

pub(crate) fn usage_item_id(raw_id: &str) -> String {
    format!("{}{}", codexbar::settings::USAGE_ITEM_METRIC_PREFIX, raw_id)
}

fn redacted_usage_item_title(title: &str, settings: &Settings) -> String {
    PersonalInfoRedactor::redact_emails_in_text(Some(title), settings.hide_personal_info)
        .unwrap_or_default()
}

/// Title shown for a persisted row the current snapshot no longer emits.
/// The special cases cover the metric lanes and the two legacy extra rows;
/// everything else falls back to title-casing the raw ID suffix.
fn unavailable_usage_item_title(id: &str) -> String {
    let raw = id
        .strip_prefix(codexbar::settings::USAGE_ITEM_METRIC_PREFIX)
        .unwrap_or(id);
    let label = match raw {
        "extra-codex-spark" => "Codex Spark".to_string(),
        "extra-codex-spark-weekly" => "Codex Spark Weekly".to_string(),
        "extra-claude-routines" => "Daily Routines".to_string(),
        "primary" => "Session".to_string(),
        "secondary" => "Weekly".to_string(),
        "model-specific" => "Model-specific".to_string(),
        "tertiary" => "Tertiary".to_string(),
        _ => raw
            .strip_prefix("extra-")
            .unwrap_or(raw)
            .split(['-', '_'])
            .filter(|part| !part.is_empty())
            .map(|part| {
                let mut chars = part.chars();
                match chars.next() {
                    Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                    None => String::new(),
                }
            })
            .collect::<Vec<_>>()
            .join(" "),
    };
    if label.is_empty() {
        "Usage item".to_string()
    } else {
        format!("{label} (unavailable)")
    }
}

/// Build visibility descriptors from the raw provider snapshot plus any
/// persisted hidden IDs that the provider no longer emits. The latter are
/// placeholders so a hidden legacy row can be restored without inventing a
/// new provider detail section.
pub(crate) fn usage_item_descriptors(
    snapshot: Option<&ProviderUsageSnapshot>,
    settings: &Settings,
    provider_id: ProviderId,
) -> Vec<ProviderUsageItemSnapshot> {
    let hidden = settings.hidden_usage_item_ids(provider_id);
    let mut seen = std::collections::HashSet::new();
    let mut items = Vec::new();

    let mut push = |raw_id: &str, title: &str, available: bool| {
        let id = usage_item_id(raw_id);
        if seen.insert(id.clone()) {
            let item_title = if available {
                redacted_usage_item_title(title, settings)
            } else {
                redacted_usage_item_title(&unavailable_usage_item_title(&id), settings)
            };
            items.push(ProviderUsageItemSnapshot {
                id,
                title: item_title,
                available,
            });
        }
    };

    if let Some(snapshot) = snapshot {
        push(
            "primary",
            snapshot.primary_label.as_deref().unwrap_or("Session"),
            true,
        );
        if snapshot.secondary.is_some() {
            push(
                "secondary",
                snapshot.secondary_label.as_deref().unwrap_or("Weekly"),
                true,
            );
        }
        if snapshot.model_specific.is_some() {
            push("model-specific", "Model-specific", true);
        }
        if snapshot.tertiary.is_some() {
            push(
                "tertiary",
                snapshot.tertiary_label.as_deref().unwrap_or("Tertiary"),
                true,
            );
        }
        for extra in &snapshot.extra_rate_windows {
            push(&format!("extra-{}", extra.id), &extra.title, true);
        }
    }

    for id in hidden {
        let raw_id = id
            .strip_prefix(codexbar::settings::USAGE_ITEM_METRIC_PREFIX)
            .unwrap_or(id.as_str());
        push(raw_id, "", false);
    }

    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_titles_cover_metric_lanes_and_legacy_rows() {
        assert_eq!(
            unavailable_usage_item_title("metric:primary"),
            "Session (unavailable)"
        );
        assert_eq!(
            unavailable_usage_item_title("metric:extra-codex-spark"),
            "Codex Spark (unavailable)"
        );
        assert_eq!(
            unavailable_usage_item_title("metric:extra-claude-routines"),
            "Daily Routines (unavailable)"
        );
        assert_eq!(
            unavailable_usage_item_title("metric:extra-new-row"),
            "New Row (unavailable)"
        );
    }
}
