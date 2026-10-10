//! Weekday/hour activity cells: built from sessions in the local zone, and
//! merged across sources.

use super::*;
use crate::codex_workspaces::{CostEstimate, UsageTotals};
use chrono::{DateTime, Datelike, Local, Timelike};
use std::collections::BTreeMap;

fn at(timestamp: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(timestamp)
        .unwrap()
        .with_timezone(&Utc)
}

fn local_cell(timestamp: DateTime<Utc>) -> (u8, u8) {
    let local = timestamp.with_timezone(&Local);
    (
        u8::try_from(local.weekday().num_days_from_monday()).unwrap(),
        u8::try_from(local.hour()).unwrap(),
    )
}

pub(in crate::spend_contract) fn cells(activity: &[SpendActivityCell]) -> Vec<(u8, u8, u32)> {
    activity
        .iter()
        .map(|cell| (cell.weekday, cell.hour, cell.conversations))
        .collect()
}

/// Expected cells for one conversation per timestamp, in key order.
pub(in crate::spend_contract) fn counted(timestamps: &[DateTime<Utc>]) -> Vec<(u8, u8, u32)> {
    let mut expected: BTreeMap<(u8, u8), u32> = BTreeMap::new();
    for timestamp in timestamps {
        *expected.entry(local_cell(*timestamp)).or_default() += 1;
    }
    expected
        .into_iter()
        .map(|((weekday, hour), conversations)| (weekday, hour, conversations))
        .collect()
}

fn session(
    started_at: Option<DateTime<Utc>>,
    latest_activity: Option<DateTime<Utc>>,
) -> SessionUsage {
    SessionUsage {
        id: "s".into(),
        project_id: "p".into(),
        display_title: "s".into(),
        cwd: None,
        started_at,
        latest_activity,
        totals: UsageTotals::from_parts(1, 0, 0),
        cost_estimate: CostEstimate::default(),
        top_model: None,
    }
}

#[test]
fn session_activity_counts_latest_activity_or_start_per_local_cell() {
    let monday = at("2026-08-17T10:15:00Z");
    let tuesday = at("2026-08-18T23:45:00Z");
    let activity = activity_from_sessions(&[
        session(Some(tuesday), Some(monday)),
        session(Some(tuesday), None),
        session(None, None),
        session(None, Some(monday)),
    ]);
    assert_eq!(cells(&activity), counted(&[monday, monday, tuesday]));
}

#[test]
fn merged_activity_sums_cells_and_keeps_the_last_duplicate_on_the_left() {
    let cell = |weekday, hour, conversations| SpendActivityCell {
        weekday,
        hour,
        conversations,
    };
    let merged = merge_activity(
        vec![cell(1, 2, 5), cell(0, 9, 1), cell(1, 2, 3)],
        &[
            cell(1, 2, 4),
            cell(6, 23, u32::MAX),
            cell(6, 23, 1),
            cell(0, 9, 2),
        ],
    );
    assert_eq!(
        cells(&merged),
        vec![(0, 9, 3), (1, 2, 7), (6, 23, u32::MAX)]
    );
}
