use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StepOccurrence {
    pub row: i64,
    pub timestamp_ms: Option<i64>,
    pub bot_id: Option<String>,
}

pub(super) fn resolve_step_timestamps(
    step_timestamps: &HashMap<String, Vec<StepOccurrence>>,
    needed_occurrences: &HashMap<String, Vec<StepOccurrence>>,
    ambiguous_bot_ids: &HashSet<String>,
    unidentified_rows_present: bool,
) -> HashMap<String, Vec<i64>> {
    let mut resolved = HashMap::new();
    for (step_uuid, timestamps) in step_timestamps {
        let Some(occurrences) = needed_occurrences.get(step_uuid) else {
            continue;
        };

        let mut sorted_occurrences = occurrences.clone();
        sorted_occurrences.sort_by_key(|occurrence| occurrence.row);
        if has_duplicate_rows(sorted_occurrences.iter().map(|occurrence| occurrence.row)) {
            continue;
        }

        let mut sorted_timestamps = timestamps.clone();
        sorted_timestamps.sort_by_key(|timestamp| timestamp.row);
        if has_duplicate_rows(sorted_timestamps.iter().map(|timestamp| timestamp.row)) {
            continue;
        }

        let needed_count = sorted_occurrences.len();
        // Keep ambiguous slots so later timestamps cannot slide into their positions.
        let ordered_timestamps = sorted_timestamps
            .iter()
            .map(|timestamp| {
                if timestamp
                    .bot_id
                    .as_deref()
                    .is_some_and(|bot_id| ambiguous_bot_ids.contains(bot_id))
                {
                    None
                } else {
                    timestamp.timestamp_ms
                }
            })
            .collect::<Vec<_>>();
        // Sharing one timestamp across reused UUIDs requires a complete identity census.
        let selected =
            if ordered_timestamps.len() == 1 && (needed_count == 1 || !unidentified_rows_present) {
                let Some(timestamp) = ordered_timestamps[0] else {
                    continue;
                };
                vec![timestamp; needed_count]
            } else {
                if ordered_timestamps.len() < needed_count {
                    continue;
                }
                let candidates = &ordered_timestamps[..needed_count];
                let Some(selected) = candidates.iter().copied().collect::<Option<Vec<_>>>() else {
                    continue;
                };
                selected
            };

        if sorted_occurrences
            .iter()
            .zip(selected.iter())
            .any(|(occurrence, timestamp)| {
                occurrence
                    .timestamp_ms
                    .is_some_and(|embedded| embedded != *timestamp)
            })
        {
            continue;
        }
        resolved.insert(step_uuid.clone(), selected);
    }
    resolved
}

fn has_duplicate_rows(rows: impl Iterator<Item = i64>) -> bool {
    let mut prior = None;
    for row in rows {
        if prior == Some(row) {
            return true;
        }
        prior = Some(row);
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `(row, timestamp)` pairs without bot ids, keyed under one `step`.
    fn step(rows: &[(i64, Option<i64>)]) -> HashMap<String, Vec<StepOccurrence>> {
        let rows = rows
            .iter()
            .map(|&(row, timestamp_ms)| StepOccurrence {
                row,
                timestamp_ms,
                bot_id: None,
            })
            .collect();
        HashMap::from([("step".to_string(), rows)])
    }

    #[test]
    fn orders_rows_and_repeats_one_shared_timestamp() {
        let occurrences = step(&[(0, None), (1, None)]);
        let timestamp_map = step(&[(20, Some(200)), (10, Some(100))]);

        assert_eq!(
            resolve_step_timestamps(&timestamp_map, &occurrences, &HashSet::new(), false)["step"],
            vec![100, 200]
        );

        let timestamps = step(&[(10, Some(100))]);
        assert_eq!(
            resolve_step_timestamps(&timestamps, &occurrences, &HashSet::new(), false)["step"],
            vec![100, 100]
        );
    }

    #[test]
    fn withholds_missing_duplicate_and_conflicting_evidence() {
        let occurrences = step(&[(0, None), (1, None)]);
        for evidence in [
            step(&[(10, None), (20, Some(200))]),
            step(&[(10, Some(100)), (10, Some(200))]),
        ] {
            assert!(
                !resolve_step_timestamps(&evidence, &occurrences, &HashSet::new(), false)
                    .contains_key("step")
            );
        }
    }

    #[test]
    fn embedded_timestamp_must_agree_with_aligned_step() {
        let occurrences = step(&[(0, Some(100)), (1, None)]);
        let timestamps = step(&[(10, Some(101)), (20, Some(200))]);

        assert!(
            !resolve_step_timestamps(&timestamps, &occurrences, &HashSet::new(), false)
                .contains_key("step")
        );
    }

    #[test]
    fn unidentified_rows_do_not_reuse_one_timestamp_for_repeated_occurrences() {
        let occurrences = step(&[(0, None), (1, None)]);
        let timestamps = step(&[(10, Some(100))]);

        assert!(
            !resolve_step_timestamps(&timestamps, &occurrences, &HashSet::new(), true)
                .contains_key("step")
        );
    }

    #[test]
    fn ambiguous_bot_timestamp_keeps_its_positional_slot() {
        let occurrences = step(&[(0, None), (1, None)]);
        let timestamps = HashMap::from([(
            "step".to_string(),
            vec![
                StepOccurrence {
                    row: 10,
                    timestamp_ms: Some(100),
                    bot_id: Some("ambiguous".to_string()),
                },
                StepOccurrence {
                    row: 20,
                    timestamp_ms: Some(200),
                    bot_id: None,
                },
            ],
        )]);
        let ambiguous = HashSet::from(["ambiguous".to_string()]);

        assert!(
            !resolve_step_timestamps(&timestamps, &occurrences, &ambiguous, false)
                .contains_key("step")
        );
    }
}
