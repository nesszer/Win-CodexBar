//! Qwen Cloud Team tests, ported from upstream `QwenCloudTeamPluginTests`.

use std::time::Duration;

use chrono::DateTime;
use mockito::Matcher;
use serde_json::{Map, Value, json};

use super::team::{
    billing_selector, choose_result, fetch, parse_response, parse_summary, rpc_result, sec_token,
    summary_params, to_result,
};
use crate::core::{ProviderError, ProviderFetchResult};

const NOW_MS: i64 = 1_789_500_000_000;

/// Sanitized upstream #3711 response.
const SUMMARY: &str = r#"{"code":"200","successResponse":true,"data":{"Code":"Success","Success":true,"Data":{
  "StartTime":1784894400000,"EndTime":1790251200000,"ProductCode":"sfm_tokenplanteams_dp_intl",
  "SubscriptionGroupList":[{"SpecType":"standard","SubscriptionAssignedNumber":1,
    "SubscriptionTotalNumber":1,"NextCycleFlushTime":1790251200000,"EquityList":[
      {"EquityCode":"credit_value","TotalValue":"25000.00000000","SurplusValue":"6373.93922996"}
    ]}]
}}}"#;

fn object(text: &str) -> Map<String, Value> {
    match serde_json::from_str(text).unwrap() {
        Value::Object(map) => map,
        other => panic!("not an object: {other}"),
    }
}

fn summary_of(envelope: &str) -> Result<Map<String, Value>, ProviderError> {
    rpc_result("GetSeatSubscriptionSummary", &object(envelope))
}

fn parse(envelope: &str, now_ms: i64) -> Result<Option<super::team::TeamUsage>, ProviderError> {
    parse_summary(&summary_of(envelope)?, now_ms)
}

/// Replace a field inside the single subscription group of [`SUMMARY`].
fn with_group(field: &str, value: Value) -> String {
    let mut root: Value = serde_json::from_str(SUMMARY).unwrap();
    root["data"]["Data"]["SubscriptionGroupList"][0][field] = value;
    root.to_string()
}

fn with_data(field: &str, value: Value) -> String {
    let mut root: Value = serde_json::from_str(SUMMARY).unwrap();
    root["data"]["Data"][field] = value;
    root.to_string()
}

fn message(error: ProviderError) -> String {
    match error {
        ProviderError::Other(message) => message,
        other => panic!("expected a verbatim message, got {other:?}"),
    }
}

fn unsupported(field: &str) -> String {
    format!("Unsupported Qwen Cloud Team {field}. Check Usage Dashboard.")
}

fn rows(result: &ProviderFetchResult) -> Vec<(String, String, Option<String>)> {
    result
        .display_details()
        .iter()
        .map(|row| {
            (
                row.title().to_string(),
                row.value().to_string(),
                row.section_title().map(str::to_string),
            )
        })
        .collect()
}

#[test]
fn maps_the_reported_credit_pool() {
    let team = parse(SUMMARY, NOW_MS).unwrap().unwrap();
    let result = to_result(&team);
    let primary = &result.usage.primary;
    assert!((primary.used_percent - 74.504_243_080_16).abs() < 1e-8);
    assert_eq!(
        primary.resets_at,
        DateTime::from_timestamp_millis(1_790_251_200_000)
    );
    assert_eq!(primary.window_minutes, None);
    assert_eq!(primary.reset_description.as_deref(), Some("Team"));
    assert_eq!(result.usage.primary_label.as_deref(), Some("Team"));
    assert_eq!(result.usage.login_method.as_deref(), Some("Standard Team"));
    let section = Some("Team Token Plan".to_string());
    assert_eq!(
        rows(&result),
        vec![
            (
                "Credits".into(),
                "18,626.06 / 25,000 credits used".into(),
                section.clone()
            ),
            (
                "Remaining".into(),
                "6,373.94 credits".into(),
                section.clone()
            ),
            ("Seats".into(), "1 / 1".into(), section),
        ]
    );
}

#[test]
fn credit_equity_and_reset_are_independent_of_list_order() {
    let payload = SUMMARY
        .replace(
            r#""EquityList":["#,
            r#""EquityList":[{"EquityCode":"seats","TotalValue":"999"},"#,
        )
        .replace(
            r#""NextCycleFlushTime":1790251200000"#,
            r#""NextCycleFlushTime":1790000000000"#,
        );
    let team = parse(&payload, NOW_MS).unwrap().unwrap();
    assert!((team.used / team.total * 100.0 - 74.504_243_080_16).abs() < 1e-8);
    assert_eq!(
        team.resets_at,
        DateTime::from_timestamp_millis(1_790_000_000_000)
    );
}

#[test]
fn no_active_subscription_leaves_individual_available() {
    for payload in [
        r#"{"successResponse":true,"data":{"Success":true,"Data":null}}"#,
        // Empty groups win before the (absent) period is read.
        r#"{"successResponse":true,"data":{"Success":true,"Data":{"SubscriptionGroupList":[]}}}"#,
    ] {
        assert_eq!(parse(payload, NOW_MS).unwrap(), None, "{payload}");
    }
}

#[test]
fn expired_and_future_subscriptions_present_no_pool() {
    assert_eq!(parse(SUMMARY, 1_800_000_000_000).unwrap(), None);
    assert_eq!(parse(SUMMARY, 1_790_251_200_000).unwrap(), None);
    assert_eq!(parse(SUMMARY, 1_784_894_399_999).unwrap(), None);
}

#[test]
fn invalid_remaining_credits_are_errors() {
    for remaining in [
        "-1",
        "NaN",
        "26000",
        "9007199254740992",
        "1e3",
        "+5",
        ".5",
        " 5",
    ] {
        let payload = SUMMARY.replace("6373.93922996", remaining);
        let error = parse(&payload, NOW_MS).unwrap_err();
        assert_eq!(message(error), unsupported("credit count"), "{remaining}");
    }
}

#[test]
fn malformed_successful_envelopes_and_multiple_pools_are_errors() {
    let error = parse(r#"{"successResponse":true,"data":{}}"#, NOW_MS).unwrap_err();
    assert_eq!(message(error), unsupported("subscription"));

    let mut root: Value = serde_json::from_str(SUMMARY).unwrap();
    let group = root["data"]["Data"]["SubscriptionGroupList"][0].clone();
    root["data"]["Data"]["SubscriptionGroupList"] = json!([group.clone(), group]);
    let error = parse(&root.to_string(), NOW_MS).unwrap_err();
    assert_eq!(message(error), unsupported("multiple credit pools"));
}

#[test]
fn duplicate_or_missing_credit_equities_are_errors() {
    let credit = json!({"EquityCode":"credit_value","TotalValue":"10","SurplusValue":"5"});
    for list in [json!([credit.clone(), credit]), json!([]), json!("x")] {
        let error = parse(&with_group("EquityList", list), NOW_MS).unwrap_err();
        assert_eq!(message(error), unsupported("credit equity"));
    }
}

#[test]
fn checks_run_in_upstream_order() {
    let cases = [
        (
            with_data("SubscriptionGroupList", json!({})),
            "subscription groups",
        ),
        (with_data("StartTime", json!("soon")), "credit count"),
        (with_data("StartTime", json!(1.5)), "subscription period"),
        (with_data("StartTime", json!(0)), "subscription period"),
        (
            with_data("EndTime", json!(1_784_894_400_000_u64)),
            "subscription period",
        ),
        (
            with_data("ProductCode", json!("other")),
            "subscription product",
        ),
        (
            with_data("SubscriptionGroupList", json!(["not-a-group"])),
            "subscription group",
        ),
        (with_group("NextCycleFlushTime", json!(1.5)), "cycle reset"),
        (with_group("NextCycleFlushTime", json!(0)), "cycle reset"),
        (
            with_group("NextCycleFlushTime", json!("later")),
            "credit count",
        ),
    ];
    for (payload, field) in cases {
        let error = parse(&payload, NOW_MS).unwrap_err();
        assert_eq!(message(error), unsupported(field), "{payload}");
    }
    // An expired period still fails on malformed groups, which come first.
    let payload = with_data("SubscriptionGroupList", json!(null));
    let error = parse(&payload, 1_800_000_000_000).unwrap_err();
    assert_eq!(message(error), unsupported("subscription groups"));
}

#[test]
fn null_reset_and_string_numbers_are_accepted() {
    let team = parse(&with_group("NextCycleFlushTime", Value::Null), NOW_MS)
        .unwrap()
        .unwrap();
    assert_eq!(team.resets_at, None);
    let payload = with_data("StartTime", json!("1784894400000"));
    assert!(parse(&payload, NOW_MS).unwrap().is_some());
}

#[test]
fn seats_row_needs_json_integers_within_range() {
    for (assigned, seats) in [
        (json!("1"), json!(1)),
        (json!(-1), json!(1)),
        (json!(3), json!(2)),
        (json!(1.5), json!(2)),
    ] {
        let payload = with_group("SubscriptionAssignedNumber", assigned.clone());
        let mut root: Value = serde_json::from_str(&payload).unwrap();
        root["data"]["Data"]["SubscriptionGroupList"][0]["SubscriptionTotalNumber"] = seats;
        let team = parse(&root.to_string(), NOW_MS).unwrap().unwrap();
        assert_eq!(team.seats, None, "{assigned}");
        assert!(
            to_result(&team)
                .display_details()
                .iter()
                .all(|r| r.title() != "Seats")
        );
    }
}

#[test]
fn spec_type_must_match_upstream_pattern() {
    let cases = [
        (json!("standard"), "Standard Team"),
        (json!("pro_2-x"), "Pro_2-x Team"),
        (json!("Standard"), "Team Token Plan"),
        (json!("édition"), "Team Token Plan"),
        (json!("has space"), "Team Token Plan"),
        (json!("a".repeat(41)), "Team Token Plan"),
        (json!(7), "Team Token Plan"),
        (Value::Null, "Team Token Plan"),
    ];
    for (spec, expected) in cases {
        let team = parse(&with_group("SpecType", spec.clone()), NOW_MS)
            .unwrap()
            .unwrap();
        assert_eq!(team.login_method, expected, "{spec}");
    }
    let team = parse(&with_group("SpecType", json!("a".repeat(40))), NOW_MS)
        .unwrap()
        .unwrap();
    assert_eq!(team.login_method, format!("A{} Team", "a".repeat(39)));
}

#[test]
fn rpc_envelopes_follow_upstream() {
    let failed = "Qwen Cloud Team GetSeatSubscriptionSummary failed.";
    for envelope in [
        r#"{"successResponse":false,"data":{"Success":true}}"#,
        r#"{"successResponse":true,"data":{"Success":false}}"#,
    ] {
        assert_eq!(message(summary_of(envelope).unwrap_err()), failed);
    }
    for envelope in [
        r#"{"data":{"Success":true}}"#,
        r#"{"successResponse":true,"data":[]}"#,
        r#"{"code":200,"data":{}}"#,
    ] {
        assert_eq!(
            message(summary_of(envelope).unwrap_err()),
            unsupported("gateway envelope"),
            "{envelope}"
        );
    }
    assert!(summary_of(r#"{"code":"200","data":{}}"#).is_ok());
}

#[test]
fn statuses_and_bodies_map_to_upstream_errors() {
    let cases: [(u16, &str, &str); 7] = [
        (401, "{}", "Qwen Cloud Team session expired. Sign in again."),
        (403, "{}", "Qwen Cloud Team session expired. Sign in again."),
        (429, "{}", "Qwen Cloud Team rate limit reached."),
        (201, "{}", "Qwen Cloud Team returned HTTP 201."),
        (500, "{}", "Qwen Cloud Team returned HTTP 500."),
        (200, "not json", ""),
        (200, "[]", ""),
    ];
    for (status, body, expected) in cases {
        let error = message(parse_response(status, body.as_bytes()).unwrap_err());
        let expected = if expected.is_empty() {
            unsupported("response")
        } else {
            expected.to_string()
        };
        assert_eq!(error, expected, "{status} {body}");
    }
    for code in [
        "ConsoleNeedLogin",
        "BailianGateway.Login.NotLogined",
        "NO_LOGIN",
    ] {
        let body = format!(r#"{{"code":"{code}"}}"#);
        assert_eq!(
            message(parse_response(200, body.as_bytes()).unwrap_err()),
            "Qwen Cloud Team login required."
        );
    }
}

#[test]
fn sec_token_comes_only_from_info_data() {
    assert_eq!(
        sec_token(&object(r#"{"data":{"secToken":"tok"}}"#)).unwrap(),
        "tok"
    );
    for info in [
        r#"{"data":{"secToken":"  "}}"#,
        r#"{"data":{"sec_token":"tok"}}"#,
        r#"{"secToken":"tok"}"#,
        r#"{"data":{"secToken":5}}"#,
    ] {
        assert_eq!(
            message(sec_token(&object(info)).unwrap_err()),
            "Qwen Cloud Team login required."
        );
    }
}

#[test]
fn billing_selector_is_exact_and_untrimmed() {
    let human = |nbid: Value| {
        let mut map = Map::new();
        map.insert("Data".into(), json!({ "SellerInfoDto": { "Nbid": nbid } }));
        map
    };
    assert_eq!(billing_selector(&Map::new()).unwrap(), None);
    assert_eq!(billing_selector(&human(Value::Null)).unwrap(), None);
    assert_eq!(billing_selector(&human(json!(""))).unwrap(), None);
    assert_eq!(
        billing_selector(&human(json!(" nb-1 ")))
            .unwrap()
            .as_deref(),
        Some(" nb-1 ")
    );
    for bad in [json!("   "), json!(5)] {
        assert_eq!(
            message(billing_selector(&human(bad)).unwrap_err()),
            unsupported("billing selector")
        );
    }
    // A nested selector elsewhere is not the console's selector.
    let mut nested = Map::new();
    nested.insert("Other".into(), json!({ "SellerInfoDto": { "Nbid": "x" } }));
    assert_eq!(billing_selector(&nested).unwrap(), None);
    assert_eq!(
        summary_params(None),
        r#"{"productCode":"sfm_tokenplanteams_dp_intl"}"#
    );
}

#[test]
fn team_error_wins_when_individual_also_fails() {
    let team = || Some(ProviderError::Other("team".into()));
    let individual_err = || Err::<u8, _>(ProviderError::Other("individual".into()));
    assert_eq!(
        message(choose_result(team(), individual_err()).unwrap_err()),
        "team"
    );
    assert_eq!(choose_result(team(), Ok(1)).unwrap(), 1);
    assert_eq!(
        message(choose_result(None, individual_err()).unwrap_err()),
        "individual"
    );
}

#[tokio::test]
async fn sends_upstream_requests_and_maps_the_pool() {
    let mut server = mockito::Server::new_async().await;
    let referer = format!("{}/analytics/token-plan/team", server.url());
    let info = server
        .mock("GET", "/tool/user/info.json")
        .match_header("cookie", "session=synthetic")
        .match_header("accept", "application/json")
        .match_header("referer", referer.as_str())
        .with_status(200)
        .with_body(r#"{"data":{"secToken":"tok"}}"#)
        .create_async()
        .await;
    let human = server
        .mock("POST", "/data/api.json")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("product".into(), "ea-service".into()),
            Matcher::UrlEncoded("action".into(), "LoadHumanInfo".into()),
        ]))
        .match_header("origin", server.url().as_str())
        .match_header("referer", referer.as_str())
        .match_body(Matcher::AllOf(vec![
            Matcher::UrlEncoded("sec_token".into(), "tok".into()),
            Matcher::UrlEncoded("region".into(), "ap-southeast-1".into()),
            Matcher::UrlEncoded("params".into(), "{}".into()),
        ]))
        .with_status(200)
        .with_body(r#"{"successResponse":true,"data":{"Data":{"SellerInfoDto":{"Nbid":"nb-1"}}}}"#)
        .create_async()
        .await;
    let summary = server
        .mock("POST", "/data/api.json")
        .match_query(Matcher::AllOf(vec![
            Matcher::UrlEncoded("product".into(), "BssOpenAPI-V3".into()),
            Matcher::UrlEncoded("action".into(), "GetSeatSubscriptionSummary".into()),
        ]))
        .match_body(Matcher::AllOf(vec![
            Matcher::UrlEncoded("sec_token".into(), "tok".into()),
            Matcher::UrlEncoded("region".into(), "cn-hangzhou".into()),
            Matcher::UrlEncoded("language".into(), "zh-CN".into()),
            Matcher::UrlEncoded(
                "params".into(),
                r#"{"productCode":"sfm_tokenplanteams_dp_intl","Nbid":"nb-1"}"#.into(),
            ),
        ]))
        .with_status(200)
        .with_body(SUMMARY)
        .create_async()
        .await;

    let client = reqwest::Client::new();
    let team = fetch(
        &client,
        &server.url(),
        "session=synthetic",
        Duration::from_secs(5),
        NOW_MS,
    )
    .await
    .unwrap()
    .unwrap();
    info.assert_async().await;
    human.assert_async().await;
    summary.assert_async().await;
    assert_eq!(team.login_method, "Standard Team");
}

#[tokio::test]
async fn load_human_info_failure_stops_the_team_lane() {
    let mut server = mockito::Server::new_async().await;
    server
        .mock("GET", "/tool/user/info.json")
        .with_status(200)
        .with_body(r#"{"data":{"secToken":"tok"}}"#)
        .create_async()
        .await;
    server
        .mock("POST", "/data/api.json")
        .match_query(Matcher::UrlEncoded("action".into(), "LoadHumanInfo".into()))
        .with_status(200)
        .with_body(r#"{"successResponse":false,"data":{}}"#)
        .create_async()
        .await;
    let summary = server
        .mock("POST", "/data/api.json")
        .match_query(Matcher::UrlEncoded(
            "action".into(),
            "GetSeatSubscriptionSummary".into(),
        ))
        .expect(0)
        .create_async()
        .await;

    let client = reqwest::Client::new();
    let error = fetch(
        &client,
        &server.url(),
        "session=synthetic",
        Duration::from_secs(5),
        NOW_MS,
    )
    .await
    .unwrap_err();
    assert_eq!(message(error), "Qwen Cloud Team LoadHumanInfo failed.");
    summary.assert_async().await;
}

#[test]
fn team_auth_errors_report_an_expired_session() {
    use crate::core::{Provider, ProviderStateKind};
    let provider = super::QwenCloudProvider::new();
    for status in [401, 403] {
        let error = super::team::parse_response(status, b"{}").unwrap_err();
        assert_eq!(
            provider.error_state_kind(&error),
            ProviderStateKind::ExpiredSession
        );
    }
    let error = super::team::parse_response(200, br#"{"code":"ConsoleNeedLogin"}"#).unwrap_err();
    assert_eq!(
        provider.error_state_kind(&error),
        ProviderStateKind::ExpiredSession
    );
    let error = super::team::parse_response(429, b"{}").unwrap_err();
    assert_ne!(
        provider.error_state_kind(&error),
        ProviderStateKind::ExpiredSession
    );
}
