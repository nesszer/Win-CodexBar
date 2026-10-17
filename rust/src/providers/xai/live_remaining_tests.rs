//! Tests for the xAI live remaining estimate (upstream 24d5d7059, #4419).
//!
//! Synthetic fixtures only. Pure cases mirror the upstream invoice matrix;
//! HTTP cases drive the real balance, preview, and usage requests against a
//! local mockito server.

use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use super::live_remaining::{
    LIVE_REMAINING_ID, LIVE_REMAINING_TITLE, live_remaining_detail, live_remaining_usd_from_preview,
};
use super::*;
use crate::core::ProviderDisplayDetail;

const TEAM_ID: &str = "team-1234";
const API_KEY: &str = "fixture-management-key";
const BEARER: &str = "Bearer fixture-management-key";
const BALANCE_PATH: &str = "/v1/billing/teams/team-1234/prepaid/balance";
const PREVIEW_PATH: &str = "/v1/billing/teams/team-1234/postpaid/invoice/preview";
const USAGE_PATH: &str = "/v1/billing/teams/team-1234/usage";

/// Posted balance: a $10.00 top-up recorded as "-1000" cents.
const BALANCE_FIXTURE: &str = r#"{"total":{"val":"-1000"}}"#;
/// Three UTC days summing to 1.75973725 USD.
const USAGE_FIXTURE: &str = r#"{
  "timeSeries": [
    {
      "group": ["Chat grok-4-fixture"],
      "dataPoints": [
        { "timestamp": "2027-01-13T00:00:00Z", "values": [0.75973725] },
        { "timestamp": "2027-01-14T00:00:00Z", "values": [0.5] },
        { "timestamp": "2027-01-15T00:00:00Z", "values": [0] }
      ]
    },
    {
      "group": ["Live search"],
      "dataPoints": [
        { "timestamp": "2027-01-13T00:00:00Z", "values": [0.5] },
        { "timestamp": "2027-01-14T00:00:00Z", "values": [0] },
        { "timestamp": "2027-01-15T00:00:00Z", "values": [0] }
      ]
    }
  ],
  "limitReached": false
}"#;
const PREVIEW_350: &str = r#"{"coreInvoice":{"amountAfterVat":"350"}}"#;

fn now() -> DateTime<Utc> {
    DateTime::from_timestamp(1_800_000_000, 0).unwrap()
}

fn invoice_body(amount: &str) -> String {
    format!(r#"{{"coreInvoice":{{"amountAfterVat":"{amount}","totalWithCorr":{{"val":"999"}}}}}}"#)
}

fn live_row(result: &ProviderFetchResult) -> Option<&ProviderDisplayDetail> {
    result
        .display_details()
        .iter()
        .find(|row| row.id() == LIVE_REMAINING_ID)
}

fn provider_for(server: &mockito::ServerGuard) -> XaiProvider {
    XaiProvider::new().with_base_url(server.url())
}

async fn mount_balance_and_usage(
    server: &mut mockito::ServerGuard,
) -> (mockito::Mock, mockito::Mock) {
    let balance = server
        .mock("GET", BALANCE_PATH)
        .match_header("authorization", BEARER)
        .with_status(200)
        .with_body(BALANCE_FIXTURE)
        .create_async()
        .await;
    let usage = server
        .mock("POST", USAGE_PATH)
        .match_header("authorization", BEARER)
        .with_status(200)
        .with_body(USAGE_FIXTURE)
        .create_async()
        .await;
    (balance, usage)
}

async fn mount_preview(
    server: &mut mockito::ServerGuard,
    status: usize,
    body: &str,
) -> mockito::Mock {
    server
        .mock("GET", PREVIEW_PATH)
        .match_header("authorization", BEARER)
        .match_header("accept", "application/json")
        .with_status(status)
        .with_body(body)
        .create_async()
        .await
}

#[test]
fn live_estimate_subtracts_documented_invoice_cents() {
    // Posted balance from the ledger value "-27481" cents.
    let balance = balance_usd_from_ledger_cents("-27481").unwrap();
    for (cents, expected_value, expected_usd) in [
        ("13364", "$141.17", 141.17),
        ("0", "$274.81", 274.81),
        ("-100", "$275.81", 275.81),
        ("30000", "$-25.19", -25.19),
    ] {
        let remaining = live_remaining_usd_from_preview(balance, 200, &invoice_body(cents))
            .unwrap_or_else(|| panic!("expected an estimate for {cents}"));
        assert!(
            (remaining - expected_usd).abs() < 1e-9,
            "cents {cents}: {remaining}"
        );
        let row = live_remaining_detail(Some(remaining)).expect("estimate row");
        assert_eq!(row.value(), expected_value, "cents {cents}");
    }
}

#[test]
fn unavailable_invoice_preview_yields_no_estimate() {
    let cases: [(u16, &str); 16] = [
        (403, PREVIEW_350),
        (404, PREVIEW_350),
        (500, PREVIEW_350),
        (200, ""),
        (200, "{}"),
        (200, r#"{"coreInvoice":{"amountAfterVat":null}}"#),
        (200, r#"{"coreInvoice":{"amountAfterVat":""}}"#),
        (200, r#"{"coreInvoice":{"amountAfterVat":"n/a"}}"#),
        (
            200,
            r#"{"coreInvoice":{"amountAfterVat":"9007199254740993"}}"#,
        ),
        (200, r#"{"coreInvoice":{"amountAfterVat":"+350"}}"#),
        (200, r#"{"coreInvoice":{"amountAfterVat":"3.50"}}"#),
        (200, r#"{"coreInvoice":{"amountAfterVat":"-"}}"#),
        (200, r#"{"coreInvoice":{"amountAfterVat":350}}"#),
        (200, r#"{"coreInvoice":"350"}"#),
        (200, r#"{"coreInvoice":{"totalWithCorr":{"val":"350"}}}"#),
        (200, "not json"),
    ];
    for (status, body) in cases {
        assert_eq!(
            live_remaining_usd_from_preview(10.0, status, body),
            None,
            "status {status} body {body:?}"
        );
    }
}

#[test]
fn safe_integer_cents_boundary() {
    assert!(
        live_remaining_usd_from_preview(10.0, 200, &invoice_body("9007199254740991")).is_some()
    );
    assert!(
        live_remaining_usd_from_preview(10.0, 200, &invoice_body("-9007199254740991")).is_some()
    );
    assert!(
        live_remaining_usd_from_preview(10.0, 200, &invoice_body("9007199254740992")).is_none()
    );
    assert!(
        live_remaining_usd_from_preview(10.0, 200, &invoice_body("-9007199254740992")).is_none()
    );
}

#[test]
fn live_row_uses_documented_id_title_and_value() {
    let row = live_remaining_detail(Some(6.5)).expect("row");
    assert_eq!(row.id(), "xai-live-remaining");
    assert_eq!(row.title(), "Live remaining (est.)");
    assert_eq!(row.value(), "$6.50");
    assert_eq!(row.title(), LIVE_REMAINING_TITLE);
    assert!(live_remaining_detail(None).is_none());
    assert!(live_remaining_detail(Some(f64::NAN)).is_none());
}

#[tokio::test]
async fn live_estimate_is_added_while_posted_credit_is_unchanged() {
    let mut server = mockito::Server::new_async().await;
    let (balance, usage) = mount_balance_and_usage(&mut server).await;
    let preview = mount_preview(&mut server, 200, PREVIEW_350).await;

    let snapshot = provider_for(&server)
        .fetch_usage_snapshot(API_KEY, TEAM_ID, now())
        .await
        .unwrap();

    balance.assert_async().await;
    preview.assert_async().await;
    usage.assert_async().await;
    assert!((snapshot.balance_usd - 10.0).abs() < 1e-9);
    assert_eq!(snapshot.daily.len(), 3);

    let result = into_fetch_result(&snapshot);
    let row = live_row(&result).expect("live remaining row");
    assert_eq!(row.title(), LIVE_REMAINING_TITLE);
    assert_eq!(row.value(), "$6.50");
    assert_eq!(result.display_details().len(), 1);
    assert_eq!(snapshot.to_cost_snapshot().balance, Some(10.0));
    assert!(
        result
            .usage
            .primary
            .reset_description
            .as_deref()
            .unwrap_or("")
            .contains("Balance: $10.00")
    );
}

#[tokio::test]
async fn unavailable_preview_over_http_omits_only_the_estimate() {
    let cases: [(usize, &str); 7] = [
        (403, PREVIEW_350),
        (404, PREVIEW_350),
        (200, ""),
        (200, "{}"),
        (200, r#"{"coreInvoice":{"amountAfterVat":null}}"#),
        (200, r#"{"coreInvoice":{"amountAfterVat":"n/a"}}"#),
        (
            200,
            r#"{"coreInvoice":{"amountAfterVat":"9007199254740993"}}"#,
        ),
    ];
    for (status, body) in cases {
        let mut server = mockito::Server::new_async().await;
        let (balance, usage) = mount_balance_and_usage(&mut server).await;
        let preview = mount_preview(&mut server, status, body).await;

        let snapshot = provider_for(&server)
            .fetch_usage_snapshot(API_KEY, TEAM_ID, now())
            .await
            .unwrap();

        balance.assert_async().await;
        preview.assert_async().await;
        usage.assert_async().await;
        assert!(
            (snapshot.balance_usd - 10.0).abs() < 1e-9,
            "status {status}"
        );
        assert_eq!(snapshot.daily.len(), 3, "status {status}");
        let result = into_fetch_result(&snapshot);
        assert!(live_row(&result).is_none(), "status {status} body {body:?}");
        assert!(result.display_details().is_empty(), "status {status}");
    }
}

#[tokio::test]
async fn preview_transport_failure_preserves_posted_credit_and_history() {
    // Records the order of the three billing requests.
    let order = Arc::new(Mutex::new(Vec::<&'static str>::new()));
    let mut server = mockito::Server::new_async().await;

    let balance_order = Arc::clone(&order);
    let balance = server
        .mock("GET", BALANCE_PATH)
        .match_header("authorization", BEARER)
        .with_status(200)
        .with_body_from_request(move |_| {
            balance_order.lock().unwrap().push("balance");
            BALANCE_FIXTURE.as_bytes().to_vec()
        })
        .create_async()
        .await;

    // Send a complete, well-formed preview body and then abort the connection
    // before the chunked stream terminates. The client must surface a transport
    // error, so the estimate stays absent even though the bytes parse cleanly.
    let preview_order = Arc::clone(&order);
    let preview = server
        .mock("GET", PREVIEW_PATH)
        .match_header("authorization", BEARER)
        .with_status(200)
        .with_chunked_body(move |writer: &mut dyn Write| {
            preview_order.lock().unwrap().push("preview");
            writer.write_all(PREVIEW_350.as_bytes())?;
            writer.flush()?;
            Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "fixture: connection dropped mid-body",
            ))
        })
        .create_async()
        .await;

    let usage_order = Arc::clone(&order);
    let usage = server
        .mock("POST", USAGE_PATH)
        .match_header("authorization", BEARER)
        .with_status(200)
        .with_body_from_request(move |_| {
            usage_order.lock().unwrap().push("usage");
            USAGE_FIXTURE.as_bytes().to_vec()
        })
        .create_async()
        .await;

    let snapshot = provider_for(&server)
        .fetch_usage_snapshot(API_KEY, TEAM_ID, now())
        .await
        .unwrap();

    balance.assert_async().await;
    preview.assert_async().await;
    usage.assert_async().await;
    assert_eq!(*order.lock().unwrap(), vec!["balance", "preview", "usage"]);
    assert!((snapshot.balance_usd - 10.0).abs() < 1e-9);
    assert_eq!(snapshot.daily.len(), 3);
    let result = into_fetch_result(&snapshot);
    assert!(live_row(&result).is_none());
}
