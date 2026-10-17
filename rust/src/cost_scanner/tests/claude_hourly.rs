//! The Claude daily walk also publishes local-hour costs, bucketed in the
//! pinned cost zone, and withholds them when any file fails to read.

use super::*;

const CHILD_MARKER: &str = "CODEXBAR_CLAUDE_HOURLY_TEST_CHILD";
const CHILD_DONE: &str = "claude hourly fixtures verified";

fn line(timestamp: DateTime<Utc>, id: &str) -> String {
    format!(
        r#"{{"type":"assistant","timestamp":"{}","requestId":"req_{id}","message":{{"id":"msg_{id}","model":"claude-sonnet-4-6","usage":{{"input_tokens":1000,"output_tokens":500}}}}}}"#,
        timestamp.to_rfc3339()
    )
}

fn child_body() {
    use crate::cost_reporting_period::{cost_bucket_zone, set_cost_bucket_zone};
    set_cost_bucket_zone("Asia/Tokyo");
    let config_dir = PathBuf::from(std::env::var_os("CLAUDE_CONFIG_DIR").unwrap());
    let project = config_dir.join("projects").join("fixture");
    std::fs::create_dir_all(&project).unwrap();

    let zone = cost_bucket_zone();
    let midnight = zone.start_of_day_utc(zone.date(Utc::now()));
    let nine = midnight + Duration::hours(9) + Duration::minutes(5);
    let ten = midnight + Duration::hours(10) + Duration::minutes(40);
    std::fs::write(
        project.join("t.jsonl"),
        format!(
            "{}\n{}\n{}\n",
            line(nine, "a"),
            line(nine + Duration::minutes(10), "b"),
            line(ten, "c")
        ),
    )
    .unwrap();

    let (daily, hourly, complete) = get_claude_daily_and_hourly_cost_history(7);
    assert!(complete);
    assert_eq!(daily.len(), 7);
    let today_cost = daily.last().unwrap().1.expect("priced day");
    assert_eq!(hourly.len(), 2, "{hourly:?}");
    assert_eq!(hourly[0].hour_start, midnight + Duration::hours(9));
    assert_eq!(hourly[0].offset_minutes, 540);
    assert_eq!(hourly[1].hour_start, midnight + Duration::hours(10));
    let hourly_total: f64 = hourly.iter().filter_map(|p| p.cost_usd).sum();
    assert!((hourly_total - today_cost).abs() < 1e-9);
    assert!(hourly[0].cost_usd.unwrap() > hourly[1].cost_usd.unwrap());

    std::fs::write(project.join("malformed.jsonl"), b"{malformed\n").unwrap();
    let (_, hourly, complete) = get_claude_daily_and_hourly_cost_history(7);
    assert!(!complete);
    assert!(hourly.is_empty(), "a partial walk publishes no hours");
    println!("{CHILD_DONE}");
}

#[test]
fn claude_hourly_costs_follow_the_pinned_zone_and_need_a_complete_walk() {
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
