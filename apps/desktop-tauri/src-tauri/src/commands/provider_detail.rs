use super::*;

// ── Provider detail pane (Phase 6b) ──────────────────────────────────

/// DTO for the provider detail pane in the Settings Providers tab.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderDetail {
    pub id: String,
    pub display_name: String,
    pub enabled: bool,

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
    pub extra_rate_windows: Vec<NamedRateWindowSnapshot>,

    // Cost / pace.
    pub cost: Option<CostSnapshotBridge>,
    pub pace: Option<PaceSnapshot>,

    // Error / state.
    pub last_error: Option<String>,

    // URLs for quick-actions (button visibility).
    pub dashboard_url: Option<String>,
    pub status_page_url: Option<String>,
    pub buy_credits_url: Option<String>,

    // True if the shared backend has produced any snapshot yet.
    pub has_snapshot: bool,

    // Phase 6c — currently-persisted cookie source & region for round-tripping
    // into the settings UI pickers. `None` for providers that do not support
    // one of the pickers.
    pub cookie_source: Option<String>,
    pub region: Option<String>,
}

pub(crate) fn build_provider_detail(provider_id: &str) -> Result<ProviderDetail, String> {
    let id = parse_provider_arg(provider_id)?;

    let settings = Settings::load();
    let enabled = settings
        .enabled_providers
        .iter()
        .any(|p| p == id.cli_name());

    let provider = instantiate_provider(id);
    let metadata = provider.metadata();

    Ok(ProviderDetail {
        id: id.cli_name().to_string(),
        display_name: id.display_name().to_string(),
        enabled,
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
        extra_rate_windows: Vec::new(),
        cost: None,
        pace: None,
        last_error: None,
        dashboard_url: metadata.dashboard_url.map(|s| s.to_string()),
        status_page_url: metadata.status_page_url.map(|s| s.to_string()),
        // Buy-credits currently mirrors the dashboard URL for providers that
        // support credit top-ups; refine once a dedicated URL lands upstream.
        buy_credits_url: if metadata.supports_credits {
            metadata.dashboard_url.map(|s| s.to_string())
        } else {
            None
        },
        has_snapshot: false,
        cookie_source: provider_cookie_source_lookup(&settings, id.cli_name()),
        region: provider_region_lookup(&settings, id.cli_name()),
    })
}

#[tauri::command]
pub fn get_provider_detail(
    app: tauri::AppHandle,
    provider_id: String,
) -> Result<ProviderDetail, String> {
    let mut detail = build_provider_detail(&provider_id)?;

    // Merge the latest cached snapshot, if any.
    let state = app.state::<Mutex<AppState>>();
    if let Ok(guard) = state.lock()
        && let Some(snap) = guard
            .provider_cache
            .iter()
            .find(|s| s.provider_id == detail.id)
    {
        detail.email = snap.account_email.clone();
        detail.plan = snap.plan_name.clone();
        detail.organization = snap.account_organization.clone();
        detail.source_label = if snap.source_label.is_empty() {
            None
        } else {
            Some(snap.source_label.clone())
        };
        detail.last_updated = Some(snap.updated_at.clone());
        if snap.error.is_none() {
            detail.session = Some(snap.primary.clone());
            detail.weekly = snap.secondary.clone();
            detail.model_specific = snap.model_specific.clone();
            detail.tertiary = snap.tertiary.clone();
            detail.extra_rate_windows = snap.extra_rate_windows.clone();
            detail.cost = snap.cost.clone();
            detail.pace = snap.pace.clone();
        }
        detail.last_error = snap.error.clone();
        detail.has_snapshot = true;
    }

    Ok(detail)
}

#[tauri::command]
pub fn revoke_provider_credentials(provider_id: String) -> Result<(), String> {
    // Best-effort: drop every app-managed credential for this provider so the
    // caller can follow up with a fresh login or import. Missing entries are
    // silently ignored; only I/O errors propagate.
    let id = parse_provider_arg(&provider_id)?;
    let provider_id = id.cli_name();

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
