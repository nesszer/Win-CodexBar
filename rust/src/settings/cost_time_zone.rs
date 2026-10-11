//! Pinned zone for local cost history (upstream 0.67.0
//! `tokenCostUsageBucketTimeZone`).
//!
//! Day buckets, month-to-date bounds, and cost caches use one saved IANA zone
//! so history keeps its day boundaries when the machine's zone changes. An
//! empty value means the machine zone. There is no settings control for it,
//! as upstream. The saved zone is stored but not yet applied at runtime:
//! nothing in the desktop app or the CLI pins it on first launch or applies it
//! to bucketing (the wiring PRs #653 and #665 were closed), so history is
//! bucketed in the machine zone.

use super::Settings;
use crate::cost_reporting_period::CostTimeZone;

/// Trim a saved bucket zone; anything but an IANA zone name reads as unpinned
/// (`""`).
pub fn normalize_cost_usage_bucket_time_zone(value: &str) -> String {
    let trimmed = value.trim();
    if CostTimeZone::is_valid_identifier(trimmed) {
        trimmed.to_string()
    } else {
        String::new()
    }
}

impl Settings {
    /// The zone local cost history is bucketed in: the pinned zone, or the
    /// machine zone while nothing valid is pinned.
    pub fn cost_usage_bucket_zone(&self) -> CostTimeZone {
        CostTimeZone::from_identifier(&self.cost_usage_bucket_time_zone)
    }

    /// Pin the machine's current zone unless a valid zone is already saved.
    ///
    /// Returns whether the setting changed, so callers know to save it. Leaves
    /// the setting unpinned when the machine zone cannot be read safely.
    pub fn pin_cost_usage_bucket_time_zone(&mut self) -> bool {
        if CostTimeZone::is_valid_identifier(self.cost_usage_bucket_time_zone.trim()) {
            return false;
        }
        match CostTimeZone::pin_identifier() {
            Some(zone) => {
                self.cost_usage_bucket_time_zone = zone;
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_zones_are_trimmed_and_unknown_zones_read_as_unpinned() {
        assert_eq!(
            normalize_cost_usage_bucket_time_zone("  Asia/Tokyo "),
            "Asia/Tokyo"
        );
        assert_eq!(normalize_cost_usage_bucket_time_zone("Mars/Olympus"), "");
        assert_eq!(normalize_cost_usage_bucket_time_zone(""), "");
    }

    #[test]
    fn settings_files_normalize_the_saved_zone() {
        let read = |json: &str| serde_json::from_str::<Settings>(json).expect("settings");
        assert_eq!(read("{}").cost_usage_bucket_time_zone, "");
        let pinned = read(r#"{"cost_usage_bucket_time_zone":" America/Los_Angeles "}"#);
        assert_eq!(pinned.cost_usage_bucket_time_zone, "America/Los_Angeles");
        assert_eq!(
            pinned.cost_usage_bucket_zone(),
            CostTimeZone::Named(chrono_tz::America::Los_Angeles)
        );
        let invalid = read(r#"{"cost_usage_bucket_time_zone":"Mars/Olympus"}"#);
        assert_eq!(invalid.cost_usage_bucket_time_zone, "");
        assert_eq!(invalid.cost_usage_bucket_zone(), CostTimeZone::Local);

        let saved = serde_json::to_value(&pinned).expect("serialize");
        assert_eq!(saved["cost_usage_bucket_time_zone"], "America/Los_Angeles");
    }

    #[test]
    fn pinning_keeps_a_saved_zone_and_fills_an_empty_one() {
        let mut settings = Settings {
            cost_usage_bucket_time_zone: "Asia/Tokyo".to_string(),
            ..Settings::default()
        };
        assert!(!settings.pin_cost_usage_bucket_time_zone());
        assert_eq!(settings.cost_usage_bucket_time_zone, "Asia/Tokyo");

        for unpinned in ["", "Mars/Olympus"] {
            let mut settings = Settings {
                cost_usage_bucket_time_zone: unpinned.to_string(),
                ..Settings::default()
            };
            let machine = CostTimeZone::pin_identifier();
            assert_eq!(
                settings.pin_cost_usage_bucket_time_zone(),
                machine.is_some()
            );
            assert_eq!(
                settings.cost_usage_bucket_time_zone,
                machine.unwrap_or_else(|| unpinned.to_string())
            );
        }
    }
}
