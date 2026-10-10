use super::legacy_status::{
    ModelFamily, UserStatus, canonical_model_id, classify_model, parse_user_status,
    resolve_plan_name,
};
use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[test]
fn cadence_labels_are_owned_by_antigravity_snapshot() {
    let mut secondary = RateWindow::new(20.0);
    secondary.window_minutes = Some(7 * 24 * 60);
    let usage = UsageSnapshot::new(RateWindow::new(10.0)).with_secondary(secondary);
    let usage = AntigravityProvider::with_cadence_labels(usage);
    assert_eq!(usage.secondary_label.as_deref(), Some("Weekly"));
}

#[test]
fn explicit_quota_summary_cadence_label_survives_normalization() {
    let mut secondary = RateWindow::new(20.0);
    secondary.window_minutes = Some(7 * 24 * 60);
    let usage = UsageSnapshot::new(RateWindow::new(10.0))
        .with_secondary(secondary)
        .with_secondary_label("Gemini Weekly");
    let usage = AntigravityProvider::with_cadence_labels(usage);
    assert_eq!(usage.secondary_label.as_deref(), Some("Gemini Weekly"));
}

#[test]
fn test_classify_model_families() {
    for (label, family) in [
        ("Claude 3.5 Sonnet", ModelFamily::Claude),
        ("claude-4-opus", ModelFamily::Claude),
        ("Claude Thinking", ModelFamily::ClaudeThinking),
        ("claude-3.5-sonnet-thinking", ModelFamily::ClaudeThinking),
        ("Gemini 2.5 Pro Low", ModelFamily::GeminiPro),
        ("gemini-pro-low", ModelFamily::GeminiPro),
        ("Pro Low Latency", ModelFamily::GeminiPro),
        ("Gemini 2.5 Flash", ModelFamily::GeminiFlash),
        ("gemini-flash", ModelFamily::GeminiFlash),
        ("Flash Model", ModelFamily::GeminiFlash),
        ("GPT-4o", ModelFamily::Other),
        ("unknown-model", ModelFamily::Other),
    ] {
        assert_eq!(classify_model(label), family, "{label}");
    }
}

#[test]
fn retired_flash_ids_collapse_to_current_wire_id() {
    for id in [
        "gemini-3.6-flash",
        "gemini-3.6-flash-high",
        "gemini-3.5-flash-extra-low",
        "gemini-3-flash-agent",
    ] {
        assert_eq!(canonical_model_id(id), "gemini-3.7-flash");
    }
    assert_eq!(canonical_model_id("gemini-3.7-flash"), "gemini-3.7-flash");
}

#[test]
fn unselected_model_window_ids_are_lowercase_ascii_slugs() {
    let response: UserStatusResponse = serde_json::from_value(serde_json::json!({
        "userStatus": {
            "cascadeModelConfigData": {
                "clientModelConfigs": [
                    {"label": "Claude Sonnet", "modelId": "claude-sonnet", "quotaInfo": {"remainingFraction": 0.5}},
                    {"label": "Other One", "modelId": "  GPT-OSS 120B (Medium)  ", "quotaInfo": {"remainingFraction": 0.5}},
                    {"label": "Other Two", "modelId": "!!!", "quotaInfo": {"remainingFraction": 0.5}},
                    {"label": "Other Three", "id": "Mod\u{e8}le_A", "quotaInfo": {"remainingFraction": 0.5}},
                    {"label": "Plain Label", "quotaInfo": {"remainingFraction": 0.5}}
                ]
            }
        }
    }))
    .unwrap();
    let snap = parse_user_status(response).unwrap();
    let mut ids: Vec<_> = snap
        .extra_rate_windows
        .iter()
        .map(|window| window.id.as_str())
        .collect();
    ids.sort_unstable();
    assert_eq!(
        ids,
        [
            "model-gpt-oss-120b--medium",
            "model-mod-le-a",
            "model-plain-label",
            "model-unknown",
        ]
    );
}

#[test]
fn local_post_sends_connect_json_with_the_request_timeout() {
    let client = reqwest::Client::new();
    let request = local_post(
        &client,
        "https://127.0.0.1:1/exa.language_server_pb.LanguageServerService/GetUserStatus",
        &user_status_body(),
        std::time::Duration::from_secs(4),
    )
    .header("X-Codeium-Csrf-Token", "tok")
    .build()
    .unwrap();

    assert_eq!(request.method(), reqwest::Method::POST);
    let headers: Vec<_> = request
        .headers()
        .iter()
        .map(|(name, value)| (name.as_str(), value.to_str().unwrap()))
        .collect();
    assert_eq!(
        headers,
        [
            ("content-type", "application/json"),
            ("connect-protocol-version", "1"),
            ("x-codeium-csrf-token", "tok"),
        ]
    );
    assert_eq!(request.timeout(), Some(&std::time::Duration::from_secs(4)));
    let body: serde_json::Value =
        serde_json::from_slice(request.body().unwrap().as_bytes().unwrap()).unwrap();
    assert_eq!(
        body,
        serde_json::json!({
            "metadata": {
                "ideName": "antigravity",
                "extensionName": "antigravity",
                "ideVersion": "unknown",
                "locale": "en"
            }
        })
    );
}

#[test]
fn ide_flags_capture_extension_token_and_prefer_the_extension_port() {
    // (command line, extension token, port)
    let cases = [
        (
            "1	ls.exe --extension_server_csrf_token ext-tok --csrf_token main --extension_server_port 54123 --https_server_port 61999",
            Some("ext-tok"),
            Some(54123),
        ),
        (
            "1	ls.exe --csrf_token=main --extension_server_port=abc --https_server_port=61999",
            None,
            Some(61999),
        ),
        (
            "1	ls.exe --csrf_token main --extension_server_port 70000",
            None,
            None,
        ),
    ];
    for (line, ext_token, port) in cases {
        let process = AntigravityProvider::parse_process_info(line).expect(line);
        assert_eq!(process.csrf_token, "main", "{line}");
        assert_eq!(
            process.extension_server_csrf_token.as_deref(),
            ext_token,
            "{line}"
        );
        assert_eq!(process.extension_port, port, "{line}");
    }
}

fn make_response(models: Vec<(&str, f64)>) -> UserStatusResponse {
    let json = serde_json::json!({
        "userStatus": {
            "cascadeModelConfigData": {
                "clientModelConfigs": models.iter().map(|(label, remaining)| {
                    serde_json::json!({
                        "label": label,
                        "quotaInfo": {
                            "remainingFraction": remaining
                        }
                    })
                }).collect::<Vec<_>>()
            }
        }
    });
    serde_json::from_value(json).unwrap()
}

#[test]
fn antigravity_extra_windows_preserve_usage_known() {
    let json = serde_json::json!({
        "userStatus": {
            "cascadeModelConfigData": {
                "clientModelConfigs": [
                    {
                        "label": "Claude 4 Sonnet",
                        "quotaInfo": {"remainingFraction": 0.8}
                    },
                    {
                        "label": "Gemini 2.5 Pro",
                        "quotaInfo": {"remainingFraction": 0.6}
                    },
                    {
                        "label": "Gemini 2.5 Flash",
                        "quotaInfo": {"remainingFraction": 0.4}
                    },
                    {
                        "label": "Claude 3.5 Sonnet",
                        "quotaInfo": {"remainingFraction": null}
                    }
                ]
            }
        }
    });
    let resp: UserStatusResponse = serde_json::from_value(json).unwrap();
    let snap = parse_user_status(resp).unwrap();
    let claude = snap
        .extra_rate_windows
        .iter()
        .find(|window| window.title.contains("Claude"))
        .unwrap();
    assert!(!claude.usage_known);
    assert_eq!(claude.window.used_percent, 0.0);
}

/// (models, primary, secondary, model-specific used %, sorted extra titles)
type WindowCase<'a> = (
    &'a [(&'a str, f64)],
    f64,
    Option<f64>,
    Option<f64>,
    &'a [&'a str],
);

#[test]
fn parse_user_status_selects_summary_windows_and_keeps_the_rest() {
    let cases: [WindowCase<'_>; 7] = [
        (
            &[
                ("Claude 3.5 Sonnet", 0.8),
                ("Gemini 2.5 Pro Low", 0.5),
                ("Gemini 2.5 Flash", 0.9),
            ],
            20.0,
            Some(50.0),
            Some(10.0),
            &[],
        ),
        // Upstream 0.50.1 #2963: every unselected config stays visible.
        (
            &[
                ("Claude 4 Sonnet", 0.8),
                ("GPT-4o", 0.8),
                ("Mistral Large", 0.6),
                ("Qwen Max", 0.6),
            ],
            20.0,
            None,
            None,
            &["GPT-4o", "Mistral Large", "Qwen Max"],
        ),
        // Thinking variants never drive the Claude window.
        (
            &[
                ("Claude Thinking", 0.6),
                ("Claude 3.5 Sonnet", 0.7),
                ("Gemini 2.5 Flash", 0.5),
            ],
            30.0,
            None,
            Some(50.0),
            &["Claude Thinking"],
        ),
        // Without a known family, the first model is primary.
        (
            &[("GPT-4o", 0.4), ("Mistral Large", 0.6)],
            60.0,
            None,
            None,
            &["Mistral Large"],
        ),
        // Noisy Gemini variants stay visible but do not drive summary windows.
        (
            &[
                ("Gemini 2.5 Flash Image", 0.01),
                ("Gemini 2.5 Pro Lite", 0.02),
                ("Gemini autocomplete internal", 0.03),
                ("Claude 4 Sonnet", 0.8),
                ("Gemini 2.5 Pro Low", 0.6),
                ("Gemini 2.5 Flash", 0.7),
            ],
            20.0,
            Some(40.0),
            Some(30.0),
            &[
                "Gemini 2.5 Flash Image",
                "Gemini 2.5 Pro Lite",
                "Gemini autocomplete internal",
            ],
        ),
        // Equal readings are not a pool identity: both unselected configs stay
        // visible even though the canonical Claude and Gemini configs are selected.
        (
            &[
                ("Claude 4 Sonnet", 0.8),
                ("Gemini 2.5 Pro Low", 0.5),
                ("Mistral Large", 0.8),
                ("Qwen Max", 0.8),
            ],
            20.0,
            Some(50.0),
            None,
            &["Mistral Large", "Qwen Max"],
        ),
        // Models in distinct quota buckets keep separate lanes.
        (
            &[
                ("Claude 3.5 Sonnet", 0.8),
                ("Claude 4 Sonnet", 0.7),
                ("Gemini 2.5 Pro Low", 0.5),
            ],
            30.0,
            Some(50.0),
            None,
            &["Claude 3.5 Sonnet"],
        ),
    ];
    let near = |actual: Option<f64>, expected: Option<f64>| match (actual, expected) {
        (Some(actual), Some(expected)) => (actual - expected).abs() < 0.1,
        (actual, expected) => actual.is_none() && expected.is_none(),
    };
    for (models, primary, secondary, model_specific, extras) in cases {
        let snap = parse_user_status(make_response(models.to_vec())).unwrap();
        let mut titles: Vec<_> = snap
            .extra_rate_windows
            .iter()
            .map(|window| window.title.as_str())
            .collect();
        titles.sort_unstable();
        let actual = (
            snap.primary.used_percent,
            snap.secondary.as_ref().map(|window| window.used_percent),
            snap.model_specific
                .as_ref()
                .map(|window| window.used_percent),
        );
        assert!(
            near(Some(actual.0), Some(primary))
                && near(actual.1, secondary)
                && near(actual.2, model_specific),
            "{models:?}: {actual:?}"
        );
        assert_eq!(titles, extras, "{models:?}");
    }
}

#[test]
fn missing_cli_error_explains_runtime_state() {
    let error = ProviderError::NotInstalled(AGY_NOT_FOUND_MESSAGE.to_string()).to_string();

    assert!(error.contains("not running"));
    assert!(error.contains("agy CLI was not found"));
}

// ── Managed lifecycle policy (fake outcomes) ───────────────────────
//
// The process lifecycle itself is covered by `crate::managed_process`; these
// exercise the provider-side policy that maps a lifecycle outcome onto a fetch
// result without spawning a real `agy`.

#[cfg(windows)]
#[test]
fn reused_user_runtime_stays_local() {
    let usage = UsageSnapshot::new(RateWindow::new(10.0));
    let result = AntigravityProvider::resolve_managed_outcome(Ok(ManagedAgyOutcome::Reused(
        ProviderFetchResult::new(usage, "local"),
    )))
    .expect("reused outcome resolves")
    .expect("reused outcome yields usage");
    assert_eq!(result.source_label, "local");
}

#[cfg(windows)]
#[test]
fn owned_cli_fetch_reports_cli_source() {
    let usage = UsageSnapshot::new(RateWindow::new(10.0));
    let result = AntigravityProvider::resolve_managed_outcome(Ok(ManagedAgyOutcome::Fetched(
        ProviderFetchResult::new(usage, "local"),
    )))
    .expect("owned outcome resolves")
    .expect("owned outcome yields usage");
    assert_eq!(result.source_label, "cli");
    assert_eq!(result.usage.primary.used_percent, 10.0);
    assert!(!result.usage.primary.is_informational);
}

#[cfg(windows)]
#[test]
fn missing_runtime_is_a_policy_no_op() {
    let result = AntigravityProvider::resolve_managed_outcome(Ok(ManagedAgyOutcome::Missing))
        .expect("a missing runtime is not an error");
    assert!(result.is_none(), "missing runtime falls through to offline");
}

#[cfg(windows)]
#[test]
fn managed_auth_required_surfaces_instead_of_offline() {
    let result = AntigravityProvider::resolve_managed_outcome(Err(ProviderError::AuthRequired));
    assert!(matches!(result, Err(ProviderError::AuthRequired)));
}

// ── agy CLI process matching ───────────────────────────────────────

#[test]
fn parse_process_info_matches_ide_servers_and_the_agy_cli() {
    // (output, expected (pid, source, csrf token, extension-server csrf token, port))
    let cases = [
        (
            r"4242	C:\Users\test\AppData\Local\Programs\Antigravity\resources\bin\language_server.exe --csrf_token 11111111-2222-3333-4444-555555555555 --extension_server_port 54123",
            Some((
                Some(4242),
                ProcessSource::Ide,
                "11111111-2222-3333-4444-555555555555",
                None,
                Some(54123),
            )),
        ),
        // No --extension_server_port: the https port is used.
        (
            "34564\tC:\\Users\\test\\AppData\\Local\\Programs\\Antigravity\\resources\\bin\\language_server.exe --standalone --override_ide_name antigravity --subclient_type hub --override_ide_version 2.0.11 --https_server_port 0 --csrf_token 68dda2fb-6b26-40c0-aeef-b9a628615714 --app_data_dir antigravity",
            Some((
                Some(34564),
                ProcessSource::Ide,
                "68dda2fb-6b26-40c0-aeef-b9a628615714",
                None,
                Some(0),
            )),
        ),
        (
            "34564\tC:\\Users\\test\\AppData\\Local\\Programs\\Antigravity\\resources\\bin\\language_server.exe --standalone --csrf_token aabbccdd-1122-3344-5566-778899001122 --app_data_dir antigravity",
            Some((
                Some(34564),
                ProcessSource::Ide,
                "aabbccdd-1122-3344-5566-778899001122",
                None,
                None,
            )),
        ),
        // `--flag=value` form.
        (
            "34564\tC:\\Users\\test\\AppData\\Local\\Programs\\Antigravity\\resources\\bin\\language_server.exe --csrf_token=68dda2fb-6b26-40c0-aeef-b9a628615714 --https_server_port=61999",
            Some((
                Some(34564),
                ProcessSource::Ide,
                "68dda2fb-6b26-40c0-aeef-b9a628615714",
                None,
                Some(61999),
            )),
        ),
        // agy.exe hosts the language server in-process with no --csrf_token.
        (
            "7777\tC:\\Users\\test\\AppData\\Local\\agy\\bin\\agy.exe session --model gemini-2.5-pro",
            Some((Some(7777), ProcessSource::Cli, "", None, None)),
        ),
        // Windows CIM quotes an executable path that contains path separators.
        (
            "7777\t\"C:\\Users\\user\\AppData\\Local\\agy\\bin\\agy.exe\" --model gemini-3.7-flash-high",
            Some((Some(7777), ProcessSource::Cli, "", None, None)),
        ),
        // The CLI may appear under the bare `agy` name (no .exe suffix).
        (
            "8888\tagy serve",
            Some((Some(8888), ProcessSource::Cli, "", None, None)),
        ),
        // Upstream also matches antigravity-cli / antigravity_cli.
        (
            "9999\t/opt/homebrew/bin/antigravity-cli status",
            Some((Some(9999), ProcessSource::Cli, "", None, None)),
        ),
        // When the desktop IDE server and the agy CLI are both running, the
        // CSRF-protected IDE match wins (mirrors upstream process-kind precedence).
        (
            "4242\tC:\\Antigravity\\language_server.exe --csrf_token deadbeef-aaaa-bbbb-cccc-dddddddddddd --extension_server_port 54123\n\
                  7777\tC:\\Users\\test\\AppData\\Local\\agy\\bin\\agy.exe session",
            Some((
                Some(4242),
                ProcessSource::Ide,
                "deadbeef-aaaa-bbbb-cccc-dddddddddddd",
                None,
                Some(54123),
            )),
        ),
        // No --csrf_token anywhere: only the agy CLI line matches.
        (
            "7777\tC:\\Users\\test\\AppData\\Local\\agy\\bin\\agy.exe",
            Some((Some(7777), ProcessSource::Cli, "", None, None)),
        ),
        // An unrelated tokenless process must not be mistaken for the agy CLI.
        ("1234\tC:\\Windows\\System32\\notepad.exe", None),
    ];
    for (output, expected) in cases {
        let actual = AntigravityProvider::parse_process_info(output).map(|process| {
            (
                process.pid,
                process.source,
                process.csrf_token,
                process.extension_server_csrf_token,
                process.extension_port,
            )
        });
        let expected = expected.map(|(pid, source, csrf, extension_csrf, port)| {
            (
                pid,
                source,
                csrf.to_string(),
                extension_csrf.map(str::to_string),
                port,
            )
        });
        assert_eq!(actual, expected, "{output}");
    }
}

#[test]
fn is_agy_cli_command_matches_only_cli_names() {
    for (command, expected) in [
        ("agy serve", true),
        (
            "C:\\Users\\test\\AppData\\Local\\agy\\bin\\agy.exe session",
            true,
        ),
        (
            "C:\\Users\\user\\AppData\\Local\\agy\\bin\\AGY.EXE --model gemini-3.7-flash-high",
            true,
        ),
        (
            "\"C:\\Users\\user\\AppData\\Local\\agy\\bin\\agy.exe\" --model gemini-3.7-flash-high",
            true,
        ),
        ("/usr/local/bin/antigravity-cli status", true),
        ("/opt/antigravity_cli run", true),
        ("\"C:\\Tools\\antigravity-cli\" status", true),
        ("\"C:\\Tools\\antigravity_cli\" run", true),
        // A leading path separator prevents `notantigravity-cli` from matching.
        ("notagy.exe --model gemini-3.7-flash-high", false),
        (
            "C:\\Tools\\someagy.exe --model gemini-3.7-flash-high",
            false,
        ),
        ("notantigravity-cli status", false),
        ("C:\\Tools\\notantigravity-cli status", false),
        ("C:\\Windows\\System32\\notepad.exe", false),
        ("language_server.exe --csrf_token abc", false),
        ("", false),
    ] {
        assert_eq!(is_agy_cli_command(command), expected, "{command}");
    }
}

#[test]
fn not_installed_maps_to_local_runtime_offline() {
    // Antigravity's `NotInstalled` reports the local language-server probe
    // finding nothing to talk to: a runtime that is not running, not a
    // credential problem.
    assert_eq!(
        AntigravityProvider::new()
            .error_state_kind(&ProviderError::NotInstalled(AGY_NOT_FOUND_MESSAGE.into())),
        crate::core::ProviderStateKind::LocalRuntimeOffline
    );
}

#[test]
fn probe_failure_maps_to_unknown() {
    // A failed probe (PowerShell unavailable etc.) says nothing about the
    // runtime itself - inconclusive, not offline.
    assert_eq!(
        AntigravityProvider::new().error_state_kind(&ProviderError::NotInstalled(
            "Failed to detect Antigravity process".into()
        )),
        crate::core::ProviderStateKind::Unknown
    );
}

// ── Offline-history fallback on probe failure ──────────────────────

const STRUCTURED_CLI_USAGE_REPORT: &[u8] = br#"{
  "status": "SUCCESS",
  "command": {
    "name": "usage",
    "data": {
      "groups": [{
        "name": "Gemini Models",
        "buckets": [
          {"id":"gemini-5h","name":"Five Hour Limit Remaining","remaining_fraction":0.6},
          {"id":"gemini-weekly","name":"Weekly Limit Remaining","remaining_fraction":0.8}
        ]
      }]
    }
  }
}"#;

fn structured_cli_result() -> ProviderFetchResult {
    let usage = quota_summary::parse_cli_usage_report(STRUCTURED_CLI_USAGE_REPORT)
        .expect("structured CLI fixture should parse");
    AntigravityProvider::fetch_result(usage, AntigravityStrategyId::Cli)
}

#[test]
fn strategy_ids_are_stable_and_reject_unknown_sources() {
    assert_eq!(
        strategy_from_source_label("local"),
        Some(AntigravityStrategyId::Local)
    );
    assert_eq!(
        strategy_from_source_label("cli"),
        Some(AntigravityStrategyId::Cli)
    );
    assert_eq!(
        strategy_from_source_label("offline"),
        Some(AntigravityStrategyId::Offline)
    );
    assert_eq!(strategy_from_source_label("managed"), None);
    assert_eq!(AntigravityStrategyId::Cli.as_str(), "cli");
}

pub(super) fn offline_result() -> ProviderFetchResult {
    ProviderFetchResult::new(
        UsageSnapshot::new(RateWindow::informational("Offline · 2 conversations"))
            .with_login_method("offline"),
        "offline",
    )
}

#[test]
fn auth_required_surfaces_instead_of_offline_history() {
    let resolved = AntigravityProvider::resolve_probe_failure(
        ProviderError::AuthRequired,
        Some(offline_result()),
    );
    assert!(matches!(resolved, Err(ProviderError::AuthRequired)));
}

#[test]
fn non_auth_failure_prefers_offline_history() {
    // A transient managed-start or local-probe failure must not discard the
    // existing offline conversation-history snapshot.
    let resolved = AntigravityProvider::resolve_probe_failure(
        ProviderError::Other("agy readiness timeout".to_string()),
        Some(offline_result()),
    );
    let resolved = resolved.expect("offline history is preserved");
    assert_eq!(resolved.source_label, "offline");
    assert_eq!(resolved.usage.login_method.as_deref(), Some("offline"));
    assert!(resolved.usage.primary.is_informational);
    assert_eq!(
        resolved.usage.primary.reset_description.as_deref(),
        Some("Offline · 2 conversations")
    );
}

#[test]
fn non_auth_failure_without_history_surfaces_error() {
    let resolved = AntigravityProvider::resolve_probe_failure(
        ProviderError::Other("agy readiness timeout".to_string()),
        None,
    );
    assert!(matches!(resolved, Err(ProviderError::Other(_))));
}

fn local_result() -> ProviderFetchResult {
    ProviderFetchResult::new(UsageSnapshot::new(RateWindow::new(10.0)), "local")
}

/// Resolve `local` with the given offline history and a CLI fallback that
/// yields `cli`. Also reports whether the CLI fallback ran.
pub(super) async fn run_fallback(
    local: Result<Option<ProviderFetchResult>, LiveFailure>,
    cli: Result<Option<ProviderFetchResult>, LiveFailure>,
    offline: Option<ProviderFetchResult>,
) -> (Result<ProviderFetchResult, ProviderError>, bool) {
    let ran = Arc::new(AtomicBool::new(false));
    let marker = Arc::clone(&ran);
    let result = AntigravityProvider::new()
        .resolve_runtime_fallback_with_offline(
            local,
            move || async move {
                marker.store(true, Ordering::SeqCst);
                cli
            },
            offline,
        )
        .await;
    (result, ran.load(Ordering::SeqCst))
}

#[tokio::test]
async fn local_probe_success_does_not_run_structured_cli_fallback() {
    let provider = AntigravityProvider::new();
    let fallback_called = Arc::new(AtomicBool::new(false));
    let marker = Arc::clone(&fallback_called);
    let result = provider
        .resolve_runtime_fallback(Ok(Some(local_result())), move || async move {
            marker.store(true, Ordering::SeqCst);
            Ok(Some(structured_cli_result()))
        })
        .await
        .expect("successful local probe should resolve");

    assert_eq!(result.source_label, "local");
    assert_eq!(result.usage.primary.used_percent, 10.0);
    assert!(!fallback_called.load(Ordering::SeqCst));
}

#[tokio::test]
async fn local_probe_success_wins_over_an_invalid_cli_override() {
    let (result, fallback_ran) = run_fallback(
        Ok(Some(local_result())),
        Err(LiveFailure::from(ProviderError::NotInstalled(
            "ANTIGRAVITY_CLI_PATH is set but unusable".to_string(),
        ))),
        Some(offline_result()),
    )
    .await;
    let result = result.expect("successful local desktop probe must remain authoritative");

    assert_eq!(result.source_label, "local");
    assert_eq!(result.usage.primary.used_percent, 10.0);
    assert!(!fallback_ran);
}

#[tokio::test]
async fn local_auth_probe_failure_uses_structured_cli_fallback() {
    let result = AntigravityProvider::new()
        .resolve_runtime_fallback(Err(ProviderError::AuthRequired.into()), || async {
            Ok(Some(structured_cli_result()))
        })
        .await
        .expect("the structured report should recover the local auth-shaped failure");

    assert_eq!(result.source_label, "cli");
    assert_eq!(result.usage.primary.used_percent, 40.0);
}

#[tokio::test]
async fn generic_local_probe_failure_uses_valid_structured_cli_json() {
    let result = AntigravityProvider::new()
        .resolve_runtime_fallback(
            Err(ProviderError::Other("local API unavailable".to_string()).into()),
            || async { Ok(Some(structured_cli_result())) },
        )
        .await
        .expect("the structured report should recover a generic local failure");

    assert_eq!(result.source_label, "cli");
    assert_eq!(result.usage.primary.used_percent, 40.0);
}

#[tokio::test]
async fn unauthenticated_local_and_unavailable_cli_paths_remain_auth_required() {
    let result = AntigravityProvider::new()
        .resolve_runtime_fallback(Err(ProviderError::AuthRequired.into()), || async {
            Ok(None)
        })
        .await;

    assert!(matches!(result, Err(ProviderError::AuthRequired)));
}

#[tokio::test]
async fn malformed_structured_cli_json_from_fallback_is_a_parse_error() {
    let error = quota_summary::parse_cli_usage_report(br#"{"status":"SUCCESS""#)
        .expect_err("malformed JSON must fail parsing");
    let (result, _) = run_fallback(
        Err(ProviderError::AuthRequired.into()),
        Err(LiveFailure::from(error)),
        None,
    )
    .await;

    assert!(matches!(result, Err(ProviderError::Parse(_))));
}

#[tokio::test]
async fn cli_fallback_error_prefers_offline_history() {
    let (result, _) = run_fallback(
        Err(ProviderError::AuthRequired.into()),
        Err(LiveFailure::from(ProviderError::Parse(
            "Antigravity CLI usage report: malformed JSON".to_string(),
        ))),
        Some(offline_result()),
    )
    .await;
    let result = result.expect("offline history should survive a failed CLI probe");

    assert_eq!(result.source_label, "offline");
    assert_eq!(result.usage.login_method.as_deref(), Some("offline"));
}

#[tokio::test]
async fn unusable_cli_override_prefers_offline_history() {
    let (result, _) = run_fallback(
        Err(ProviderError::AuthRequired.into()),
        Err(LiveFailure::from(ProviderError::NotInstalled(
            "ANTIGRAVITY_CLI_PATH is set but does not point to a usable agy file".into(),
        ))),
        Some(offline_result()),
    )
    .await;
    let result = result.expect("offline history should survive an unusable CLI override");

    assert_eq!(result.source_label, "offline");
    assert_eq!(result.usage.login_method.as_deref(), Some("offline"));
}

#[tokio::test]
async fn unusable_cli_override_without_history_reports_the_override() {
    let (result, _) = run_fallback(
        Err(ProviderError::Other("local API unavailable".to_string()).into()),
        Err(LiveFailure::from(ProviderError::NotInstalled(
            "ANTIGRAVITY_CLI_PATH is set but does not point to a usable agy file".into(),
        ))),
        None,
    )
    .await;

    match result {
        Err(ProviderError::NotInstalled(message)) => {
            assert!(message.contains("ANTIGRAVITY_CLI_PATH"), "{message}");
        }
        other => panic!("expected the actionable override error, got {other:?}"),
    }
}

#[test]
fn user_tier_name_wins_over_the_plan_status_name() {
    // (userTier, login method)
    for (user_tier, plan) in [
        (
            Some(serde_json::json!({
                "id": "g1-ultra-tier",
                "name": "Google AI Ultra",
                "description": "Google AI Ultra"
            })),
            "Google AI Ultra",
        ),
        (None, "Pro"),
    ] {
        let mut status = serde_json::json!({
            "email": "user@example.com",
            "planStatus": {"planInfo": {"planName": "Pro"}},
            "cascadeModelConfigData": {
                "clientModelConfigs": [
                    {"label": "Gemini 2.5 Pro", "quotaInfo": {"remainingFraction": 0.8}}
                ]
            }
        });
        if let Some(user_tier) = user_tier {
            status["userTier"] = user_tier;
        }
        let resp: UserStatusResponse =
            serde_json::from_value(serde_json::json!({ "userStatus": status })).unwrap();
        let snap = parse_user_status(resp).unwrap();
        assert_eq!(snap.login_method.as_deref(), Some(plan));
        assert_eq!(snap.account_email.as_deref(), Some("user@example.com"));
    }
}

#[test]
fn plan_name_fallback_skips_blank_names() {
    for (status, plan) in [
        (
            serde_json::json!({
                "userTier": {"name": "   ", "description": " Google AI Ultra "}
            }),
            "Google AI Ultra",
        ),
        (
            serde_json::json!({
                "planStatus": {"planInfo": {"planDisplayName": " ", "planName": " Pro "}}
            }),
            "Pro",
        ),
    ] {
        let status: UserStatus = serde_json::from_value(status).unwrap();
        assert_eq!(resolve_plan_name(&status).as_deref(), Some(plan));
    }
}
