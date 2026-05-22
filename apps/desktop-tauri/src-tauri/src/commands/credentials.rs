use super::*;

// ── Credential store commands ─────────────────────────────────────────

/// Bridge-friendly API key info (secrets masked).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyInfoBridge {
    pub provider_id: String,
    pub provider: String,
    pub masked_key: String,
    pub saved_at: String,
    pub label: Option<String>,
}

/// Bridge-friendly saved cookie info.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CookieInfoBridge {
    pub provider_id: String,
    pub provider: String,
    pub saved_at: String,
}

/// Bridge-friendly provider config info for the API keys tab.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ApiKeyProviderInfoBridge {
    pub id: String,
    pub display_name: String,
    pub env_var: Option<String>,
    pub help: Option<String>,
    pub dashboard_url: Option<String>,
}

/// App metadata for the About tab.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfoBridge {
    pub name: String,
    pub version: String,
    pub build_number: String,
    pub update_channel: String,
    pub tagline: String,
}

#[tauri::command]
pub fn get_api_keys() -> Vec<ApiKeyInfoBridge> {
    let keys = ApiKeys::load();
    keys.get_all_for_display()
        .into_iter()
        .map(|info| ApiKeyInfoBridge {
            provider_id: info.provider_id,
            provider: info.provider,
            masked_key: info.masked_key,
            saved_at: info.saved_at,
            label: info.label,
        })
        .collect()
}

#[tauri::command]
pub fn get_api_key_providers() -> Vec<ApiKeyProviderInfoBridge> {
    codexbar::settings::get_api_key_providers()
        .into_iter()
        .map(|p| ApiKeyProviderInfoBridge {
            id: p.id.cli_name().to_string(),
            display_name: p.name.to_string(),
            env_var: p.api_key_env_var.map(|s| s.to_string()),
            help: p.api_key_help.map(|s| s.to_string()),
            dashboard_url: p.dashboard_url.map(|s| s.to_string()),
        })
        .collect()
}

#[tauri::command]
pub fn set_api_key(
    provider_id: String,
    api_key: String,
    label: Option<String>,
) -> Result<Vec<ApiKeyInfoBridge>, String> {
    let canonical_provider = canonical_provider_arg(&provider_id)?;
    if !codexbar::settings::get_api_key_providers()
        .iter()
        .any(|p| p.id.cli_name() == canonical_provider)
    {
        return Err(format!(
            "Provider '{canonical_provider}' does not support API-key storage"
        ));
    }
    validate_single_line_secret(&api_key, "API key", MAX_API_KEY_LEN)?;
    let label = sanitize_optional_label(label)?;

    let mut keys = ApiKeys::load();
    keys.set(&canonical_provider, api_key.trim(), label.as_deref());
    keys.save().map_err(|e| e.to_string())?;
    Ok(get_api_keys())
}

#[tauri::command]
pub fn remove_api_key(provider_id: String) -> Result<Vec<ApiKeyInfoBridge>, String> {
    let canonical_provider = canonical_provider_arg(&provider_id)?;
    let mut keys = ApiKeys::load();
    keys.remove(&canonical_provider);
    keys.save().map_err(|e| e.to_string())?;
    Ok(get_api_keys())
}

#[tauri::command]
pub fn get_manual_cookies() -> Vec<CookieInfoBridge> {
    let cookies = ManualCookies::load();
    cookies
        .get_all_for_display()
        .into_iter()
        .map(|info| CookieInfoBridge {
            provider_id: info.provider_id,
            provider: info.provider,
            saved_at: info.saved_at,
        })
        .collect()
}

#[tauri::command]
pub fn set_manual_cookie(
    provider_id: String,
    cookie_header: String,
) -> Result<Vec<CookieInfoBridge>, String> {
    let id = parse_provider_arg(&provider_id)?;
    if id.cookie_domain().is_none() {
        return Err(format!(
            "Provider '{}' does not support manual cookie storage",
            id.cli_name()
        ));
    }
    validate_single_line_secret(&cookie_header, "Cookie header", MAX_COOKIE_HEADER_LEN)?;

    let mut cookies = ManualCookies::load();
    cookies.set(id.cli_name(), cookie_header.trim());
    cookies.save().map_err(|e| e.to_string())?;
    Ok(get_manual_cookies())
}

#[tauri::command]
pub fn remove_manual_cookie(provider_id: String) -> Result<Vec<CookieInfoBridge>, String> {
    let canonical_provider = canonical_provider_arg(&provider_id)?;
    let mut cookies = ManualCookies::load();
    cookies.remove(&canonical_provider);
    cookies.save().map_err(|e| e.to_string())?;
    Ok(get_manual_cookies())
}
