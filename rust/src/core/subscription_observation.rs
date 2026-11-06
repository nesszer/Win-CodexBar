//! 提供方整理后的订阅观测数据，共享监控层只读取，不选择日期来源。

use chrono::{DateTime, Utc};

/// 日期来源用于区分实时接口与历史记录，不携带提供方或账号身份。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SubscriptionDateSource {
    ProviderApi,
    CredentialRecord,
}

/// 由提供方显式填充的临时观测；历史周期终点与确认到期日期分别保存。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SubscriptionObservation {
    pub starts_at: Option<DateTime<Utc>>,
    pub expires_at: Option<DateTime<Utc>>,
    pub renews_at: Option<DateTime<Utc>>,
    pub period_end: Option<DateTime<Utc>>,
    pub recorded_at: Option<DateTime<Utc>>,
    pub source: SubscriptionDateSource,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::{RateWindow, UsageSnapshot};

    #[test]
    fn subscription_observation_stays_out_of_serialized_snapshots() {
        // 临时观测不能经持久化或桌面桥接流出，也不能从外部 JSON 注入。
        let mut usage = UsageSnapshot::new(RateWindow::new(10.0));
        usage.updated_at = "2026-01-01T00:00:00Z".parse().unwrap();
        let at = "2026-11-06T13:04:47Z".parse().unwrap();
        usage.subscription_observation = Some(SubscriptionObservation {
            starts_at: None,
            expires_at: None,
            renews_at: None,
            period_end: Some(at),
            recorded_at: None,
            source: SubscriptionDateSource::CredentialRecord,
        });
        let mut json = serde_json::to_value(&usage).unwrap();
        assert!(json.get("subscription_observation").is_none());
        assert!(!json.to_string().contains("2026-11-06"));
        json["subscription_observation"] = serde_json::json!({
            "expires_at": "2026-11-06T13:04:47Z", "source": "ProviderApi"
        });
        let restored: UsageSnapshot = serde_json::from_value(json).unwrap();
        assert!(restored.subscription_observation.is_none());
    }
}
