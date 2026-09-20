use serde::Serialize;

use crate::core::RateWindow;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowPayload {
    pub kind: String,
    pub label: String,
    pub used_percent: f64,
    pub remaining_percent: f64,
    pub reset_at: Option<chrono::DateTime<chrono::Utc>>,
    /// Display-only hint. Script clients can ignore this additive schema-v1 key.
    #[serde(skip_serializing_if = "is_false")]
    pub idle: bool,
    /// Whether the provider supplied a quota percentage. A false value keeps
    /// an informational or missing lane from being mistaken for numeric zero.
    #[serde(rename = "usageKnown", skip_serializing_if = "is_true")]
    pub usage_known: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn is_true(value: &bool) -> bool {
    *value
}

pub(super) fn make_window_with_idle(
    kind: &str,
    label: &str,
    window: &RateWindow,
    idle: bool,
) -> WindowPayload {
    make_window_with_known(kind, label, window, idle, window.usage_known())
}

pub(super) fn make_window_with_known(
    kind: &str,
    label: &str,
    window: &RateWindow,
    idle: bool,
    usage_known: bool,
) -> WindowPayload {
    let used = window.used_percent.clamp(0.0, 100.0);
    WindowPayload {
        kind: kind.to_string(),
        label: label.to_string(),
        used_percent: used,
        remaining_percent: (100.0 - used).clamp(0.0, 100.0),
        reset_at: window.resets_at,
        idle,
        usage_known,
    }
}
