//! Stable metric names and help text for the Prometheus exposition.

pub(in crate::cli::serve::metrics) const METRIC_DEFINITIONS: &[(&str, &str)] = &[
    (
        "codexbar_up",
        "Whether CodexBar has a successfully collected metrics snapshot to export.",
    ),
    (
        "codexbar_snapshot_schema_version",
        "Dashboard snapshot schema version used for collection.",
    ),
    (
        "codexbar_snapshot_generated_timestamp_seconds",
        "Unix timestamp when the collection snapshot was generated.",
    ),
    (
        "codexbar_snapshot_age_seconds",
        "Age of the collection snapshot.",
    ),
    (
        "codexbar_snapshot_stale_after_seconds",
        "Age after which the collection snapshot is stale.",
    ),
    (
        "codexbar_snapshot_stale",
        "Whether the collection snapshot is older than its stale threshold.",
    ),
    (
        "codexbar_refresh_interval_seconds",
        "Configured dashboard refresh interval.",
    ),
    (
        "codexbar_provider_up",
        "Whether the provider usage fetch succeeded.",
    ),
    (
        "codexbar_provider_updated_timestamp_seconds",
        "Unix timestamp of the provider usage data update.",
    ),
    (
        "codexbar_provider_data_age_seconds",
        "Age of the provider usage data.",
    ),
    (
        "codexbar_quota_session_used_ratio",
        "Used quota ratio for the Codex session window.",
    ),
    (
        "codexbar_quota_session_remaining_ratio",
        "Remaining quota ratio for the Codex session window.",
    ),
    (
        "codexbar_quota_session_reset_timestamp_seconds",
        "Unix timestamp when the Codex session window resets.",
    ),
    (
        "codexbar_quota_weekly_used_ratio",
        "Used quota ratio for the Codex weekly window.",
    ),
    (
        "codexbar_quota_weekly_remaining_ratio",
        "Remaining quota ratio for the Codex weekly window.",
    ),
    (
        "codexbar_quota_weekly_reset_timestamp_seconds",
        "Unix timestamp when the Codex weekly window resets.",
    ),
    (
        "codexbar_quota_monthly_used_ratio",
        "Used quota ratio for the Codex monthly window.",
    ),
    (
        "codexbar_quota_monthly_remaining_ratio",
        "Remaining quota ratio for the Codex monthly window.",
    ),
    (
        "codexbar_quota_monthly_reset_timestamp_seconds",
        "Unix timestamp when the Codex monthly window resets.",
    ),
    (
        "codexbar_quota_code_review_used_ratio",
        "Used quota ratio for the Codex code review window.",
    ),
    (
        "codexbar_quota_code_review_remaining_ratio",
        "Remaining quota ratio for the Codex code review window.",
    ),
    (
        "codexbar_quota_code_review_reset_timestamp_seconds",
        "Unix timestamp when the Codex code review window resets.",
    ),
    (
        "codexbar_cost_today_usd",
        "Available local Codex cost today in US dollars.",
    ),
    (
        "codexbar_reset_credits_available",
        "Available Codex reset credits; -1 means unavailable or unsupported.",
    ),
    (
        "codexbar_reset_credits_next_expiry_timestamp_seconds",
        "Unix timestamp of the earliest expiry among available Codex reset credits.",
    ),
    // 订阅日期与重置卡有效期、额度重置时间分别建模，缺失时不输出样本。
    (
        "codexbar_subscription_start_timestamp_seconds",
        "Codex 明确报告的订阅开始时间；来源由 dates_from_id_token 指标区分。",
    ),
    (
        "codexbar_subscription_expiry_timestamp_seconds",
        "Codex 订阅到期时间的 Unix 秒时间戳，包含已过期的日期。",
    ),
    (
        "codexbar_subscription_period_end_timestamp_seconds",
        "Codex 登录凭据记录的订阅周期结束时间，不代表已确认取消后的到期时间。",
    ),
    (
        "codexbar_subscription_dates_recorded_timestamp_seconds",
        "Codex 订阅周期声明记录时间（id_token iat），不是本次监控采集时间。",
    ),
    (
        "codexbar_subscription_dates_from_id_token",
        "Codex 订阅日期来源：1 为登录凭据记录，0 为订阅接口；无日期时省略。",
    ),
    (
        "codexbar_cost_last_30_days_usd",
        "Available local Codex cost over the last 30 days in US dollars.",
    ),
];
