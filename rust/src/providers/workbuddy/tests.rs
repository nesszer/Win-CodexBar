use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, Utc};
use reqwest::redirect::Policy;

use super::model::{
    chrome_major, credits, cycle_end, earliest_reset, parse_summary, response_data, result,
    user_agents,
};
use super::*;

/// Upstream `WorkBuddyPluginTests.summary`.
const SUMMARY: &str = r#"{
  "code": 0,
  "msg": "OK",
  "requestId": "fixture-request",
  "data": {
    "Packages": [{
      "PackageCode": "FIXTURE_FREE",
      "CycleTotalCapacity": "500",
      "CycleRemainCapacity": "450",
      "CycleUsedCapacity": "50",
      "CycleFrozenCapacity": "0",
      "CapacityUnit": "credits",
      "TotalCount": 1
    }],
    "SubscriptionPackageCode": "",
    "SubscriptionPackageName": "体验版",
    "IsPaidUser": false,
    "IsProtectedPriceUser": false
  }
}"#;

/// Upstream `WorkBuddyPluginTests.emptyPackages`.
const EMPTY_PACKAGES: &str = r#"{"code":0,"msg":"OK","data":{"Accounts":[],"TotalCount":0}}"#;

/// Upstream `WorkBuddyPluginTests.freePackages`.
const FREE_PACKAGES: &str = r#"{"code":0,"msg":"OK","data":{"TotalCount":1,"Accounts":[{
  "CapacityUnit": "credits",
  "CycleStartTime": "2026-10-01 00:00:00",
  "CycleEndTime": "2026-10-31 23:59:59",
  "Status": 0
}]}}"#;

/// Upstream `earliest future cycle end across packages is the reset`.
const PAID_PACKAGES: &str = r#"{"code":0,"msg":"OK","data":{"Accounts":[
  {"CapacityUnit":"credits","CycleEndTime":"2026-09-30 23:59:59"},
  {"CapacityUnit":"credits","CycleEndTime":"2026-10-20 23:59:59"},
  {"CapacityUnit":"requests","CycleEndTime":"2026-10-05 23:59:59"},
  {"CapacityUnit":"credits","CycleEndTime":"2026-10-04"}
]}}"#;

/// Upstream `credit packages are summed and other units ignored`.
const SUMMED: &str = r#"{"code":0,"msg":"OK","data":{"SubscriptionPackageName":"专业版","IsPaidUser":true,"Packages":[
  {"CycleTotalCapacity":"2000","CycleRemainCapacity":"1500.5","CycleFrozenCapacity":"12.25","CapacityUnit":"credits"},
  {"CycleTotalCapacity":"500","CycleRemainCapacity":"500","CycleFrozenCapacity":"0","CapacityUnit":"credits"},
  {"CycleTotalCapacity":"9","CycleRemainCapacity":"1","CapacityUnit":"requests"},
  {"CycleTotalCapacity":"99","CycleRemainCapacity":"1"}
]}}"#;

/// Upstream `zero allowance shows left and total rows instead of a meter`.
const ZERO: &str = r#"{"code":0,"msg":"OK","data":{"SubscriptionPackageName":"体验版","Packages":[
  {"CycleTotalCapacity":"0","CycleRemainCapacity":"0","CycleFrozenCapacity":"2.5","CapacityUnit":"credits"}
]}}"#;

/// Upstream docs/workbuddy.md: rollover above the grant shows 0% used.
const ROLLOVER: &str = r#"{"code":0,"msg":"OK","data":{"Packages":[
  {"CycleTotalCapacity":"500","CycleRemainCapacity":"600","CapacityUnit":"credits"}
]}}"#;

const SUMMARY_FIXTURE_PATH: &str = "/billing/meter/get-user-resource-summary";
const PAID_FIXTURE_PATH: &str = "/billing/meter/get-user-resource-paid-packages";
const FREE_FIXTURE_PATH: &str = "/billing/meter/get-user-resource-free-packages";
const FIXTURE_COOKIE: &str = "session=fixture-session; locale=zh";
const REJECTED_COOKIE: &str = "session=rejected-fixture";
/// 2026-10-03T00:00:00Z
const NOW: i64 = 1_790_985_600;
/// 2026-10-31 23:59:59 China Standard Time, plus one second.
const OCTOBER_RESET: i64 = 1_793_462_400;

fn at(seconds: i64) -> Option<DateTime<Utc>> {
    DateTime::from_timestamp(seconds, 0)
}

fn fixture_now() -> DateTime<Utc> {
    DateTime::from_timestamp(NOW, 0).expect("fixture time")
}

fn windows_agent(major: u32) -> String {
    format!(
        "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{major}.0.0.0 Safari/537.36"
    )
}

fn data(body: &str) -> Map<String, Value> {
    response_data(body.as_bytes()).expect("billing data")
}

fn summary_result(body: &str) -> Result<ProviderFetchResult, ProviderError> {
    let summary = parse_summary(&response_data(body.as_bytes())?)?;
    Ok(result(&summary, None))
}

fn rows(result: &ProviderFetchResult) -> Vec<(&str, &str, &str)> {
    result
        .display_details
        .iter()
        .map(|row| (row.section_title().unwrap_or(""), row.title(), row.value()))
        .collect()
}

// ---------------------------------------------------------------- mapping

#[test]
fn monthly_credits_become_one_meter_with_the_plan() {
    let result = summary_result(SUMMARY).expect("summary");
    let primary = &result.usage.primary;
    assert_eq!(primary.used_percent, 10.0);
    assert_eq!(primary.window_minutes, None);
    assert_eq!(primary.resets_at, None);
    assert_eq!(
        primary.reset_description.as_deref(),
        Some("450 / 500 credits left")
    );
    assert!(primary.description_is_detail);
    assert!(!primary.is_informational);
    assert_eq!(result.usage.login_method.as_deref(), Some("体验版"));
    assert!(result.usage.secondary.is_none());
    assert!(result.display_details.is_empty());
    assert!(result.cost.is_none());
    assert_eq!(result.source_label, "web");
}

#[test]
fn credit_packages_are_summed_and_other_units_ignored() {
    let result = summary_result(SUMMED).expect("summed packages");
    let primary = &result.usage.primary;
    assert!((primary.used_percent - 19.98).abs() < 0.001);
    assert_eq!(
        primary.reset_description.as_deref(),
        Some("2,000.5 / 2,500 credits left")
    );
    assert_eq!(result.usage.login_method.as_deref(), Some("专业版"));
    assert_eq!(rows(&result), vec![("Credits", "Reserved", "12.25")]);
}

#[test]
fn a_zero_allowance_lists_left_and_total_instead_of_a_meter() {
    let result = summary_result(ZERO).expect("zero allowance");
    let primary = &result.usage.primary;
    // Upstream has no meter; the Windows primary lane is informational.
    assert!(primary.is_informational);
    assert_eq!(primary.reset_description.as_deref(), Some("0 credits left"));
    assert_eq!(
        rows(&result),
        vec![
            ("Credits", "Left", "0"),
            ("Credits", "Total", "0"),
            ("Credits", "Reserved", "2.5"),
        ]
    );
    assert_eq!(result.usage.login_method.as_deref(), Some("体验版"));
}

#[test]
fn rolled_over_credits_above_the_allowance_show_no_usage() {
    let result = summary_result(ROLLOVER).expect("rollover");
    let primary = &result.usage.primary;
    assert_eq!(primary.used_percent, 0.0);
    assert_eq!(
        primary.reset_description.as_deref(),
        Some("600 / 500 credits left")
    );
    assert!(primary.description_is_detail);
    assert!(!primary.is_informational);
    assert!(result.display_details.is_empty());
}

#[test]
fn credits_use_two_fraction_digits_and_thousands_groups() {
    assert_eq!(credits(2000.5), "2,000.5");
    assert_eq!(credits(362.5), "362.5");
    assert_eq!(credits(12.256), "12.26");
    assert_eq!(credits(0.0), "0");
}

#[test]
fn invalid_amounts_fail_instead_of_publishing_a_balance() {
    for value in [
        "true", "-5", "\"\"", "\"NaN\"", "\"1e3\"", "\"-1\"", "[]", "null",
    ] {
        let body = format!(
            r#"{{"code":0,"data":{{"Packages":[{{"CycleTotalCapacity":"500","CycleRemainCapacity":{value},"CapacityUnit":"credits"}}]}}}}"#
        );
        let error = summary_result(&body).expect_err(value);
        assert_eq!(
            error.to_string(),
            "Parse error: Invalid WorkBuddy billing response: CycleRemainCapacity",
            "{value}"
        );
    }
    // Whitespace around a decimal string is trimmed, numbers are accepted.
    let body = r#"{"code":0,"data":{"Packages":[{"CycleTotalCapacity":200,"CycleRemainCapacity":" 50.5 ","CapacityUnit":"credits"}]}}"#;
    assert_eq!(
        summary_result(body)
            .expect("numeric amounts")
            .usage
            .primary
            .used_percent,
        74.75
    );
}

#[test]
fn malformed_summaries_stay_unavailable() {
    for (body, field) in [
        ("not JSON", "expected a JSON object"),
        ("[]", "expected a JSON object"),
        (r#"{"code":0}"#, "data"),
        (r#"{"code":0,"data":{}}"#, "Packages"),
        (r#"{"code":0,"data":{"Packages":{}}}"#, "Packages"),
        (r#"{"code":0,"data":{"Packages":[1]}}"#, "package"),
        (
            r#"{"code":0,"data":{"Packages":[{"CapacityUnit":"credits","CycleTotalCapacity":"1","CycleRemainCapacity":"1","CycleFrozenCapacity":null}]}}"#,
            "CycleFrozenCapacity",
        ),
    ] {
        let error = summary_result(body).expect_err(body);
        assert_eq!(
            error.to_string(),
            format!("Parse error: Invalid WorkBuddy billing response: {field}"),
            "{body}"
        );
    }
}

#[test]
fn non_zero_codes_name_the_code() {
    for (body, message) in [
        (
            r#"{"code":10001,"msg":"fixture"}"#,
            "WorkBuddy billing API returned code 10001.",
        ),
        (
            r#"{"code":"0","data":{}}"#,
            "WorkBuddy billing API returned code 0.",
        ),
        (
            r#"{"msg":"x"}"#,
            "WorkBuddy billing API returned code undefined.",
        ),
        (
            r#"{"code":null}"#,
            "WorkBuddy billing API returned code null.",
        ),
    ] {
        let error = summary_result(body).expect_err(body);
        assert!(matches!(error, ProviderError::Other(_)), "{body}");
        assert_eq!(error.to_string(), message, "{body}");
    }
}

#[test]
fn cycle_ends_are_china_standard_time_plus_one_second() {
    assert_eq!(
        cycle_end(Some(&json!("2026-10-31 23:59:59"))),
        at(OCTOBER_RESET)
    );
    assert_eq!(
        cycle_end(Some(&json!(" 2026-10-20 23:59:59 "))),
        at(1_792_512_000)
    );
    for raw in [
        json!("2026-10-04"),
        json!("2026-10-04T23:59:59"),
        json!("2026-13-01 00:00:00"),
        json!(""),
        json!(1_793_462_400),
        Value::Null,
    ] {
        assert_eq!(cycle_end(Some(&raw)), None, "{raw}");
    }
    assert_eq!(cycle_end(None), None);
}

#[test]
fn the_earliest_future_credits_cycle_end_is_the_reset() {
    // Skips a past end, the requests unit and a date-only end.
    assert_eq!(
        earliest_reset(&data(PAID_PACKAGES), fixture_now()),
        at(1_792_512_000)
    );
    assert_eq!(
        earliest_reset(&data(FREE_PACKAGES), fixture_now()),
        at(OCTOBER_RESET)
    );
    assert_eq!(earliest_reset(&data(EMPTY_PACKAGES), fixture_now()), None);
    assert_eq!(
        earliest_reset(&data(r#"{"code":0,"data":{"Accounts":{}}}"#), fixture_now()),
        None
    );
}

// ---------------------------------------------------------------- user agent

#[test]
fn chrome_versions_yield_a_major_above_one() {
    assert_eq!(chrome_major("143.0.7499.170"), Some(143));
    assert_eq!(chrome_major(" 154.0.1.2\n"), Some(154));
    assert_eq!(chrome_major("154"), Some(154));
    assert_eq!(chrome_major("1.0.0.0"), None);
    assert_eq!(chrome_major("0.0.0.0"), None);
    assert_eq!(chrome_major("SetupMetrics"), None);
    assert_eq!(chrome_major("143..1.2"), None);
    assert_eq!(chrome_major(""), None);
}

#[test]
fn user_agents_are_the_installed_major_then_the_previous_one() {
    assert_eq!(
        user_agents(Some(154)),
        vec![Some(windows_agent(154)), Some(windows_agent(153))]
    );
    assert_eq!(user_agents(Some(1)), vec![None]);
    assert_eq!(user_agents(None), vec![None]);
}

#[test]
fn the_newest_chrome_version_folder_wins() {
    let dir = tempfile::tempdir().expect("temp dir");
    for name in ["143.0.7499.170", "144.0.7559.60", "SetupMetrics", "145.0"] {
        std::fs::create_dir(dir.path().join(name)).expect("version folder");
    }
    // A file named like a version is not an install.
    std::fs::write(dir.path().join("146.0.0.0"), b"").expect("decoy file");
    assert_eq!(newest_version_major(dir.path()), Some(144));
    assert_eq!(newest_version_major(&dir.path().join("missing")), None);
}

// ---------------------------------------------------------------- cookies

fn cookie(name: &str, domain: &str, path: &str) -> Cookie {
    Cookie {
        name: name.into(),
        value: format!("{name}-value"),
        domain: domain.into(),
        path: path.into(),
        expires: None,
        is_secure: true,
        is_http_only: true,
    }
}

#[test]
fn browser_cookies_are_the_ones_a_request_to_www_would_carry() {
    let cookies = [
        cookie("session", ".workbuddy.cn", "/"),
        cookie("locale", "www.workbuddy.cn", "/billing"),
        cookie("www_only", ".www.workbuddy.cn", "/"),
        cookie("api", "api.workbuddy.cn", "/"),
        cookie("bare", "workbuddy.cn", "/"),
        cookie("other_path", "www.workbuddy.cn", "/profile"),
        cookie("prefix_path", "www.workbuddy.cn", "/bill"),
        cookie("session", "www.workbuddy.cn", "/"),
    ];
    assert_eq!(
        request_cookie_header(&cookies, SUMMARY_PATH).as_deref(),
        Some("session=session-value; locale=locale-value; www_only=www_only-value")
    );
    assert_eq!(
        request_cookie_header(&[cookie("api", "api.workbuddy.cn", "/")], SUMMARY_PATH),
        None
    );
}

#[test]
fn timeouts_follow_the_upstream_clamps() {
    assert_eq!(request_timeout(22), Duration::from_secs(22));
    assert_eq!(request_timeout(0), Duration::from_secs(1));
    assert_eq!(request_timeout(60), Duration::from_secs(30));
    assert_eq!(
        listing_timeout(Duration::from_secs(22), Duration::from_secs(5)),
        Some(Duration::from_secs(5))
    );
    assert_eq!(
        listing_timeout(Duration::from_secs(3), Duration::from_millis(4_500)),
        Some(Duration::from_secs(3))
    );
    assert_eq!(
        listing_timeout(Duration::from_secs(22), Duration::from_millis(999)),
        None
    );
}

// ---------------------------------------------------------------- local server

#[derive(Clone, Debug)]
struct Seen {
    path: String,
    headers: Vec<(String, String)>,
    body: String,
}

impl Seen {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.as_str())
    }
}

fn read_request(stream: &mut TcpStream) -> Option<Seen> {
    let mut data = Vec::new();
    let mut buffer = [0_u8; 4096];
    let header_end = loop {
        let read = stream.read(&mut buffer).ok()?;
        if read == 0 {
            return None;
        }
        data.extend_from_slice(&buffer[..read]);
        if let Some(end) = data.windows(4).position(|window| window == b"\r\n\r\n") {
            break end + 4;
        }
    };
    let head = String::from_utf8_lossy(&data[..header_end]).into_owned();
    let mut lines = head.split("\r\n");
    let mut request_line = lines.next()?.split(' ');
    let method = request_line.next()?.to_owned();
    assert_eq!(method, "POST");
    let path = request_line.next()?.to_owned();
    let headers: Vec<(String, String)> = lines
        .filter_map(|line| line.split_once(':'))
        .map(|(key, value)| (key.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .collect();
    let length = headers
        .iter()
        .find(|(key, _)| key == "content-length")
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);
    while data.len() < header_end + length {
        let read = stream.read(&mut buffer).ok()?;
        if read == 0 {
            break;
        }
        data.extend_from_slice(&buffer[..read]);
    }
    let body = String::from_utf8_lossy(&data[header_end..]).into_owned();
    Some(Seen {
        path,
        headers,
        body,
    })
}

type Log = Arc<Mutex<Vec<Seen>>>;

/// A local www.workbuddy.cn stand-in; `route` returns status and body.
fn serve<F>(route: F) -> (String, Log)
where
    F: Fn(&Seen) -> (u16, String) + Send + Sync + 'static,
{
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("bind local test server");
    let address = listener.local_addr().expect("local server address");
    let log: Log = Arc::new(Mutex::new(Vec::new()));
    let route = Arc::new(route);
    let server_log = Arc::clone(&log);
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let log = Arc::clone(&server_log);
            let route = Arc::clone(&route);
            std::thread::spawn(move || {
                let Some(seen) = read_request(&mut stream) else {
                    return;
                };
                log.lock().expect("request log").push(seen.clone());
                let (status, body) = route(&seen);
                let head = format!(
                    "HTTP/1.1 {status} Fixture\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                if stream.write_all(head.as_bytes()).is_ok() {
                    stream.write_all(body.as_bytes()).unwrap_or_default();
                }
            });
        }
    });
    (format!("http://{address}/"), log)
}

/// Upstream `WorkBuddyPluginTests.route`.
fn default_route(path: &str) -> (u16, String) {
    match path {
        SUMMARY_FIXTURE_PATH => (200, SUMMARY.to_owned()),
        FREE_FIXTURE_PATH => (200, FREE_PACKAGES.to_owned()),
        _ => (200, EMPTY_PACKAGES.to_owned()),
    }
}

fn chrome_154() -> Option<u32> {
    Some(154)
}

fn no_chrome() -> Option<u32> {
    None
}

fn sessions(cookies: &[&str]) -> BrowserSessions {
    BrowserSessions {
        cookies: cookies.iter().map(|&cookie| cookie.to_owned()).collect(),
        ..BrowserSessions::default()
    }
}

fn no_browser_sessions() -> BrowserSessions {
    sessions(&[])
}

fn two_browser_sessions() -> BrowserSessions {
    sessions(&[REJECTED_COOKIE, FIXTURE_COOKIE])
}

fn fixture_then_other_sessions() -> BrowserSessions {
    sessions(&[FIXTURE_COOKIE, "session=other"])
}

/// The shared App-Bound Encryption message, as the provider reports it.
const ABE_ERROR: &str = "Failed to read browser cookies: Chromium App-Bound Encryption blocked automatic import from Google Chrome. Paste the Cookie header manually, or use Firefox if that browser has the same login.";

/// Two blocked browsers (Chrome, then Arc) in one error.
const ABE_ERROR_CHROME_AND_ARC: &str = "Failed to read browser cookies: Chromium App-Bound Encryption blocked automatic import from Google Chrome and Arc. Paste the Cookie header manually, or sign in with Firefox instead.";

/// Chrome is skipped by App-Bound Encryption; Edge holds a session.
fn chrome_protected_edge_signed_in() -> BrowserSessions {
    collect_sessions([
        (
            BrowserType::Chrome,
            Err(CookieError::AppBoundEncryption {
                browsers: vec![BrowserType::Chrome],
            }),
        ),
        (
            BrowserType::Edge,
            Ok(vec![cookie("session", ".workbuddy.cn", "/")]),
        ),
    ])
}

/// Chrome is skipped by App-Bound Encryption; Edge only has cookies a
/// request to www.workbuddy.cn would not carry.
fn chrome_protected_edge_without_session() -> BrowserSessions {
    collect_sessions([
        (
            BrowserType::Chrome,
            Err(CookieError::AppBoundEncryption {
                browsers: vec![BrowserType::Chrome],
            }),
        ),
        (
            BrowserType::Edge,
            Ok(vec![cookie("api", "api.workbuddy.cn", "/")]),
        ),
    ])
}

/// Firefox cannot be read; Edge holds a session.
fn firefox_unreadable_edge_signed_in() -> BrowserSessions {
    collect_sessions([
        (
            BrowserType::Firefox,
            Err(CookieError::Decryption("fixture".into())),
        ),
        (
            BrowserType::Edge,
            Ok(vec![cookie("session", ".workbuddy.cn", "/")]),
        ),
    ])
}

/// Only a read failure that is not App-Bound Encryption, and no session.
fn only_unreadable_browsers() -> BrowserSessions {
    collect_sessions([
        (BrowserType::Firefox, Err(CookieError::NoEncryptionKey)),
        (BrowserType::Brave, Err(CookieError::BrowserNotInstalled)),
        (BrowserType::Edge, Err(CookieError::NotFound("x".into()))),
    ])
}

fn provider(
    base_url: &str,
    chrome_major: fn() -> Option<u32>,
    browser_sessions: SessionSource,
) -> WorkBuddyProvider {
    let client = Client::builder()
        .redirect(Policy::none())
        .build()
        .expect("test HTTP client");
    WorkBuddyProvider::with_parts(
        base_url,
        client,
        chrome_major,
        fixture_now,
        browser_sessions,
    )
}

fn manual(cookie: &str) -> FetchContext {
    FetchContext {
        source_mode: SourceMode::Web,
        web_timeout: 22,
        manual_cookie_header: Some(cookie.to_owned()),
        ..FetchContext::default()
    }
}

fn automatic() -> FetchContext {
    FetchContext {
        source_mode: SourceMode::Auto,
        ..FetchContext::default()
    }
}

fn paths(log: &Log) -> Vec<String> {
    log.lock()
        .expect("request log")
        .iter()
        .map(|seen| seen.path.clone())
        .collect()
}

fn summary_requests(log: &Log) -> Vec<Seen> {
    log.lock()
        .expect("request log")
        .iter()
        .filter(|seen| seen.path == SUMMARY_FIXTURE_PATH)
        .cloned()
        .collect()
}

#[tokio::test]
async fn requests_post_json_with_the_website_session_and_no_bearer() {
    let (base_url, log) = serve(|seen| default_route(&seen.path));
    let result = provider(&base_url, chrome_154, no_browser_sessions)
        .fetch_usage(&manual(FIXTURE_COOKIE))
        .await
        .expect("fetch");
    assert_eq!(result.usage.primary.used_percent, 10.0);
    assert_eq!(result.usage.primary.resets_at, at(OCTOBER_RESET));
    assert_eq!(
        result.usage.primary.reset_description.as_deref(),
        Some("450 / 500 credits left")
    );
    assert_eq!(result.usage.login_method.as_deref(), Some("体验版"));
    assert_eq!(
        paths(&log),
        vec![SUMMARY_FIXTURE_PATH, PAID_FIXTURE_PATH, FREE_FIXTURE_PATH]
    );
    for seen in log.lock().expect("request log").iter() {
        assert_eq!(seen.header("authorization"), None);
        assert_eq!(seen.header("cookie"), Some(FIXTURE_COOKIE));
        assert_eq!(seen.header("accept"), Some("application/json"));
        assert_eq!(seen.header("content-type"), Some("application/json"));
        assert_eq!(seen.header("origin"), Some("https://www.workbuddy.cn"));
        assert_eq!(
            seen.header("referer"),
            Some("https://www.workbuddy.cn/profile/plans-usage")
        );
        assert_eq!(seen.header("user-agent"), Some(windows_agent(154).as_str()));
        let body: Value = serde_json::from_str(&seen.body).expect("JSON body");
        if seen.path == SUMMARY_FIXTURE_PATH {
            assert_eq!(body, json!({}));
            continue;
        }
        assert_eq!(body["PageNumber"], json!(1));
        assert_eq!(body["PageSize"], json!(100));
        assert_eq!(body["Status"], json!([0, 3]));
        let codes = body["PackageCodes"].as_array().expect("package codes");
        if seen.path == FREE_FIXTURE_PATH {
            assert_eq!(codes.len(), 11);
            assert!(codes.contains(&json!("TCACA_code_008_cfWoLwvjU4")));
        } else {
            assert_eq!(codes.len(), 9);
            assert!(codes.contains(&json!("TCACA_code_002_AkiJS3ZHF5")));
        }
    }
}

#[tokio::test]
async fn a_future_paid_reset_is_used_without_querying_free_grants() {
    let (base_url, log) = serve(|seen| match seen.path.as_str() {
        PAID_FIXTURE_PATH => (200, PAID_PACKAGES.to_owned()),
        path => default_route(path),
    });
    let result = provider(&base_url, chrome_154, no_browser_sessions)
        .fetch_usage(&manual(FIXTURE_COOKIE))
        .await
        .expect("fetch");
    // 2026-10-20 23:59:59 +08:00, plus one second.
    assert_eq!(result.usage.primary.resets_at, at(1_792_512_000));
    assert_eq!(paths(&log), vec![SUMMARY_FIXTURE_PATH, PAID_FIXTURE_PATH]);
}

/// Upstream #4767 `the paid cycle outranks an earlier free grant`.
#[tokio::test]
async fn the_paid_cycle_outranks_an_earlier_free_grant() {
    const PAID: &str = r#"{"code":0,"msg":"OK","data":{"Accounts":[
      {"CapacityUnit":"credits","CycleEndTime":"2026-11-24 12:44:13"}
    ]}}"#;
    // The trial grant closes with the calendar month and the reward grants end piecewise.
    const FREE: &str = r#"{"code":0,"msg":"OK","data":{"Accounts":[
      {"CapacityUnit":"credits","CycleEndTime":"2026-10-31 23:59:59"},
      {"CapacityUnit":"credits","CycleEndTime":"2026-11-11 23:09:21"}
    ]}}"#;
    let (base_url, _) = serve(|seen| match seen.path.as_str() {
        PAID_FIXTURE_PATH => (200, PAID.to_owned()),
        FREE_FIXTURE_PATH => (200, FREE.to_owned()),
        path => default_route(path),
    });
    let result = provider(&base_url, chrome_154, no_browser_sessions)
        .fetch_usage(&manual(FIXTURE_COOKIE))
        .await
        .expect("fetch");
    // 2026-11-24 12:44:13 +08:00, plus one second.
    assert_eq!(result.usage.primary.resets_at, at(1_795_495_454));
}

/// Upstream #4767 `an exhausted paid cycle falls back to the free grants`.
#[tokio::test]
async fn an_exhausted_paid_cycle_falls_back_to_the_free_grants() {
    const PAID: &str = r#"{"code":0,"msg":"OK","data":{"Accounts":[
      {"CapacityUnit":"credits","CycleEndTime":"2026-09-30 23:59:59"}
    ]}}"#;
    let (base_url, log) = serve(|seen| match seen.path.as_str() {
        PAID_FIXTURE_PATH => (200, PAID.to_owned()),
        path => default_route(path),
    });
    let result = provider(&base_url, chrome_154, no_browser_sessions)
        .fetch_usage(&manual(FIXTURE_COOKIE))
        .await
        .expect("fetch");
    assert_eq!(result.usage.primary.resets_at, at(OCTOBER_RESET));
    assert_eq!(
        paths(&log),
        vec![SUMMARY_FIXTURE_PATH, PAID_FIXTURE_PATH, FREE_FIXTURE_PATH]
    );
}

#[tokio::test]
async fn failed_package_listings_keep_the_balance_without_a_reset() {
    for (status, body) in [
        (401, "{}"),
        (500, "{}"),
        (200, r#"{"code":4001,"msg":"denied"}"#),
        (200, "not JSON"),
        (200, r#"{"code":0}"#),
    ] {
        let (base_url, _) = serve(move |seen| match seen.path.as_str() {
            SUMMARY_FIXTURE_PATH => default_route(&seen.path),
            _ => (status, body.to_owned()),
        });
        let result = provider(&base_url, chrome_154, no_browser_sessions)
            .fetch_usage(&manual(FIXTURE_COOKIE))
            .await
            .expect("balance survives");
        let primary = &result.usage.primary;
        assert_eq!(primary.used_percent, 10.0, "{status} {body}");
        assert_eq!(primary.resets_at, None, "{status} {body}");
        assert_eq!(
            primary.reset_description.as_deref(),
            Some("450 / 500 credits left")
        );
    }
}

#[tokio::test]
async fn a_failed_paid_listing_preserves_the_free_listing_reset() {
    let (base_url, _) = serve(|seen| match seen.path.as_str() {
        PAID_FIXTURE_PATH => (401, "{}".to_owned()),
        path => default_route(path),
    });
    let result = provider(&base_url, chrome_154, no_browser_sessions)
        .fetch_usage(&manual(FIXTURE_COOKIE))
        .await
        .expect("fetch");
    assert_eq!(result.usage.primary.used_percent, 10.0);
    assert_eq!(result.usage.primary.resets_at, at(OCTOBER_RESET));
}

#[tokio::test]
async fn stalled_listings_keep_the_balance_within_the_shared_budget() {
    let (base_url, log) = serve(|seen| {
        if seen.path != SUMMARY_FIXTURE_PATH {
            std::thread::sleep(Duration::from_secs(12));
        }
        default_route(&seen.path)
    });
    let ctx = FetchContext {
        web_timeout: 60,
        ..manual(FIXTURE_COOKIE)
    };
    let started = Instant::now();
    let result = provider(&base_url, chrome_154, no_browser_sessions)
        .fetch_usage(&ctx)
        .await
        .expect("balance survives");
    let elapsed = started.elapsed();
    assert!(elapsed < Duration::from_secs(8), "{elapsed:?}");
    assert_eq!(result.usage.primary.used_percent, 10.0);
    assert_eq!(result.usage.primary.resets_at, None);
    // The paid listing used the whole budget, so the free one never ran.
    assert_eq!(paths(&log), vec![SUMMARY_FIXTURE_PATH, PAID_FIXTURE_PATH]);
}

#[tokio::test]
async fn a_zero_allowance_skips_the_reset_lookup() {
    let (base_url, log) = serve(|seen| match seen.path.as_str() {
        SUMMARY_FIXTURE_PATH => (200, ZERO.to_owned()),
        path => default_route(path),
    });
    let result = provider(&base_url, chrome_154, no_browser_sessions)
        .fetch_usage(&manual(FIXTURE_COOKIE))
        .await
        .expect("fetch");
    assert!(result.usage.primary.is_informational);
    assert_eq!(paths(&log), vec![SUMMARY_FIXTURE_PATH]);
}

#[tokio::test]
async fn summary_failures_keep_their_classification() {
    for (status, message) in [
        (401, SESSION_EXPIRED.to_owned()),
        (403, "WorkBuddy denied access to billing data.".to_owned()),
        (
            429,
            "WorkBuddy billing requests are rate limited.".to_owned(),
        ),
        (503, "WorkBuddy billing API returned HTTP 503.".to_owned()),
        (404, "WorkBuddy billing API returned HTTP 404.".to_owned()),
    ] {
        let (base_url, _) = serve(move |_| (status, "<html></html>".to_owned()));
        let provider = provider(&base_url, no_chrome, no_browser_sessions);
        let error = provider
            .fetch_usage(&manual(FIXTURE_COOKIE))
            .await
            .expect_err("summary failure");
        assert_eq!(error.to_string(), message, "{status}");
        let expired = provider.error_state_kind(&error) == ProviderStateKind::ExpiredSession;
        assert_eq!(expired, status == 401, "{status}");
    }
}

#[tokio::test]
async fn a_pending_chrome_update_retries_the_previous_major_user_agent() {
    let accepted = windows_agent(153);
    let (base_url, log) = serve(move |seen| {
        if seen.header("user-agent") == Some(accepted.as_str()) {
            default_route(&seen.path)
        } else {
            (401, "{}".to_owned())
        }
    });
    let result = provider(&base_url, chrome_154, no_browser_sessions)
        .fetch_usage(&manual(FIXTURE_COOKIE))
        .await
        .expect("fetch");
    let agents: Vec<Option<String>> = summary_requests(&log)
        .iter()
        .map(|seen| seen.header("user-agent").map(str::to_owned))
        .collect();
    assert_eq!(
        agents,
        vec![Some(windows_agent(154)), Some(windows_agent(153))]
    );
    // The listings reuse the accepted User-Agent.
    assert_eq!(result.usage.primary.resets_at, at(OCTOBER_RESET));
}

#[tokio::test]
async fn a_session_rejected_for_every_user_agent_is_expired() {
    for (chrome_major, expected) in [
        (
            chrome_154 as fn() -> Option<u32>,
            vec![Some(windows_agent(154)), Some(windows_agent(153))],
        ),
        (no_chrome as fn() -> Option<u32>, vec![None]),
    ] {
        let (base_url, log) = serve(|_| (401, "{}".to_owned()));
        let provider = provider(&base_url, chrome_major, no_browser_sessions);
        let error = provider
            .fetch_usage(&manual(FIXTURE_COOKIE))
            .await
            .expect_err("expired");
        assert_eq!(error.to_string(), SESSION_EXPIRED);
        assert_eq!(
            provider.error_state_kind(&error),
            ProviderStateKind::ExpiredSession
        );
        let agents: Vec<Option<String>> = summary_requests(&log)
            .iter()
            .map(|seen| seen.header("user-agent").map(str::to_owned))
            .collect();
        assert_eq!(agents, expected);
    }
}

#[tokio::test]
async fn rejected_browser_sessions_advance_within_the_same_refresh() {
    let (base_url, log) = serve(|seen| {
        if seen.header("cookie") == Some(REJECTED_COOKIE) {
            (401, "{}".to_owned())
        } else {
            default_route(&seen.path)
        }
    });
    let result = provider(&base_url, no_chrome, two_browser_sessions)
        .fetch_usage(&automatic())
        .await
        .expect("second session");
    assert_eq!(result.usage.primary.used_percent, 10.0);
    let cookies: Vec<String> = summary_requests(&log)
        .iter()
        .filter_map(|seen| seen.header("cookie").map(str::to_owned))
        .collect();
    assert_eq!(cookies, vec![REJECTED_COOKIE, FIXTURE_COOKIE]);
}

#[tokio::test]
async fn a_pasted_header_never_advances_to_a_browser_session() {
    let (base_url, log) = serve(|_| (401, "{}".to_owned()));
    let error = provider(&base_url, no_chrome, two_browser_sessions)
        .fetch_usage(&manual(REJECTED_COOKIE))
        .await
        .expect_err("expired");
    assert_eq!(error.to_string(), SESSION_EXPIRED);
    assert_eq!(paths(&log), vec![SUMMARY_FIXTURE_PATH]);
}

#[tokio::test]
async fn non_authentication_errors_stop_without_trying_other_sessions() {
    for status in [403_u16, 429, 503] {
        let (base_url, log) = serve(move |_| (status, "{}".to_owned()));
        provider(&base_url, no_chrome, fixture_then_other_sessions)
            .fetch_usage(&automatic())
            .await
            .expect_err("stops");
        assert_eq!(paths(&log).len(), 1, "{status}");
    }
}

#[tokio::test]
async fn no_session_is_a_sign_in_prompt_without_a_request() {
    let (base_url, log) = serve(|seen| default_route(&seen.path));
    let provider = provider(&base_url, chrome_154, no_browser_sessions);
    let missing = FetchContext {
        manual_cookie_missing: true,
        ..automatic()
    };
    for ctx in [automatic(), missing, manual("   ")] {
        match provider.fetch_usage(&ctx).await {
            Err(ProviderError::BrowserSignInRequired {
                message,
                sign_in_url,
            }) => {
                assert_eq!(message, MISSING_SESSION);
                assert_eq!(
                    message,
                    "No WorkBuddy session cookies found. Sign in at www.workbuddy.cn in Chrome (WorkBuddy binds the session to Chrome's User-Agent) or paste a Cookie header."
                );
                assert_eq!(sign_in_url, "https://www.workbuddy.cn/profile/plans-usage");
            }
            other => panic!("expected a sign-in prompt, got {other:?}"),
        }
    }
    assert!(paths(&log).is_empty());
}

#[test]
fn browser_scans_keep_sessions_in_order_and_the_first_failure_of_each_kind() {
    let found = collect_sessions([
        (
            BrowserType::Chrome,
            Err(CookieError::AppBoundEncryption {
                browsers: vec![BrowserType::Chrome],
            }),
        ),
        (
            BrowserType::Edge,
            Ok(vec![cookie("session", ".workbuddy.cn", "/")]),
        ),
        (BrowserType::Brave, Ok(Vec::new())),
        (
            BrowserType::Firefox,
            Err(CookieError::Decryption("first".into())),
        ),
        (
            BrowserType::Arc,
            Err(CookieError::AppBoundEncryption {
                browsers: vec![BrowserType::Arc],
            }),
        ),
        (
            BrowserType::Chrome,
            Err(CookieError::AppBoundEncryption {
                browsers: vec![BrowserType::Chrome],
            }),
        ),
        (BrowserType::Chromium, Err(CookieError::NoEncryptionKey)),
        (
            BrowserType::ChromeBeta,
            Ok(vec![cookie("locale", "www.workbuddy.cn", "/")]),
        ),
    ]);
    assert_eq!(
        found.cookies,
        vec!["session=session-value", "locale=locale-value"]
    );
    // Every blocked browser is named once, in browser order, in one error.
    assert_eq!(
        found.protected.map(|error| error.to_string()).as_deref(),
        Some(ABE_ERROR_CHROME_AND_ARC)
    );
    assert_eq!(
        found.unreadable.map(|error| error.to_string()).as_deref(),
        Some("Failed to read browser cookies: Decryption error: first")
    );

    let quiet = only_unreadable_browsers();
    assert!(quiet.cookies.is_empty());
    assert!(quiet.protected.is_none());
    assert_eq!(
        quiet.unreadable.map(|error| error.to_string()).as_deref(),
        Some("Failed to read browser cookies: No encryption key found")
    );
}

#[tokio::test]
async fn a_rejected_session_reports_chrome_app_bound_encryption_instead_of_expiry() {
    let (base_url, log) = serve(|_| (401, "{}".to_owned()));
    let provider = provider(&base_url, no_chrome, chrome_protected_edge_signed_in);
    let error = provider
        .fetch_usage(&automatic())
        .await
        .expect_err("Chrome is protected");
    assert_eq!(error.to_string(), ABE_ERROR);
    assert_ne!(
        provider.error_state_kind(&error),
        ProviderStateKind::ExpiredSession
    );
    // Edge's session was still tried.
    let cookies: Vec<String> = summary_requests(&log)
        .iter()
        .filter_map(|seen| seen.header("cookie").map(str::to_owned))
        .collect();
    assert_eq!(cookies, vec!["session=session-value"]);
}

#[tokio::test]
async fn a_rejected_session_reports_another_browser_read_failure_instead_of_expiry() {
    let (base_url, log) = serve(|_| (401, "{}".to_owned()));
    let error = provider(&base_url, no_chrome, firefox_unreadable_edge_signed_in)
        .fetch_usage(&automatic())
        .await
        .expect_err("Firefox is unreadable");
    assert_eq!(
        error.to_string(),
        "Failed to read browser cookies: Decryption error: fixture"
    );
    assert_eq!(paths(&log), vec![SUMMARY_FIXTURE_PATH]);
}

#[tokio::test]
async fn a_protected_chrome_without_any_session_reports_app_bound_encryption() {
    let (base_url, log) = serve(|seen| default_route(&seen.path));
    let error = provider(&base_url, chrome_154, chrome_protected_edge_without_session)
        .fetch_usage(&automatic())
        .await
        .expect_err("Chrome is protected");
    assert_eq!(error.to_string(), ABE_ERROR);
    assert!(paths(&log).is_empty());
}

#[tokio::test]
async fn other_read_failures_without_a_session_still_prompt_sign_in() {
    let (base_url, log) = serve(|seen| default_route(&seen.path));
    let error = provider(&base_url, chrome_154, only_unreadable_browsers)
        .fetch_usage(&automatic())
        .await
        .expect_err("no session");
    assert!(
        matches!(
            &error,
            ProviderError::BrowserSignInRequired { message, .. } if message == MISSING_SESSION
        ),
        "{error:?}"
    );
    assert!(paths(&log).is_empty());
}

#[tokio::test]
async fn a_pasted_header_is_expired_even_when_chrome_is_protected() {
    let (base_url, log) = serve(|_| (401, "{}".to_owned()));
    let error = provider(&base_url, no_chrome, chrome_protected_edge_signed_in)
        .fetch_usage(&manual(REJECTED_COOKIE))
        .await
        .expect_err("expired");
    assert_eq!(error.to_string(), SESSION_EXPIRED);
    assert_eq!(paths(&log), vec![SUMMARY_FIXTURE_PATH]);
}

#[tokio::test]
async fn off_and_oauth_sources_never_fetch() {
    let (base_url, log) = serve(|seen| default_route(&seen.path));
    let provider = provider(&base_url, chrome_154, no_browser_sessions);
    let off = FetchContext {
        source_mode: SourceMode::Cli,
        ..manual(FIXTURE_COOKIE)
    };
    assert_eq!(
        provider
            .fetch_usage(&off)
            .await
            .expect_err("cookies off")
            .to_string(),
        COOKIES_OFF
    );
    let oauth = FetchContext {
        source_mode: SourceMode::OAuth,
        ..manual(FIXTURE_COOKIE)
    };
    assert!(matches!(
        provider.fetch_usage(&oauth).await,
        Err(ProviderError::UnsupportedSource(SourceMode::OAuth))
    ));
    assert!(paths(&log).is_empty());
}

#[test]
fn metadata_matches_upstream() {
    let provider = WorkBuddyProvider::new();
    let metadata = provider.metadata();
    assert_eq!(provider.id(), ProviderId::WorkBuddy);
    assert_eq!(metadata.display_name, "WorkBuddy");
    assert_eq!(metadata.session_label, "Credits");
    assert_eq!(metadata.weekly_label, "Plan");
    assert!(!metadata.default_enabled);
    assert_eq!(
        metadata.dashboard_url,
        Some("https://www.workbuddy.cn/profile/plans-usage")
    );
    assert_eq!(
        provider.available_sources(),
        vec![SourceMode::Auto, SourceMode::Web]
    );
    assert!(provider.supports_web());
    assert!(provider.owns_browser_cookie_resolution());
    assert_eq!(
        provider.manual_empty_cookie_policy(),
        ManualEmptyCookiePolicy::FailClosedWeb
    );
    assert_eq!(ProviderId::WorkBuddy.cli_name(), "workbuddy");
    assert_eq!(
        ProviderId::from_cli_name("workbuddy"),
        Some(ProviderId::WorkBuddy)
    );
}
