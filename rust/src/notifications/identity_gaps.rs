//! Threshold-warning continuity across account-identity gaps.
//!
//! Some sources (Claude CLI / OAuth) alternate between samples that resolve an
//! account and samples that cannot. Keyed independently, each identity would
//! fire the same threshold toast once. This module keeps one warning episode
//! across those gaps (upstream CodexBar 0.66.0, `UsageStore+QuotaWarnings`):
//!
//! - A resolved sample moves any fallback ("unresolved") history for the same
//!   lane onto the account and marks the account as `shared_with_unresolved`.
//! - An unresolved sample with no fallback history yet follows the most recently
//!   known account when the window `resets_at` is unchanged and usage has not
//!   dropped; otherwise it keeps the already-fired thresholds if the account
//!   history has previously joined the fallback, and starts an independent
//!   episode if not.
//!
//! The manager stays provider-agnostic: the caller decides which identities are
//! resolved, unresolved or independent via [`WarningScope`].

use std::collections::HashMap;

use chrono::{DateTime, Utc};

use super::{NotificationManager, ThresholdKey, session_depleted_key};
use crate::core::ProviderId;
use crate::settings::Settings;

/// How a sample's account key relates to identity-gap continuity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WarningScope {
    /// No gap semantics: the key is used as-is and never merged.
    Independent(String),
    /// A resolved account. `unresolved` is the fallback key whose history this
    /// account absorbs (the same source lane's unresolved scope).
    Resolved { account: String, unresolved: String },
    /// The source could not resolve an account; carries its fallback key.
    Unresolved(String),
}

impl WarningScope {
    pub fn key(&self) -> &str {
        match self {
            Self::Independent(key) | Self::Unresolved(key) => key,
            Self::Resolved { account, .. } => account,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct LaneKey {
    provider: ProviderId,
    account: String,
    window: String,
}

impl LaneKey {
    fn new(provider: ProviderId, account: &str, window: &str) -> Self {
        Self {
            provider,
            account: account.to_string(),
            window: window.to_string(),
        }
    }
}

/// Last observation for one (provider, account, window) lane.
#[derive(Debug, Clone)]
struct LaneEpisode {
    resets_at: Option<DateTime<Utc>>,
    used_percent: f64,
    /// Monotonic observation order; the newer episode wins when merging.
    sequence: u64,
    shared_with_unresolved: bool,
}

#[derive(Debug, Default)]
pub(super) struct IdentityGapState {
    last_known: HashMap<ProviderId, String>,
    lanes: HashMap<LaneKey, LaneEpisode>,
    sequence: u64,
}

/// Owner identity used for session baseline resets. Upstream keeps one unresolved Claude
/// owner (`claude-account:unknown`) for OAuth and CLI samples, so an OAuth-to-CLI source
/// fallback alone is not an owner change. Threshold lanes stay scoped per source.
pub(super) fn session_owner_of(provider: ProviderId, account: &str) -> &str {
    if provider == ProviderId::Claude
        && matches!(account, "claude:oauth:unknown" | "claude:cli:unknown")
    {
        return "claude:unknown";
    }
    account
}

impl NotificationManager {
    /// Resolve which account key a lane sample should be deduped under.
    ///
    /// Call once per non-informational lane sample, before `check_and_notify` /
    /// `check_session_lane`, and pass the returned key to them.
    pub fn resolve_warning_account(
        &mut self,
        provider: ProviderId,
        scope: &WarningScope,
        window: &str,
        used_percent: f64,
        resets_at: Option<DateTime<Utc>>,
        settings: &Settings,
    ) -> String {
        if matches!(scope, WarningScope::Independent(_)) || !settings.show_notifications {
            return scope.key().to_string();
        }
        let key = match scope {
            WarningScope::Resolved {
                account,
                unresolved,
            } => {
                self.merge_unresolved_into_account(provider, unresolved, account);
                self.identity_gaps
                    .last_known
                    .insert(provider, account.clone());
                account.clone()
            }
            WarningScope::Unresolved(unresolved) => {
                self.unresolved_lane_account(provider, unresolved, window, used_percent, resets_at)
            }
            WarningScope::Independent(key) => key.clone(),
        };
        self.record_lane(provider, &key, window, used_percent, resets_at);
        key
    }

    fn unresolved_lane_account(
        &mut self,
        provider: ProviderId,
        unresolved: &str,
        window: &str,
        used_percent: f64,
        resets_at: Option<DateTime<Utc>>,
    ) -> String {
        let own_lane = LaneKey::new(provider, unresolved, window);
        if self.identity_gaps.lanes.contains_key(&own_lane) {
            return unresolved.to_string();
        }
        let Some(account) = self.identity_gaps.last_known.get(&provider).cloned() else {
            return unresolved.to_string();
        };
        let Some(prior) = self
            .identity_gaps
            .lanes
            .get(&LaneKey::new(provider, &account, window))
            .cloned()
        else {
            return unresolved.to_string();
        };
        let continues_episode = resets_at.is_some()
            && resets_at == prior.resets_at
            && used_percent >= prior.used_percent;
        if continues_episode {
            return account;
        }
        if prior.shared_with_unresolved {
            // A discontinuity may change the key but cannot discard reconciled thresholds.
            // Session state is not copied back: it is owner-keyed, and the account's owner
            // differs from the unresolved owner, so the next sample starts a new baseline.
            self.copy_lane_state(provider, &account, unresolved, window);
            self.identity_gaps.lanes.insert(own_lane, prior);
        }
        unresolved.to_string()
    }

    fn record_lane(
        &mut self,
        provider: ProviderId,
        account: &str,
        window: &str,
        used_percent: f64,
        resets_at: Option<DateTime<Utc>>,
    ) {
        let gaps = &mut self.identity_gaps;
        gaps.sequence += 1;
        let lane = gaps
            .lanes
            .entry(LaneKey::new(provider, account, window))
            .or_insert_with(|| LaneEpisode {
                resets_at,
                used_percent,
                sequence: 0,
                shared_with_unresolved: false,
            });
        lane.resets_at = resets_at;
        lane.used_percent = used_percent;
        lane.sequence = gaps.sequence;
    }

    /// Move every lane's fired-threshold state from the fallback key onto the
    /// account, keeping whichever episode was observed most recently.
    fn merge_unresolved_into_account(
        &mut self,
        provider: ProviderId,
        unresolved: &str,
        account: &str,
    ) {
        let windows: Vec<String> = self
            .identity_gaps
            .lanes
            .keys()
            .filter(|lane| lane.provider == provider && lane.account == unresolved)
            .map(|lane| lane.window.clone())
            .collect();
        for window in windows {
            let Some(prior) = self
                .identity_gaps
                .lanes
                .remove(&LaneKey::new(provider, unresolved, &window))
            else {
                continue;
            };
            let account_lane = LaneKey::new(provider, account, &window);
            let account_sequence = self
                .identity_gaps
                .lanes
                .get(&account_lane)
                .map(|lane| lane.sequence);
            if account_sequence.is_none_or(|sequence| prior.sequence >= sequence) {
                self.copy_lane_state(provider, unresolved, account, &window);
                if window == "session" {
                    self.transfer_session_state(provider, unresolved, account);
                }
                self.identity_gaps.lanes.insert(account_lane.clone(), prior);
            }
            if let Some(lane) = self.identity_gaps.lanes.get_mut(&account_lane) {
                lane.shared_with_unresolved = true;
            }
            self.drop_lane_state(provider, unresolved, &window);
        }
    }

    /// Replace `to`'s fired thresholds with `from`'s for one window. Depleted markers and
    /// the session baseline are owner-keyed and move through `transfer_session_state`.
    fn copy_lane_state(&mut self, provider: ProviderId, from: &str, to: &str, window: &str) {
        let fired: Vec<ThresholdKey> = self
            .sent_notifications
            .iter()
            .filter(|(p, a, w, t)| {
                *p == provider && a == from && w == window && t.is_threshold_toast()
            })
            .cloned()
            .collect();
        self.sent_notifications.retain(|(p, a, w, t)| {
            *p != provider || a != to || w != window || !t.is_threshold_toast()
        });
        self.sent_notifications.extend(
            fired
                .into_iter()
                .map(|(p, _, w, t)| (p, to.to_string(), w, t)),
        );
    }

    /// Move the session baseline and depleted marker from `from`'s session owner to
    /// `to`'s. When `from`'s owner is the current one, `to`'s owner inherits it: the copied
    /// baseline continues the same episode, so the next sample is not an owner change.
    fn transfer_session_state(&mut self, provider: ProviderId, from: &str, to: &str) {
        let from_owner = session_owner_of(provider, from);
        let to_owner = session_owner_of(provider, to);
        if from_owner == to_owner {
            return;
        }
        if self
            .session_owner
            .get(&provider)
            .is_some_and(|owner| owner == from_owner)
        {
            self.session_owner.insert(provider, to_owner.to_string());
        }
        match self
            .previous_session_percent
            .get(&(provider, from_owner.to_string()))
            .copied()
        {
            Some(percent) => {
                self.previous_session_percent
                    .insert((provider, to_owner.to_string()), percent);
            }
            None => {
                self.previous_session_percent
                    .remove(&(provider, to_owner.to_string()));
            }
        }
        if self
            .sent_notifications
            .contains(&session_depleted_key(provider, from_owner))
        {
            self.sent_notifications
                .insert(session_depleted_key(provider, to_owner));
        } else {
            self.sent_notifications
                .remove(&session_depleted_key(provider, to_owner));
        }
    }

    fn drop_lane_state(&mut self, provider: ProviderId, account: &str, window: &str) {
        self.sent_notifications
            .retain(|(p, a, w, _)| *p != provider || a != account || w != window);
        if window == "session" {
            // The session owner's live baseline and marker are never dropped with a lane.
            let owner = session_owner_of(provider, account);
            if self
                .session_owner
                .get(&provider)
                .is_none_or(|current| current != owner)
            {
                self.previous_session_percent
                    .remove(&(provider, owner.to_string()));
                self.sent_notifications
                    .remove(&session_depleted_key(provider, owner));
            }
        }
    }
}

#[cfg(test)]
mod tests;
