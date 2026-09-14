use std::collections::{HashMap, HashSet};

use crate::core::{NamedRateWindow, UsageSnapshot};

use super::window::{WindowPayload, make_window_with_idle};

/// Project the provider-owned quota-summary layout into dashboard rows.
///
/// The parser removes the selected representatives from `extra_rate_windows`,
/// so every core and extra row is emitted exactly once. Row identity comes from
/// the provider's selected slots and stable bucket ids; quota measurements are
/// never used as an identity key.
pub(super) fn quota_summary_windows(
    usage: &UsageSnapshot,
    session_label: &str,
    weekly_label: &str,
) -> Vec<WindowPayload> {
    let active_core_families = active_core_families(usage, session_label, weekly_label);
    let idle_ids = idle_window_ids(&usage.extra_rate_windows, &active_core_families);
    let mut windows = Vec::with_capacity(4 + usage.extra_rate_windows.len());
    windows.push(make_window_with_idle(
        "session",
        usage.primary_label.as_deref().unwrap_or(session_label),
        &usage.primary,
        false,
    ));
    if let Some(secondary) = &usage.secondary {
        windows.push(make_window_with_idle(
            "weekly",
            usage.secondary_label.as_deref().unwrap_or(weekly_label),
            secondary,
            false,
        ));
    }
    if let Some(model) = &usage.model_specific {
        windows.push(make_window_with_idle("model", "Model", model, false));
    }
    if let Some(tertiary) = &usage.tertiary {
        windows.push(make_window_with_idle(
            "tertiary", "Tertiary", tertiary, false,
        ));
    }
    windows.extend(usage.extra_rate_windows.iter().map(|extra| {
        make_window_with_idle(
            &extra.id,
            &extra.title,
            &extra.window,
            idle_ids.contains(&extra.id),
        )
    }));
    windows
}

/// Identify model-family lanes that are known untouched. Unknown-zero lanes stay
/// visible, and an all-zero global reset keeps every family visible when no
/// selected core family is active.
pub(super) fn idle_window_ids(
    windows: &[NamedRateWindow],
    active_core_families: &HashSet<String>,
) -> HashSet<String> {
    if windows.is_empty() {
        return HashSet::new();
    }
    let mut families: HashMap<String, Vec<&NamedRateWindow>> = HashMap::new();
    for window in windows {
        families.entry(family_key(window)).or_default().push(window);
    }
    let idle_families: Vec<_> = families
        .iter()
        .filter(|(family, lanes)| {
            !active_core_families.contains(*family)
                && lanes
                    .iter()
                    .all(|lane| lane.usage_known && lane.window.used_percent <= 0.0)
        })
        .map(|(family, _)| family.clone())
        .collect();
    if idle_families.len() == families.len() && active_core_families.is_empty() {
        return HashSet::new();
    }
    idle_families
        .into_iter()
        .flat_map(|family| {
            families
                .get(&family)
                .into_iter()
                .flatten()
                .map(|lane| lane.id.clone())
                .collect::<Vec<_>>()
        })
        .collect()
}

fn active_core_families(
    usage: &UsageSnapshot,
    session_label: &str,
    weekly_label: &str,
) -> HashSet<String> {
    let mut active = HashSet::new();
    let mut add = |used_percent: f64, is_informational: bool, label: &str| {
        if !is_informational && used_percent > 0.0 {
            active.insert(family_key_parts("", label));
        }
    };
    add(
        usage.primary.used_percent,
        usage.primary.is_informational,
        usage.primary_label.as_deref().unwrap_or(session_label),
    );
    if let Some(secondary) = &usage.secondary {
        add(
            secondary.used_percent,
            secondary.is_informational,
            usage.secondary_label.as_deref().unwrap_or(weekly_label),
        );
    }
    if let Some(tertiary) = &usage.tertiary {
        add(tertiary.used_percent, tertiary.is_informational, "Tertiary");
    }
    active
}

fn family_key(window: &NamedRateWindow) -> String {
    family_key_parts(&window.id, &window.title)
}

fn family_key_parts(id: &str, title: &str) -> String {
    let id = id.to_ascii_lowercase();
    if id.contains("gemini") {
        return "gemini".to_string();
    }
    if id.contains("3p") || id.contains("third-party") {
        return "claude-gpt".to_string();
    }
    let title = title.trim().to_ascii_lowercase();
    if title.contains("gemini") {
        return "gemini".to_string();
    }
    if title.contains("claude") || title.contains("gpt") {
        return "claude-gpt".to_string();
    }
    for suffix in [" 5-hour", " weekly"] {
        if let Some(stripped) = title.strip_suffix(suffix) {
            let stripped = stripped.trim();
            if !stripped.is_empty() {
                return stripped.to_string();
            }
        }
    }
    title
}
