//! Hook transition detector (upstream #2536 / `HookTransitionDetector`).
//!
//! Turns successive provider observations into edge-triggered hook events.
//! Platform-neutral and side-effect free: decides *what fired* and never
//! fetches or runs commands. State is in-memory only — a restart starts fresh
//! and the first sample of any lane establishes a baseline without firing.
//!
//! # Rules implemented
//!
//! - **Baseline-only first sample**: first reading of a lane/status never fires.
//! - **quota_low**: fires only when usage fraction crosses a watched threshold
//!   upward (`previous < t && current >= t`). Rules with an explicit `threshold`
//!   watch only that value; rules without one use the lane's
//!   `fallback_thresholds` (provider notification thresholds as used fractions).
//! - **quota_low rule narrowing**: only rules whose own threshold crossed this
//!   poll are attached to the dispatch (avoids re-firing lower thresholds).
//! - **quota_reached**: session lane only; fires on upward edge into
//!   `reached_threshold` (default 1.0). Weekly lanes never fire `quota_reached`.
//! - **quota_reset**: fires when the reset boundary advances
//!   (`current_resets_at > previous_resets_at`) **or** usage drops by at least
//!   `reset_drop_threshold` (default 0.2). A reset suppresses depletion edges
//!   (`quota_low` / `quota_reached`) in the same poll.
//! - **provider_unavailable / provider_recovered**: edge on definite outage
//!   state (`minor`/`major`/`critical` ↔ `none`). `maintenance` and `unknown`
//!   never flip tracked state.
//! - **refresh_failed**: emits a coarse failure status without disturbing quota
//!   or status baselines.
//! - **Lane lifecycle**: synthetic/informational or missing lanes forget their
//!   baseline; lanes that disappear between polls are pruned so reappearance
//!   starts fresh.
//! - **Config revision**: `reset_if_configuration_changed` clears all baselines
//!   so rule edits do not fire for crossings that spanned the change.
//! - **Disabled / over-capacity config**: `enabled == false` or more than
//!   `HooksConfig::MAX_RULES` rules → no events.

use chrono::{DateTime, Utc};
use std::collections::{HashMap, HashSet};

use super::hooks::{HookEvent, HookEventType, HookRule, HooksConfig, spawn_hook_dispatch};
use super::rate_window::RateWindow;
use super::usage_snapshot::UsageSnapshot;

/// Quota-window view consumed by the canonical `usage_updated` builder.
///
/// Carries exactly the payload-relevant fields of [`RateWindow`] so both the
/// core `UsageSnapshot` and shell-side bridge snapshots feed one builder.
#[derive(Debug, Clone)]
pub struct HookUsageWindow {
    pub used_percent: f64,
    pub window_minutes: Option<u32>,
    pub resets_at: Option<String>,
    pub is_informational: bool,
}

impl From<&RateWindow> for HookUsageWindow {
    fn from(window: &RateWindow) -> Self {
        Self {
            used_percent: window.used_percent,
            window_minutes: window.window_minutes,
            resets_at: window.resets_at.map(|reset| reset.to_rfc3339()),
            is_informational: window.is_informational,
        }
    }
}

/// Identifies one quota lane for hook transition tracking.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HookQuotaLaneKey {
    pub provider: String,
    pub window: HookQuotaWindow,
    pub account_discriminator: Option<String>,
    pub window_id: Option<String>,
}

impl HookQuotaLaneKey {
    pub fn new(
        provider: impl Into<String>,
        window: HookQuotaWindow,
        account_discriminator: Option<String>,
        window_id: Option<String>,
    ) -> Self {
        Self {
            provider: provider.into(),
            window,
            account_discriminator,
            window_id,
        }
    }
}

/// Quota lane kind mirrored from upstream `QuotaWarningWindow`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HookQuotaWindow {
    Session,
    Weekly,
}

impl HookQuotaWindow {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Weekly => "weekly",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::Session => "Session",
            Self::Weekly => "Weekly",
        }
    }
}

/// One quota lane observed in a single poll.
#[derive(Debug, Clone)]
pub struct HookQuotaLaneObservation {
    pub key: HookQuotaLaneKey,
    /// Display label for the event payload (e.g. "Session", "Weekly").
    pub label: String,
    /// `None` means the lane was not reported this poll.
    pub rate_window: Option<RateWindow>,
    /// Provider notification thresholds as usage fractions (0…1).
    pub fallback_thresholds: Vec<f64>,
    pub account_display_name: Option<String>,
}

/// Coarse provider availability, mirroring status-indicator semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum HookProviderStatus {
    None,
    Minor,
    Major,
    Critical,
    Maintenance,
    #[default]
    Unknown,
}

impl HookProviderStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Minor => "minor",
            Self::Major => "major",
            Self::Critical => "critical",
            Self::Maintenance => "maintenance",
            Self::Unknown => "unknown",
        }
    }

    /// `maintenance` and `unknown` never flip tracked state.
    pub fn outage_state(self) -> Option<bool> {
        match self {
            Self::Minor | Self::Major | Self::Critical => Some(true),
            Self::None => Some(false),
            Self::Maintenance | Self::Unknown => None,
        }
    }
}

/// Everything observed for one provider in a single poll.
#[derive(Debug, Clone)]
pub struct HookProviderObservation {
    pub provider: String,
    pub lanes: Vec<HookQuotaLaneObservation>,
    pub status: HookProviderStatus,
    /// Coarse failure category when the refresh itself failed (never a raw error).
    pub refresh_failure_status: Option<String>,
    pub account_display_name: Option<String>,
    /// Present only after a successful fetch. Failed observations never emit an
    /// update and leave quota/status baselines unchanged.
    pub successful_usage: Option<UsageSnapshot>,
    /// Private account identity used only for usage-updated rate limiting.
    pub account_discriminator: Option<String>,
}

impl HookProviderObservation {
    pub fn new(provider: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            lanes: Vec::new(),
            status: HookProviderStatus::Unknown,
            refresh_failure_status: None,
            account_display_name: None,
            successful_usage: None,
            account_discriminator: None,
        }
    }
}

/// One event to dispatch, optionally narrowed to specific rules (`quota_low`).
#[derive(Debug, Clone)]
pub struct HookDispatch {
    pub event: HookEvent,
    /// When set (currently only for `quota_low`), only these rules should run.
    pub rules: Option<Vec<HookRule>>,
    /// Private limiter scope for [`HookEventType::UsageUpdated`] dispatches
    /// (e.g. the account discriminator). Never serialized or exported to hook
    /// processes; `None` falls back to event-identity keys.
    pub rate_limit_scope: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct LaneSample {
    usage: f64,
    resets_at: Option<DateTime<Utc>>,
}

/// Turns successive provider observations into hook events.
#[derive(Debug, Default)]
pub struct HookTransitionDetector {
    window_observation: HashMap<HookQuotaLaneKey, LaneSample>,
    provider_status_had_issue: HashMap<String, bool>,
    config_revision: Option<i64>,
    reached_threshold: f64,
    reset_drop_threshold: f64,
}

impl HookTransitionDetector {
    pub fn new() -> Self {
        Self {
            reached_threshold: 1.0,
            reset_drop_threshold: 0.2,
            ..Self::default()
        }
    }

    /// Drops every baseline when the hook configuration changed.
    pub fn reset_if_configuration_changed(&mut self, revision: i64) {
        if self.config_revision == Some(revision) {
            return;
        }
        self.window_observation.clear();
        self.provider_status_had_issue.clear();
        self.config_revision = Some(revision);
    }

    /// Evaluates one poll of one provider and returns the events to dispatch.
    pub fn evaluate(
        &mut self,
        observation: &HookProviderObservation,
        config: &HooksConfig,
    ) -> Vec<HookDispatch> {
        self.evaluate_at(observation, config, Utc::now())
    }

    pub fn evaluate_at(
        &mut self,
        observation: &HookProviderObservation,
        config: &HooksConfig,
        now: DateTime<Utc>,
    ) -> Vec<HookDispatch> {
        if !config.enabled || config.events.len() > HooksConfig::MAX_RULES {
            return Vec::new();
        }

        if let Some(failure) = observation.refresh_failure_status.as_deref() {
            let event = HookEvent::new(HookEventType::RefreshFailed, observation.provider.clone())
                .with_status(failure)
                .with_timestamp(now);
            let event = match &observation.account_display_name {
                Some(account) => event.with_account(account.clone()),
                None => event,
            };
            // Failed refresh must not disturb baselines.
            return vec![HookDispatch {
                event,
                rules: None,
                rate_limit_scope: None,
            }];
        }

        let mut dispatches = self.status_events(observation, now);
        if let Some(usage) = &observation.successful_usage {
            dispatches.push(HookDispatch {
                event: build_usage_updated_event(
                    &observation.provider,
                    &HookUsageWindow::from(&usage.primary),
                    usage.secondary.as_ref().map(HookUsageWindow::from).as_ref(),
                    observation.account_display_name.as_deref(),
                    now,
                ),
                rules: None,
                rate_limit_scope: observation
                    .account_discriminator
                    .clone()
                    .filter(|scope| !scope.is_empty()),
            });
        }

        let observed_keys: HashSet<HookQuotaLaneKey> =
            observation.lanes.iter().map(|l| l.key.clone()).collect();
        for lane in &observation.lanes {
            dispatches.extend(self.lane_events(lane, &observation.provider, config, now));
        }
        self.prune_lanes(&observation.provider, &observed_keys);

        dispatches
    }

    fn status_events(
        &mut self,
        observation: &HookProviderObservation,
        now: DateTime<Utc>,
    ) -> Vec<HookDispatch> {
        let Some(is_outage) = observation.status.outage_state() else {
            return Vec::new();
        };
        let previous = self
            .provider_status_had_issue
            .insert(observation.provider.clone(), is_outage);
        let Some(previous) = previous else {
            return Vec::new();
        };
        if previous == is_outage {
            return Vec::new();
        }

        let event_type = if is_outage {
            HookEventType::ProviderUnavailable
        } else {
            HookEventType::ProviderRecovered
        };
        let event = HookEvent::new(event_type, observation.provider.clone())
            .with_status(observation.status.as_str())
            .with_timestamp(now);
        let event = match &observation.account_display_name {
            Some(account) => event.with_account(account.clone()),
            None => event,
        };
        vec![HookDispatch {
            event,
            rules: None,
            rate_limit_scope: None,
        }]
    }

    fn lane_events(
        &mut self,
        lane: &HookQuotaLaneObservation,
        provider: &str,
        config: &HooksConfig,
        now: DateTime<Utc>,
    ) -> Vec<HookDispatch> {
        // Informational / synthetic stand-ins carry no usage to compare. Forget
        // so a later real reading starts fresh.
        let Some(rate_window) = lane.rate_window.as_ref() else {
            self.window_observation.remove(&lane.key);
            return Vec::new();
        };
        if rate_window.is_informational {
            self.window_observation.remove(&lane.key);
            return Vec::new();
        }

        let current = (rate_window.used_percent / 100.0).clamp(0.0, 1.0);
        let previous_sample = self.window_observation.insert(
            lane.key.clone(),
            LaneSample {
                usage: current,
                resets_at: rate_window.resets_at,
            },
        );

        let Some(previous) = previous_sample else {
            return Vec::new();
        };

        if let Some(reset_event) =
            self.reset_event(lane, provider, previous, current, rate_window, now)
        {
            return vec![HookDispatch {
                event: reset_event,
                rules: None,
                rate_limit_scope: None,
            }];
        }

        let mut dispatches = self.quota_low_events(lane, provider, previous, current, config, now);

        if lane.key.window == HookQuotaWindow::Session
            && previous.usage < self.reached_threshold
            && current >= self.reached_threshold
        {
            dispatches.push(HookDispatch {
                event: build_lane_event(HookEventType::QuotaReached, provider, lane, current, now),
                rules: None,
                rate_limit_scope: None,
            });
        }

        dispatches
    }

    fn reset_event(
        &self,
        lane: &HookQuotaLaneObservation,
        provider: &str,
        previous: LaneSample,
        current: f64,
        rate_window: &RateWindow,
        now: DateTime<Utc>,
    ) -> Option<HookEvent> {
        let boundary_moved = match (previous.resets_at, rate_window.resets_at) {
            (Some(prev), Some(curr)) => curr > prev,
            _ => false,
        };
        let usage_dropped = previous.usage - current >= self.reset_drop_threshold;
        if !boundary_moved && !usage_dropped {
            return None;
        }
        Some(build_lane_event(
            HookEventType::QuotaReset,
            provider,
            lane,
            current,
            now,
        ))
    }

    fn quota_low_events(
        &self,
        lane: &HookQuotaLaneObservation,
        provider: &str,
        previous: LaneSample,
        current: f64,
        config: &HooksConfig,
        now: DateTime<Utc>,
    ) -> Vec<HookDispatch> {
        let rules: Vec<&HookRule> = config
            .events
            .iter()
            .filter(|rule| {
                rule.enabled
                    && rule_watches_quota_low(rule)
                    && (rule.provider.is_none() || rule.provider.as_deref() == Some(provider))
            })
            .collect();
        if rules.is_empty() {
            return Vec::new();
        }

        let crossed: Vec<HookRule> = rules
            .into_iter()
            .filter(|rule| {
                quota_low_threshold_crossed(
                    rule.threshold,
                    previous.usage,
                    current,
                    &lane.fallback_thresholds,
                )
            })
            .cloned()
            .collect();
        if crossed.is_empty() {
            return Vec::new();
        }

        vec![HookDispatch {
            event: build_lane_event(HookEventType::QuotaLow, provider, lane, current, now),
            rules: Some(crossed),
            rate_limit_scope: None,
        }]
    }

    fn prune_lanes(&mut self, provider: &str, keeping: &HashSet<HookQuotaLaneKey>) {
        self.window_observation
            .retain(|key, _| key.provider != provider || keeping.contains(key));
    }
}

fn rule_watches_quota_low(rule: &HookRule) -> bool {
    rule.event == Some(HookEventType::QuotaLow) || rule.events.contains(&HookEventType::QuotaLow)
}

/// Returns true when any watched threshold was crossed upward.
pub fn quota_low_threshold_crossed(
    rule_threshold: Option<f64>,
    previous_usage: f64,
    current_usage: f64,
    fallback_thresholds: &[f64],
) -> bool {
    let watched: Vec<f64> = match rule_threshold {
        Some(t) => vec![t],
        None => fallback_thresholds.to_vec(),
    };
    watched
        .into_iter()
        .any(|t| previous_usage < t && current_usage >= t)
}

fn build_lane_event(
    event_type: HookEventType,
    provider: &str,
    lane: &HookQuotaLaneObservation,
    usage_fraction: f64,
    now: DateTime<Utc>,
) -> HookEvent {
    let mut event = HookEvent::new(event_type, provider)
        .with_window(lane.label.clone())
        .with_usage_fraction(usage_fraction)
        .with_timestamp(now);
    if let Some(account) = &lane.account_display_name {
        event = event.with_account(account.clone());
    }
    event
}

/// Builds the canonical `usage_updated` event.
///
/// The single place that assembles the payload from quota windows: non-
/// informational windows are exported (primary and secondary), informational
/// ones are omitted. `account` is the display account for the payload. The
/// private limiter scope travels in `HookDispatch`, not in the payload.
pub fn build_usage_updated_event(
    provider: &str,
    primary: &HookUsageWindow,
    secondary: Option<&HookUsageWindow>,
    account: Option<&str>,
    now: DateTime<Utc>,
) -> HookEvent {
    let mut event = HookEvent::new(HookEventType::UsageUpdated, provider).with_timestamp(now);

    if !primary.is_informational {
        event = event
            .with_used_percent(primary.used_percent)
            .with_window_minutes(primary.window_minutes)
            .with_reset_at(primary.resets_at.clone());
    }
    if let Some(secondary) = secondary.filter(|window| !window.is_informational) {
        event = event
            .with_secondary_usage_fraction(secondary.used_percent / 100.0)
            .with_secondary_window_minutes(secondary.window_minutes)
            .with_secondary_reset_at(secondary.resets_at.clone());
    }
    if let Some(account) = account {
        event = event.with_account(account.to_string());
    }
    event
}

/// Loads settings, gates on hooks, builds the canonical `usage_updated` event
/// and dispatches it on a background thread. Shared by every publishing
/// surface (desktop refresh, CLI watch) so payload assembly and limiter scoping
/// stay in one place.
///
/// `primary`/`secondary` windows are exported only when non-informational;
/// `rate_limit_scope` is the private account discriminator used only for the
/// in-memory ten-minute limiter (never serialized or exported).
pub fn dispatch_usage_updated_hook(
    hooks_enabled: bool,
    provider: &str,
    primary: &HookUsageWindow,
    secondary: Option<&HookUsageWindow>,
    account: Option<&str>,
    rate_limit_scope: Option<String>,
) {
    if !hooks_enabled {
        return;
    }
    let event = build_usage_updated_event(provider, primary, secondary, account, Utc::now());
    spawn_hook_dispatch(event, true, rate_limit_scope);
}

#[cfg(test)]
mod tests;
