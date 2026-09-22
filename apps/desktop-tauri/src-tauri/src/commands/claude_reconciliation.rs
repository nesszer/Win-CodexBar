use super::{ProviderRefreshOutcome, ProviderRefreshSkipReason};
use serde::Serialize;
use std::sync::{LazyLock, Mutex, MutexGuard};
use tauri::Emitter;

static COORDINATOR: LazyLock<Mutex<ClaudeReconciliationCoordinator>> =
    LazyLock::new(|| Mutex::new(ClaudeReconciliationCoordinator::default()));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ClaudeReconciliationToken(u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ClaudeReconciliationResult {
    status: ClaudeReconciliationStatus,
    provider_refresh_generation: Option<u64>,
    detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeReconciliationTerminal {
    generation: u64,
    status: ClaudeReconciliationStatus,
    provider_refresh_generation: Option<u64>,
    detail: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
enum ClaudeReconciliationStatus {
    Succeeded,
    Failed,
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

    pub(super) fn command_result(&self) -> Result<(), String> {
        match self.status {
            ClaudeReconciliationStatus::Succeeded => Ok(()),
            ClaudeReconciliationStatus::Failed => Err(self.detail.clone()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CompletionDisposition {
    Current(ClaudeReconciliationTerminal),
    Superseded(ClaudeReconciliationTerminal),
}

#[derive(Debug, Default)]
struct ClaudeReconciliationCoordinator {
    next_generation: u64,
    active_generation: Option<u64>,
}

impl ClaudeReconciliationCoordinator {
    fn begin(&mut self) -> ClaudeReconciliationToken {
        self.next_generation = self.next_generation.wrapping_add(1);
        let token = ClaudeReconciliationToken(self.next_generation);
        self.active_generation = Some(token.0);
        token
    }

    fn complete(
        &mut self,
        token: ClaudeReconciliationToken,
        result: ClaudeReconciliationResult,
    ) -> CompletionDisposition {
        if self.active_generation != Some(token.0) {
            let successor = self.active_generation.unwrap_or(self.next_generation);
            return CompletionDisposition::Superseded(ClaudeReconciliationTerminal {
                generation: token.0,
                status: ClaudeReconciliationStatus::Failed,
                provider_refresh_generation: result.provider_refresh_generation,
                detail: format!("superseded by Claude reconciliation generation {successor}"),
            });
        }
        self.active_generation = None;
        CompletionDisposition::Current(ClaudeReconciliationTerminal {
            generation: token.0,
            status: result.status,
            provider_refresh_generation: result.provider_refresh_generation,
            detail: result.detail,
        })
    }
}

fn coordinator() -> MutexGuard<'static, ClaudeReconciliationCoordinator> {
    COORDINATOR
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Begin a Claude reconciliation and publish its token while holding the same
/// lock used by terminal publication. This prevents an old detached worker's
/// terminal event from being interleaved after a newer reconciling event.
pub(super) fn begin(app: &tauri::AppHandle) -> ClaudeReconciliationToken {
    let mut coordinator = coordinator();
    let token = coordinator.begin();
    let _ = app.emit(
        "claude-accounts-reconciling",
        serde_json::json!({ "generation": token.0 }),
    );
    token
}

/// Publish a terminal event only when `token` still owns the current Claude
/// reconciliation. The ownership check and event emission are serialized with
/// `begin`, so a stale worker cannot settle a newer operation.
pub(super) fn complete(
    app: &tauri::AppHandle,
    token: ClaudeReconciliationToken,
    result: ClaudeReconciliationResult,
) -> bool {
    let mut coordinator = coordinator();
    match coordinator.complete(token, result) {
        CompletionDisposition::Current(terminal) => {
            let _ = app.emit("claude-accounts-reconciled", terminal);
            true
        }
        CompletionDisposition::Superseded(terminal) => {
            tracing::debug!(
                generation = terminal.generation,
                detail = %terminal.detail,
                "Claude reconciliation completed after it was superseded"
            );
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlapping_reconciliations_only_allow_the_latest_terminal() {
        let mut coordinator = ClaudeReconciliationCoordinator::default();
        let first = coordinator.begin();
        let second = coordinator.begin();

        assert!(matches!(
            coordinator.complete(first, ClaudeReconciliationResult::succeeded(None, "first")),
            CompletionDisposition::Superseded(ClaudeReconciliationTerminal {
                generation,
                status: ClaudeReconciliationStatus::Failed,
                ..
            }) if generation == first.0
        ));
        assert!(matches!(
            coordinator.complete(second, ClaudeReconciliationResult::succeeded(None, "second")),
            CompletionDisposition::Current(ClaudeReconciliationTerminal {
                generation,
                status: ClaudeReconciliationStatus::Succeeded,
                ..
            }) if generation == second.0
        ));
    }

    #[test]
    fn superseded_provider_refresh_is_an_explicit_failure() {
        let terminal =
            ClaudeReconciliationResult::from_refresh(Ok(ProviderRefreshOutcome::Superseded {
                generation: 4,
                current_generation: 5,
            }));

        assert_eq!(terminal.status, ClaudeReconciliationStatus::Failed);
        assert_eq!(terminal.provider_refresh_generation, Some(5));
        assert!(terminal.command_result().is_err());
    }

    #[test]
    fn active_provider_refresh_is_an_explicit_failure() {
        let terminal =
            ClaudeReconciliationResult::from_refresh(Ok(ProviderRefreshOutcome::Skipped {
                reason: ProviderRefreshSkipReason::Active { generation: 9 },
            }));

        assert_eq!(terminal.status, ClaudeReconciliationStatus::Failed);
        assert_eq!(terminal.provider_refresh_generation, Some(9));
        assert!(terminal.command_result().is_err());
    }

    #[test]
    fn no_enabled_provider_is_an_explicit_success() {
        let terminal =
            ClaudeReconciliationResult::from_refresh(Ok(ProviderRefreshOutcome::Skipped {
                reason: ProviderRefreshSkipReason::NoEnabledProviders,
            }));

        assert_eq!(terminal.status, ClaudeReconciliationStatus::Succeeded);
        assert_eq!(terminal.detail, "noEnabledProviders");
        assert_eq!(terminal.command_result(), Ok(()));
    }

    #[test]
    fn provider_refresh_error_is_an_explicit_failure() {
        let terminal = ClaudeReconciliationResult::from_refresh(Err("refresh failed".into()));

        assert_eq!(terminal.status, ClaudeReconciliationStatus::Failed);
        assert_eq!(terminal.detail, "refresh failed");
        assert_eq!(terminal.command_result(), Err("refresh failed".into()));
    }
}
