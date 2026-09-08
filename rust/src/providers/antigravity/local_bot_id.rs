use std::collections::{HashMap, HashSet};

use super::super::local_step_resolver::StepOccurrence;
use super::{Event, PendingTimestampRow, StepTimestampScan};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ExactStepTimestamp {
    pub(super) step_uuid: String,
    pub(super) timestamp_ms: i64,
}

pub(super) fn record_exact_bot_id(
    bot_id: &str,
    step_uuid: &str,
    timestamp_ms: i64,
    by_bot_id: &mut HashMap<String, ExactStepTimestamp>,
    ambiguous_bot_ids: &mut HashSet<String>,
) {
    if ambiguous_bot_ids.contains(bot_id) {
        return;
    }

    let candidate = ExactStepTimestamp {
        step_uuid: step_uuid.to_string(),
        timestamp_ms,
    };
    match by_bot_id.get(bot_id) {
        None => {
            by_bot_id.insert(bot_id.to_string(), candidate);
        }
        Some(existing) if existing == &candidate => {}
        Some(_) => {
            by_bot_id.remove(bot_id);
            ambiguous_bot_ids.insert(bot_id.to_string());
        }
    }
}

pub(super) fn embedded_timestamps_agree(
    occurrences: &HashMap<String, Vec<StepOccurrence>>,
    step_scan: &StepTimestampScan,
    bot_id_uses: &HashMap<String, usize>,
) -> bool {
    for (step_uuid, occurrences) in occurrences {
        for occurrence in occurrences {
            let Some(bot_id) = occurrence.bot_id.as_deref() else {
                continue;
            };
            if bot_id_uses.get(bot_id).copied() != Some(1) {
                continue;
            }
            let Some(exact) = step_scan.by_bot_id.get(bot_id) else {
                continue;
            };
            if step_scan.ambiguous_bot_ids.contains(bot_id)
                || exact.step_uuid != *step_uuid
                || occurrence
                    .timestamp_ms
                    .is_some_and(|ts| ts != exact.timestamp_ms)
            {
                return false;
            }
        }
    }
    true
}

pub(super) fn append_recovered_events(
    events: &mut Vec<Event>,
    session: &str,
    pending: &[PendingTimestampRow],
    resolved: &HashMap<String, Vec<i64>>,
    occurrences: &HashMap<String, Vec<StepOccurrence>>,
    step_scan: &StepTimestampScan,
    bot_id_uses: &HashMap<String, usize>,
) -> usize {
    let mut occurrence_offsets = HashMap::<String, HashMap<i64, usize>>::new();
    for (step_uuid, occurrences) in occurrences {
        let mut rows = occurrences
            .iter()
            .map(|occurrence| occurrence.row)
            .collect::<Vec<_>>();
        rows.sort_unstable();
        if rows.windows(2).any(|pair| pair[0] == pair[1]) {
            continue;
        }
        occurrence_offsets.insert(
            step_uuid.clone(),
            rows.into_iter()
                .enumerate()
                .map(|(offset, row)| (row, offset))
                .collect(),
        );
    }

    let mut recovered = 0;
    let mut pending_rows = pending.iter().collect::<Vec<_>>();
    pending_rows.sort_by_key(|pending| pending.row);
    for pending in pending_rows {
        if let Some(bot_id) = pending
            .turn
            .usage
            .as_ref()
            .and_then(|usage| usage.bot_id.as_deref())
        {
            if step_scan.ambiguous_bot_ids.contains(bot_id) {
                continue;
            }
            if let Some(exact) = step_scan.by_bot_id.get(bot_id) {
                if exact.step_uuid != pending.step_uuid {
                    continue;
                }
                if bot_id_uses.get(bot_id) == Some(&1) {
                    let mut turn = pending.turn.clone();
                    turn.timestamp_ms = Some(exact.timestamp_ms);
                    events.push(Event {
                        session: session.to_string(),
                        row: pending.row,
                        turn,
                        total: pending.total,
                    });
                    recovered += 1;
                    continue;
                }
            }
        }
        let Some(timestamps) = resolved.get(&pending.step_uuid) else {
            continue;
        };
        let Some(offset) = occurrence_offsets
            .get(&pending.step_uuid)
            .and_then(|rows| rows.get(&pending.row))
        else {
            continue;
        };
        let Some(timestamp_ms) = timestamps.get(*offset) else {
            continue;
        };
        let mut turn = pending.turn.clone();
        turn.timestamp_ms = Some(*timestamp_ms);
        events.push(Event {
            session: session.to_string(),
            row: pending.row,
            turn,
            total: pending.total,
        });
        recovered += 1;
    }
    events.sort_by_key(|event| event.row);
    recovered
}
