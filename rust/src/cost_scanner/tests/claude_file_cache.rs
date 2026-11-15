//! Per-file Claude transcript cache: appended bytes are parsed once, and a
//! cached replay matches a cold scan.

use super::*;
use crate::cost_scanner::claude_file_cache::{cached_line_count, cached_repeat_count};
use std::fs::OpenOptions;

fn usage_row(request: &str, input: u64) -> String {
    usage_row_at(request, input, Utc::now() - Duration::hours(1))
}

fn usage_row_at(request: &str, input: u64, at: DateTime<Utc>) -> String {
    let ts = at.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string();
    format!(
        r#"{{"type":"assistant","timestamp":"{ts}","requestId":"{request}","message":{{"id":"msg_{request}","model":"claude-sonnet-4-6","stop_reason":"end_turn","usage":{{"input_tokens":{input},"output_tokens":10}}}}}}"#
    )
}

/// Scan one file with fresh walk state, returning (counted, input tokens,
/// malformed lines).
fn scan(path: &Path) -> (usize, u64, u32) {
    let cutoff = Utc::now() - Duration::days(1);
    let mut seen = HashSet::new();
    let mut pricing = ClaudeScanPricingResolver::default();
    let mut incomplete = ClaudeIncompleteTracker::default();
    let mut input = 0;
    let result = scan_claude_file_with_pricing(
        path,
        &cutoff,
        &mut seen,
        None,
        &mut pricing,
        &mut incomplete,
        |record| input += record.input,
    );
    (result.counted, input, result.malformed_lines)
}

fn append(path: &Path, text: &str) {
    let mut file = OpenOptions::new().append(true).open(path).unwrap();
    file.write_all(text.as_bytes()).unwrap();
}

#[test]
fn appended_rows_are_added_to_the_cached_rows() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("transcript.jsonl");
    std::fs::write(
        &path,
        format!("{}\n{}\n", usage_row("a", 100), usage_row("b", 200)),
    )
    .unwrap();

    assert_eq!(scan(&path), (2, 300, 0));
    assert_eq!(cached_line_count(&path), Some(2));

    append(&path, &format!("{}\n", usage_row("c", 400)));
    assert_eq!(scan(&path), (3, 700, 0));
    assert_eq!(cached_line_count(&path), Some(3));

    // Unchanged file: replayed from the cache with the same result.
    assert_eq!(scan(&path), (3, 700, 0));
}

#[test]
fn unterminated_last_line_is_counted_but_not_committed() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("transcript.jsonl");
    std::fs::write(
        &path,
        format!("{}\n{}", usage_row("a", 1), usage_row("b", 2)),
    )
    .unwrap();

    assert_eq!(scan(&path), (2, 3, 0));
    assert_eq!(cached_line_count(&path), Some(1));

    // The writer finishes the line and adds another: nothing counts twice.
    append(&path, &format!("\n{}\n", usage_row("c", 4)));
    assert_eq!(scan(&path), (3, 7, 0));
    assert_eq!(cached_line_count(&path), Some(3));
}

#[test]
fn rewritten_file_is_parsed_again() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("transcript.jsonl");
    std::fs::write(
        &path,
        format!("{}\n{}\n", usage_row("a", 100), usage_row("b", 200)),
    )
    .unwrap();
    assert_eq!(scan(&path), (2, 300, 0));

    // Shorter replacement.
    std::fs::write(&path, format!("{}\n", usage_row("x", 5))).unwrap();
    assert_eq!(scan(&path), (1, 5, 0));

    // Longer replacement whose earlier bytes differ from the cached prefix.
    std::fs::write(
        &path,
        format!(
            "{}\n{}\n{}\n",
            usage_row("y", 7),
            usage_row("z", 11),
            usage_row("w", 13)
        ),
    )
    .unwrap();
    assert_eq!(scan(&path), (3, 31, 0));
}

#[test]
fn malformed_lines_and_cross_scan_dedup_survive_the_cache() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("transcript.jsonl");
    std::fs::write(
        &path,
        format!(
            "{}\n{{broken\n{}\n",
            usage_row("a", 100),
            usage_row("a", 100)
        ),
    )
    .unwrap();

    // The duplicate request counts once and the broken line stays malformed,
    // on the cold scan and on the cached replay alike.
    assert_eq!(scan(&path), (1, 100, 1));
    assert_eq!(scan(&path), (1, 100, 1));

    // A shared `seen` set still de-duplicates across files on replay.
    let other = root.path().join("copy.jsonl");
    std::fs::write(&other, format!("{}\n", usage_row("a", 100))).unwrap();
    let cutoff = Utc::now() - Duration::days(1);
    let mut seen = HashSet::new();
    let mut pricing = ClaudeScanPricingResolver::default();
    let mut incomplete = ClaudeIncompleteTracker::default();
    let mut counted = 0;
    for file in [&path, &other] {
        counted += scan_claude_file_with_pricing(
            file,
            &cutoff,
            &mut seen,
            None,
            &mut pricing,
            &mut incomplete,
            |_| {},
        )
        .counted;
    }
    assert_eq!(counted, 1);
}

#[test]
fn repeated_content_block_lines_are_stored_compactly() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("transcript.jsonl");
    let at = Utc::now() - Duration::hours(2);
    // One assistant message split over three lines, as Claude Code writes it.
    std::fs::write(
        &path,
        format!(
            "{}\n{}\n{}\n",
            usage_row_at("a", 100, at),
            usage_row_at("a", 100, at + Duration::milliseconds(5)),
            usage_row_at("a", 100, at + Duration::milliseconds(9)),
        ),
    )
    .unwrap();

    assert_eq!(scan(&path), (1, 100, 0));
    assert_eq!(cached_line_count(&path), Some(3));
    assert_eq!(cached_repeat_count(&path), Some(2));
    assert_eq!(scan(&path), (1, 100, 0));
}

#[test]
fn repeat_after_the_cutoff_counts_when_its_anchor_is_before_it() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("transcript.jsonl");
    let before = Utc::now() - Duration::days(2);
    let after = Utc::now() - Duration::hours(1);
    std::fs::write(
        &path,
        format!(
            "{}\n{}\n",
            usage_row_at("a", 100, before),
            usage_row_at("a", 100, after),
        ),
    )
    .unwrap();

    // The cutoff (one day ago) falls between the two lines, so the second one
    // is the row a cold scan counts, with its own timestamp.
    for _ in 0..2 {
        let cutoff = Utc::now() - Duration::days(1);
        let mut timestamps = Vec::new();
        let result = scan_claude_file_with_pricing(
            &path,
            &cutoff,
            &mut HashSet::new(),
            None,
            &mut ClaudeScanPricingResolver::default(),
            &mut ClaudeIncompleteTracker::default(),
            |record| timestamps.push(record.timestamp),
        );
        assert_eq!(result.counted, 1);
        assert_eq!(timestamps.len(), 1);
        let counted = timestamps[0].expect("timestamped row");
        assert!(counted > cutoff, "the in-window line is the one counted");
    }
    assert_eq!(cached_repeat_count(&path), Some(1));
}

#[test]
fn repeat_with_different_usage_is_kept_as_its_own_row() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("transcript.jsonl");
    std::fs::write(
        &path,
        format!("{}\n{}\n", usage_row("a", 100), usage_row("a", 150)),
    )
    .unwrap();

    // Same key, different usage: not compacted, and the first one still wins.
    assert_eq!(scan(&path), (1, 100, 0));
    assert_eq!(cached_repeat_count(&path), Some(0));
}
