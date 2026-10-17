//! Additional Gemini profile homes: a conversation copied into a second home
//! must count once, while distinct conversations in each home stay separate.

use super::tests::valid_turn_blob;
use super::*;
use rusqlite::params;

fn write_conversation(root: &Path, name: &str, rows: &[(i64, Vec<u8>)]) {
    fs::create_dir_all(root).unwrap();
    let conn = Connection::open(root.join(format!("{name}.db"))).unwrap();
    conn.execute("CREATE TABLE gen_metadata(idx INTEGER, data BLOB)", [])
        .unwrap();
    for (idx, blob) in rows {
        conn.execute(
            "INSERT INTO gen_metadata(idx, data) VALUES(?1, ?2)",
            params![idx, blob],
        )
        .unwrap();
    }
}

fn complete_summary(roots: &[PathBuf], identity: SessionIdentity) -> LocalTokenHistorySummary {
    match summarize_with_identity(roots, Utc::now(), 30, identity) {
        SQLiteScan::Summary(summary) => summary,
        _ => panic!("supported databases should produce a summary"),
    }
}

fn conversation_roots(home: &Path) -> Vec<PathBuf> {
    super::database_roots(&home.join(".gemini")).to_vec()
}

#[test]
fn copied_conversation_across_homes_counts_once_by_file_stem() {
    let dir = tempfile::tempdir().unwrap();
    let timestamp = u64::try_from(Utc::now().timestamp()).unwrap();
    let rows = [(1_i64, valid_turn_blob(100, timestamp))];
    let primary = conversation_roots(&dir.path().join("primary"));
    let profile = conversation_roots(&dir.path().join("profile"));
    write_conversation(&primary[0], "conv-a", &rows);
    write_conversation(&profile[0], "conv-a", &rows);

    let single = complete_summary(&primary, SessionIdentity::FileStem);
    let union = [primary.clone(), profile.clone()].concat();
    let summary = complete_summary(&union, SessionIdentity::FileStem);

    assert_eq!(summary.coverage, LocalHistoryCoverage::Complete);
    assert!(single.total_tokens > 0);
    assert_eq!(summary.total_tokens, single.total_tokens);
    assert_eq!(summary.session_count, 1);
}

#[test]
fn copied_conversation_keeps_rows_the_copy_adds() {
    let dir = tempfile::tempdir().unwrap();
    let timestamp = u64::try_from(Utc::now().timestamp()).unwrap();
    let first = valid_turn_blob(100, timestamp);
    let second = valid_turn_blob(300, timestamp);
    let primary = conversation_roots(&dir.path().join("primary"));
    let profile = conversation_roots(&dir.path().join("profile"));
    write_conversation(&primary[0], "conv-a", &[(1, first.clone())]);
    write_conversation(&profile[0], "conv-a", &[(1, first), (2, second)]);

    // The profile copy is a superset of the primary copy, so the union must
    // equal the profile alone rather than the sum of both.
    let profile_alone = complete_summary(&profile, SessionIdentity::FileStem);
    let union = [primary.clone(), profile.clone()].concat();
    let summary = complete_summary(&union, SessionIdentity::FileStem);

    assert_eq!(summary.coverage, LocalHistoryCoverage::Complete);
    assert_eq!(summary.total_tokens, profile_alone.total_tokens);
    assert_eq!(summary.session_count, 1);
}

#[test]
fn conflicting_copies_across_homes_are_withheld() {
    let dir = tempfile::tempdir().unwrap();
    let timestamp = u64::try_from(Utc::now().timestamp()).unwrap();
    let primary = conversation_roots(&dir.path().join("primary"));
    let profile = conversation_roots(&dir.path().join("profile"));
    write_conversation(
        &primary[0],
        "conv-a",
        &[(1, valid_turn_blob(100, timestamp))],
    );
    write_conversation(
        &profile[0],
        "conv-a",
        &[(1, valid_turn_blob(200, timestamp))],
    );

    let union = [primary, profile].concat();
    let summary = complete_summary(&union, SessionIdentity::FileStem);

    // Two copies that disagree on one row cannot publish either total.
    assert_ne!(summary.coverage, LocalHistoryCoverage::Complete);
    assert_eq!(summary.total_tokens, 0);
}

#[test]
fn distinct_conversations_in_two_homes_are_both_counted() {
    let dir = tempfile::tempdir().unwrap();
    let timestamp = u64::try_from(Utc::now().timestamp()).unwrap();
    let primary = conversation_roots(&dir.path().join("primary"));
    let profile = conversation_roots(&dir.path().join("profile"));
    write_conversation(
        &primary[0],
        "conv-a",
        &[(1, valid_turn_blob(100, timestamp))],
    );
    write_conversation(
        &profile[0],
        "conv-b",
        &[(1, valid_turn_blob(200, timestamp))],
    );

    let primary_alone = complete_summary(&primary, SessionIdentity::FileStem);
    let profile_alone = complete_summary(&profile, SessionIdentity::FileStem);
    let union = [primary, profile].concat();
    let summary = complete_summary(&union, SessionIdentity::FileStem);

    assert_eq!(summary.coverage, LocalHistoryCoverage::Complete);
    assert_eq!(
        summary.total_tokens,
        primary_alone.total_tokens + profile_alone.total_tokens
    );
    assert_eq!(summary.session_count, 2);
}

#[test]
fn missing_home_roots_contribute_nothing_and_stay_complete() {
    let dir = tempfile::tempdir().unwrap();
    let timestamp = u64::try_from(Utc::now().timestamp()).unwrap();
    let primary = conversation_roots(&dir.path().join("primary"));
    let missing = conversation_roots(&dir.path().join("missing"));
    write_conversation(
        &primary[0],
        "conv-a",
        &[(1, valid_turn_blob(100, timestamp))],
    );

    let alone = complete_summary(&primary, SessionIdentity::FileStem);
    let union = [primary, missing].concat();
    let summary = complete_summary(&union, SessionIdentity::FileStem);

    assert_eq!(summary.coverage, LocalHistoryCoverage::Complete);
    assert_eq!(summary.total_tokens, alone.total_tokens);
}

#[test]
fn same_stem_in_a_different_relative_root_is_not_a_copy_across_homes() {
    // A copy keeps its relative root (profile/antigravity-cli/conversations
    // mirrors primary/antigravity-cli/conversations) and dedupes; the same stem
    // under another relative root is a separate conversation, as it is in one home.
    let dir = tempfile::tempdir().unwrap();
    let timestamp = u64::try_from(Utc::now().timestamp()).unwrap();
    let rows = [(1_i64, valid_turn_blob(100, timestamp))];
    let primary = conversation_roots(&dir.path().join("primary"));
    let profile = conversation_roots(&dir.path().join("profile"));
    write_conversation(&primary[0], "conv-a", &rows);
    write_conversation(&profile[0], "conv-a", &rows);
    write_conversation(&profile[1], "conv-a", &rows);

    let one_conversation = complete_summary(&primary[..1], SessionIdentity::FileStem);
    let union = [primary, profile].concat();
    let summary = complete_summary(&union, SessionIdentity::FileStem);

    assert_eq!(summary.coverage, LocalHistoryCoverage::Complete);
    assert_eq!(summary.session_count, 2);
    assert_eq!(summary.total_tokens, one_conversation.total_tokens * 2);
}

#[test]
fn path_identity_still_keeps_same_named_databases_in_one_home_distinct() {
    // Single-home default: the reader keys sessions by full path, so a same-named
    // database in another root of the same home is a different session.
    let dir = tempfile::tempdir().unwrap();
    let timestamp = u64::try_from(Utc::now().timestamp()).unwrap();
    let home = conversation_roots(&dir.path().join("home"));
    write_conversation(&home[0], "conv-a", &[(1, valid_turn_blob(100, timestamp))]);
    write_conversation(&home[1], "conv-a", &[(1, valid_turn_blob(200, timestamp))]);

    let summary = complete_summary(&home, SessionIdentity::Path);
    assert_eq!(summary.coverage, LocalHistoryCoverage::Complete);
    assert_eq!(summary.session_count, 2);
}
