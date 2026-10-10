use super::*;

fn empty_snapshot() -> ProviderUsageSnapshot {
    let metadata = codexbar::core::instantiate_provider(ProviderId::Claude)
        .metadata()
        .clone();
    ProviderUsageSnapshot::from_error(
        ProviderId::Claude,
        &metadata,
        "unused".to_string(),
        codexbar::core::ProviderStateKind::Unknown,
    )
}

#[test]
fn quota_notification_account_identity_prefers_token_then_email() {
    let account_id = uuid::Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap();
    let mut snapshot = empty_snapshot();
    snapshot.account_email = Some("Person@Example.com".to_string());
    snapshot.account_organization = Some("Acme Org".to_string());
    snapshot.plan_name = Some("Pro".to_string());

    assert_eq!(
        quota_notification_account_identity(&snapshot, Some(account_id)),
        "token-account:aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa"
    );
    assert_eq!(
        quota_notification_account_identity(&snapshot, None),
        "person@example.com"
    );

    snapshot.account_email = None;
    assert_eq!(
        quota_notification_account_identity(&snapshot, None),
        "org:acme org"
    );

    snapshot.account_organization = None;
    snapshot.source_label = "oauth".to_string();
    assert_eq!(
        quota_notification_account_identity(&snapshot, None),
        "claude:oauth:unknown"
    );

    snapshot.plan_name = None;
    snapshot.source_label = "cli (reduced fidelity)".to_string();
    assert_eq!(
        quota_notification_account_identity(&snapshot, None),
        "claude:cli:unknown"
    );
}

/// The forecast scope key and the notification identity must never disagree.
/// If they did, one account would be seen as two identities and its burn history
/// would be split, silently halving the sample count behind every forecast.
#[test]
fn forecast_account_key_matches_notification_identity() {
    use crate::commands::bridge::forecast_account_key;

    let token = uuid::Uuid::parse_str("aaaaaaaa-aaaa-aaaa-aaaa-aaaaaaaaaaaa").unwrap();
    let mut usage = codexbar::core::UsageSnapshot::new(codexbar::core::RateWindow::new(1.0));
    let mut snapshot = empty_snapshot();

    for (email, org) in [
        (Some("Person@Example.com"), Some("Acme Org")),
        (Some("Person@Example.com"), None),
        (None, Some("Acme Org")),
        (None, None),
    ] {
        usage.account_email = email.map(str::to_string);
        usage.account_organization = org.map(str::to_string);
        snapshot.account_email = usage.account_email.clone();
        snapshot.account_organization = usage.account_organization.clone();

        for tok in [Some(token), None] {
            assert_eq!(
                forecast_account_key(&usage, tok).unwrap_or_default(),
                quota_notification_account_identity(&snapshot, tok),
                "identity drift for email={email:?} org={org:?} token={tok:?}"
            );
        }
    }
}

#[test]
fn provider_timeout_error_message_is_plain_timeout() {
    // The elapsed-timeout path reuses the provider-error path, which
    // redacts the message; the user must still see "Timeout".
    assert_eq!(
        codexbar::logging::safe_error_message(codexbar::core::ProviderError::Timeout),
        "Timeout"
    );
}
