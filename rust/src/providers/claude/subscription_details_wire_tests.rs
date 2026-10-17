//! Wire tests for the optional Claude Web subscription_details request.

use super::{AccountResponse, ClaudeWebApiFetcher};
use mockito::{Server, ServerGuard};

const PATH: &str = "/organizations/org-123/subscription_details";
const RENEWING_BODY: &str = r#"{"status":"active","next_charge_at":null,
    "next_charge_date":"2026-12-01","plan_ending_at":null,"plan_ending_before":null}"#;

async fn fetch(server: &ServerGuard) -> Option<crate::core::SubscriptionMetadata> {
    let fetcher = ClaudeWebApiFetcher::new().with_base_url(server.url());
    let headers = ClaudeWebApiFetcher::build_headers("sessionKey=sk-ant-fixture-token");
    fetcher.get_subscription_details("org-123", &headers).await
}

#[tokio::test]
async fn recognized_response_yields_calendar_renewal() {
    let mut server = Server::new_async().await;
    let mock = server
        .mock("GET", PATH)
        .match_header("cookie", "sessionKey=sk-ant-fixture-token")
        .with_status(200)
        .with_body(RENEWING_BODY)
        .expect(1)
        .create_async()
        .await;

    let metadata = fetch(&server).await.expect("metadata");

    assert!(metadata.renews_at.is_some());
    assert!(metadata.renews_at_is_date_only);
    mock.assert_async().await;
}

#[tokio::test]
async fn unavailable_billing_access_yields_nothing() {
    for status in [401, 403, 404, 500] {
        let mut server = Server::new_async().await;
        let _mock = server
            .mock("GET", PATH)
            .with_status(status)
            .with_body(RENEWING_BODY)
            .create_async()
            .await;
        assert!(fetch(&server).await.is_none(), "status {status}");
    }
}

#[tokio::test]
async fn malformed_body_yields_nothing() {
    let mut server = Server::new_async().await;
    let _mock = server
        .mock("GET", PATH)
        .with_status(200)
        .with_body(r#"{"status":"active"}"#)
        .create_async()
        .await;
    assert!(fetch(&server).await.is_none());
}

#[test]
fn membership_check_requires_the_fetched_organization() {
    let account: AccountResponse = serde_json::from_str(
        r#"{"email_address":"a@example.com","memberships":[
            {"organization":{"uuid":"org-123"}},
            {"uuid":"org-legacy"}]}"#,
    )
    .unwrap();
    assert!(account.is_member_of("org-123"));
    assert!(!account.is_member_of("org-legacy"));
    assert!(!account.is_member_of("org-other"));
}
