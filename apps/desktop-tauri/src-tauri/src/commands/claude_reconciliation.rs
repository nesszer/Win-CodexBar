use super::{ProviderRefreshOutcome, ProviderRefreshSkipReason};
use serde::Serialize;
use tauri::Emitter;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ClaudeReconciliationToken(u64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ClaudeReconciliationStatus {
    Pending,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeReconciliationSnapshot {
    pub generation: u64,
    pub status: ClaudeReconciliationStatus,
    pub provider_refresh_generation: Option<u64>,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClaudeReconciliationResult {
    status: ClaudeReconciliationStatus,
    provider_refresh_generation: Option<u64>,
    detail: String,
}

impl ClaudeReconciliationResult {
    pub(super) fn from_refresh(result: Result<ProviderRefreshOutcome, String>) -> Self {
        match result {
            Ok(ProviderRefreshOutcome::Published { generation }) => {
                Self::succeeded(Some(generation), "published")
            }
            Ok(ProviderRefreshOutcome::Skipped {
                reason: ProviderRefreshSkipReason::NoEnabledProviders,
            }) => Self::succeeded(None, "noEnabledProviders"),
            Ok(ProviderRefreshOutcome::Skipped {
                reason: ProviderRefreshSkipReason::CacheFresh { generation },
            }) => Self::succeeded(Some(generation), "cacheFresh"),
            Ok(ProviderRefreshOutcome::Skipped {
                reason: ProviderRefreshSkipReason::Active { generation },
            }) => Self::failed(
                Some(generation),
                format!("provider refresh generation {generation} is already active"),
            ),
            Ok(ProviderRefreshOutcome::Skipped {
                reason: ProviderRefreshSkipReason::InputSuperseded { expected, current },
            }) => Self::failed(
                Some(current),
                format!("provider refresh input generation {expected} was superseded by {current}"),
            ),
            Ok(ProviderRefreshOutcome::Superseded {
                generation,
                current_generation,
            }) => Self::failed(
                Some(current_generation),
                format!(
                    "provider refresh generation {generation} was superseded by {current_generation}"
                ),
            ),
            Err(error) => Self::failed(None, error),
        }
    }

    fn succeeded(provider_refresh_generation: Option<u64>, detail: impl Into<String>) -> Self {
        Self {
            status: ClaudeReconciliationStatus::Succeeded,
            provider_refresh_generation,
            detail: detail.into(),
        }
    }

    fn failed(provider_refresh_generation: Option<u64>, detail: impl Into<String>) -> Self {
        Self {
            status: ClaudeReconciliationStatus::Failed,
            provider_refresh_generation,
            detail: detail.into(),
        }
    }
}

#[derive(Debug, Default)]
pub struct ClaudeReconciliationState {
    next_generation: u64,
    current: Option<ClaudeReconciliationSnapshot>,
}

impl ClaudeReconciliationState {
    pub(crate) fn begin(&mut self) -> (ClaudeReconciliationToken, ClaudeReconciliationSnapshot) {
        self.next_generation = self.next_generation.wrapping_add(1);
        let token = ClaudeReconciliationToken(self.next_generation);
        let snapshot = ClaudeReconciliationSnapshot {
            generation: token.0,
            status: ClaudeReconciliationStatus::Pending,
            provider_refresh_generation: None,
            detail: "refreshing".to_string(),
        };
        self.current = Some(snapshot.clone());
        (token, snapshot)
    }

    pub(super) fn complete(
        &mut self,
        token: ClaudeReconciliationToken,
        result: ClaudeReconciliationResult,
    ) -> (bool, ClaudeReconciliationSnapshot) {
        let is_current = self.current.as_ref().is_some_and(|snapshot| {
            snapshot.generation == token.0 && snapshot.status == ClaudeReconciliationStatus::Pending
        });
        let snapshot = if is_current {
            ClaudeReconciliationSnapshot {
                generation: token.0,
                status: result.status,
                provider_refresh_generation: result.provider_refresh_generation,
                detail: result.detail,
            }
        } else {
            let successor = self
                .current
                .as_ref()
                .map(|snapshot| snapshot.generation)
                .unwrap_or(self.next_generation);
            ClaudeReconciliationSnapshot {
                generation: token.0,
                status: ClaudeReconciliationStatus::Failed,
                provider_refresh_generation: result.provider_refresh_generation,
                detail: format!("superseded by Claude reconciliation generation {successor}"),
            }
        };
        if is_current {
            self.current = Some(snapshot.clone());
        }
        (is_current, snapshot)
    }

    pub(crate) fn snapshot(&self) -> Option<ClaudeReconciliationSnapshot> {
        self.current.clone()
    }
}

pub(super) fn emit(app: &tauri::AppHandle, snapshot: &ClaudeReconciliationSnapshot) {
    if let Err(error) = app.emit("claude-reconciliation-changed", snapshot) {
        tracing::warn!(
            %error,
            generation = snapshot.generation,
            "Failed to emit Claude reconciliation state"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_reconciliations_keep_the_latest_generation_authoritative() {
        let mut state = ClaudeReconciliationState::default();
        let (first, _) = state.begin();
        let (second, second_pending) = state.begin();

        let (accepted, stale) =
            state.complete(first, ClaudeReconciliationResult::succeeded(None, "first"));
        assert!(!accepted);
        assert_eq!(stale.generation, first.0);
        assert_eq!(stale.status, ClaudeReconciliationStatus::Failed);
        assert_eq!(state.snapshot(), Some(second_pending));

        let (accepted, terminal) = state.complete(
            second,
            ClaudeReconciliationResult::succeeded(Some(8), "second"),
        );
        assert!(accepted);
        assert_eq!(terminal.generation, second.0);
        assert_eq!(terminal.status, ClaudeReconciliationStatus::Succeeded);
        assert_eq!(state.snapshot(), Some(terminal));
    }

    #[test]
    fn pending_snapshot_survives_until_late_failure_replaces_it() {
        let mut state = ClaudeReconciliationState::default();
        let (token, pending) = state.begin();
        assert_eq!(pending.status, ClaudeReconciliationStatus::Pending);
        assert_eq!(state.snapshot(), Some(pending));

        let (accepted, failed) = state.complete(
            token,
            ClaudeReconciliationResult::failed(None, "late failure"),
        );
        assert!(accepted);
        assert_eq!(failed.status, ClaudeReconciliationStatus::Failed);
        assert_eq!(failed.detail, "late failure");
        assert_eq!(state.snapshot(), Some(failed));
    }

    #[test]
    fn provider_refresh_supersession_is_an_explicit_failure() {
        let result =
            ClaudeReconciliationResult::from_refresh(Ok(ProviderRefreshOutcome::Superseded {
                generation: 4,
                current_generation: 5,
            }));
        assert_eq!(result.status, ClaudeReconciliationStatus::Failed);
        assert_eq!(result.provider_refresh_generation, Some(5));
    }
}
