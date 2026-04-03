//! InfiniClient API 测试

use codexbar::providers::infini::{InfiniClient, InfiniError, InfiniUsage, UsagePeriod};
use codexbar::providers::InfiniProvider;
use codexbar::core::{Provider, ProviderId, FetchContext, SourceMode};

#[tokio::test]
async fn test_fetch_usage_success() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/maas/coding/usage")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
            "5_hour": {"quota": 5000, "used": 1000, "remain": 4000},
            "7_day": {"quota": 30000, "used": 5000, "remain": 25000},
            "30_day": {"quota": 60000, "used": 10000, "remain": 50000}
        }"#,
        )
        .create();

    let client = InfiniClient::new("sk-cp-test-key".to_string()).with_base_url(server.url());

    let usage = client.fetch_usage().await.unwrap();

    mock.assert();
    assert_eq!(usage.five_hour.quota, 5000);
    assert_eq!(usage.seven_day.used, 5000);
}

#[tokio::test]
async fn test_fetch_usage_unauthorized() {
    let mut server = mockito::Server::new();
    let mock = server.mock("GET", "/maas/coding/usage").with_status(401).create();

    let client = InfiniClient::new("invalid-key".to_string()).with_base_url(server.url());

    let result = client.fetch_usage().await;

    mock.assert();
    assert!(matches!(result, Err(InfiniError::Unauthorized)));
}

// ==================== InfiniProvider Tests ====================

#[test]
fn test_infini_provider_id() {
    let provider = InfiniProvider::new("sk-cp-test".to_string());
    assert_eq!(provider.id(), ProviderId::Infini);
}

#[test]
fn test_infini_provider_metadata() {
    let provider = InfiniProvider::new("sk-cp-test".to_string());
    let meta = provider.metadata();
    assert_eq!(meta.id, ProviderId::Infini);
    assert_eq!(meta.display_name, "Infini");
}

#[test]
fn test_infini_provider_available_sources() {
    let provider = InfiniProvider::new("sk-cp-test".to_string());
    let sources = provider.available_sources();
    assert!(sources.contains(&SourceMode::Auto));
    assert!(sources.contains(&SourceMode::Web));
}

#[test]
fn test_infini_provider_supports_web() {
    let provider = InfiniProvider::new("sk-cp-test".to_string());
    assert!(provider.supports_web());
}

#[tokio::test]
async fn test_infini_provider_fetch_usage_success() {
    let mut server = mockito::Server::new();
    let mock = server
        .mock("GET", "/maas/coding/usage")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(
            r#"{
            "5_hour": {"quota": 5000, "used": 2500, "remain": 2500},
            "7_day": {"quota": 30000, "used": 15000, "remain": 15000},
            "30_day": {"quota": 60000, "used": 30000, "remain": 30000}
        }"#,
        )
        .create();

    let provider = InfiniProvider::new(String::new()).with_base_url(server.url());
    let ctx = FetchContext {
        api_key: Some("sk-cp-test-key".to_string()),
        ..Default::default()
    };
    let result = provider.fetch_usage(&ctx).await.unwrap();

    mock.assert();
    assert_eq!(result.usage.primary.used_percent, 50.0);
    assert!(result.usage.secondary.is_some());
    let secondary = result.usage.secondary.unwrap();
    assert_eq!(secondary.used_percent, 50.0);
}

#[tokio::test]
async fn test_infini_provider_fetch_usage_unauthorized() {
    let mut server = mockito::Server::new();
    let mock = server.mock("GET", "/maas/coding/usage").with_status(401).create();

    let provider = InfiniProvider::new(String::new()).with_base_url(server.url());
    let ctx = FetchContext {
        api_key: Some("invalid-key".to_string()),
        ..Default::default()
    };
    let result = provider.fetch_usage(&ctx).await;

    mock.assert();
    assert!(result.is_err());
}
