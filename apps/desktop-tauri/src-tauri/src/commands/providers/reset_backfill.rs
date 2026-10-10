use crate::commands::bridge::{ProviderUsageSnapshot, RateWindowSnapshot};

/// F6 (upstream 0.48.0 UsageStore+CodexResetBackfill): backfill missing
/// `resets_at` / `reset_description` on fresh Codex windows from the cached
/// lane data when the cached reset is still future. z.ai five-hour cached
/// resets use the same plausibility bound as the provider parser, so an
/// impossible rejected reset cannot be restored from the cache. Fresh
/// `used_percent` is untouched; only reset metadata is backfilled.
///
/// This remains provider-scoped by design (upstream: "Provider-specific by
/// design"): only Codex and z.ai carry the relevant bounded reset semantics.
///
/// Applies to the bridge snapshot before publishing so every surface (tray,
/// CLI, frontend) sees the backfilled reset instead of a missing one.
pub(super) fn codex_reset_backfill(
    snapshot: &mut ProviderUsageSnapshot,
    cached: Option<&ProviderUsageSnapshot>,
) {
    let Some(cached) = cached else { return };
    if !matches!(snapshot.provider_id.as_str(), "codex" | "zai") {
        return;
    }
    // A subscription change starts a new quota baseline: reset times from the
    // previous plan are not evidence for the new one. Unknown plans never block.
    if snapshot.provider_id == "codex" && codex_plan_changed(cached, snapshot) {
        return;
    }

    // Backfill each slot from the corresponding cached slot.
    backfill_slot_window(
        &snapshot.provider_id,
        &mut snapshot.primary,
        &cached.primary,
    );
    if let (Some(fresh), Some(cached_sec)) = (&mut snapshot.secondary, &cached.secondary) {
        backfill_slot_window(&snapshot.provider_id, fresh, cached_sec);
    }
    // Codex rarely populates tertiary windows, but keep this forward-compatible.
    if let (Some(fresh), Some(cached_ter)) = (&mut snapshot.tertiary, &cached.tertiary) {
        backfill_slot_window(&snapshot.provider_id, fresh, cached_ter);
    }
}

/// True only when both snapshots report a known plan and the plans differ.
fn codex_plan_changed(cached: &ProviderUsageSnapshot, fresh: &ProviderUsageSnapshot) -> bool {
    let normalize = |plan: &Option<String>| {
        plan.as_deref()
            .map(str::trim)
            .filter(|plan| !plan.is_empty())
            .map(str::to_lowercase)
    };
    matches!(
        (normalize(&cached.plan_name), normalize(&fresh.plan_name)),
        (Some(cached), Some(fresh)) if cached != fresh
    )
}

/// Backfill `resets_at` and `reset_description` on a fresh window from the
/// cached window whose reset is still in the future. `used_percent` is never
/// overwritten (upstream: "fresh used_percent untouched").
fn backfill_slot_window(
    provider_id: &str,
    fresh: &mut RateWindowSnapshot,
    cached: &RateWindowSnapshot,
) {
    if fresh.resets_at.is_some() {
        return;
    }
    let Some(cached_reset) = &cached.resets_at else {
        return;
    };
    // A stale reset is worse than a missing one.
    let Ok(cached_dt) = chrono::DateTime::parse_from_rfc3339(cached_reset) else {
        return;
    };
    let now = chrono::Utc::now();
    if cached_dt <= now {
        return;
    }
    // A missing z.ai reset can represent a rejected timestamp; do not restore
    // equally implausible cached evidence.
    if provider_id == "zai"
        && fresh.window_minutes == Some(300)
        && cached_dt > now + chrono::Duration::minutes(5 * 60 + 1)
    {
        return;
    }
    fresh.resets_at = Some(cached_reset.clone());
    fresh.reset_description = fresh
        .reset_description
        .clone()
        .or_else(|| cached.reset_description.clone());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::bridge::RateWindowSnapshot;

    fn win(used: f64, resets_at: Option<&str>) -> RateWindowSnapshot {
        RateWindowSnapshot {
            used_percent: used,
            remaining_percent: 100.0 - used,
            window_minutes: Some(300),
            resets_at: resets_at.map(String::from),
            reset_description: None,
            is_exhausted: false,
            is_informational: false,
            reserve_percent: None,
            reserve_description: None,
            reserve_will_last_to_reset: false,
            reserve_eta_seconds: None,
            pace: None,
            monthly_limit_block: None,
            description_is_detail: false,
        }
    }

    fn codex_snapshot(primary: RateWindowSnapshot) -> ProviderUsageSnapshot {
        ProviderUsageSnapshot {
            provider_id: "codex".into(),
            display_name: "Codex".into(),
            primary,
            primary_label: None,
            secondary: None,
            secondary_label: None,
            model_specific: None,
            tertiary: None,
            tertiary_label: None,
            extra_rate_windows: Vec::new(),
            inventory: Vec::new(),
            display_details: Vec::new(),
            cost: None,
            plan_name: None,
            account_email: None,
            subscription: None,
            source_label: String::new(),
            has_successful_claude_cli_quota: false,
            updated_at: "2026-01-01T00:00:00Z".into(),
            error: None,
            error_state: codexbar::core::ProviderStateKind::Ready,
            pace: None,
            account_organization: None,
            tray_status_label: None,
            fetch_duration_ms: None,
            wayfinder_usage: None,
            quota_burndown: None,
            open_ai_api_usage: None,
            session_equivalent_forecast: None,
        }
    }

    #[test]
    fn f6_backfills_future_cached_reset() {
        let future = (chrono::Utc::now() + chrono::Duration::hours(2)).to_rfc3339();
        let cached = codex_snapshot(win(50.0, Some(&future)));
        let mut fresh = codex_snapshot(win(30.0, None));
        codex_reset_backfill(&mut fresh, Some(&cached));
        assert_eq!(fresh.primary.resets_at.as_deref(), Some(future.as_str()));
        assert!((fresh.primary.used_percent - 30.0).abs() < f64::EPSILON);
    }

    #[test]
    fn f6_does_not_backfill_stale_cached_reset() {
        let past = (chrono::Utc::now() - chrono::Duration::hours(2)).to_rfc3339();
        let cached = codex_snapshot(win(50.0, Some(&past)));
        let mut fresh = codex_snapshot(win(30.0, None));
        codex_reset_backfill(&mut fresh, Some(&cached));
        assert!(
            fresh.primary.resets_at.is_none(),
            "stale reset not backfilled"
        );
    }

    #[test]
    fn f6_does_not_overwrite_existing_resets_at() {
        let future1 = (chrono::Utc::now() + chrono::Duration::hours(3)).to_rfc3339();
        let future2 = (chrono::Utc::now() + chrono::Duration::hours(5)).to_rfc3339();
        let cached = codex_snapshot(win(50.0, Some(&future2)));
        let mut fresh = codex_snapshot(win(30.0, Some(&future1)));
        codex_reset_backfill(&mut fresh, Some(&cached));
        assert_eq!(fresh.primary.resets_at.as_deref(), Some(future1.as_str()));
    }

    #[test]
    fn codex_backfill_skips_when_known_plans_differ() {
        let future = (chrono::Utc::now() + chrono::Duration::hours(2)).to_rfc3339();
        let mut cached = codex_snapshot(win(80.0, Some(&future)));
        cached.plan_name = Some("Plus".into());
        let mut fresh = codex_snapshot(win(5.0, None));
        fresh.plan_name = Some("Pro".into());
        codex_reset_backfill(&mut fresh, Some(&cached));
        assert!(fresh.primary.resets_at.is_none(), "plan change baseline");
    }

    #[test]
    fn codex_backfill_keeps_baseline_for_same_or_unknown_plan() {
        let future = (chrono::Utc::now() + chrono::Duration::hours(2)).to_rfc3339();
        for (cached_plan, fresh_plan) in [
            (Some("Plus"), Some(" plus ")),
            (Some("Plus"), None),
            (None, Some("Pro")),
            (Some("Plus"), Some("  ")),
        ] {
            let mut cached = codex_snapshot(win(80.0, Some(&future)));
            cached.plan_name = cached_plan.map(str::to_string);
            let mut fresh = codex_snapshot(win(5.0, None));
            fresh.plan_name = fresh_plan.map(str::to_string);
            codex_reset_backfill(&mut fresh, Some(&cached));
            assert_eq!(
                fresh.primary.resets_at.as_deref(),
                Some(future.as_str()),
                "{cached_plan:?} -> {fresh_plan:?}"
            );
        }
    }

    #[test]
    fn f6_skips_non_codex_provider() {
        let future = (chrono::Utc::now() + chrono::Duration::hours(2)).to_rfc3339();
        let mut cached = codex_snapshot(win(50.0, Some(&future)));
        cached.provider_id = "claude".into();
        let mut fresh = codex_snapshot(win(30.0, None));
        fresh.provider_id = "claude".into();
        codex_reset_backfill(&mut fresh, Some(&cached));
        assert!(fresh.primary.resets_at.is_none(), "non-codex skip");
    }

    #[test]
    fn zai_five_hour_backfill_rejects_impossible_cached_reset() {
        for (offset, should_backfill) in [
            (chrono::Duration::hours(1), true),
            (chrono::Duration::hours(10), false),
        ] {
            let future = (chrono::Utc::now() + offset).to_rfc3339();
            let mut cached = codex_snapshot(win(50.0, Some(&future)));
            cached.provider_id = "zai".into();
            let mut fresh = codex_snapshot(win(30.0, None));
            fresh.provider_id = "zai".into();
            fresh.primary.reset_description = Some("5-hour".into());
            codex_reset_backfill(&mut fresh, Some(&cached));
            assert_eq!(fresh.primary.resets_at.is_some(), should_backfill);
            assert!((fresh.primary.used_percent - 30.0).abs() < f64::EPSILON);
        }
    }

    #[test]
    fn f6_skips_when_no_cached_snapshot() {
        let mut fresh = codex_snapshot(win(30.0, None));
        codex_reset_backfill(&mut fresh, None);
        assert!(fresh.primary.resets_at.is_none());
    }
}
