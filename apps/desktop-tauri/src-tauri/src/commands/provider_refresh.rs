use crate::state::AppState;
use codexbar::core::ProviderId;

use super::PROVIDER_CACHE_STALE_AFTER;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderRefreshOutcome {
    Skipped {
        reason: ProviderRefreshSkipReason,
    },
    Published {
        generation: u64,
    },
    Superseded {
        generation: u64,
        current_generation: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProviderRefreshSkipReason {
    NoEnabledProviders,
    Active { generation: u64 },
    InputSuperseded { expected: u64, current: u64 },
    CacheFresh { generation: u64 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProviderRefreshReservation {
    Reserved { generation: u64 },
    Skipped(ProviderRefreshSkipReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ProviderRefreshCompletion {
    Published { error_count: usize },
    Superseded { current_generation: u64 },
}

pub(crate) fn is_provider_cache_fresh(
    updated_at: Option<std::time::Instant>,
    stale_after: std::time::Duration,
) -> bool {
    updated_at
        .map(|updated| updated.elapsed() <= stale_after)
        .unwrap_or(false)
}

pub(super) fn reserve_provider_refresh(
    state: &mut AppState,
    force: bool,
    provider_ids: &[ProviderId],
    expected_generation: u64,
) -> ProviderRefreshReservation {
    if state.is_refreshing {
        return ProviderRefreshReservation::Skipped(ProviderRefreshSkipReason::Active {
            generation: state.provider_refresh_generation,
        });
    }
    if state.provider_refresh_generation != expected_generation {
        return ProviderRefreshReservation::Skipped(ProviderRefreshSkipReason::InputSuperseded {
            expected: expected_generation,
            current: state.provider_refresh_generation,
        });
    }
    if provider_cache_can_skip_refresh(state, force, provider_ids) {
        return ProviderRefreshReservation::Skipped(ProviderRefreshSkipReason::CacheFresh {
            generation: state.provider_refresh_generation,
        });
    }

    state.provider_refresh_generation = state.provider_refresh_generation.wrapping_add(1);
    let generation = state.provider_refresh_generation;
    state.is_refreshing = true;
    state.provider_refresh_started_at = Some(std::time::Instant::now());
    ProviderRefreshReservation::Reserved { generation }
}

fn provider_cache_can_skip_refresh(
    state: &AppState,
    force: bool,
    provider_ids: &[ProviderId],
) -> bool {
    let cache_has_all = provider_ids.iter().all(|id| {
        state
            .provider_cache
            .iter()
            .any(|snapshot| snapshot.provider_id == id.cli_name())
    });
    if !force && state.provider_cache_seeded && cache_has_all {
        return true;
    }
    !force
        && cache_has_all
        && provider_ids.iter().all(|id| {
            is_provider_cache_fresh(
                state.provider_cache_updated_at_by_provider.get(id).copied(),
                PROVIDER_CACHE_STALE_AFTER,
            )
        })
}

pub(super) fn complete_provider_refresh(
    state: &mut AppState,
    generation: u64,
) -> ProviderRefreshCompletion {
    if state.provider_refresh_generation != generation {
        return ProviderRefreshCompletion::Superseded {
            current_generation: state.provider_refresh_generation,
        };
    }
    state.is_refreshing = false;
    state.provider_refresh_started_at = None;
    state.provider_cache_updated_at = Some(std::time::Instant::now());
    ProviderRefreshCompletion::Published {
        error_count: state
            .provider_cache
            .iter()
            .filter(|snapshot| snapshot.error.is_some())
            .count(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validated_seed_pins_only_complete_nonforced_cache() {
        let mut state = AppState::new();
        state.provider_cache.push(
            crate::proof_harness::parse_seed_usage_snapshot(
                r#"{"providerId":"codex","primary":{"usedPercent":25.0}}"#,
            )
            .unwrap(),
        );
        assert!(!provider_cache_can_skip_refresh(
            &state,
            false,
            &[ProviderId::Codex]
        ));
        state.provider_cache_seeded = true;
        assert!(provider_cache_can_skip_refresh(
            &state,
            false,
            &[ProviderId::Codex]
        ));
        assert!(!provider_cache_can_skip_refresh(
            &state,
            true,
            &[ProviderId::Codex]
        ));
        assert!(!provider_cache_can_skip_refresh(
            &state,
            false,
            &[ProviderId::Codex, ProviderId::Claude]
        ));
    }

    #[test]
    fn stale_inputs_cannot_reserve_a_new_generation_after_invalidation() {
        let mut state = AppState::new();
        let expected_generation = state.provider_refresh_generation;
        state.provider_refresh_generation = state.provider_refresh_generation.wrapping_add(1);

        assert_eq!(
            reserve_provider_refresh(&mut state, true, &[ProviderId::Codex], expected_generation,),
            ProviderRefreshReservation::Skipped(ProviderRefreshSkipReason::InputSuperseded {
                expected: expected_generation,
                current: state.provider_refresh_generation,
            })
        );
        assert!(!state.is_refreshing);
    }

    #[test]
    fn matching_generation_reserves_the_next_generation() {
        let mut state = AppState::new();
        let expected_generation = state.provider_refresh_generation;

        let ProviderRefreshReservation::Reserved { generation } =
            reserve_provider_refresh(&mut state, true, &[ProviderId::Codex], expected_generation)
        else {
            panic!("refresh should be reserved");
        };

        assert_eq!(generation, expected_generation.wrapping_add(1));
        assert!(state.is_refreshing);
    }

    #[test]
    fn superseded_generation_cannot_publish_or_release_its_successor() {
        let mut state = AppState::new();
        let initial_generation = state.provider_refresh_generation;
        let ProviderRefreshReservation::Reserved {
            generation: first_generation,
        } = reserve_provider_refresh(&mut state, true, &[ProviderId::Claude], initial_generation)
        else {
            panic!("first refresh should be reserved");
        };

        state.provider_refresh_generation = state.provider_refresh_generation.wrapping_add(1);
        state.is_refreshing = false;
        let successor_input_generation = state.provider_refresh_generation;
        let ProviderRefreshReservation::Reserved {
            generation: successor_generation,
        } = reserve_provider_refresh(
            &mut state,
            true,
            &[ProviderId::Claude],
            successor_input_generation,
        )
        else {
            panic!("successor refresh should be reserved");
        };

        assert_eq!(
            complete_provider_refresh(&mut state, first_generation),
            ProviderRefreshCompletion::Superseded {
                current_generation: successor_generation
            }
        );
        assert!(state.is_refreshing);

        assert!(matches!(
            complete_provider_refresh(&mut state, successor_generation),
            ProviderRefreshCompletion::Published { .. }
        ));
        assert!(!state.is_refreshing);
    }

    #[test]
    fn public_outcome_preserves_generation_ownership_and_skip_reason() {
        assert_eq!(
            ProviderRefreshOutcome::Published { generation: 42 },
            ProviderRefreshOutcome::Published { generation: 42 }
        );
        assert_eq!(
            ProviderRefreshOutcome::Superseded {
                generation: 41,
                current_generation: 42,
            },
            ProviderRefreshOutcome::Superseded {
                generation: 41,
                current_generation: 42,
            }
        );
        assert_eq!(
            ProviderRefreshOutcome::Skipped {
                reason: ProviderRefreshSkipReason::Active { generation: 42 },
            },
            ProviderRefreshOutcome::Skipped {
                reason: ProviderRefreshSkipReason::Active { generation: 42 },
            }
        );
    }
}
