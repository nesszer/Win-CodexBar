use super::*;

#[test]
fn authentication_status_recovers_and_rejects_stale_accounts() {
    let mut state = AppState::new();
    let generation = state.provider_refresh_generation;
    let account = sample_account();
    publish_codex_authentication(
        &mut state,
        generation,
        std::slice::from_ref(&account),
        vec![(account.clone(), true)],
    );
    assert!(state.codex_account_needs_authentication[&account.id]);
    publish_codex_authentication(
        &mut state,
        generation + 1,
        std::slice::from_ref(&account),
        vec![(account.clone(), false)],
    );
    assert!(state.codex_account_needs_authentication[&account.id]);
    publish_codex_authentication(
        &mut state,
        generation,
        std::slice::from_ref(&account),
        vec![(account.clone(), false)],
    );
    assert!(!state.codex_account_needs_authentication[&account.id]);
    publish_codex_authentication(&mut state, generation, &[], vec![(account, true)]);
    assert!(state.codex_account_needs_authentication.is_empty());
}

#[test]
fn committed_switch_tolerates_unreadable_account_metadata() {
    use codexbar::codex_accounts::file_locations;
    let root = std::env::temp_dir().join(format!("codex-switch-metadata-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    file_locations::with_app_support_directory(root.clone());
    let account_file = file_locations::accounts_file();
    std::fs::write(&account_file, "invalid account metadata").unwrap();
    // This post-commit operation cannot propagate an error to the switch
    // command and skip the refresh/events that follow it.
    persist_materialized_account(Some(&sample_account()));
    assert_eq!(
        std::fs::read_to_string(&account_file).unwrap(),
        "invalid account metadata"
    );
    file_locations::clear_app_support_directory_override();
    assert!(root.starts_with(std::env::temp_dir()));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn superseded_lanes_cannot_overwrite_newer_snapshots() {
    use codexbar::codex_accounts::{AccountUsageSnapshot, file_locations};
    let root = std::env::temp_dir().join(format!("codex-lane-generation-{}", Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    file_locations::with_app_support_directory(root.clone());
    let mut state = AppState::new();
    let old_generation = state.provider_refresh_generation;
    invalidate_account_usage(&mut state, ProviderId::Codex);
    let id = Uuid::new_v4();
    let mut account = sample_account();
    account.id = id;
    account.provider_account_id = Some("new".into());
    persist_codex_accounts(&[account.clone()]).unwrap();
    let snapshot = AccountUsageSnapshot {
        email: Some("new@example.com".into()),
        provider_account_id: Some("new".into()),
        plan: None,
        allowed: None,
        limit_reached: None,
        primary_window: None,
        secondary_window: None,
        credits: None,
        cost: None,
        subscription: None,
        updated_at: codexbar::codex_accounts::utc_now(),
    };
    assert!(
        save_codex_lane_results(
            &state,
            state.provider_refresh_generation,
            vec![(account.clone(), snapshot.clone())],
            &[account.clone()]
        )
        .unwrap()
    );
    let before = std::fs::read(file_locations::snapshots_file()).unwrap();
    let stale = AccountUsageSnapshot {
        email: Some("old@example.com".into()),
        ..snapshot
    };
    assert!(
        !save_codex_lane_results(
            &state,
            old_generation,
            vec![(account.clone(), stale)],
            &[account]
        )
        .unwrap()
    );
    assert_eq!(
        std::fs::read(file_locations::snapshots_file()).unwrap(),
        before
    );
    file_locations::clear_app_support_directory_override();
    assert!(root.starts_with(std::env::temp_dir()));
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn codex_switch_supersedes_inflight_usage_and_keeps_other_providers() {
    let mut state = AppState::new();
    let other = invalidate_account_usage(&mut state, ProviderId::Claude);
    let mut old = invalidate_account_usage(&mut state, ProviderId::Codex);
    old.account_email = Some("old@example.com".into());
    old.primary.used_percent = 80.0;
    old.error = None;
    state.provider_cache = vec![other, old];
    state.is_refreshing = true;
    let generation = state.provider_refresh_generation;
    let pending = invalidate_account_usage(&mut state, ProviderId::Codex);
    assert!(!is_current_provider_refresh_generation(&state, generation));
    assert!(!state.is_refreshing);
    assert_eq!(state.provider_cache.len(), 2);
    assert!(
        state
            .provider_cache
            .iter()
            .any(|s| s.provider_id == "claude")
    );
    assert!(pending.account_email.is_none() && pending.error.is_some());
    assert_eq!(pending.primary.used_percent, 0.0);
}

#[test]
fn reconciliation_replaces_changed_managed_identity_without_inheriting_metadata() {
    let mut stale = sample_account();
    stale.nickname = Some("Former account".into());
    let mut fresh = sample_account();
    fresh.provider_account_id = Some("replacement".into());
    fresh.email_hint = Some("replacement@example.com".into());
    let accounts = reconcile_codex_accounts(&[stale.clone()], &[fresh.clone()], None);
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].id, fresh.id);
    assert_ne!(accounts[0].id, stale.id);
    assert_eq!(accounts[0].nickname, None);
    assert_eq!(accounts[0].email_hint, fresh.email_hint);
}

#[test]
fn reconciliation_preserves_metadata_for_unchanged_managed_identity() {
    let mut stored = sample_account();
    stored.nickname = Some("Work".into());
    let fresh = sample_account();
    let accounts = reconcile_codex_accounts(&[stored.clone()], &[fresh], None);
    assert_eq!(accounts.len(), 1);
    assert_eq!(accounts[0].id, stored.id);
    assert_eq!(accounts[0].nickname, stored.nickname);
}

fn sample_account() -> CodexAccount {
    CodexAccount::new(
        Uuid::new_v4(),
        None,
        Some("user@example.com".to_string()),
        Some("auth0|acct".to_string()),
        Some("acct".to_string()),
        std::path::PathBuf::from("/tmp/fake-home"),
        codexbar::codex_accounts::CodexAccountSource::ManagedByApp,
        codexbar::codex_accounts::utc_now(),
        codexbar::codex_accounts::utc_now(),
        Some(codexbar::codex_accounts::utc_now()),
    )
}

#[test]
fn into_user_message_preserves_friendly_text() {
    assert_eq!(
        into_user_message(CodexAccountManagerError::Message(
            "The `codex` command could not be found.".to_string()
        )),
        "The `codex` command could not be found."
    );
}

#[test]
fn ambient_account_selects_only_the_ambient_identity() {
    let managed = sample_account();
    let mut ambient = managed.clone();
    ambient.source = codexbar::codex_accounts::CodexAccountSource::Ambient;

    let selected = ambient_account(&[managed, ambient.clone()]).unwrap();

    assert_eq!(selected.id, ambient.id);
    assert_eq!(
        selected.source,
        codexbar::codex_accounts::CodexAccountSource::Ambient
    );
}

#[test]
fn ambient_account_reports_when_no_ambient_identity_exists() {
    assert_eq!(
        ambient_account(&[sample_account()]).unwrap_err(),
        "No ambient Codex account found."
    );
}

#[test]
fn reconciled_ambient_identity_change_replaces_the_login_result() {
    let mut stored = sample_account();
    stored.source = codexbar::codex_accounts::CodexAccountSource::Ambient;
    stored.provider_account_id = Some("old-workspace".into());
    stored.email_hint = Some("old@example.com".into());

    // Logging in as a different identity at the same ambient home.
    let mut fresh = sample_account();
    fresh.source = codexbar::codex_accounts::CodexAccountSource::Ambient;
    fresh.provider_account_id = Some("new-workspace".into());
    fresh.email_hint = Some("new@example.com".into());

    let reconciled = reconcile_codex_accounts(&[stored.clone()], &[], Some(fresh));
    assert_eq!(reconciled.len(), 1);
    assert_ne!(reconciled[0].id, stored.id);

    // `reauthenticate` reuses the pre-login id; the command must report the
    // reconciled record so it agrees with the persisted store and events.
    let mut authenticated = stored.clone();
    authenticated.email_hint = Some("new@example.com".into());
    let account = canonical_reauthenticated_account(&reconciled, &authenticated).unwrap();
    assert_eq!(account.id, reconciled[0].id);
    assert_ne!(account.id, authenticated.id);
    assert_eq!(
        account.source,
        codexbar::codex_accounts::CodexAccountSource::Ambient
    );
    assert_eq!(
        account.provider_account_id.as_deref(),
        Some("new-workspace")
    );
}

#[test]
fn canonical_reauthenticated_account_returns_the_persisted_replacement() {
    let mut authenticated = sample_account();
    authenticated.source = codexbar::codex_accounts::CodexAccountSource::Ambient;
    authenticated.provider_account_id = Some("old-workspace".into());
    authenticated.email_hint = Some("old@example.com".into());

    let mut persisted = sample_account();
    persisted.source = codexbar::codex_accounts::CodexAccountSource::Ambient;
    persisted.provider_account_id = Some("new-workspace".into());
    persisted.email_hint = Some("new@example.com".into());

    let account = canonical_reauthenticated_account(&[persisted.clone()], &authenticated).unwrap();
    assert_eq!(account.id, persisted.id);
    assert_ne!(account.id, authenticated.id);
    assert_eq!(
        account.provider_account_id.as_deref(),
        Some("new-workspace")
    );
}

#[test]
fn canonical_reauthenticated_account_returns_the_unchanged_persisted_reauth() {
    let mut persisted = sample_account();
    persisted.source = codexbar::codex_accounts::CodexAccountSource::Ambient;
    persisted.nickname = Some("Work".into());
    persisted.provider_account_id = Some("workspace".into());

    // The login helper does not carry optional stored metadata.
    let mut authenticated = persisted.clone();
    authenticated.nickname = None;

    let account = canonical_reauthenticated_account(&[persisted.clone()], &authenticated).unwrap();
    assert_eq!(account.id, persisted.id);
    assert_eq!(account.nickname.as_deref(), Some("Work"));
}

#[test]
fn canonical_reauthenticated_account_prefers_ambient_over_matching_managed() {
    let mut managed = sample_account();
    managed.provider_account_id = Some("shared-workspace".into());

    let mut ambient = managed.clone();
    ambient.id = Uuid::new_v4();
    ambient.source = codexbar::codex_accounts::CodexAccountSource::Ambient;

    let account = canonical_reauthenticated_account(&[managed, ambient.clone()], &ambient)
        .expect("persisted ambient account should be canonical");
    assert_eq!(account.id, ambient.id);
    assert_eq!(
        account.source,
        codexbar::codex_accounts::CodexAccountSource::Ambient
    );
}

#[test]
fn canonical_reauthenticated_account_does_not_return_a_dropped_login() {
    let mut authenticated = sample_account();
    authenticated.source = codexbar::codex_accounts::CodexAccountSource::Ambient;
    authenticated.provider_account_id = Some("dropped-workspace".into());
    authenticated.email_hint = Some("dropped@example.com".into());

    // The reconciled set dropped the login identity and holds no ambient
    // record to replace it with.
    let error = canonical_reauthenticated_account(&[sample_account()], &authenticated).unwrap_err();
    assert_eq!(error, "No ambient Codex account found.");
}

#[test]
fn canonical_reauthenticated_account_rejects_an_uncommitted_persistence_failure() {
    let authenticated = sample_account();

    // A failed persistence leaves no committed reconciled set; the transient
    // login result must not be surfaced in its place.
    let error = canonical_reauthenticated_account(&[], &authenticated).unwrap_err();
    assert_eq!(error, "Codex account login was not persisted.");
}

#[test]
fn reauthentication_targets_saved_home_without_switching_to_ambient() {
    let managed = sample_account();
    let mut ambient = sample_account();
    ambient.source = codexbar::codex_accounts::CodexAccountSource::Ambient;
    let accounts = [managed.clone(), ambient.clone()];
    let selected = reauthentication_target(&accounts, Some(&managed.id.to_string())).unwrap();
    assert_eq!(selected.id, managed.id);
    assert_eq!(selected.codex_home_path, managed.codex_home_path);
    assert_eq!(
        reauthentication_target(&accounts, None).unwrap().id,
        ambient.id
    );
    assert!(reauthentication_target(&accounts, Some("missing")).is_err());
}

#[test]
fn managed_reauthentication_reports_its_persisted_home_not_ambient() {
    let authenticated = sample_account();
    let mut replacement = authenticated.clone();
    replacement.id = Uuid::new_v4();
    replacement.provider_account_id = Some("replacement-workspace".into());
    let mut ambient = sample_account();
    ambient.source = codexbar::codex_accounts::CodexAccountSource::Ambient;
    let selected =
        canonical_reauthenticated_account(&[ambient, replacement.clone()], &authenticated).unwrap();
    assert_eq!(selected.id, replacement.id);
    assert_eq!(selected.source, authenticated.source);
}

#[test]
fn sample_account_serializes_camel_case() {
    let json = serde_json::to_value(sample_account()).unwrap();
    assert!(json.get("codexHomePath").is_some());
    assert!(json.get("providerAccountId").is_some());
}
