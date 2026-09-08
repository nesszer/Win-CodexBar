use chrono::{DateTime, TimeDelta, Utc};
use std::collections::HashMap;

const CAPACITY: usize = 4;
const MAX_AGE: TimeDelta = TimeDelta::hours(24);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CachedPlan {
    pub(super) plan_id: String,
    pub(super) period_end: DateTime<Utc>,
    pub(super) stored_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
struct Observation {
    entry: Option<CachedPlan>,
    observed_at: DateTime<Utc>,
}

#[derive(Default)]
pub(super) struct CommandCodePlanCache {
    observations: HashMap<String, Observation>,
}

impl CommandCodePlanCache {
    pub(super) fn store(
        &mut self,
        plan_id: &str,
        period_end: Option<DateTime<Utc>>,
        fingerprint: &str,
        now: DateTime<Utc>,
    ) {
        let entry = period_end
            .filter(|period_end| *period_end > now)
            .map(|period_end| CachedPlan {
                plan_id: plan_id.to_owned(),
                period_end,
                stored_at: now,
            });
        self.record(fingerprint, entry, now);
    }

    pub(super) fn clear(&mut self, fingerprint: &str, now: DateTime<Utc>) {
        self.record(fingerprint, None, now);
    }

    pub(super) fn entry(&self, fingerprint: &str, now: DateTime<Utc>) -> Option<CachedPlan> {
        let observation = self.observations.get(fingerprint)?;
        let entry = observation.entry.as_ref()?;
        if now < entry.stored_at || entry.period_end <= now || now - entry.stored_at >= MAX_AGE {
            return None;
        }
        Some(entry.clone())
    }

    fn record(&mut self, fingerprint: &str, entry: Option<CachedPlan>, now: DateTime<Utc>) {
        if self
            .observations
            .get(fingerprint)
            .is_some_and(|observation| observation.observed_at > now)
        {
            return;
        }

        self.observations.insert(
            fingerprint.to_owned(),
            Observation {
                entry,
                observed_at: now,
            },
        );
        self.observations
            .retain(|_, observation| now - observation.observed_at < MAX_AGE);

        while self.observations.len() > CAPACITY {
            let oldest = self
                .observations
                .iter()
                .min_by_key(|(_, observation)| observation.observed_at)
                .map(|(fingerprint, _)| fingerprint.clone());
            if let Some(fingerprint) = oldest {
                self.observations.remove(&fingerprint);
            } else {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn timestamp(seconds: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(seconds, 0).expect("valid timestamp")
    }

    #[test]
    fn period_exact_boundary_is_not_stored() {
        let now = timestamp(1_000);
        let mut cache = CommandCodePlanCache::default();

        cache.store("plan", Some(now), "fingerprint", now);

        assert_eq!(cache.entry("fingerprint", now), None);
    }

    #[test]
    fn exact_24_hour_boundary_is_expired() {
        let now = timestamp(1_000);
        let mut cache = CommandCodePlanCache::default();

        cache.store("plan", Some(now + TimeDelta::hours(1)), "fingerprint", now);

        assert_eq!(cache.entry("fingerprint", now + MAX_AGE), None);
    }

    #[test]
    fn backdated_read_is_rejected() {
        let stored_at = timestamp(1_000);
        let mut cache = CommandCodePlanCache::default();
        cache.store(
            "plan",
            Some(stored_at + TimeDelta::hours(1)),
            "fingerprint",
            stored_at,
        );

        assert_eq!(
            cache.entry("fingerprint", stored_at - TimeDelta::seconds(1)),
            None
        );
    }

    #[test]
    fn older_store_cannot_overwrite_newer() {
        let newer = timestamp(2_000);
        let mut cache = CommandCodePlanCache::default();
        cache.store(
            "new",
            Some(newer + TimeDelta::hours(1)),
            "fingerprint",
            newer,
        );
        cache.store(
            "old",
            Some(newer + TimeDelta::hours(2)),
            "fingerprint",
            newer - TimeDelta::seconds(1),
        );

        assert_eq!(
            cache.entry("fingerprint", newer),
            Some(CachedPlan {
                plan_id: "new".to_owned(),
                period_end: newer + TimeDelta::hours(1),
                stored_at: newer,
            })
        );
    }

    #[test]
    fn older_clear_cannot_clear_newer() {
        let newer = timestamp(3_000);
        let mut cache = CommandCodePlanCache::default();
        cache.store(
            "plan",
            Some(newer + TimeDelta::hours(1)),
            "fingerprint",
            newer,
        );
        cache.clear("fingerprint", newer - TimeDelta::seconds(1));

        assert!(cache.entry("fingerprint", newer).is_some());
    }

    #[test]
    fn newer_clear_blocks_older_resurrection() {
        let now = timestamp(4_000);
        let mut cache = CommandCodePlanCache::default();
        cache.clear("fingerprint", now);
        cache.store(
            "plan",
            Some(now + TimeDelta::hours(1)),
            "fingerprint",
            now - TimeDelta::seconds(1),
        );

        assert_eq!(cache.entry("fingerprint", now), None);
    }

    #[test]
    fn fingerprints_are_isolated() {
        let now = timestamp(5_000);
        let mut cache = CommandCodePlanCache::default();
        cache.store(
            "plan-a",
            Some(now + TimeDelta::hours(1)),
            "fingerprint-a",
            now,
        );
        cache.store(
            "plan-b",
            Some(now + TimeDelta::hours(2)),
            "fingerprint-b",
            now,
        );

        assert_eq!(cache.entry("fingerprint-a", now).unwrap().plan_id, "plan-a");
        assert_eq!(cache.entry("fingerprint-b", now).unwrap().plan_id, "plan-b");
    }

    #[test]
    fn capacity_eviction_includes_clear_observations() {
        let now = timestamp(6_000);
        let mut cache = CommandCodePlanCache::default();
        cache.clear("clear", now);
        for index in 0..4 {
            cache.store(
                &format!("plan-{index}"),
                Some(now + TimeDelta::hours(1)),
                &format!("fingerprint-{index}"),
                now + TimeDelta::seconds(index + 1),
            );
        }

        assert_eq!(cache.observations.len(), CAPACITY);
        assert!(!cache.observations.contains_key("clear"));
    }
}
