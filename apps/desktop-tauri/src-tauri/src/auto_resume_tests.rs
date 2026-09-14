#[cfg(test)]
mod tests {
    use super::super::*;
    use chrono::Utc;

    fn target(provider: ProviderId) -> ResumeTarget {
        ResumeTarget {
            provider,
            quota_source: match provider {
                ProviderId::Codex => ResumeSource::CodexOAuth,
                ProviderId::Claude => ResumeSource::ClaudeCli,
                _ => ResumeSource::CodexOAuth,
            },
            session_id: "12345678-1234-1234-1234-123456789abc".to_string(),
            cwd: PathBuf::from(if cfg!(windows) {
                r"C:\work space"
            } else {
                "/tmp/work space"
            }),
            transcript_path: None,
            token_account_id: None,
        }
    }

    fn operation(generation: u64, token: u128) -> InFlightOp {
        InFlightOp {
            generation,
            token: uuid::Uuid::from_u128(token),
        }
    }

    fn window(used_percent: f64, exhausted: bool, informational: bool) -> RateWindowSnapshot {
        RateWindowSnapshot {
            used_percent,
            remaining_percent: 100.0 - used_percent,
            window_minutes: Some(300),
            resets_at: Some(Utc::now().to_rfc3339()),
            reset_description: None,
            is_exhausted: exhausted,
            is_informational: informational,
            reserve_percent: None,
            reserve_description: None,
            reserve_will_last_to_reset: false,
            reserve_eta_seconds: None,
        }
    }

    #[test]
    fn blocking_slots_ignore_informational_windows() {
        let primary = window(100.0, true, false);
        let secondary = window(100.0, true, true);
        assert_eq!(
            blocking_slots(&primary, Some(&secondary)),
            vec![QuotaSlot::Primary]
        );
    }

    #[test]
    fn an_arm_can_be_consumed_only_after_its_blocked_window_is_available() {
        let arm = ResumeArm {
            target: target(ProviderId::Codex),
            blocked_slots: vec![QuotaSlot::Primary],
            account_identity: None,
        };
        let exhausted = window(100.0, true, false);
        let available = window(20.0, false, false);
        assert!(!restored(&arm, &exhausted, None));
        assert!(restored(&arm, &available, None));
    }

    #[test]
    fn exact_cli_arguments_have_no_prompt_argument() {
        let codex =
            build_resume_command(&target(ProviderId::Codex), PathBuf::from("codex.exe")).unwrap();
        assert_eq!(
            codex.args,
            vec!["resume", "12345678-1234-1234-1234-123456789abc"]
        );
        assert_eq!(codex.cwd, target(ProviderId::Codex).cwd);

        let claude =
            build_resume_command(&target(ProviderId::Claude), PathBuf::from("claude.exe")).unwrap();
        assert_eq!(
            claude.args,
            vec!["--resume", "12345678-1234-1234-1234-123456789abc"]
        );
    }

    #[cfg(windows)]
    #[test]
    fn cmd_wrapper_preserves_exact_resume_arguments() {
        let command = build_resume_command(
            &target(ProviderId::Claude),
            PathBuf::from(r"C:\Program Files\Claude\claude.cmd"),
        )
        .unwrap();

        assert_eq!(command.program, PathBuf::from("cmd.exe"));
        assert_eq!(&command.args[..3], ["/d", "/s", "/c"]);
        assert_eq!(
            command.args[3],
            r#""C:\Program Files\Claude\claude.cmd" "--resume" "12345678-1234-1234-1234-123456789abc""#
        );
        assert!(!command.args[3].contains("prompt"));
    }

    #[test]
    fn unsafe_or_pid_session_ids_are_rejected() {
        let mut invalid = target(ProviderId::Codex);
        invalid.session_id = "pid:42".to_string();
        assert!(build_resume_command(&invalid, PathBuf::from("codex.exe")).is_err());
        invalid.session_id = "id with spaces".to_string();
        assert!(build_resume_command(&invalid, PathBuf::from("codex.exe")).is_err());
    }

    fn active_cli_session(id: &str, cwd: &Path) -> AgentSession {
        AgentSession {
            id: id.to_string(),
            provider: AgentSessionProvider::Codex,
            dialect: None,
            session_name: None,
            source: AgentSessionSource::Cli,
            state: AgentSessionState::Active,
            pid: Some(1234),
            transcript_path: None,
            host: "localhost".to_string(),
            workspace: codexbar::agent_sessions::AgentSessionWorkspace {
                cwd: Some(cwd.to_string_lossy().into_owned()),
                project_name: None,
            },
            activity: codexbar::agent_sessions::AgentSessionActivity {
                started_at: None,
                last_activity_at: None,
            },
            focus_target: codexbar::agent_sessions::AgentSessionFocusTarget::Process { pid: 1234 },
        }
    }

    fn test_workspace() -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("codexbar-auto-resume-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&path).expect("temporary workspace");
        path
    }

    #[test]
    fn capture_requires_one_active_local_cli_session() {
        let directory = test_workspace();
        let session = active_cli_session("one", &directory);

        assert!(
            capture_target_from_sessions(
                ProviderId::Codex,
                std::slice::from_ref(&session),
                None,
                ResumeSource::CodexOAuth,
            )
            .is_some()
        );
        assert!(
            capture_target_from_sessions(
                ProviderId::Codex,
                &[session.clone(), active_cli_session("two", &directory)],
                None,
                ResumeSource::CodexOAuth,
            )
            .is_none()
        );
        std::fs::remove_dir(directory).expect("remove temporary workspace");
    }

    #[test]
    fn managed_codex_without_account_correlation_stays_unarmed() {
        let directory = test_workspace();
        let session = active_cli_session("one", &directory);

        assert!(
            capture_target_from_sessions(
                ProviderId::Codex,
                &[session],
                Some(uuid::Uuid::new_v4()),
                ResumeSource::CodexOAuth,
            )
            .is_none()
        );
        std::fs::remove_dir(directory).expect("remove temporary workspace");
    }

    #[test]
    fn claude_requires_proven_cli_quota_for_auto_resume() {
        let cached: ProviderUsageSnapshot = serde_json::from_value(serde_json::json!({
            "providerId": "claude",
            "primary": {"usedPercent": 100.0, "remainingPercent": 0.0},
            "sourceLabel": "web",
            "errorState": "ready",
            "hasSuccessfulClaudeCliQuota": false
        }))
        .expect("snapshot fixture");
        assert!(resume_source(ProviderId::Claude, &cached).is_none());

        let cli: ProviderUsageSnapshot = serde_json::from_value(serde_json::json!({
            "providerId": "claude",
            "primary": {"usedPercent": 100.0, "remainingPercent": 0.0},
            "sourceLabel": "cli",
            "errorState": "ready",
            "hasSuccessfulClaudeCliQuota": true
        }))
        .expect("snapshot fixture");
        assert!(resume_source(ProviderId::Claude, &cli).is_some());
    }

    #[test]
    fn auto_resume_source_is_bound_to_the_provider_snapshot() {
        let oauth: ProviderUsageSnapshot = serde_json::from_value(serde_json::json!({
            "providerId": "codex",
            "primary": {"usedPercent": 100.0, "remainingPercent": 0.0},
            "sourceLabel": "oauth",
            "errorState": "ready"
        }))
        .expect("snapshot fixture");
        let pat = ProviderUsageSnapshot {
            source_label: "pat".to_string(),
            ..oauth.clone()
        };
        let web: ProviderUsageSnapshot = serde_json::from_value(serde_json::json!({
            "providerId": "codex",
            "primary": {"usedPercent": 100.0, "remainingPercent": 0.0},
            "sourceLabel": "web",
            "errorState": "ready"
        }))
        .expect("snapshot fixture");

        assert_eq!(
            resume_source(ProviderId::Codex, &oauth),
            Some(ResumeSource::CodexOAuth)
        );
        assert_eq!(
            resume_source(ProviderId::Codex, &pat),
            Some(ResumeSource::CodexPat)
        );
        assert!(resume_source(ProviderId::Codex, &web).is_none());
    }

    #[test]
    fn codex_binding_requires_a_stable_account_identity() {
        assert_eq!(normalize_account_identity(None), None);
        assert_eq!(normalize_account_identity(Some("  ")), None);
        assert_eq!(
            normalize_account_identity(Some(" acct-a ")).as_deref(),
            Some("acct-a")
        );

        let arm = ResumeArm {
            target: target(ProviderId::Codex),
            blocked_slots: vec![QuotaSlot::Primary],
            account_identity: Some("acct-a".to_string()),
        };
        assert!(resume_binding_matches(
            &arm,
            ResumeSource::CodexOAuth,
            Some("acct-a")
        ));
        assert!(!resume_binding_matches(
            &arm,
            ResumeSource::CodexPat,
            Some("acct-a")
        ));
        assert!(!resume_binding_matches(
            &arm,
            ResumeSource::CodexOAuth,
            Some("acct-b")
        ));
        assert!(!resume_binding_matches(
            &arm,
            ResumeSource::CodexOAuth,
            None
        ));
    }

    #[test]
    fn managed_token_account_lanes_are_not_auto_resume_eligible() {
        assert!(auto_resume_lane_is_available(ProviderId::Codex, None, true));
        assert!(!auto_resume_lane_is_available(
            ProviderId::Claude,
            Some(uuid::Uuid::new_v4()),
            true
        ));
        assert!(!auto_resume_lane_is_available(
            ProviderId::Codex,
            None,
            false
        ));
    }

    #[test]
    fn disabled_providers_clear_resume_state() {
        let mut state = AutoResumeState::default();
        state.arms.insert(
            ProviderId::Codex,
            ResumeArm {
                target: target(ProviderId::Codex),
                blocked_slots: vec![QuotaSlot::Primary],
                account_identity: None,
            },
        );
        state.arms.insert(
            ProviderId::Claude,
            ResumeArm {
                target: target(ProviderId::Claude),
                blocked_slots: vec![QuotaSlot::Primary],
                account_identity: None,
            },
        );

        state.clear_disabled(&[ProviderId::Claude]);

        assert!(!state.arms.contains_key(&ProviderId::Codex));
        assert!(state.arms.contains_key(&ProviderId::Claude));
    }

    #[test]
    fn failed_resume_attempt_keeps_the_arm_for_a_later_snapshot() {
        let target = target(ProviderId::Codex);
        let operation = operation(0, 1);
        let mut state = AutoResumeState::default();
        state.arms.insert(
            ProviderId::Codex,
            ResumeArm {
                target: target.clone(),
                blocked_slots: vec![QuotaSlot::Primary],
                account_identity: None,
            },
        );
        state
            .resumes_in_progress
            .insert(ProviderId::Codex, operation);

        finish_resume_attempt_state(&mut state, &target, operation, false);

        assert!(state.arms.contains_key(&ProviderId::Codex));
        assert!(!state.resumes_in_progress.contains_key(&ProviderId::Codex));
    }

    #[test]
    fn stale_capture_completion_cannot_clear_a_newer_capture_marker() {
        let first = operation(0, 1);
        let second = operation(1, 2);
        let mut state = AutoResumeState::default();
        state.captures_in_progress.insert(ProviderId::Codex, first);
        state.clear_provider(ProviderId::Codex);
        state.captures_in_progress.insert(ProviderId::Codex, second);

        finish_capture_state(
            &mut state,
            ProviderId::Codex,
            first,
            true,
            Some(target(ProviderId::Codex)),
            vec![QuotaSlot::Primary],
            None,
        );

        assert_eq!(
            state.captures_in_progress.get(&ProviderId::Codex),
            Some(&second)
        );
        assert!(state.arms.is_empty());
    }

    #[test]
    fn stale_resume_completion_cannot_clear_a_newer_resume_marker_or_arm() {
        let target = target(ProviderId::Codex);
        let first = operation(0, 1);
        let second = operation(1, 2);
        let mut state = AutoResumeState::default();
        state.arms.insert(
            ProviderId::Codex,
            ResumeArm {
                target: target.clone(),
                blocked_slots: vec![QuotaSlot::Primary],
                account_identity: None,
            },
        );
        state.resumes_in_progress.insert(ProviderId::Codex, first);

        state.clear_provider(ProviderId::Codex);
        state.arms.insert(
            ProviderId::Codex,
            ResumeArm {
                target: target.clone(),
                blocked_slots: vec![QuotaSlot::Primary],
                account_identity: None,
            },
        );
        state.resumes_in_progress.insert(ProviderId::Codex, second);

        finish_resume_attempt_state(&mut state, &target, first, true);

        assert_eq!(
            state.resumes_in_progress.get(&ProviderId::Codex),
            Some(&second)
        );
        assert!(state.arms.contains_key(&ProviderId::Codex));
    }

    #[test]
    fn stale_resume_invalidation_cannot_clear_a_newer_lease() {
        let target = target(ProviderId::Codex);
        let first = operation(0, 1);
        let second = operation(1, 2);
        let mut state = AutoResumeState::default();
        state.clear_provider(ProviderId::Codex);
        state.arms.insert(
            ProviderId::Codex,
            ResumeArm {
                target,
                blocked_slots: vec![QuotaSlot::Primary],
                account_identity: None,
            },
        );
        state.resumes_in_progress.insert(ProviderId::Codex, second);

        clear_provider_if_resume_owner_state(&mut state, ProviderId::Codex, first);

        assert!(state.arms.contains_key(&ProviderId::Codex));
        assert_eq!(
            state.resumes_in_progress.get(&ProviderId::Codex),
            Some(&second)
        );
        assert_eq!(state.capture_generations.get(&ProviderId::Codex), Some(&1));
    }

    #[test]
    fn disabling_resume_cancels_an_in_flight_attempt() {
        let operation = operation(0, 1);
        let mut state = AutoResumeState::default();
        state.arms.insert(
            ProviderId::Codex,
            ResumeArm {
                target: target(ProviderId::Codex),
                blocked_slots: vec![QuotaSlot::Primary],
                account_identity: None,
            },
        );
        state
            .resumes_in_progress
            .insert(ProviderId::Codex, operation);
        let generation = state.capture_generations.get(&ProviderId::Codex).copied();

        state.clear_provider(ProviderId::Codex);

        assert!(state.arms.is_empty());
        assert!(state.resumes_in_progress.is_empty());
        assert_ne!(
            state.capture_generations.get(&ProviderId::Codex).copied(),
            generation
        );
    }
}
