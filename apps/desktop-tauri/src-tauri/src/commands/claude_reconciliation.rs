use super::{ProviderRefreshOutcome, ProviderRefreshSkipReason};
use serde::Serialize;
use std::collections::VecDeque;
use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::Duration;
use tauri::Emitter;

static COORDINATOR: LazyLock<Mutex<ClaudeReconciliationCoordinator>> =
    LazyLock::new(|| Mutex::new(ClaudeReconciliationCoordinator::default()));
const REPLAY_DELAY_MIN: Duration = Duration::from_millis(250);
const REPLAY_DELAY_MAX: Duration = Duration::from_secs(5);

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeReconciliationStarted {
    generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum PendingEvent {
    Reconciling(ClaudeReconciliationStarted),
    Reconciled(ClaudeReconciliationTerminal),
}

impl PendingEvent {
    fn publish(&self, app: &tauri::AppHandle) -> Result<(), String> {
        match self {
            Self::Reconciling(payload) => app
                .emit("claude-accounts-reconciling", payload)
                .map_err(|error| error.to_string()),
            Self::Reconciled(payload) => app
                .emit("claude-accounts-reconciled", payload)
                .map_err(|error| error.to_string()),
        }
    }
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
    pending_events: VecDeque<PendingEvent>,
    publisher_active: bool,
    replay_scheduled: bool,
}

impl ClaudeReconciliationCoordinator {
    fn begin(&mut self) -> ClaudeReconciliationToken {
        self.next_generation = self.next_generation.wrapping_add(1);
        let token = ClaudeReconciliationToken(self.next_generation);
        self.active_generation = Some(token.0);
        self.pending_events
            .push_back(PendingEvent::Reconciling(ClaudeReconciliationStarted {
                generation: token.0,
            }));
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
        let terminal = ClaudeReconciliationTerminal {
            generation: token.0,
            status: result.status,
            provider_refresh_generation: result.provider_refresh_generation,
            detail: result.detail,
        };
        self.pending_events
            .push_back(PendingEvent::Reconciled(terminal.clone()));
        CompletionDisposition::Current(terminal)
    }

    fn claim_pending_event(&mut self) -> Option<PendingEvent> {
        if self.publisher_active {
            return None;
        }
        let event = self.pending_events.front()?.clone();
        self.publisher_active = true;
        Some(event)
    }

    fn finish_publish(&mut self, event: &PendingEvent, succeeded: bool) {
        debug_assert!(self.publisher_active);
        self.publisher_active = false;
        if succeeded {
            let published = self.pending_events.pop_front();
            debug_assert_eq!(published.as_ref(), Some(event));
        }
    }

    fn schedule_replay(&mut self) -> bool {
        if self.replay_scheduled {
            return false;
        }
        self.replay_scheduled = true;
        true
    }

    fn finish_replay_if_idle(&mut self) -> bool {
        if self.pending_events.is_empty() && !self.publisher_active {
            self.replay_scheduled = false;
            true
        } else {
            false
        }
    }
}

fn coordinator() -> MutexGuard<'static, ClaudeReconciliationCoordinator> {
    COORDINATOR
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn publish_pending_with<F>(
    state: &Mutex<ClaudeReconciliationCoordinator>,
    mut publish: F,
) -> Result<(), String>
where
    F: FnMut(&PendingEvent) -> Result<(), String>,
{
    loop {
        let Some(event) = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .claim_pending_event()
        else {
            return Ok(());
        };

        let result = publish(&event);
        state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .finish_publish(&event, result.is_ok());
        result?;
    }
}

fn publish_pending(app: &tauri::AppHandle) -> Result<(), String> {
    publish_pending_with(&COORDINATOR, |event| event.publish(app))
}

fn schedule_replay(app: tauri::AppHandle) {
    if !coordinator().schedule_replay() {
        return;
    }

    tauri::async_runtime::spawn(async move {
        let mut delay = REPLAY_DELAY_MIN;
        loop {
            tokio::time::sleep(delay).await;
            match publish_pending(&app) {
                Ok(()) => {
                    if coordinator().finish_replay_if_idle() {
                        break;
                    }
                    delay = REPLAY_DELAY_MIN;
                }
                Err(error) => {
                    tracing::debug!(
                        %error,
                        "Claude reconciliation event replay remains pending"
                    );
                    delay = delay.saturating_mul(2).min(REPLAY_DELAY_MAX);
                }
            }
        }
    });
}

fn publish_or_schedule_replay(app: &tauri::AppHandle) {
    if let Err(error) = publish_pending(app) {
        tracing::warn!(%error, "Claude reconciliation event queued for replay");
        schedule_replay(app.clone());
    }
}

/// Begin a Claude reconciliation and queue its event in generation order.
/// Publication occurs after releasing the coordinator lock. Failed events stay
/// at the front of the queue and are retried before newer generations.
pub(super) fn begin(app: &tauri::AppHandle) -> ClaudeReconciliationToken {
    let token = coordinator().begin();
    publish_or_schedule_replay(app);
    token
}

/// Queue a terminal event only when `token` still owns the current Claude
/// reconciliation. The coordinator orders state transitions; the outbox orders
/// external publication without holding the global mutex across `app.emit`.
pub(super) fn complete(
    app: &tauri::AppHandle,
    token: ClaudeReconciliationToken,
    result: ClaudeReconciliationResult,
) -> bool {
    let disposition = coordinator().complete(token, result);
    let is_current = match disposition {
        CompletionDisposition::Current(_) => true,
        CompletionDisposition::Superseded(terminal) => {
            tracing::debug!(
                generation = terminal.generation,
                detail = %terminal.detail,
                "Claude reconciliation completed after it was superseded"
            );
            false
        }
    };
    publish_or_schedule_replay(app);
    is_current
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
    fn failed_terminal_publish_is_retained_and_replayed_before_a_new_generation() {
        let state = Mutex::new(ClaudeReconciliationCoordinator::default());
        let first = state.lock().unwrap().begin();
        publish_pending_with(&state, |_| Ok(())).expect("begin event should publish");
        state.lock().unwrap().complete(
            first,
            ClaudeReconciliationResult::succeeded(Some(7), "published"),
        );

        assert_eq!(
            publish_pending_with(&state, |_| Err("event transport unavailable".into())),
            Err("event transport unavailable".into())
        );
        {
            let coordinator = state.lock().unwrap();
            assert_eq!(coordinator.pending_events.len(), 1);
            assert!(!coordinator.publisher_active);
        }

        let second = state.lock().unwrap().begin();
        let mut replayed = Vec::new();
        publish_pending_with(&state, |event| {
            replayed.push(event.clone());
            Ok(())
        })
        .expect("queued events should replay");

        assert!(matches!(
            replayed.as_slice(),
            [
                PendingEvent::Reconciled(ClaudeReconciliationTerminal {
                    generation: first_generation,
                    ..
                }),
                PendingEvent::Reconciling(ClaudeReconciliationStarted {
                    generation: second_generation,
                }),
            ] if *first_generation == first.0 && *second_generation == second.0
        ));
        assert!(state.lock().unwrap().pending_events.is_empty());
    }

    #[test]
    fn publisher_runs_without_the_coordinator_lock_and_drains_reentrant_work_in_order() {
        let state = Mutex::new(ClaudeReconciliationCoordinator::default());
        let first = state.lock().unwrap().begin();
        let mut injected = false;
        let mut published = Vec::new();

        publish_pending_with(&state, |event| {
            let guard = state
                .try_lock()
                .expect("external publisher must run outside the coordinator lock");
            drop(guard);
            published.push(event.clone());
            if !injected {
                injected = true;
                state.lock().unwrap().begin();
            }
            Ok(())
        })
        .expect("reentrant enqueue should drain");

        assert!(matches!(
            published.as_slice(),
            [
                PendingEvent::Reconciling(ClaudeReconciliationStarted {
                    generation: first_generation,
                }),
                PendingEvent::Reconciling(ClaudeReconciliationStarted {
                    generation: second_generation,
                }),
            ] if *first_generation == first.0 && *second_generation == first.0.wrapping_add(1)
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
