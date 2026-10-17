//! Usage-item visibility descriptors for the provider detail pane.

use super::{ProviderUsageSnapshot, Settings};
use codexbar::core::{PersonalInfoRedactor, ProviderId};
use codexbar::locale::{self, LocaleKey};
use codexbar::settings::Language;
use serde::{Deserialize, Serialize};

/// Presentation descriptor for one quota metric or provider-emitted extra
/// usage row, a provider detail section, or a provider-owned `section:` card
/// block (Claude reset credits, Extra usage, plan dates). Other inventory
/// rows have no visibility choice.
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
    /// The row is also suppressed while Cost summary display is Hidden, so
    /// the pane explains why checking it may show nothing.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub requires_cost_summary: bool,
}

pub(crate) fn usage_item_id(raw_id: &str) -> String {
    format!("{}{}", codexbar::settings::USAGE_ITEM_METRIC_PREFIX, raw_id)
}

/// Provider-specific by design (upstream 0.75.0 #4484): Claude's saved reset
/// credits, Extra usage / prepaid balance and plan dates are card sections
/// with their own visibility choice. Returns the title and whether the row is
/// also gated by Cost summary display.
fn claude_section_title(id: &str, lang: Language) -> Option<(String, bool)> {
    use codexbar::settings::{
        CLAUDE_EXTRA_USAGE_USAGE_ITEM_ID, CLAUDE_RESET_CREDITS_USAGE_ITEM_ID,
        SUBSCRIPTION_USAGE_ITEM_ID,
    };
    let (key, requires_cost_summary) = match id {
        SUBSCRIPTION_USAGE_ITEM_ID => (LocaleKey::UsageItemPlanRenewalExpiry, false),
        CLAUDE_RESET_CREDITS_USAGE_ITEM_ID => (LocaleKey::ProviderLabelLimitResetCredits, false),
        CLAUDE_EXTRA_USAGE_USAGE_ITEM_ID => (LocaleKey::ExtraUsage, true),
        _ => return None,
    };
    Some((locale::get_text(lang, key), requires_cost_summary))
}

/// The Claude section IDs the current snapshot reports, in card order: plan
/// dates sit under the quota rows, then saved resets, then Extra usage.
fn claude_reported_sections(snapshot: &ProviderUsageSnapshot) -> Vec<&'static str> {
    use codexbar::settings::{
        CLAUDE_EXTRA_USAGE_USAGE_ITEM_ID, CLAUDE_RESET_CREDITS_USAGE_ITEM_ID,
        SUBSCRIPTION_USAGE_ITEM_ID,
    };
    let mut ids = Vec::new();
    if snapshot
        .subscription
        .as_ref()
        .is_some_and(|s| s.renews_at.is_some() || s.expires_at.is_some())
    {
        ids.push(SUBSCRIPTION_USAGE_ITEM_ID);
    }
    if snapshot
        .inventory
        .iter()
        .any(|item| item.id == CLAUDE_RESET_CREDITS_INVENTORY_ID)
    {
        ids.push(CLAUDE_RESET_CREDITS_USAGE_ITEM_ID);
    }
    if is_claude_extra_usage_cost(snapshot) {
        ids.push(CLAUDE_EXTRA_USAGE_USAGE_ITEM_ID);
    }
    ids
}

/// Inventory ID the Claude provider assigns to saved limit resets.
pub(crate) const CLAUDE_RESET_CREDITS_INVENTORY_ID: &str = "reset-credits";

/// Claude's Extra usage / prepaid balance block. Admin API spend is a
/// separate cost and keeps showing regardless of this choice.
pub(crate) fn is_claude_extra_usage_cost(snapshot: &ProviderUsageSnapshot) -> bool {
    snapshot
        .cost
        .as_ref()
        .is_some_and(|cost| !cost.always_visible)
        && snapshot.source_label != "admin-api"
}

fn redacted_usage_item_title(title: &str, settings: &Settings) -> String {
    PersonalInfoRedactor::hide_emails_in_text(Some(title), settings.hide_personal_info)
        .unwrap_or_default()
}

/// Title shown for a persisted row the current snapshot no longer emits.
/// The special cases cover the metric lanes and the two legacy extra rows;
/// everything else falls back to title-casing the raw ID suffix.
fn unavailable_usage_item_title(id: &str, lang: Language) -> String {
    let raw = id
        .strip_prefix(codexbar::settings::USAGE_ITEM_METRIC_PREFIX)
        .unwrap_or(id);
    let label = match raw {
        "extra-codex-spark" => "Codex Spark".to_string(),
        "extra-codex-spark-weekly" => "Codex Spark Weekly".to_string(),
        "extra-claude-routines" => "Daily Routines".to_string(),
        "primary" => locale::get_text(lang, LocaleKey::ProviderSessionLabel),
        "secondary" => locale::get_text(lang, LocaleKey::ProviderWeeklyLabel),
        "model-specific" => locale::get_text(lang, LocaleKey::DetailWindowModelSpecific),
        "tertiary" => locale::get_text(lang, LocaleKey::DetailWindowTertiary),
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
        locale::get_text(lang, LocaleKey::UsageItemFallbackTitle)
    } else {
        locale::format_locale(lang, LocaleKey::UsageItemUnavailableTitle, &[&label])
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
    let lang = settings.ui_language;
    let mut seen = std::collections::HashSet::new();
    let mut items = Vec::new();

    // Detail sections (upstream 0.62.0 #3638): one descriptor per distinct
    // display-detail title, hidden independently of the metric rows. Detail
    // IDs round-trip as-is (no metric prefix), so they bypass the `push`
    // helper below.
    if let Some(snapshot) = snapshot {
        let mut detail_titles = std::collections::HashSet::new();
        for detail in &snapshot.display_details {
            if detail_titles.insert(detail.title.clone()) {
                // The ID keeps the "Hidden" marker it has always used, so
                // sections hidden under Hide Personal Info stay hidden.
                let id_title = PersonalInfoRedactor::redact_emails_in_text(
                    Some(&detail.title),
                    settings.hide_personal_info,
                )
                .unwrap_or_default();
                let id = format!(
                    "{}{}",
                    codexbar::settings::USAGE_ITEM_DETAIL_SECTION_PREFIX,
                    id_title
                );
                if seen.insert(id.clone()) {
                    items.push(ProviderUsageItemSnapshot {
                        id,
                        title: redacted_usage_item_title(&detail.title, settings),
                        available: true,
                        requires_cost_summary: false,
                    });
                }
            }
        }
    }
    for id in &hidden {
        if !id.starts_with(codexbar::settings::USAGE_ITEM_DETAIL_SECTION_PREFIX) {
            continue;
        }
        // Placeholder for a hidden detail section the provider no longer
        // reports: the title is recovered from the stored ID.
        if seen.insert(id.clone()) {
            items.push(ProviderUsageItemSnapshot {
                id: id.clone(),
                title: redacted_usage_item_title(
                    id.strip_prefix(codexbar::settings::USAGE_ITEM_DETAIL_SECTION_PREFIX)
                        .unwrap_or(id.as_str()),
                    settings,
                ),
                available: false,
                requires_cost_summary: false,
            });
        }
    }

    // A card-hidden window is never drawn as a row, so neither a live toggle
    // nor a saved-ID placeholder for it would control anything. Marking it
    // seen suppresses both; windows absent from the snapshot keep placeholders.
    if let Some(snapshot) = snapshot {
        let windows = [
            ("primary", Some(&snapshot.primary)),
            ("secondary", snapshot.secondary.as_ref()),
            ("model-specific", snapshot.model_specific.as_ref()),
            ("tertiary", snapshot.tertiary.as_ref()),
        ];
        for (raw_id, window) in windows {
            if window.is_some_and(|w| w.hidden_on_card) {
                seen.insert(usage_item_id(raw_id));
            }
        }
        for extra in &snapshot.extra_rate_windows {
            if extra.window.hidden_on_card {
                seen.insert(usage_item_id(&format!("extra-{}", extra.id)));
            }
        }
    }

    let mut push = |raw_id: &str, title: &str, available: bool| {
        let id = usage_item_id(raw_id);
        if seen.insert(id.clone()) {
            let item_title = if available {
                redacted_usage_item_title(title, settings)
            } else {
                redacted_usage_item_title(&unavailable_usage_item_title(&id, lang), settings)
            };
            items.push(ProviderUsageItemSnapshot {
                id,
                title: item_title,
                available,
                requires_cost_summary: false,
            });
        }
    };

    if let Some(snapshot) = snapshot {
        let session = locale::get_text(lang, LocaleKey::ProviderSessionLabel);
        push(
            "primary",
            snapshot.primary_label.as_deref().unwrap_or(&session),
            true,
        );
        if snapshot.secondary.is_some() {
            let weekly = locale::get_text(lang, LocaleKey::ProviderWeeklyLabel);
            push(
                "secondary",
                snapshot.secondary_label.as_deref().unwrap_or(&weekly),
                true,
            );
        }
        if snapshot.model_specific.is_some() {
            let model = locale::get_text(lang, LocaleKey::DetailWindowModelSpecific);
            push("model-specific", &model, true);
        }
        if snapshot.tertiary.is_some() {
            let tertiary = locale::get_text(lang, LocaleKey::DetailWindowTertiary);
            push(
                "tertiary",
                snapshot.tertiary_label.as_deref().unwrap_or(&tertiary),
                true,
            );
        }
        for extra in &snapshot.extra_rate_windows {
            push(&format!("extra-{}", extra.id), &extra.title, true);
        }
    }

    for id in &hidden {
        if id.starts_with(codexbar::settings::USAGE_ITEM_DETAIL_SECTION_PREFIX)
            || id.starts_with(codexbar::settings::USAGE_ITEM_SECTION_PREFIX)
        {
            continue;
        }
        let raw_id = id
            .strip_prefix(codexbar::settings::USAGE_ITEM_METRIC_PREFIX)
            .unwrap_or(id.as_str());
        push(raw_id, "", false);
    }

    if provider_id == ProviderId::Claude {
        let reported = snapshot.map(claude_reported_sections).unwrap_or_default();
        let saved = hidden.iter().map(String::as_str).filter(|id| {
            id.starts_with(codexbar::settings::USAGE_ITEM_SECTION_PREFIX) && !reported.contains(id)
        });
        let rows = reported
            .iter()
            .map(|id| (*id, true))
            .chain(saved.map(|id| (id, false)))
            .collect::<Vec<_>>();
        for (id, available) in rows {
            let Some((title, requires_cost_summary)) = claude_section_title(id, lang) else {
                continue;
            };
            if seen.insert(id.to_string()) {
                items.push(ProviderUsageItemSnapshot {
                    id: id.to_string(),
                    title: if available {
                        title
                    } else {
                        locale::format_locale(lang, LocaleKey::UsageItemUnavailableTitle, &[&title])
                    },
                    available,
                    requires_cost_summary,
                });
            }
        }
    }

    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unavailable_titles_cover_metric_lanes_and_legacy_rows() {
        assert_eq!(
            unavailable_usage_item_title("metric:primary", Language::English),
            "Session (unavailable)"
        );
        assert_eq!(
            unavailable_usage_item_title("metric:extra-codex-spark", Language::English),
            "Codex Spark (unavailable)"
        );
        assert_eq!(
            unavailable_usage_item_title("metric:extra-claude-routines", Language::English),
            "Daily Routines (unavailable)"
        );
        assert_eq!(
            unavailable_usage_item_title("metric:extra-new-row", Language::English),
            "New Row (unavailable)"
        );
    }

    #[test]
    fn unavailable_titles_follow_the_ui_language() {
        assert_eq!(
            unavailable_usage_item_title("metric:secondary", Language::Russian),
            "Еженедельно (недоступно)"
        );
        assert_eq!(
            unavailable_usage_item_title("metric:", Language::Japanese),
            "使用量項目"
        );
    }
}
