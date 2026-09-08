use chrono::{DateTime, Utc};

const ZAI_FIVE_HOUR_WINDOW_MINUTES: u32 = 300;

/// Five-hour Coding Plan resets cannot be more than five hours away, plus one
/// minute for clock skew. Past resets remain valid because the API may report
/// a boundary that has just elapsed.
pub(super) fn is_plausible_five_hour_reset(
    window_minutes: Option<u32>,
    reset: DateTime<Utc>,
    now: DateTime<Utc>,
) -> bool {
    window_minutes != Some(ZAI_FIVE_HOUR_WINDOW_MINUTES)
        || reset <= now + chrono::Duration::minutes(5 * 60 + 1)
}
