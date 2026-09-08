use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StepOccurrence {
    pub row: i64,
    pub timestamp_ms: Option<i64>,
    pub bot_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct StepTimestamp {
    pub row: i64,
    pub timestamp_ms: Option<i64>,
    pub bot_id: Option<String>,
}

pub(super) fn resolve_step_timestamps(
    step_timestamps: &HashMap<String, Vec<StepTimestamp>>,
    needed_occurrences: &HashMap<String, Vec<StepOccurrence>>,
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
        let ordered_timestamps = sorted_timestamps
            .iter()
            .map(|timestamp| timestamp.timestamp_ms)
            .collect::<Vec<_>>();
        let selected = if ordered_timestamps.len() == 1 {
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

    fn occurrences(rows: &[(i64, Option<i64>)]) -> Vec<StepOccurrence> {
        rows.iter()
            .map(|&(row, timestamp_ms)| StepOccurrence {
                row,
                timestamp_ms,
                bot_id: None,
            })
            .collect()
    }

    fn timestamps(rows: &[(i64, Option<i64>)]) -> Vec<StepTimestamp> {
        rows.iter()
            .map(|&(row, timestamp_ms)| StepTimestamp {
                row,
                timestamp_ms,
                bot_id: None,
            })
            .collect()
    }

    #[test]
    fn orders_rows_and_repeats_one_shared_timestamp() {
        let occurrences =
            HashMap::from([("step".to_string(), occurrences(&[(0, None), (1, None)]))]);
        let timestamp_map = HashMap::from([(
            "step".to_string(),
            timestamps(&[(20, Some(200)), (10, Some(100))]),
        )]);

        assert_eq!(
            resolve_step_timestamps(&timestamp_map, &occurrences)["step"],
            vec![100, 200]
        );

        let timestamps = HashMap::from([("step".to_string(), timestamps(&[(10, Some(100))]))]);
        assert_eq!(
            resolve_step_timestamps(&timestamps, &occurrences)["step"],
            vec![100, 100]
        );
    }

    #[test]
    fn withholds_missing_duplicate_and_conflicting_evidence() {
        let occurrences =
            HashMap::from([("step".to_string(), occurrences(&[(0, None), (1, None)]))]);
        for timestamps in [
            timestamps(&[(10, None), (20, Some(200))]),
            timestamps(&[(10, Some(100)), (10, Some(200))]),
        ] {
            let evidence = HashMap::from([("step".to_string(), timestamps)]);
            assert!(!resolve_step_timestamps(&evidence, &occurrences).contains_key("step"));
        }
    }

    #[test]
    fn embedded_timestamp_must_agree_with_aligned_step() {
        let occurrences = HashMap::from([(
            "step".to_string(),
            occurrences(&[(0, Some(100)), (1, None)]),
        )]);
        let timestamps = HashMap::from([(
            "step".to_string(),
            timestamps(&[(10, Some(101)), (20, Some(200))]),
        )]);

        assert!(!resolve_step_timestamps(&timestamps, &occurrences).contains_key("step"));
    }
}
