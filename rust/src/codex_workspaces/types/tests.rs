use super::*;

fn session(id: &str, cost: Option<f64>, tokens: u64, minutes: i64) -> SessionUsage {
    SessionUsage {
        id: id.to_string(),
        project_id: "p".to_string(),
        display_title: id.to_string(),
        cwd: None,
        started_at: None,
        latest_activity: Some(DateTime::<Utc>::UNIX_EPOCH + chrono::Duration::minutes(minutes)),
        totals: UsageTotals::from_parts(tokens, 0, 0),
        cost_estimate: match cost {
            Some(known_usd) => CostEstimate {
                known_usd,
                unknown_tokens: 0,
            },
            None => CostEstimate {
                known_usd: 0.0,
                unknown_tokens: tokens,
            },
        },
        top_model: None,
        turn_performance: None,
    }
}

fn ranked(mut sessions: Vec<SessionUsage>) -> Vec<String> {
    sessions.sort_by(SessionUsage::rank_cmp);
    sessions.into_iter().map(|s| s.id).collect()
}

#[test]
fn ranking_orders_by_cost_then_tokens_then_activity_then_id_with_unpriced_last() {
    let sessions = vec![
        session("b", Some(1.0), 10, 5),
        session("a", Some(1.0), 10, 5),
        session("older", Some(1.0), 10, 1),
        session("more-tokens", Some(1.0), 20, 5),
        session("free", Some(0.0), 10, 5),
        session("unpriced", None, 999, 5),
        session("expensive", Some(5.0), 1, 0),
    ];
    let expected = [
        "expensive",
        "more-tokens",
        "a",
        "b",
        "older",
        "free",
        "unpriced",
    ];
    for offset in 0..sessions.len() {
        let mut rotated = sessions.clone();
        rotated.rotate_left(offset);
        assert_eq!(ranked(rotated.clone()), expected);
        rotated.reverse();
        assert_eq!(ranked(rotated), expected);
    }
}

#[test]
fn partially_priced_sessions_rank_by_their_known_cost() {
    let mut partial = session("partial", Some(2.0), 10, 0);
    partial.cost_estimate.unknown_tokens = 4;
    assert_eq!(partial.ranking_cost_usd(), Some(2.0));
    let unpriced = session("unpriced", None, 10, 0);
    assert_eq!(unpriced.ranking_cost_usd(), None);
}

#[test]
fn short_session_id_keeps_first_four_and_last_eight_of_long_ids() {
    assert_eq!(short_session_id("abc"), "abc");
    assert_eq!(short_session_id("  exactly-12ch "), "exactly-12ch");
    assert_eq!(
        short_session_id("019f79b9-1790-7921-8d6f-258a1e92b191"),
        "019f...1e92b191"
    );
    assert_eq!(short_session_id("fixture-session"), "fixt...-session");
    assert_eq!(
        untitled_session_label("019f79b9-1790-7921-8d6f-258a1e92b191"),
        "Session 019f...1e92b191"
    );
}
