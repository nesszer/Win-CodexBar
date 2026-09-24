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
    pub model_specific: Option<RateWindowSnapshot>,
    pub tertiary: Option<RateWindowSnapshot>,
    /// Locale key naming the tertiary lane when it carries a semantic label
    /// beyond "Tertiary" (upstream F5). Drives the settings metric picker.
    pub tertiary_label_key: Option<&'static str>,
    pub extra_rate_windows: Vec<NamedRateWindowSnapshot>,
    pub usage_items: Vec<ProviderUsageItemSnapshot>,
    pub hidden_usage_item_ids: Vec<String>,
    pub inventory: Vec<ProviderInventoryItemSnapshot>,
    pub display_details: Vec<ProviderDisplayDetailSnapshot>,

    // Cost / pace.
    pub cost: Option<CostSnapshotBridge>,
    pub pace: Option<PaceSnapshot>,

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
        email: None,
        plan: None,
        auth_type: None,
        source_label: None,
        organization: None,
        last_updated: None,
        session: None,
        weekly: None,
        model_specific: None,
        tertiary: None,
        tertiary_label_key: metadata.tertiary_label_key,
        extra_rate_windows: Vec::new(),
        usage_items: Vec::new(),
        hidden_usage_item_ids: settings.hidden_usage_item_ids(id),
        inventory: Vec::new(),
        display_details: Vec::new(),
        cost: None,
        pace: None,
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
            detail.model_specific = snapshot.model_specific.clone();
            detail.tertiary = snapshot.tertiary.clone();
            detail.extra_rate_windows = snapshot.extra_rate_windows.clone();
            detail.inventory = snapshot.inventory.clone();
            detail.display_details = snapshot.display_details.clone();
            detail.cost = snapshot.cost.clone();
            detail.pace = snapshot.pace.clone();
        }
        detail.last_error = snapshot.error.clone();
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
