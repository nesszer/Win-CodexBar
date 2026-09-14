//! Token account commands and DTOs.
//!
//! Token values themselves are never exposed to the frontend; only masked
//! metadata flows through the bridge. Active-account selection is clamped
//! against the current list size before each response.

use codexbar::core::{ProviderId, TokenAccount, TokenAccountStore, TokenAccountSupport};
use serde::Serialize;

/// Bridge-friendly token account support descriptor for a provider.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenAccountSupportBridge {
    pub provider_id: String,
    pub display_name: String,
    pub title: String,
    pub subtitle: String,
    pub placeholder: String,
}

/// Bridge-friendly token account (token value is never exposed).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenAccountBridge {
    pub id: String,
    pub label: String,
    pub added_at: String,
    pub last_used: Option<String>,
    pub is_active: bool,
}

/// Bridge-friendly provider token accounts snapshot.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderTokenAccountsBridge {
    pub provider_id: String,
    pub support: TokenAccountSupportBridge,
    pub accounts: Vec<TokenAccountBridge>,
    pub active_index: usize,
}

fn format_token_account_date(ts: i64) -> String {
    chrono::DateTime::from_timestamp(ts, 0)
        .map(|dt: chrono::DateTime<chrono::Utc>| dt.format("%b %d, %Y").to_string())
        .unwrap_or_else(|| "Unknown".to_string())
}

fn build_provider_token_accounts(
    provider_id: ProviderId,
    support: &TokenAccountSupport,
    accounts: Vec<TokenAccount>,
    active_index: usize,
) -> ProviderTokenAccountsBridge {
    let support_bridge = TokenAccountSupportBridge {
        provider_id: provider_id.cli_name().to_string(),
        display_name: provider_id.display_name().to_string(),
        title: support.title.to_string(),
        subtitle: support.subtitle.to_string(),
        placeholder: support.placeholder.to_string(),
    };
    let account_bridges: Vec<TokenAccountBridge> = accounts
        .iter()
        .enumerate()
        .map(|(i, a)| TokenAccountBridge {
            id: a.id.to_string(),
            label: a.label.clone(),
            added_at: format_token_account_date(a.added_at),
            last_used: a.last_used.map(format_token_account_date),
            is_active: i == active_index,
        })
        .collect();
    ProviderTokenAccountsBridge {
        provider_id: provider_id.cli_name().to_string(),
        support: support_bridge,
        accounts: account_bridges,
        active_index,
    }
}

/// List all providers that support token accounts.
#[tauri::command]
pub fn get_token_account_providers() -> Vec<TokenAccountSupportBridge> {
    ProviderId::all()
        .iter()
        .filter_map(|&id| {
            TokenAccountSupport::for_provider(id).map(|s| TokenAccountSupportBridge {
                provider_id: id.cli_name().to_string(),
                display_name: id.display_name().to_string(),
                title: s.title.to_string(),
                subtitle: s.subtitle.to_string(),
                placeholder: s.placeholder.to_string(),
            })
        })
        .collect()
}

/// Load token accounts for a single provider.
#[tauri::command]
pub fn get_token_accounts(provider_id: String) -> Result<ProviderTokenAccountsBridge, String> {
    let id = super::parse_provider_arg(&provider_id)?;
    let support = TokenAccountSupport::for_provider(id)
        .ok_or_else(|| format!("Provider {provider_id} does not support token accounts"))?;
    let store = TokenAccountStore::new();
    let data = store.load_provider(id).map_err(|e| e.to_string())?;
    let active = data.clamped_active_index();
    Ok(build_provider_token_accounts(
        id,
        &support,
        data.accounts,
        active,
    ))
}

/// Add a token account for a provider.
#[tauri::command]
pub fn add_token_account(
    app: tauri::AppHandle,
    provider_id: String,
    label: String,
    token: String,
) -> Result<ProviderTokenAccountsBridge, String> {
    let id = super::parse_provider_arg(&provider_id)?;
    let support = TokenAccountSupport::for_provider(id)
        .ok_or_else(|| format!("Provider {provider_id} does not support token accounts"))?;
    super::validate_single_line_secret(&token, "Token", super::MAX_COOKIE_HEADER_LEN)?;
    let label = super::sanitize_optional_label(Some(label))?
        .unwrap_or_else(|| id.display_name().to_string());
    let store = TokenAccountStore::new();
    let mut data = store.load_provider(id).map_err(|e| e.to_string())?;
    data.add_account(TokenAccount::new(label, token.trim()));
    store.save_provider(id, &data).map_err(|e| e.to_string())?;
    crate::auto_resume::clear(&app, id);
    let active = data.clamped_active_index();
    Ok(build_provider_token_accounts(
        id,
        &support,
        data.accounts,
        active,
    ))
}

/// Remove a token account by UUID string.
#[tauri::command]
pub fn remove_token_account(
    app: tauri::AppHandle,
    provider_id: String,
    account_id: String,
) -> Result<ProviderTokenAccountsBridge, String> {
    let id = super::parse_provider_arg(&provider_id)?;
    let support = TokenAccountSupport::for_provider(id)
        .ok_or_else(|| format!("Provider {provider_id} does not support token accounts"))?;
    let uuid = uuid::Uuid::parse_str(&account_id).map_err(|e| e.to_string())?;
    let store = TokenAccountStore::new();
    let mut data = store.load_provider(id).map_err(|e| e.to_string())?;
    data.remove_account(uuid);
    store.save_provider(id, &data).map_err(|e| e.to_string())?;
    crate::auto_resume::clear(&app, id);
    let active = data.clamped_active_index();
    Ok(build_provider_token_accounts(
        id,
        &support,
        data.accounts,
        active,
    ))
}

/// Set the active token account for a provider by UUID string.
#[tauri::command]
pub fn set_active_token_account(
    app: tauri::AppHandle,
    provider_id: String,
    account_id: String,
) -> Result<ProviderTokenAccountsBridge, String> {
    let id = super::parse_provider_arg(&provider_id)?;
    let support = TokenAccountSupport::for_provider(id)
        .ok_or_else(|| format!("Provider {provider_id} does not support token accounts"))?;
    let uuid = uuid::Uuid::parse_str(&account_id).map_err(|e| e.to_string())?;
    let store = TokenAccountStore::new();
    let mut data = store.load_provider(id).map_err(|e| e.to_string())?;
    data.set_active_by_id(uuid);
    store.save_provider(id, &data).map_err(|e| e.to_string())?;
    crate::auto_resume::clear(&app, id);
    let active = data.clamped_active_index();
    Ok(build_provider_token_accounts(
        id,
        &support,
        data.accounts,
        active,
    ))
}
