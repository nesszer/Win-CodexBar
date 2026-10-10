use super::*;

#[test]
fn provider_cache_is_fresh_inside_stale_window() {
    assert!(crate::commands::is_provider_cache_fresh(
        Some(std::time::Instant::now()),
        std::time::Duration::from_secs(30),
    ));
}

#[test]
fn provider_cache_is_stale_when_missing_timestamp() {
    assert!(!crate::commands::is_provider_cache_fresh(
        None,
        std::time::Duration::from_secs(30),
    ));
}

#[test]
fn provider_cache_is_stale_after_window() {
    assert!(!crate::commands::is_provider_cache_fresh(
        Some(std::time::Instant::now() - std::time::Duration::from_secs(31)),
        std::time::Duration::from_secs(30),
    ));
}

#[test]
fn provider_fetch_timeout_allows_slower_authenticated_providers() {
    let ctx = FetchContext {
        web_timeout: 30,
        ..FetchContext::default()
    };
    assert_eq!(
        crate::commands::provider_fetch_timeout(ProviderId::Claude, &ctx),
        std::time::Duration::from_secs(75)
    );
    assert_eq!(
        crate::commands::provider_fetch_timeout(ProviderId::Codex, &ctx),
        std::time::Duration::from_secs(75)
    );
    assert_eq!(
        crate::commands::provider_fetch_timeout(ProviderId::Copilot, &ctx),
        std::time::Duration::from_secs(75)
    );
    assert_eq!(
        crate::commands::provider_fetch_timeout(ProviderId::DeepSeek, &ctx),
        std::time::Duration::from_secs(35)
    );

    let optional_litellm_ctx = FetchContext {
        web_timeout: 30,
        optional_details_enabled: true,
        ..FetchContext::default()
    };
    assert_eq!(
        crate::commands::provider_fetch_timeout(ProviderId::LiteLLM, &optional_litellm_ctx),
        std::time::Duration::from_secs(40)
    );
}

#[test]
fn provider_fetch_timeout_respects_context_web_timeout_with_cap() {
    let ctx = FetchContext {
        web_timeout: 60,
        ..FetchContext::default()
    };
    assert_eq!(
        crate::commands::provider_fetch_timeout(ProviderId::T3Chat, &ctx),
        std::time::Duration::from_secs(65)
    );

    let ctx = FetchContext {
        web_timeout: 120,
        ..FetchContext::default()
    };
    assert_eq!(
        crate::commands::provider_fetch_timeout(ProviderId::AzureOpenAI, &ctx),
        std::time::Duration::from_secs(65)
    );
}

#[test]
fn provider_cache_upsert_replaces_existing_provider() {
    let metadata = instantiate_provider(ProviderId::Codex).metadata().clone();
    let result = ProviderFetchResult::new(
        codexbar::core::UsageSnapshot::new(codexbar::core::RateWindow::new(10.0)),
        "CLI",
    );
    let mut first =
        ProviderUsageSnapshot::from_fetch_result(ProviderId::Codex, &metadata, &result, None);
    let mut second = first.clone();
    first.error = Some("old".to_string());
    second.error = Some("new".to_string());

    let mut cache = vec![first];
    crate::commands::upsert_provider_cache(&mut cache, second);

    assert_eq!(cache.len(), 1);
    assert_eq!(cache[0].provider_id, "codex");
    assert_eq!(cache[0].error.as_deref(), Some("new"));
}

#[test]
fn provider_cache_prunes_disabled_providers() {
    let metadata = instantiate_provider(ProviderId::Codex).metadata().clone();
    let result = ProviderFetchResult::new(
        codexbar::core::UsageSnapshot::new(codexbar::core::RateWindow::new(10.0)),
        "CLI",
    );
    let codex =
        ProviderUsageSnapshot::from_fetch_result(ProviderId::Codex, &metadata, &result, None);
    let claude_meta = instantiate_provider(ProviderId::Claude).metadata().clone();
    let claude =
        ProviderUsageSnapshot::from_fetch_result(ProviderId::Claude, &claude_meta, &result, None);

    let mut cache = vec![codex, claude];
    crate::commands::prune_provider_cache_to_enabled(&mut cache, &[ProviderId::Codex]);

    assert_eq!(cache.len(), 1);
    assert_eq!(cache[0].provider_id, "codex");
}

#[test]
fn superseded_refresh_generation_is_not_current() {
    let mut state = AppState::new();
    state.provider_refresh_generation = 3;
    assert!(crate::commands::is_current_provider_refresh_generation(
        &state, 3
    ));
    assert!(!crate::commands::is_current_provider_refresh_generation(
        &state, 2
    ));
}

#[test]

fn claude_transient_auth_failure_preserves_first_last_good_snapshot() {
    let metadata = instantiate_provider(ProviderId::Claude).metadata().clone();
    let result = ProviderFetchResult::new(
        codexbar::core::UsageSnapshot::new(codexbar::core::RateWindow::new(42.0)),
        "OAuth",
    );
    let good =
        ProviderUsageSnapshot::from_fetch_result(ProviderId::Claude, &metadata, &result, None);
    let snapshot = ProviderUsageSnapshot::from_error(
        ProviderId::Claude,
        &metadata,
        "Unauthorized".to_string(),
        codexbar::core::ProviderStateKind::NeedsAuthentication,
    );
    let error = ProviderError::AuthRequired;
    let mut state = crate::state::AppState::new();
    state.provider_cache.push(good.clone());

    let preserved = crate::commands::providers::preserve_last_good_transient_failure(
        &mut state,
        ProviderId::Claude,
        snapshot,
        &error,
    );

    assert_eq!(preserved.error, None);
    assert_eq!(preserved.primary.used_percent, 42.0);
}

#[test]
fn codex_transient_transport_failure_helper_uses_typed_policy() {
    let metadata = instantiate_provider(ProviderId::Codex).metadata().clone();
    let result = ProviderFetchResult::new(
        codexbar::core::UsageSnapshot::new(codexbar::core::RateWindow::new(42.0)),
        "OAuth",
    );
    let good =
        ProviderUsageSnapshot::from_fetch_result(ProviderId::Codex, &metadata, &result, None);
    let snapshot = ProviderUsageSnapshot::from_error(
        ProviderId::Codex,
        &metadata,
        "Timeout".to_string(),
        codexbar::core::ProviderStateKind::Unknown,
    );
    let mut state = crate::state::AppState::new();
    state.provider_cache.push(good);

    let preserved = crate::commands::providers::preserve_last_good_transient_failure(
        &mut state,
        ProviderId::Codex,
        snapshot,
        &ProviderError::Timeout,
    );

    assert_eq!(preserved.error, None);
    assert_eq!(preserved.primary.used_percent, 42.0);
}

#[test]
fn claude_repeated_auth_failure_surfaces_error() {
    let metadata = instantiate_provider(ProviderId::Claude).metadata().clone();
    let result = ProviderFetchResult::new(
        codexbar::core::UsageSnapshot::new(codexbar::core::RateWindow::new(42.0)),
        "OAuth",
    );
    let good =
        ProviderUsageSnapshot::from_fetch_result(ProviderId::Claude, &metadata, &result, None);
    let first_error = ProviderUsageSnapshot::from_error(
        ProviderId::Claude,
        &metadata,
        "Unauthorized".to_string(),
        codexbar::core::ProviderStateKind::NeedsAuthentication,
    );
    let second_error = first_error.clone();
    let failure = ProviderError::AuthRequired;
    let mut state = crate::state::AppState::new();
    state.provider_cache.push(good);

    let _ = crate::commands::providers::preserve_last_good_transient_failure(
        &mut state,
        ProviderId::Claude,
        first_error,
        &failure,
    );
    let surfaced = crate::commands::providers::preserve_last_good_transient_failure(
        &mut state,
        ProviderId::Claude,
        second_error,
        &failure,
    );

    assert!(surfaced.error.is_some());
}

#[test]
fn claude_cloudflare_challenge_retains_prior_usage_while_surfaceing_guidance() {
    let metadata = instantiate_provider(ProviderId::Claude).metadata().clone();
    let result = ProviderFetchResult::new(
        codexbar::core::UsageSnapshot::new(codexbar::core::RateWindow::new(42.0)),
        "OAuth",
    );
    let good =
        ProviderUsageSnapshot::from_fetch_result(ProviderId::Claude, &metadata, &result, None);
    let challenge = codexbar::providers::claude::CLOUDFLARE_CHALLENGE_MESSAGE;
    let error = ProviderUsageSnapshot::from_error(
        ProviderId::Claude,
        &metadata,
        challenge.to_string(),
        codexbar::core::ProviderStateKind::Unknown,
    );
    let failure = ProviderError::Other(challenge.to_string());
    let mut state = crate::state::AppState::new();
    state.provider_cache.push(good);

    let surfaced = crate::commands::providers::preserve_last_good_transient_failure(
        &mut state,
        ProviderId::Claude,
        error,
        &failure,
    );

    assert_eq!(surfaced.error, None);
    assert_eq!(surfaced.primary.used_percent, 42.0);
    assert_eq!(
        crate::commands::providers::preserve_last_good_transient_failure(
            &mut state,
            ProviderId::Claude,
            ProviderUsageSnapshot::from_error(
                ProviderId::Claude,
                &metadata,
                challenge.to_string(),
                codexbar::core::ProviderStateKind::Unknown,
            ),
            &failure,
        )
        .error
        .as_deref(),
        Some(challenge)
    );
}

#[test]
fn claude_cloudflare_challenge_keeps_prior_usage_when_guidance_surfaces() {
    let metadata = instantiate_provider(ProviderId::Claude).metadata().clone();
    let result = ProviderFetchResult::new(
        codexbar::core::UsageSnapshot::new(codexbar::core::RateWindow::new(42.0)),
        "Web",
    );
    let mut good =
        ProviderUsageSnapshot::from_fetch_result(ProviderId::Claude, &metadata, &result, None);
    good.updated_at = "2026-09-01T00:00:00Z".to_string();
    let error = ProviderUsageSnapshot::from_error(
        ProviderId::Claude,
        &metadata,
        codexbar::providers::claude::CLOUDFLARE_CHALLENGE_MESSAGE.to_string(),
        codexbar::core::ProviderStateKind::Unknown,
    );
    let failure =
        ProviderError::Other(codexbar::providers::claude::CLOUDFLARE_CHALLENGE_MESSAGE.to_string());
    let mut state = crate::state::AppState::new();
    state.provider_cache.push(good.clone());

    let first = crate::commands::providers::preserve_last_good_transient_failure(
        &mut state,
        ProviderId::Claude,
        error.clone(),
        &failure,
    );
    let second = crate::commands::providers::preserve_last_good_transient_failure(
        &mut state,
        ProviderId::Claude,
        error,
        &failure,
    );

    assert_eq!(first.error, None);
    assert_eq!(first.primary.used_percent, 42.0);
    assert_eq!(
        second.error.as_deref(),
        Some(codexbar::providers::claude::CLOUDFLARE_CHALLENGE_MESSAGE,)
    );
    assert_eq!(second.primary.used_percent, 42.0);
    assert_eq!(second.updated_at, good.updated_at);
}

#[test]
fn claude_cli_parse_failure_keeps_last_good_every_time() {
    let metadata = instantiate_provider(ProviderId::Claude).metadata().clone();
    let mut result = ProviderFetchResult::new(
        codexbar::core::UsageSnapshot::new(codexbar::core::RateWindow::new(17.0)),
        "CLI",
    );
    result.has_successful_claude_cli_quota = true;
    let good =
        ProviderUsageSnapshot::from_fetch_result(ProviderId::Claude, &metadata, &result, None);
    let err = ProviderUsageSnapshot::from_error(
        ProviderId::Claude,
        &metadata,
        "Parse error: Empty output from Claude CLI".to_string(),
        codexbar::core::ProviderStateKind::Unknown,
    );
    let failure = ProviderError::Parse("Empty output from Claude CLI".to_string());
    let mut state = crate::state::AppState::new();
    state.provider_cache.push(good.clone());

    let first = crate::commands::providers::preserve_last_good_transient_failure(
        &mut state,
        ProviderId::Claude,
        err.clone(),
        &failure,
    );
    let second = crate::commands::providers::preserve_last_good_transient_failure(
        &mut state,
        ProviderId::Claude,
        err,
        &failure,
    );

    assert_eq!(first.error, None);
    assert_eq!(first.primary.used_percent, 17.0);
    assert!(!first.has_successful_claude_cli_quota);
    // Parse failures keep last-good on every refresh (upstream #2247), unlike one-shot auth.
    assert_eq!(second.error, None);
    assert_eq!(second.primary.used_percent, 17.0);
}

#[test]
fn claude_hard_credentials_missing_does_not_preserve_stale() {
    let metadata = instantiate_provider(ProviderId::Claude).metadata().clone();
    let result = ProviderFetchResult::new(
        codexbar::core::UsageSnapshot::new(codexbar::core::RateWindow::new(17.0)),
        "OAuth",
    );
    let good =
        ProviderUsageSnapshot::from_fetch_result(ProviderId::Claude, &metadata, &result, None);
    let err = ProviderUsageSnapshot::from_error(
        ProviderId::Claude,
        &metadata,
        "OAuth error: Claude OAuth credentials not found. Run `claude` to authenticate."
            .to_string(),
        codexbar::core::ProviderStateKind::NeedsAuthentication,
    );
    let failure = ProviderError::OAuth(
        "Claude OAuth credentials not found. Run `claude` to authenticate.".to_string(),
    );
    let mut state = crate::state::AppState::new();
    state.provider_cache.push(good);

    let out = crate::commands::providers::preserve_last_good_transient_failure(
        &mut state,
        ProviderId::Claude,
        err,
        &failure,
    );
    assert!(out.error.is_some());
    assert_eq!(
        out.error_state,
        codexbar::core::ProviderStateKind::NeedsAuthentication,
        "hard auth failure must carry its classification on the snapshot"
    );
}

#[test]
fn claude_error_message_removes_upstream_swift_cancellation() {
    let message = crate::commands::friendly_provider_error(
        ProviderId::Claude,
        "The operation couldn't be completed. (Swift.CancellationError error 1.)",
    );

    assert!(!message.contains("Swift"));
    assert!(message.contains("Claude usage fetch was cancelled"));
    assert!(message.contains("Refresh Claude"));
}

#[test]
fn claude_error_message_explains_missing_sign_in() {
    let message = crate::commands::friendly_provider_error(
        ProviderId::Claude,
        "OAuth error: Claude OAuth credentials not found. Run `claude` to authenticate.",
    );

    assert_eq!(
        message,
        "Claude sign-in was not found. Run `claude` once to authenticate, then refresh Claude in Win-CodexBar."
    );
}

#[test]
fn claude_cloudflare_error_preserves_distinct_recovery_guidance() {
    let challenge = codexbar::providers::claude::CLOUDFLARE_CHALLENGE_MESSAGE;
    let message = crate::commands::friendly_provider_error(ProviderId::Claude, challenge);

    assert_eq!(message, challenge);
    assert!(message.contains("OAuth"));
    assert!(message.contains("different network"));
}

#[test]
fn non_claude_error_message_is_preserved() {
    let message = crate::commands::friendly_provider_error(
        ProviderId::Codex,
        "OAuth error: Claude OAuth credentials not found. Run `claude` to authenticate.",
    );

    assert_eq!(
        message,
        "OAuth error: Claude OAuth credentials not found. Run `claude` to authenticate."
    );
}
