use super::*;

fn bucket(model: Option<&str>, fraction: Option<f64>, reset: Option<&str>) -> QuotaBucket {
    QuotaBucket {
        remaining_fraction: fraction,
        reset_time: reset.map(str::to_string),
        model_id: model.map(str::to_string),
        token_type: None,
    }
}

fn parse_buckets(
    buckets: Vec<QuotaBucket>,
    creds: Option<&OAuthCredentials>,
) -> Result<(RateWindow, Option<RateWindow>, Option<String>), ProviderError> {
    GeminiApi::new().parse_quota_response(
        QuotaResponse {
            buckets: Some(buckets),
        },
        creds,
    )
}

fn at(rfc3339: &str) -> Option<DateTime<Utc>> {
    Some(
        DateTime::parse_from_rfc3339(rfc3339)
            .unwrap()
            .with_timezone(&Utc),
    )
}

#[test]
fn quota_pro_is_primary_and_flash_is_model_specific() {
    use base64::Engine;
    let payload =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(br#"{"email":"user@example.com"}"#);
    let creds = OAuthCredentials {
        access_token: None,
        id_token: Some(format!("header.{payload}.sig")),
        refresh_token: None,
        expiry_date: None,
    };
    let (primary, model_specific, email) = parse_buckets(
        vec![
            bucket(
                Some("gemini-2.5-pro"),
                Some(0.6),
                Some("2026-01-14T00:00:00Z"),
            ),
            bucket(
                Some("gemini-2.5-pro"),
                Some(0.4),
                Some("2026-01-15T00:00:00Z"),
            ),
            bucket(
                Some("gemini-2.5-flash"),
                Some(0.9),
                Some("2026-01-16T00:00:00.5Z"),
            ),
            bucket(Some("gemini-2.0-flash"), Some(0.95), None),
            bucket(None, Some(0.0), None),
            bucket(Some("gemini-2.5-pro"), None, None),
        ],
        Some(&creds),
    )
    .unwrap();
    assert_eq!(primary.used_percent, (1.0 - 0.4) * 100.0);
    assert_eq!(primary.window_minutes, Some(1440));
    assert_eq!(primary.resets_at, at("2026-01-15T00:00:00Z"));
    assert_eq!(primary.reset_description, None);
    let flash = model_specific.expect("flash window when pro is primary");
    assert_eq!(flash.used_percent, (1.0 - 0.9) * 100.0);
    assert_eq!(flash.window_minutes, Some(1440));
    assert_eq!(flash.resets_at, at("2026-01-16T00:00:00.5Z"));
    assert_eq!(email.as_deref(), Some("user@example.com"));
}

#[test]
fn quota_falls_back_to_flash_then_any_model() {
    let (primary, model_specific, email) = parse_buckets(
        vec![
            bucket(Some("gemini-2.5-flash"), Some(0.3), Some("not a date")),
            bucket(Some("other-model"), Some(0.1), None),
        ],
        None,
    )
    .unwrap();
    assert_eq!(primary.used_percent, (1.0 - 0.3) * 100.0);
    assert_eq!(primary.resets_at, None);
    assert!(model_specific.is_none());
    assert_eq!(email, None);

    let (primary, model_specific, _) = parse_buckets(
        vec![bucket(
            Some("other-model"),
            Some(0.25),
            Some("2026-01-15T00:00:00+02:00"),
        )],
        None,
    )
    .unwrap();
    assert_eq!(primary.used_percent, (1.0 - 0.25) * 100.0);
    assert_eq!(primary.resets_at, at("2026-01-14T22:00:00Z"));
    assert!(model_specific.is_none());
}

#[test]
fn quota_without_usable_model_buckets_reports_an_unused_window() {
    // A fraction of 1.0 never beats the per-model starting value, so its
    // reset time is dropped.
    let (primary, model_specific, _) = parse_buckets(
        vec![
            bucket(
                Some("gemini-2.5-pro"),
                Some(1.0),
                Some("2026-01-15T00:00:00Z"),
            ),
            bucket(None, Some(0.2), Some("2026-01-15T00:00:00Z")),
        ],
        None,
    )
    .unwrap();
    assert_eq!(primary.used_percent, 0.0);
    assert_eq!(primary.resets_at, None);
    assert!(model_specific.is_none());

    let (primary, model_specific, _) =
        parse_buckets(vec![bucket(None, Some(0.2), None)], None).unwrap();
    assert_eq!(primary.used_percent, 0.0);
    assert_eq!(primary.window_minutes, Some(1440));
    assert_eq!(primary.resets_at, None);
    assert!(model_specific.is_none());
}

#[test]
fn quota_without_buckets_is_a_parse_error() {
    let empty = parse_buckets(Vec::new(), None).unwrap_err();
    assert!(matches!(empty, ProviderError::Parse(msg) if msg == "Empty quota buckets"));
    let missing = GeminiApi::new()
        .parse_quota_response(QuotaResponse { buckets: None }, None)
        .unwrap_err();
    assert!(matches!(missing, ProviderError::Parse(msg) if msg == "No quota buckets in response"));
}

#[test]
fn bundled_cli_layout_yields_oauth_client_credentials() {
    // npm global layout on Windows: %APPDATA%\npm\gemini.cmd next to
    // node_modules\@google\gemini-cli\bundle\chunk-*.js (no gemini-cli-core/dist).
    let dir = tempfile::tempdir().unwrap();
    let bin_dir = dir.path();
    let bundle = bin_dir
        .join("node_modules")
        .join("@google")
        .join("gemini-cli")
        .join("bundle");
    std::fs::create_dir_all(&bundle).unwrap();
    std::fs::write(bundle.join("chunk-AAA.js"), "var x = 1;").unwrap();
    std::fs::write(
            bundle.join("chunk-BBB.js"),
            r#"var OAUTH_CLIENT_ID = "id-123.apps.googleusercontent.com"; var OAUTH_CLIENT_SECRET = 'secret-xyz';"#,
        )
        .unwrap();

    assert!(
        GeminiApi::oauth_credentials_from_candidates(GeminiApi::binary_oauth_candidates(bin_dir))
            .is_none(),
        "legacy dist layout must not match"
    );
    let creds =
        GeminiApi::bundled_cli_oauth_credentials(bin_dir).expect("bundle chunks should be scanned");
    assert_eq!(creds.client_id, "id-123.apps.googleusercontent.com");
    assert_eq!(creds.client_secret, "secret-xyz");
}

const BUNDLE_CHUNK_WITH_CONSTANTS: &str = r#"var OAUTH_CLIENT_ID = "id-456.apps.googleusercontent.com"; var OAUTH_CLIENT_SECRET = "secret-abc";"#;

fn write_gemini_bundle(node_modules: &Path) -> PathBuf {
    let bundle = node_modules
        .join("@google")
        .join("gemini-cli")
        .join("bundle");
    std::fs::create_dir_all(&bundle).unwrap();
    std::fs::write(bundle.join("gemini.js"), "import './chunk-A.js';").unwrap();
    std::fs::write(bundle.join("chunk-A.js"), BUNDLE_CHUNK_WITH_CONSTANTS).unwrap();
    bundle
}

#[test]
fn symlinked_binary_inside_bundle_yields_oauth_client_credentials() {
    // Unix npm/Homebrew: bin/gemini canonicalizes to .../gemini-cli/bundle/gemini.js.
    let dir = tempfile::tempdir().unwrap();
    let bundle = write_gemini_bundle(&dir.path().join("lib").join("node_modules"));

    let creds = GeminiApi::bundled_cli_oauth_credentials(&bundle)
        .expect("the bundle that holds the binary should be scanned");
    assert_eq!(creds.client_id, "id-456.apps.googleusercontent.com");
    assert_eq!(creds.client_secret, "secret-abc");
}

#[test]
fn unrelated_bundle_directory_is_not_scanned() {
    let dir = tempfile::tempdir().unwrap();
    let other = dir.path().join("other-tool").join("bundle");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::write(other.join("chunk.js"), BUNDLE_CHUNK_WITH_CONSTANTS).unwrap();

    assert!(GeminiApi::bundled_cli_oauth_credentials(&other).is_none());
}

#[test]
fn npm_global_node_modules_bundle_yields_oauth_client_credentials() {
    // %APPDATA%\npm\node_modules fallback when `gemini` is not on PATH.
    let dir = tempfile::tempdir().unwrap();
    let node_modules = dir.path().join("npm").join("node_modules");
    write_gemini_bundle(&node_modules);

    let creds = GeminiApi::node_modules_oauth_credentials(&node_modules)
        .expect("bundle under the npm global node_modules should be scanned");
    assert_eq!(creds.client_secret, "secret-abc");
}

#[test]
fn fnm_windows_and_unix_layouts_yield_bundle_credentials() {
    let dir = tempfile::tempdir().unwrap();
    let versions = dir.path().join("node-versions");
    // Windows fnm keeps global packages directly under installation\node_modules.
    write_gemini_bundle(
        &versions
            .join("v22.0.0")
            .join("installation")
            .join("node_modules"),
    );
    let creds = GeminiApi::fnm_oauth_credentials_from(&versions)
        .expect("Windows fnm layout should be scanned");
    assert_eq!(creds.client_id, "id-456.apps.googleusercontent.com");

    let unix_dir = tempfile::tempdir().unwrap();
    let unix_versions = unix_dir.path().join("node-versions");
    write_gemini_bundle(
        &unix_versions
            .join("v22.0.0")
            .join("installation")
            .join("lib")
            .join("node_modules"),
    );
    assert!(GeminiApi::fnm_oauth_credentials_from(&unix_versions).is_some());
}

#[test]
fn paid_tier_name_overrides_generic_tier_fallbacks() {
    let status = parse_code_assist_status(
        r#"{
                "currentTier": { "id": "free-tier" },
                "paidTier": { "name": "Gemini Code Assist in Google One AI Pro" }
            }"#,
    );

    assert_eq!(
        resolve_account_plan(&status, Some("example.com")),
        Some("Gemini Code Assist in Google One AI Pro".to_string())
    );

    let standard = parse_code_assist_status(
        r#"{
                "currentTier": { "id": "standard-tier" },
                "paidTier": { "name": "Plus" }
            }"#,
    );

    assert_eq!(
        resolve_account_plan(&standard, None),
        Some("Plus".to_string())
    );
}

#[test]
fn consumer_shutdown_signal_excludes_paid_and_workspace_accounts() {
    let shutdown = parse_code_assist_status(
        r#"{
                "ineligibleTiers": [
                    {"tier":{"id":"free-tier"},"reason":"UNSUPPORTED_CLIENT"}
                ]
            }"#,
    );
    assert!(is_consumer_client_unsupported(&shutdown, None));
    assert!(!is_consumer_client_unsupported(
        &shutdown,
        Some("example.com")
    ));

    let paid = parse_code_assist_status(
        r#"{
                "paidTier":{"name":"Gemini Code Assist Standard"},
                "ineligibleTiers":[
                    {"tier":{"id":"free-tier"},"reason":"UNSUPPORTED_CLIENT"}
                ]
            }"#,
    );
    assert!(!is_consumer_client_unsupported(&paid, None));

    let standard = parse_code_assist_status(
        r#"{
                "currentTier":{"id":"standard-tier"},
                "ineligibleTiers":[
                    {"tier":{"id":"free-tier"},"reason":"UNSUPPORTED_CLIENT"}
                ]
            }"#,
    );
    assert!(!is_consumer_client_unsupported(&standard, None));
}

#[test]
fn generic_tier_fallbacks_remain_when_paid_tier_is_absent() {
    let free_tier = parse_code_assist_status(r#"{"currentTier":{"id":"free-tier"}}"#);
    let paid = parse_code_assist_status(r#"{"currentTier":{"id":"standard-tier"}}"#);

    assert_eq!(
        resolve_account_plan(&free_tier, Some("example.com")),
        Some("Workspace".to_string())
    );
    assert_eq!(
        resolve_account_plan(&free_tier, None),
        Some("Free".to_string())
    );
    assert_eq!(resolve_account_plan(&paid, None), Some("Paid".to_string()));
}

#[test]
fn invalid_code_assist_response_does_not_create_a_generic_plan() {
    let status = parse_code_assist_status("not json");

    assert_eq!(resolve_account_plan(&status, Some("example.com")), None);
}

#[test]
fn malformed_paid_tier_preserves_current_tier_fallback() {
    let status = parse_code_assist_status(r#"{"currentTier":{"id":"free-tier"},"paidTier":[]}"#);

    assert_eq!(
        resolve_account_plan(&status, Some("example.com")),
        Some("Workspace".to_string())
    );
}
