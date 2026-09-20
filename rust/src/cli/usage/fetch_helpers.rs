//! Fetch plumbing shared by the text, JSON, and toon output paths.

use super::UsageCommand;
use super::render::{
    render_brief_text, render_json_result, render_text_error, render_text_with_status,
};
use crate::core::{
    ProviderFetchResult, ProviderId, TokenAccountStore, TokenAccountSupport, instantiate_provider,
};
use crate::settings::ApiKeys;
use crate::status::{ProviderStatus as StatusInfo, fetch_provider_status};

pub async fn fetch_provider_text_output(provider_id: ProviderId, command: &UsageCommand) -> String {
    match fetch_provider_result(provider_id, command).await {
        Ok((result, status)) => {
            if command.brief {
                render_brief_text(provider_id, &result)
            } else {
                render_text_with_status(provider_id, &result, status.as_ref(), command.use_color)
            }
        }
        Err(e) => render_text_error(provider_id, &e.to_string(), command.use_color),
    }
}

pub async fn fetch_provider_json_output(
    provider_id: ProviderId,
    command: &UsageCommand,
) -> serde_json::Value {
    match fetch_provider_result(provider_id, command).await {
        Ok((result, status)) => render_json_result(provider_id, result, status.as_ref()),
        Err(e) => serde_json::json!({
            "provider": provider_id.cli_name(),
            "error": e.to_string(),
        }),
    }
}

pub async fn fetch_provider_result(
    provider_id: ProviderId,
    command: &UsageCommand,
) -> anyhow::Result<(ProviderFetchResult, Option<StatusInfo>)> {
    let provider = instantiate_provider(provider_id);
    let status_future = command
        .fetch_status
        .then(|| fetch_provider_status(provider_id.cli_name()));
    let mut ctx = command.ctx.clone();
    if ctx.api_key.is_none() {
        ctx.api_key = resolve_cli_api_key(provider_id, command.account.as_deref())?;
    }
    let result = provider.fetch_usage(&ctx).await?;
    let status = if let Some(fut) = status_future {
        fut.await
    } else {
        None
    };
    Ok((result, status))
}

/// Resolve an API key from token accounts (active or `--account`) then stored keys.
///
/// Token-account env injection takes precedence over `api_keys.json` so multi-key
/// providers (OpenRouter, z.ai, ...) honor the selected labeled account.
fn resolve_cli_api_key(
    provider_id: ProviderId,
    account_ref: Option<&str>,
) -> anyhow::Result<Option<String>> {
    if TokenAccountSupport::is_supported(provider_id)
        && let Ok(data) = TokenAccountStore::new().load_provider(provider_id)
        && !data.accounts.is_empty()
    {
        let account = if let Some(account_ref) = account_ref {
            find_token_account(&data, account_ref)?
        } else {
            data.active_account().ok_or_else(|| {
                anyhow::anyhow!("No active token account for {}", provider_id.display_name())
            })?
        };
        if let Some(env) = TokenAccountSupport::env_override(provider_id, &account.token)
            && let Some(key) = env.into_values().next()
        {
            return Ok(Some(key));
        }
    }

    Ok(ApiKeys::load()
        .get(provider_id.cli_name())
        .map(|s| s.to_string()))
}

pub(super) fn find_token_account<'a>(
    data: &'a crate::core::ProviderAccountData,
    account_ref: &str,
) -> anyhow::Result<&'a crate::core::TokenAccount> {
    if let Ok(idx) = account_ref.parse::<usize>()
        && idx > 0
        && idx <= data.accounts.len()
    {
        return Ok(&data.accounts[idx - 1]);
    }
    if let Some(account) = data
        .accounts
        .iter()
        .find(|a| a.label.eq_ignore_ascii_case(account_ref))
    {
        return Ok(account);
    }
    anyhow::bail!(
        "Account '{}' not found. Use 'codexbar account list <provider>' to see accounts.",
        account_ref
    )
}
