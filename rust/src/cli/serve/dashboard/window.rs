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
}

fn is_false(value: &bool) -> bool {
    !*value
}

pub(super) fn make_window_with_idle(
    kind: &str,
    label: &str,
    window: &RateWindow,
    idle: bool,
) -> WindowPayload {
    let used = window.used_percent.clamp(0.0, 100.0);
    WindowPayload {
        kind: kind.to_string(),
        label: label.to_string(),
        used_percent: used,
        remaining_percent: (100.0 - used).clamp(0.0, 100.0),
        reset_at: window.resets_at,
        idle,
    }
}
