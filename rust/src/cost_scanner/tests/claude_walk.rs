//! Every Claude history path (summary, chart snapshot, daily cost, daily
//! tokens) walks the same transcript roots. These fixtures pin what each path
//! reports for duplicates, incomplete requests, timestamp-less rows and
//! missing roots.

use super::*;

const CHILD_MARKER: &str = "CODEXBAR_CLAUDE_WALK_TEST_CHILD";
const CHILD_DONE: &str = "claude walk fixtures verified";

fn completed(timestamp: Option<&str>, id: &str) -> String {
    let timestamp = timestamp
        .map(|timestamp| format!(r#""timestamp":"{timestamp}","#))
        .unwrap_or_default();
    format!(
        r#"{{"type":"assistant",{timestamp}"requestId":"req_{id}","message":{{"id":"msg_{id}","model":"claude-sonnet-4-6","usage":{{"input_tokens":1000,"output_tokens":500}}}}}}"#
    )
}

fn preliminary(timestamp: &str, id: &str) -> String {
    format!(
        r#"{{"type":"assistant","timestamp":"{timestamp}","requestId":"req_{id}","message":{{"id":"msg_{id}","model":"claude-sonnet-4-6","stop_reason":null,"usage":{{"input_tokens":1000}}}}}}"#
    )
}

fn write_transcript(path: &Path, lines: &[String]) {
    std::fs::write(path, format!("{}\n", lines.join("\n"))).unwrap();
}

fn summary_line(summary: &CostSummary) -> String {
    format!(
        "sessions={} input={} output={} cost={:.6} coverage={} known_zero={} incomplete={}",
        summary.sessions_count,
        summary.input_tokens,
        summary.output_tokens,
        summary.total_cost_usd,
        summary.history_coverage_established,
        summary.known_zero,
        summary.incomplete_request_count,
    )
}

/// Each history path's view of the fixtures. The daily series span two days,
/// so yesterday shows whether a complete scan zero-fills an empty day.
fn observe(today: &str) -> String {
    let summary = CostScanner::new(1).scan_claude();
    let chart = CostScanner::new(1).scan_claude_chart_snapshot_with_cancel(None);
    let (daily_cost, daily_incomplete) = get_daily_cost_and_incomplete_history("claude", 2);
    let (daily_tokens, tokens_incomplete) = get_daily_token_history("claude", 2);
    let daily_cost: Vec<_> = daily_cost
        .iter()
        .map(|(day, cost)| (day == today, cost.map(|cost| format!("{cost:.6}"))))
        .collect();
    let daily_tokens: Vec<_> = daily_tokens
        .iter()
        .map(|(day, tokens)| (day == today, *tokens))
        .collect();
    format!(
        "summary {}\nchart {}\nchart today={} cost={:?} incomplete={} quota={} quota_coverage={}\ndaily cost={daily_cost:?} incomplete={:?}\ndaily tokens={daily_tokens:?} incomplete={tokens_incomplete}\n",
        summary_line(&summary),
        summary_line(&chart.summary),
        chart.today.tokens,
        chart.today.cost_usd.map(|cost| format!("{cost:.6}")),
        chart.daily_incomplete.len(),
        chart.quota_history.records.len(),
        chart.quota_history.history_coverage_established,
        daily_incomplete
            .iter()
            .map(|(day, count)| (day == today, *count))
            .collect::<Vec<_>>(),
    )
}

fn child_body() {
    use crate::cost_reporting_period::cost_bucket_zone;
    let config_dir = PathBuf::from(std::env::var_os("CLAUDE_CONFIG_DIR").unwrap());
    let projects = config_dir.join("projects");
    let project = projects.join("fixture");
    std::fs::create_dir_all(&project).unwrap();
    let now = Utc::now();
    let today = super::super::today::day_key(cost_bucket_zone().date(now));
    let stamp = now.to_rfc3339();
    let ts = Some(stamp.as_str());

    // A: req1 appears in three files and counts once; c.jsonl only repeats
    // it, so it is not a session.
    write_transcript(
        &project.join("a.jsonl"),
        &[completed(ts, "1"), completed(ts, "2")],
    );
    write_transcript(
        &project.join("b.jsonl"),
        &[completed(ts, "1"), completed(ts, "3")],
    );
    write_transcript(&project.join("c.jsonl"), &[completed(ts, "1")]);
    assert_eq!(
        observe(&today),
        "\
summary sessions=2 input=3000 output=1500 cost=0.031500 coverage=true known_zero=false incomplete=0
chart sessions=2 input=3000 output=1500 cost=0.031500 coverage=true known_zero=false incomplete=0
chart today=4500 cost=Some(\"0.031500\") incomplete=0 quota=3 quota_coverage=true
daily cost=[(false, Some(\"0.000000\")), (true, Some(\"0.031500\"))] incomplete=[]
daily tokens=[(false, 0), (true, 4500)] incomplete=false
"
    );

    // B: a preliminary row stays incomplete unless another file completes
    // the same request.
    write_transcript(
        &project.join("d.jsonl"),
        &[preliminary(&stamp, "4"), preliminary(&stamp, "2")],
    );
    assert_eq!(
        observe(&today),
        "\
summary sessions=2 input=3000 output=1500 cost=0.031500 coverage=false known_zero=false incomplete=1
chart sessions=2 input=3000 output=1500 cost=0.031500 coverage=false known_zero=false incomplete=1
chart today=4500 cost=Some(\"0.031500\") incomplete=1 quota=3 quota_coverage=false
daily cost=[(false, None), (true, Some(\"0.031500\"))] incomplete=[(true, 1)]
daily tokens=[(false, 0), (true, 4500)] incomplete=true
"
    );

    // C: rows without a timestamp still count but keep coverage unknown.
    write_transcript(&project.join("d.jsonl"), &[]);
    write_transcript(
        &project.join("e.jsonl"),
        &[completed(None, "5"), completed(None, "6")],
    );
    assert_eq!(
        observe(&today),
        "\
summary sessions=3 input=5000 output=2500 cost=0.052500 coverage=false known_zero=false incomplete=0
chart sessions=3 input=5000 output=2500 cost=0.052500 coverage=false known_zero=false incomplete=0
chart today=4500 cost=Some(\"0.031500\") incomplete=0 quota=3 quota_coverage=false
daily cost=[(false, None), (true, Some(\"0.031500\"))] incomplete=[]
daily tokens=[(false, 0), (true, 4500)] incomplete=true
"
    );

    // D: no transcript root at all.
    std::fs::rename(&projects, config_dir.join("projects-off")).unwrap();
    assert_eq!(
        observe(&today),
        "\
summary sessions=0 input=0 output=0 cost=0.000000 coverage=false known_zero=false incomplete=0
chart sessions=0 input=0 output=0 cost=0.000000 coverage=false known_zero=false incomplete=0
chart today=0 cost=None incomplete=0 quota=0 quota_coverage=false
daily cost=[(false, None), (true, None)] incomplete=[]
daily tokens=[(false, 0), (true, 0)] incomplete=true
"
    );
    println!("{CHILD_DONE}");
}

#[test]
fn claude_root_walk_results_agree_across_scan_paths() {
    if std::env::var_os(CHILD_MARKER).is_some() {
        child_body();
        return;
    }
    let config_dir = tempfile::tempdir().unwrap();
    let test_thread = std::thread::current();
    let test_name = test_thread.name().expect("test harness names this thread");
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test_name, "--nocapture", "--test-threads=1"])
        .env(CHILD_MARKER, "1")
        .env("CLAUDE_CONFIG_DIR", config_dir.path())
        // Every home-relative root (claude-swap, Pi, OMP) resolves here.
        .env("CODEXBAR_TEST_HOME", config_dir.path())
        .output()
        .expect("spawn isolated exact-test child");
    assert!(
        output.status.success() && String::from_utf8_lossy(&output.stdout).contains(CHILD_DONE),
        "fixture child failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
