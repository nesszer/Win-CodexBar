#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;

    /// Write an auth.json carrying a JWT identity for the given account id.
    fn write_auth(home_path: &Path, email: &str, account_id: &str) {
        write_user_auth(home_path, email, account_id, &format!("auth0|{account_id}"));
    }

    fn write_user_auth(home_path: &Path, email: &str, account_id: &str, subject: &str) {
        let payload = serde_json::json!({
            "email": email,
            "sub": subject,
            "https://api.openai.com/auth": {
                "chatgpt_plan_type": "team",
                "chatgpt_account_id": account_id,
            },
        });
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&payload).unwrap());
        let auth_payload = serde_json::json!({
            "tokens": {
                "access_token": format!("access-{account_id}"),
                "refresh_token": format!("refresh-{account_id}"),
                "id_token": format!("header.{encoded}.signature"),
                "account_id": account_id,
            },
            "last_refresh": "2026-04-23T00:00:00Z",
        });
        std::fs::write(
            home_path.join("auth.json"),
            serde_json::to_vec_pretty(&auth_payload).unwrap(),
        )
        .unwrap();
    }

    /// Write an auth.json with no `last_refresh` field, so its freshness is
    /// unknown to `credentials_are_at_least_as_fresh`.
    fn write_auth_undated(home_path: &Path, email: &str, account_id: &str) {
        write_auth(home_path, email, account_id);
        let auth_path = home_path.join("auth.json");
        let mut auth: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&auth_path).unwrap()).unwrap();
        auth.as_object_mut()
            .unwrap()
            .remove("last_refresh")
            .unwrap();
        std::fs::write(&auth_path, serde_json::to_vec_pretty(&auth).unwrap()).unwrap();
    }

    fn make_account(home_path: PathBuf, email: &str, account_id: &str) -> CodexAccount {
        CodexAccount::new(
            Uuid::new_v4(),
            None,
            Some(email.to_string()),
            Some(format!("auth0|{account_id}")),
            Some(account_id.to_string()),
            home_path,
            CodexAccountSource::ManagedByApp,
            utc_now(),
            utc_now(),
            Some(utc_now()),
        )
    }

    #[test]
    fn remove_managed_account_removes_duplicate_homes_for_same_provider() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        super::super::file_locations::with_app_support_directory(root.to_path_buf());

        let account_id = "83c5ae92-f5ee-41f8-9528-199110d1d0f9";
        let first_home = root.join("managed-homes").join("first");
        let duplicate_home = root.join("managed-homes").join("duplicate");
        let other_home = root.join("managed-homes").join("other");
        for home in [&first_home, &duplicate_home, &other_home] {
            std::fs::create_dir_all(home).unwrap();
        }
        write_auth(&first_home, "user@example.com", account_id);
        write_auth(&duplicate_home, "user@example.com", account_id);
        write_auth(&other_home, "user@example.com", "different-provider");

        let account = make_account(first_home.clone(), "user@example.com", account_id);
        let manager = CodexAccountManager::new();
        manager.remove_managed_files_if_owned(&account).unwrap();

        assert!(!first_home.exists());
        assert!(!duplicate_home.exists());
        assert!(other_home.exists());

        super::super::file_locations::clear_app_support_directory_override();
    }

    #[test]
    fn shared_team_users_keep_separate_discovery_and_managed_homes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        super::super::file_locations::with_app_support_directory(root.to_path_buf());
        let first_home = root.join("managed-homes").join("first");
        let second_home = root.join("managed-homes").join("second");
        let ambient_home = root.join("ambient");
        for home in [&first_home, &second_home, &ambient_home] {
            std::fs::create_dir_all(home).unwrap();
        }
        write_user_auth(&first_home, "a@x.test", "shared-team", "user-a");
        write_user_auth(&second_home, "b@x.test", "shared-team", "user-b");
        write_user_auth(&ambient_home, "a@x.test", "shared-team", "user-a");
        let second_auth = std::fs::read(second_home.join("auth.json")).unwrap();
        let mut first = make_account(first_home.clone(), "a@x.test", "shared-team");
        first.auth_subject = Some("user-a".into());
        let manager = CodexAccountManager::new();
        let discovered = manager
            .discover_managed_accounts(std::slice::from_ref(&first))
            .unwrap();
        assert_eq!(discovered.len(), 2);
        let second = discovered
            .iter()
            .find(|a| a.codex_home_path == second_home)
            .unwrap();
        assert_ne!(second.id, first.id);
        assert!(!first.matches(second));

        let mut ambient = first.clone();
        ambient.codex_home_path = ambient_home;
        manager.remove_managed_files_if_owned(&first).unwrap();
        assert!(!first_home.exists());
        assert!(second_home.exists());
        let materialized = manager.materialize_as_managed(&ambient).unwrap();
        assert_ne!(materialized.codex_home_path, second_home);
        assert_eq!(
            std::fs::read(second_home.join("auth.json")).unwrap(),
            second_auth
        );
        assert_eq!(managed_home_count(root), 2);
        super::super::file_locations::clear_app_support_directory_override();
    }

    /// Write an auth.json whose credentials carry an explicit refresh time.
    fn write_auth_refreshed_at(
        home_path: &Path,
        email: &str,
        account_id: &str,
        last_refresh: &str,
    ) {
        write_auth(home_path, email, account_id);
        let auth_path = home_path.join("auth.json");
        let mut auth: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&auth_path).unwrap()).unwrap();
        auth["last_refresh"] = serde_json::Value::String(last_refresh.to_string());
        std::fs::write(&auth_path, serde_json::to_vec_pretty(&auth).unwrap()).unwrap();
    }

    fn managed_home_count(root: &Path) -> usize {
        std::fs::read_dir(root.join("managed-homes"))
            .map(|entries| entries.filter_map(Result::ok).count())
            .unwrap_or(0)
    }

    #[test]
    fn materialize_reuses_the_existing_home_for_the_same_account() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        super::super::file_locations::with_app_support_directory(root.to_path_buf());

        let account_id = "b3f1c0de-0000-4000-8000-00000000cafe";
        let ambient_home = root.join("ambient");
        let existing_home = root.join("managed-homes").join("existing");
        for home in [&ambient_home, &existing_home] {
            std::fs::create_dir_all(home).unwrap();
        }
        write_auth(&ambient_home, "user@example.com", account_id);
        write_auth(&existing_home, "user@example.com", account_id);

        let ambient = make_account(ambient_home, "user@example.com", account_id);
        let manager = CodexAccountManager::new();

        // Repeated switches must not leave a trail of duplicate homes.
        let first = manager.materialize_as_managed(&ambient).unwrap();
        let second = manager.materialize_as_managed(&ambient).unwrap();

        assert_eq!(first.codex_home_path, existing_home);
        assert_eq!(second.codex_home_path, existing_home);
        assert_eq!(managed_home_count(root), 1);

        super::super::file_locations::clear_app_support_directory_override();
    }

    #[test]
    fn materialize_creates_a_home_when_no_managed_copy_matches() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        super::super::file_locations::with_app_support_directory(root.to_path_buf());

        let ambient_home = root.join("ambient");
        let unrelated_home = root.join("managed-homes").join("unrelated");
        for home in [&ambient_home, &unrelated_home] {
            std::fs::create_dir_all(home).unwrap();
        }
        write_auth(&ambient_home, "user@example.com", "account-one");
        write_auth(&unrelated_home, "other@example.com", "account-two");

        let ambient = make_account(ambient_home, "user@example.com", "account-one");
        let materialized = CodexAccountManager::new()
            .materialize_as_managed(&ambient)
            .unwrap();

        assert_ne!(materialized.codex_home_path, unrelated_home);
        assert!(materialized.codex_home_path.join("auth.json").exists());
        assert_eq!(managed_home_count(root), 2);

        super::super::file_locations::clear_app_support_directory_override();
    }

    #[test]
    fn materialize_does_not_overwrite_newer_managed_credentials() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        super::super::file_locations::with_app_support_directory(root.to_path_buf());

        let account_id = "0ff1ce00-0000-4000-8000-0000000000aa";
        let ambient_home = root.join("ambient");
        let existing_home = root.join("managed-homes").join("existing");
        for home in [&ambient_home, &existing_home] {
            std::fs::create_dir_all(home).unwrap();
        }
        // The ambient file is the stale one here: expired, left behind by an
        // earlier session. Reuse must not copy it over live credentials.
        write_auth_refreshed_at(
            &ambient_home,
            "user@example.com",
            account_id,
            "2026-01-01T00:00:00Z",
        );
        write_auth_refreshed_at(
            &existing_home,
            "user@example.com",
            account_id,
            "2026-06-01T00:00:00Z",
        );
        let preserved = std::fs::read(existing_home.join("auth.json")).unwrap();

        let ambient = make_account(ambient_home, "user@example.com", account_id);
        let materialized = CodexAccountManager::new()
            .materialize_as_managed(&ambient)
            .unwrap();

        assert_eq!(materialized.codex_home_path, existing_home);
        assert_eq!(
            std::fs::read(existing_home.join("auth.json")).unwrap(),
            preserved
        );

        super::super::file_locations::clear_app_support_directory_override();
    }

    #[test]
    fn materialize_does_not_clobber_when_freshness_is_unknown() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        super::super::file_locations::with_app_support_directory(root.to_path_buf());

        let account_id = "0ff1ce00-0000-4000-8000-0000000000bb";
        let ambient_home = root.join("ambient");
        let existing_home = root.join("managed-homes").join("existing");
        for home in [&ambient_home, &existing_home] {
            std::fs::create_dir_all(home).unwrap();
        }
        // Neither file carries a `last_refresh`: freshness is unknown on both
        // sides. The incumbent managed credentials must survive untouched.
        write_auth_undated(&ambient_home, "user@example.com", account_id);
        write_auth_undated(&existing_home, "user@example.com", account_id);
        let preserved = std::fs::read(existing_home.join("auth.json")).unwrap();

        let ambient = make_account(ambient_home, "user@example.com", account_id);
        let materialized = CodexAccountManager::new()
            .materialize_as_managed(&ambient)
            .unwrap();

        assert_eq!(materialized.codex_home_path, existing_home);
        assert_eq!(
            std::fs::read(existing_home.join("auth.json")).unwrap(),
            preserved
        );

        super::super::file_locations::clear_app_support_directory_override();
    }

    #[test]
    fn freshness_unknown_when_both_files_are_unreadable() {
        let dir = tempfile::tempdir().unwrap();
        let candidate = dir.path().join("candidate").join("auth.json");
        let incumbent = dir.path().join("incumbent").join("auth.json");
        std::fs::create_dir_all(candidate.parent().unwrap()).unwrap();
        std::fs::create_dir_all(incumbent.parent().unwrap()).unwrap();
        std::fs::write(&candidate, b"{not json").unwrap();
        std::fs::write(&incumbent, b"{not json either").unwrap();

        // Unknown freshness must keep the incumbent, never clobber it.
        assert!(!super::credentials_are_at_least_as_fresh(
            &candidate, &incumbent
        ));
    }

    #[test]
    fn freshness_unknown_when_last_refresh_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let candidate = dir.path().join("candidate").join("auth.json");
        let incumbent = dir.path().join("incumbent").join("auth.json");
        std::fs::create_dir_all(candidate.parent().unwrap()).unwrap();
        std::fs::create_dir_all(incumbent.parent().unwrap()).unwrap();
        write_auth_undated(candidate.parent().unwrap(), "user@example.com", "cafe");
        write_auth_undated(incumbent.parent().unwrap(), "user@example.com", "cafe");

        assert!(!super::credentials_are_at_least_as_fresh(
            &candidate, &incumbent
        ));
    }

    #[test]
    fn materialize_reuses_the_lowest_keyed_matching_home() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        super::super::file_locations::with_app_support_directory(root.to_path_buf());

        let account_id = "0ff1ce00-0000-4000-8000-0000000000dd";
        let ambient_home = root.join("ambient");
        let newer_home = root.join("managed-homes").join("aaa-newer");
        let older_home = root.join("managed-homes").join("zzz-older");
        for home in [&ambient_home, &newer_home, &older_home] {
            std::fs::create_dir_all(home).unwrap();
        }
        // Two managed homes for one account: the lowest-keyed ("aaa-newer")
        // holds the fresher credentials. Reuse must pick it, and the ambient
        // copy (older) must not downgrade it.
        write_auth_refreshed_at(
            &ambient_home,
            "user@example.com",
            account_id,
            "2026-03-01T00:00:00Z",
        );
        write_auth_refreshed_at(
            &newer_home,
            "user@example.com",
            account_id,
            "2026-06-01T00:00:00Z",
        );
        write_auth_refreshed_at(
            &older_home,
            "user@example.com",
            account_id,
            "2026-01-01T00:00:00Z",
        );

        let ambient = make_account(ambient_home, "user@example.com", account_id);
        let materialized = CodexAccountManager::new()
            .materialize_as_managed(&ambient)
            .unwrap();

        assert_eq!(materialized.codex_home_path, newer_home);
        let reused: serde_json::Value =
            serde_json::from_slice(&std::fs::read(newer_home.join("auth.json")).unwrap())
                .unwrap();
        assert_eq!(reused["last_refresh"], "2026-06-01T00:00:00Z");

        super::super::file_locations::clear_app_support_directory_override();
    }

    #[test]
    fn removing_stale_account_preserves_replacement_credentials() {
        let dir = tempfile::tempdir().unwrap();
        super::super::file_locations::with_app_support_directory(dir.path().to_path_buf());
        let home = dir.path().join("managed-homes/replaced");
        std::fs::create_dir_all(&home).unwrap();
        let stale = make_account(home.clone(), "old@example.com", "old-id");
        write_auth(&home, "new@example.com", "new-id");
        let before = std::fs::read(home.join("auth.json")).unwrap();
        assert!(
            CodexAccountManager::new()
                .remove_managed_files_if_owned(&stale)
                .is_err()
        );
        assert_eq!(std::fs::read(home.join("auth.json")).unwrap(), before);
        super::super::file_locations::clear_app_support_directory_override();
    }

    #[test]
    fn same_identity_switch_preserves_auth_and_has_no_session_restore() {
        let dir = tempfile::tempdir().unwrap();
        let ambient = dir.path().join("ambient");
        let saved = dir.path().join("managed-homes/saved");
        let session = dir.path().join("session");
        for path in [&ambient, &saved, &session] {
            fs::create_dir_all(path).unwrap();
        }
        write_auth(&ambient, "same@example.com", "same-id");
        write_auth(&saved, "same@example.com", "same-id");
        fs::write(session.join("Preferences"), "current-session").unwrap();
        let auth_before = fs::read(ambient.join("auth.json")).unwrap();
        super::super::file_locations::with_app_support_directory(dir.path().to_path_buf());
        super::super::file_locations::with_ambient_codex_home(ambient.clone());
        super::super::file_locations::with_codex_desktop_session_root(session.clone());
        for home in [ambient.clone(), saved] {
            let target = make_account(home, "same@example.com", "same-id");
            let result = CodexAccountManager::new()
                .switch_active_account(&target, &[])
                .unwrap();
            assert!(result.backup_path.is_none());
            assert!(result.materialized_account.is_none());
            assert!(result.desktop_session_backup_path.is_none());
            assert!(result.desktop_session_restore_path.is_none());
            assert_eq!(fs::read(ambient.join("auth.json")).unwrap(), auth_before);
            assert_eq!(
                fs::read_to_string(session.join("Preferences")).unwrap(),
                "current-session"
            );
        }
        super::super::file_locations::clear_app_support_directory_override();
        super::super::file_locations::clear_ambient_codex_home_override();
        super::super::file_locations::clear_codex_desktop_session_root_override();
    }

    #[test]
    fn switch_waits_for_credential_refresh_before_materializing_outgoing_auth() {
        let dir = tempfile::tempdir().unwrap();
        let ambient = dir.path().join("ambient");
        let saved = dir.path().join("managed-homes/target");
        fs::create_dir_all(&ambient).unwrap();
        fs::create_dir_all(&saved).unwrap();
        write_auth(&ambient, "old@example.com", "old-id");
        write_auth(&saved, "new@example.com", "new-id");
        let refresh_guard = super::super::CREDENTIAL_OPERATIONS.blocking_read();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let root = dir.path().to_path_buf();
        let worker_ambient = ambient.clone();
        let worker = std::thread::spawn(move || {
            super::super::file_locations::with_app_support_directory(root.clone());
            super::super::file_locations::with_ambient_codex_home(worker_ambient);
            super::super::file_locations::with_codex_desktop_session_root(root.join("session"));
            let target = make_account(saved, "new@example.com", "new-id");
            ready_tx.send(()).unwrap();
            let result = CodexAccountManager::new().switch_active_account(&target, &[]);
            done_tx.send(result).unwrap();
        });
        ready_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(
            done_rx.recv_timeout(Duration::from_millis(100)),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout)
        ));
        let mut refreshed: serde_json::Value =
            serde_json::from_slice(&fs::read(ambient.join("auth.json")).unwrap()).unwrap();
        refreshed["tokens"]["access_token"] = serde_json::json!("rotated-old-token");
        fs::write(
            ambient.join("auth.json"),
            serde_json::to_vec(&refreshed).unwrap(),
        )
        .unwrap();
        drop(refresh_guard);
        let result = done_rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .unwrap();
        worker.join().unwrap();
        let materialized = result.materialized_account.unwrap();
        let outgoing: serde_json::Value = serde_json::from_slice(
            &fs::read(materialized.codex_home_path.join("auth.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(outgoing["tokens"]["access_token"], "rotated-old-token");
        assert_eq!(
            load_identity(&ambient)
                .unwrap()
                .provider_account_id
                .as_deref(),
            Some("new-id")
        );
    }

    #[test]
    fn switch_active_account_updates_global_state_creator_id() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        super::super::file_locations::with_app_support_directory(root.to_path_buf());

        let old_account_id = "1ea93d04-5c50-42e3-857b-3db850785967";
        let new_account_id = "83c5ae92-f5ee-41f8-9528-199110d1d0f9";

        let ambient_home = root.join(".codex");
        let target_home = root.join("managed-homes").join("target");
        let desktop_session_root = root.join("package-session");

        std::fs::create_dir_all(&ambient_home).unwrap();
        std::fs::create_dir_all(&target_home).unwrap();
        std::fs::create_dir_all(&desktop_session_root).unwrap();

        write_auth(&ambient_home, "old@example.com", old_account_id);
        write_auth(&target_home, "new@example.com", new_account_id);
        let target_session_dir = target_home.join("desktop-session").join("Network");
        std::fs::create_dir_all(&target_session_dir).unwrap();
        std::fs::write(target_session_dir.join("Cookies"), "cookie-data").unwrap();

        let global_state = serde_json::json!({
            "electron-persisted-atom-state": {
                "environment": {
                    "creator_id": format!("user-e9H3MsspGTF7UZJ8uaXuML55__{old_account_id}"),
                }
            }
        });
        for file_name in [".codex-global-state.json", ".codex-global-state.json.bak"] {
            std::fs::write(
                ambient_home.join(file_name),
                serde_json::to_vec_pretty(&global_state).unwrap(),
            )
            .unwrap();
        }

        super::super::file_locations::with_ambient_codex_home(ambient_home.clone());
        super::super::file_locations::with_codex_desktop_session_root(desktop_session_root.clone());

        let manager = CodexAccountManager::new();
        let target_account = make_account(target_home.clone(), "new@example.com", new_account_id);
        let result = manager
            .switch_active_account(&target_account, std::slice::from_ref(&target_account))
            .unwrap();

        let ambient_auth: serde_json::Value =
            serde_json::from_slice(&std::fs::read(ambient_home.join("auth.json")).unwrap())
                .unwrap();
        assert_eq!(ambient_auth["tokens"]["account_id"], new_account_id);
        assert_eq!(
            result
                .ambient_account
                .unwrap()
                .provider_account_id
                .as_deref(),
            Some(new_account_id)
        );
        let materialized = result.materialized_account.unwrap();
        assert_eq!(
            materialized.provider_account_id.as_deref(),
            Some(old_account_id)
        );
        assert_eq!(
            result.desktop_session_backup_path.unwrap(),
            materialized.codex_home_path.join("desktop-session")
        );
        assert_eq!(
            result.desktop_session_restore_path.unwrap(),
            target_home.join("desktop-session")
        );
        assert!(result.desktop_session_restore_exists);

        let backup_files: Vec<PathBuf> = std::fs::read_dir(root.join("auth-backups"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("ambient-auth-")
            })
            .collect();
        assert_eq!(backup_files.len(), 1);
        let backup: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&backup_files[0]).unwrap()).unwrap();
        assert_eq!(backup["tokens"]["account_id"], old_account_id);

        for file_name in [".codex-global-state.json", ".codex-global-state.json.bak"] {
            let payload: serde_json::Value =
                serde_json::from_slice(&std::fs::read(ambient_home.join(file_name)).unwrap())
                    .unwrap();
            let creator_id = payload["electron-persisted-atom-state"]["environment"]["creator_id"]
                .as_str()
                .unwrap();
            assert_eq!(
                creator_id,
                format!("user-e9H3MsspGTF7UZJ8uaXuML55__{new_account_id}")
            );
        }

        super::super::file_locations::clear_app_support_directory_override();
        super::super::file_locations::clear_ambient_codex_home_override();
        super::super::file_locations::clear_codex_desktop_session_root_override();
    }
}
