use super::*;

// ── Provider detail pane (Phase 6b) ──────────────────────────────────

/// DTO for the provider detail pane in the Settings Providers tab.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDetail {
    pub id: String,
    pub display_name: String,
    pub enabled: bool,
    pub auto_resume_after_quota_reset: bool,
    pub auto_resume_supported: bool,
    /// LiteLLM/Claude expose one opt-in extra breakdown; other providers do not.
    pub optional_details_supported: bool,
    pub optional_details_enabled: bool,

    // Identity
    pub email: Option<String>,
    pub plan: Option<String>,
    pub auth_type: Option<String>,
    pub source_label: Option<String>,
    pub organization: Option<String>,
    pub last_updated: Option<String>,

    // Usage windows — reuse existing RateWindowSnapshot shape.
    pub session: Option<RateWindowSnapshot>,
    pub weekly: Option<RateWindowSnapshot>,
    /// Provider-declared label for the session (primary) lane; None when the
    /// provider uses the generic "Session" wording.
    pub primary_label: Option<String>,
    /// Provider-declared label for the weekly (secondary) lane; None when the
    /// provider uses the generic "Weekly" wording.
    pub secondary_label: Option<String>,
    pub model_specific: Option<RateWindowSnapshot>,
    pub tertiary: Option<RateWindowSnapshot>,
    /// Locale key naming the tertiary lane when it carries a semantic label
    /// beyond "Tertiary" (upstream F5). Drives the settings metric picker.
    pub tertiary_label_key: Option<&'static str>,
    /// Id of the extra rate window holding the provider's monthly plan
    /// allowance (upstream 0.70.0 #4072). Offers the Monthly Plan metric in
    /// the settings metric picker.
    pub monthly_plan_window_id: Option<&'static str>,
    /// Metric picker label for the primary lane when it is not a session
    /// window (upstream `menuBarLayoutPrimaryLabel`).
    pub primary_metric_label: Option<&'static str>,
    pub extra_rate_windows: Vec<NamedRateWindowSnapshot>,
    pub usage_items: Vec<ProviderUsageItemSnapshot>,
    pub hidden_usage_item_ids: Vec<String>,
    pub inventory: Vec<ProviderInventoryItemSnapshot>,
    pub display_details: Vec<ProviderDisplayDetailSnapshot>,
    /// Plan renewal / expiry dates for the Settings usage preview.
    pub subscription: Option<SubscriptionMetadataSnapshot>,

    // Cost / pace.
    pub cost: Option<CostSnapshotBridge>,
    pub pace: Option<PaceSnapshot>,
    /// Per-day OpenAI Admin API history for the Settings daily usage chart.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_ai_api_usage: Option<OpenAiApiUsageSnapshot>,

    // Error / state.
    pub last_error: Option<String>,
    /// Backend-classified availability state for the latest refresh.
    pub error_state: Option<codexbar::core::ProviderStateKind>,

    // URLs for quick-actions (button visibility).
    pub dashboard_url: Option<String>,
    pub status_page_url: Option<String>,
    pub buy_credits_url: Option<String>,

    // True if the shared backend has produced any snapshot yet.
    pub has_snapshot: bool,

    // Phase 6c — currently-persisted cookie source & region for round-tripping
    // into the settings UI pickers. `None` for providers that do not support
    // one of the pickers.
    pub usage_source: Option<String>,
    pub cookie_source: Option<String>,
    pub region: Option<String>,
    /// True when the Manual cookie source has no usable header and the
    /// provider opted in to failing closed (`manual_empty_cookie_policy`).
    /// Drives the "No cookie header pasted." hint and the switch-to-Auto action.
    pub manual_cookie_missing: bool,
}

/// Same decision the refresh path makes in `build_fetch_context`, gated on the
/// provider's fail-closed policy so opening the pane never triggers a browser
/// cookie import.
fn manual_cookie_missing_for(id: ProviderId, settings: &Settings) -> bool {
    if settings.cookie_source(id) != "manual"
        || instantiate_provider(id).manual_empty_cookie_policy()
            != ManualEmptyCookiePolicy::FailClosedWeb
    {
        return false;
    }
    let token_accounts = TokenAccountStore::new().load().unwrap_or_default();
    build_fetch_context(
        id,
        settings,
        &ManualCookies::load(),
        &ApiKeys::load(),
        &token_accounts,
    )
    .manual_cookie_missing
}

pub(crate) fn build_provider_detail(
    provider_id: &str,
) -> Result<(ProviderDetail, Settings, ProviderId), String> {
    let id = parse_provider_arg(provider_id)?;

    let settings = Settings::load();
    let enabled = settings
        .enabled_providers
        .iter()
        .any(|p| p == id.cli_name());

    let provider = instantiate_provider(id);
    let metadata = provider.metadata();
    let resume_supported = auto_resume_supported(id);
    let dashboard_url = provider_dashboard_url(id, &settings);

    let detail = ProviderDetail {
        id: id.cli_name().to_string(),
        display_name: id.display_name().to_string(),
        enabled,
        auto_resume_after_quota_reset: settings.auto_resume_after_quota_reset(id),
        auto_resume_supported: resume_supported,
        optional_details_supported: codexbar::settings::provider_has_optional_details(id),
        optional_details_enabled: settings.optional_details_enabled(id),
        email: None,
        plan: None,
        auth_type: None,
        source_label: None,
        organization: None,
        last_updated: None,
        session: None,
        weekly: None,
        primary_label: None,
        secondary_label: None,
        model_specific: None,
        tertiary: None,
        tertiary_label_key: metadata.tertiary_label_key,
        monthly_plan_window_id: provider.monthly_plan_window_id(),
        primary_metric_label: provider.menu_bar_primary_label(),
        extra_rate_windows: Vec::new(),
        usage_items: Vec::new(),
        hidden_usage_item_ids: settings.hidden_usage_item_ids(id),
        inventory: Vec::new(),
        display_details: Vec::new(),
        subscription: None,
        cost: None,
        pace: None,
        open_ai_api_usage: None,
        last_error: None,
        error_state: None,
        dashboard_url: dashboard_url.clone(),
        status_page_url: metadata.status_page_url.map(|s| s.to_string()),
        // Buy-credits currently mirrors the dashboard URL for providers that
        // support credit top-ups; refine once a dedicated URL lands upstream.
        buy_credits_url: if metadata.supports_credits {
            dashboard_url
        } else {
            None
        },
        has_snapshot: false,
        usage_source: provider_usage_source_lookup(&settings, id.cli_name()),
        cookie_source: provider_cookie_source_lookup(&settings, id.cli_name()),
        region: provider_region_lookup(&settings, id.cli_name()),
        manual_cookie_missing: manual_cookie_missing_for(id, &settings),
    };

    Ok((detail, settings, id))
}

/// Return whether the exact-session resume control can safely be offered for
/// the currently selected credential lane. Managed token-account lanes cannot
/// be correlated with local process discovery, so the UI and command both fail
/// closed while one is active (or when its store cannot be read).
pub(crate) fn auto_resume_supported(id: ProviderId) -> bool {
    crate::auto_resume::supports_auto_resume(id) && crate::auto_resume::is_auto_resume_available(id)
}

#[tauri::command]
pub fn get_provider_detail(
    app: tauri::AppHandle,
    provider_id: String,
) -> Result<ProviderDetail, String> {
    let (mut detail, settings, parsed_provider_id) = build_provider_detail(&provider_id)?;

    // Merge the latest cached snapshot, if any.
    let state = app.state::<Mutex<AppState>>();
    if let Ok(guard) = state.lock()
        && let Some(snap) = guard
            .provider_cache
            .iter()
            .find(|s| s.provider_id == detail.id)
    {
        let snapshot = snap.clone();
        detail.email = snapshot.account_email.clone();
        detail.plan = snapshot.plan_name.clone();
        detail.organization = snapshot.account_organization.clone();
        if parsed_provider_id == ProviderId::Helmcode {
            detail.dashboard_url = Some(
                codexbar::providers::helmcode::dashboard_url_for_organization(
                    snapshot.account_organization.as_deref(),
                )
                .to_string(),
            );
        }
        detail.source_label = if snapshot.source_label.is_empty() {
            None
        } else {
            Some(snapshot.source_label.clone())
        };
        detail.last_updated = Some(snapshot.updated_at.clone());
        if snapshot.error.is_none() {
            detail.usage_items =
                super::usage_item_descriptors(Some(&snapshot), &settings, parsed_provider_id);
            detail.session = Some(snapshot.primary.clone());
            detail.weekly = snapshot.secondary.clone();
            detail.primary_label = snapshot.primary_label.clone();
            detail.secondary_label = snapshot.secondary_label.clone();
            detail.tertiary = snapshot.tertiary.clone();
            detail.extra_rate_windows = snapshot.extra_rate_windows.clone();
            detail.inventory = snapshot.inventory.clone();
            detail.display_details = snapshot.display_details.clone();
            detail.subscription = snapshot.subscription.clone();
            detail.cost = snapshot.cost.clone();
            detail.pace = snapshot.pace.clone();
            detail.open_ai_api_usage = snapshot.open_ai_api_usage.clone();
        }
        // Shown verbatim behind "Show details"; redact again so every cache
        // path (refresh, seed, restore) reaches the pane scrubbed.
        detail.last_error = snapshot
            .error
            .as_deref()
            .map(codexbar::core::SecretRedactor::redact);
        detail.error_state = Some(snapshot.error_state);
        detail.has_snapshot = true;
    }

    if detail.usage_items.is_empty() {
        detail.usage_items = super::usage_item_descriptors(None, &settings, parsed_provider_id);
    }

    Ok(detail)
}

#[tauri::command]
pub fn revoke_provider_credentials(
    app: tauri::AppHandle,
    provider_id: String,
) -> Result<(), String> {
    // Best-effort: drop every app-managed credential for this provider so the
    // caller can follow up with a fresh login or import. Missing entries are
    // silently ignored; only I/O errors propagate.
    let id = parse_provider_arg(&provider_id)?;
    let provider_id = id.cli_name();
    crate::auto_resume::clear(&app, id);

    let mut keys = ApiKeys::load();
    keys.remove(provider_id);
    keys.save().map_err(|e| e.to_string())?;

    let mut cookies = ManualCookies::load();
    cookies.remove(provider_id);
    cookies.save().map_err(|e| e.to_string())?;

    let token_store = TokenAccountStore::new();
    let mut token_accounts = token_store.load().map_err(|e| e.to_string())?;
    if token_accounts.remove(&id).is_some() {
        token_store
            .save(&token_accounts)
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialStorageStatusBridge {
    pub manual_cookies: String,
    pub api_keys: String,
    pub token_accounts: String,
}

pub(crate) fn credential_file_status_label(status: SecureFileStatus) -> String {
    match status {
        SecureFileStatus::Missing => "missing".to_string(),
        SecureFileStatus::Plaintext => "plaintext".to_string(),
        SecureFileStatus::Protected(protection) => format!("protected:{protection}"),
        SecureFileStatus::Unreadable(_) => "unreadable".to_string(),
    }
}

fn optional_credential_status(path: Option<std::path::PathBuf>) -> String {
    path.map(|path| credential_file_status_label(secure_file::status(&path)))
        .unwrap_or_else(|| "unavailable".to_string())
}

#[tauri::command]
pub fn get_credential_storage_status() -> CredentialStorageStatusBridge {
    CredentialStorageStatusBridge {
        manual_cookies: optional_credential_status(ManualCookies::cookies_path()),
        api_keys: optional_credential_status(ApiKeys::keys_path()),
        token_accounts: credential_file_status_label(secure_file::status(
            &TokenAccountStore::default_path(),
        )),
    }
}
