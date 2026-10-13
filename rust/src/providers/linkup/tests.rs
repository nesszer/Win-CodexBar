use crate::core::{Provider, ProviderId};
use crate::providers::LinkupProvider;

#[test]
fn provider_metadata() {
    let provider = LinkupProvider::new();
    assert_eq!(provider.id(), ProviderId::Linkup);
    assert_eq!(provider.metadata().display_name, "Linkup");
    assert_eq!(provider.metadata().session_label, "Balance");
    assert!(!provider.metadata().supports_opus);
    assert!(!provider.metadata().supports_credits);
    assert!(!provider.metadata().default_enabled);
    assert!(!provider.metadata().is_primary);
}

#[test]
fn balance_parsing_basic() {
    let result =
        crate::providers::linkup::parse_balance(&serde_json::json!({ "balance": 123.456 }))
            .unwrap();

    assert_eq!(result.display_details.len(), 1);
    assert_eq!(result.display_details[0].id(), "linkup-balance");
    assert_eq!(result.display_details[0].title(), "Credit balance");
    assert_eq!(result.display_details[0].value(), "$123.46");
    assert_eq!(
        result.display_details[0].section_title(),
        Some("Account balance")
    );
    assert_eq!(
        result.usage.primary.reset_description.as_deref(),
        Some("Credit balance: $123.46")
    );
    assert!(result.usage.primary.is_informational);
}

#[test]
fn balance_parsing_zero_and_negative() {
    let test_cases = vec![(0.0, "$0.00"), (-1.25, "-$1.25")];

    for (value, expected) in test_cases {
        let result =
            crate::providers::linkup::parse_balance(&serde_json::json!({ "balance": value }))
                .unwrap();
        assert_eq!(result.display_details[0].value(), expected);
    }
}

#[test]
fn balance_parsing_rejects_malformed() {
    let test_cases = vec![
        serde_json::json!(null),
        serde_json::json!([]),
        serde_json::json!({}),
        serde_json::json!({ "balance": null }),
        serde_json::json!({ "balance": true }),
        serde_json::json!({ "balance": "12" }),
        serde_json::json!({ "balance": [1] }),
    ];

    for value in test_cases {
        let err = crate::providers::linkup::parse_balance(&value);
        assert!(err.is_err(), "Expected failure for: {:?}", value);
        let err_str = err.unwrap_err().to_string();
        assert!(
            err_str.contains("unrecognized credit balance"),
            "Error should mention unrecognized balance, got: {}",
            err_str
        );
    }
}

#[test]
fn only_a_rejected_key_reports_an_expired_session() {
    use crate::core::{ProviderError, ProviderStateKind};
    let provider = LinkupProvider::new();
    let kind =
        |status| provider.error_state_kind(&ProviderError::Other(super::status_message(status)));
    assert_eq!(super::status_message(401), "Linkup returned HTTP 401.");
    assert_eq!(kind(401), ProviderStateKind::ExpiredSession);
    for status in [403, 429, 503, 418] {
        assert_eq!(kind(status), ProviderStateKind::Unknown, "status {status}");
    }
}
