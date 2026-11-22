//! Fetch plumbing shared by the text, JSON, and toon output paths.

use super::UsageCommand;
use super::render::{
    render_brief_text, render_json_result, render_text_error, render_text_with_status,
};
use crate::cli::fetch_context::populate_api_region_from_settings;
use crate::cli_usage_cache;
use crate::core::{
    ProviderFetchResult, ProviderId, TokenAccountKind, TokenAccountOverride, TokenAccountStore,
    TokenAccountSupport, instantiate_provider,
};
use crate::settings::{ApiKeys, ManualCookies, Settings};
use crate::status::{ProviderStatus as StatusInfo, fetch_provider_status};

/// `source_label_suffix` is appended to the source label of the usage that
/// answers (for example ` (claude-swap fallback)`); pass `""` for none.
pub async fn fetch_provider_text_output(
    provider_id: ProviderId,
    command: &UsageCommand,
    source_label_suffix: &str,
) -> String {
    match fetch_provider_result(provider_id, command, source_label_suffix).await {
        Ok((result, status)) => {
            if command.brief {
                render_brief_text(provider_id, &result)
            } else {
                render_text_with_status(
                    provider_id,
                    &result,
                    status.as_ref(),
                    command.use_color,
                    command.weekly_work_days,
                )
            }
        }
        Err(e) => match app_cache_fallback(provider_id, command, &e).await {
            Some(mut fallback) => {
                fallback.result.source_label.push_str(source_label_suffix);
                render_app_cache_text(provider_id, command, &fallback)
            }
            None => render_text_error(provider_id, &e.to_string(), command.use_color),
        },
    }
}

pub async fn fetch_provider_json_output(
    provider_id: ProviderId,
    command: &UsageCommand,
    source_label_suffix: &str,
) -> serde_json::Value {
    match fetch_provider_result(provider_id, command, source_label_suffix).await {
        Ok((result, status)) => render_json_result(
            provider_id,
            result,
            status.as_ref(),
            command.weekly_work_days,
        ),
        Err(e) => match app_cache_fallback(provider_id, command, &e).await {
            Some(mut fallback) => {
                fallback.result.source_label.push_str(source_label_suffix);
                render_app_cache_json(provider_id, command, fallback)
            }
            None => crate::cli::error_kind::usage_error_row(provider_id, &e),
        },
    }
}

/// Usage served from the desktop app's cache after a failed fetch.
pub(super) struct AppCacheFallback {
    pub(super) result: ProviderFetchResult,
    pub(super) age_seconds: i64,
    pub(super) status: Option<StatusInfo>,
}

/// The app-cache answer for a failed default-account fetch (upstream
/// `CLIUsageCache.fallbackReader`), or `None` to report the error.
pub(super) async fn app_cache_fallback(
    provider_id: ProviderId,
    command: &UsageCommand,
    error: &anyhow::Error,
) -> Option<AppCacheFallback> {
    let provider_error = error.downcast_ref::<crate::core::ProviderError>()?;
    let hit = cli_usage_cache::fallback_hit(
        command.app_cache_gate(),
        provider_id,
        provider_error,
        command.app_cache_path.as_deref(),
        chrono::Utc::now(),
    )?;
    let status = if command.fetch_status {
        fetch_provider_status(provider_id).await
    } else {
        None
    };
    Some(AppCacheFallback {
        // Cached rows can be up to MAXIMUM_AGE_SECONDS old, so they must not
        // feed pace or run-out advice.
        result: ProviderFetchResult::new(hit.usage, cli_usage_cache::SOURCE_LABEL)
            .with_non_authoritative_pace(),
        age_seconds: hit.age_seconds,
        status,
    })
}

pub(super) fn render_app_cache_json(
    provider_id: ProviderId,
    command: &UsageCommand,
    fallback: AppCacheFallback,
) -> serde_json::Value {
    let mut row = render_json_result(
        provider_id,
        fallback.result,
        fallback.status.as_ref(),
        command.weekly_work_days,
    );
    row["ageSeconds"] = serde_json::json!(fallback.age_seconds);
    row
}

pub(super) fn render_app_cache_text(
    provider_id: ProviderId,
    command: &UsageCommand,
    fallback: &AppCacheFallback,
) -> String {
    let note = app_cache_note(fallback.age_seconds);
    if command.brief {
        return format!(
            "{} ({note})",
            render_brief_text(provider_id, &fallback.result)
        );
    }
    let mut text = render_text_with_status(
        provider_id,
        &fallback.result,
        fallback.status.as_ref(),
        command.use_color,
        command.weekly_work_days,
    );
    text.push_str(&format!("\n  {note}"));
    text
}

fn app_cache_note(age_seconds: i64) -> String {
    let age = if age_seconds < 60 {
        format!("{age_seconds}s")
    } else {
        format!("{}m", age_seconds / 60)
    };
    format!(
        "Cached by the CodexBar app {age} ago (credentials could not be decrypted in this session)."
    )
}

pub async fn fetch_provider_result(
    provider_id: ProviderId,
    command: &UsageCommand,
    source_label_suffix: &str,
) -> anyhow::Result<(ProviderFetchResult, Option<StatusInfo>)> {
    let provider = instantiate_provider(provider_id);
    let status_future = command
        .fetch_status
        .then(|| fetch_provider_status(provider_id));
    let mut ctx = command.ctx.clone();
    let account_projected = project_cli_account(provider_id, command.account.as_deref(), &mut ctx)?;
    let settings = Settings::load();
    populate_api_region_from_settings(provider_id, &settings, &mut ctx);
    if !account_projected && command.account.is_none() && !command.all_accounts {
        populate_saved_cookie(provider_id, &settings, ManualCookies::load, &mut ctx);
    }
    if !account_projected && ctx.api_key.is_none() {
        ctx.api_key = resolve_cli_api_key(provider_id, command.account.as_deref())?;
    }
    let mut result = provider.fetch_usage(&ctx).await?;
    result.source_label.push_str(source_label_suffix);
    let status = if let Some(fut) = status_future {
        fut.await
    } else {
        None
    };
    Ok((result, status))
}

pub(super) fn populate_saved_cookie(
    provider: ProviderId,
    settings: &Settings,
    load_cookies: impl FnOnce() -> ManualCookies,
    ctx: &mut crate::core::FetchContext,
) {
    if settings.cookie_source(provider) != "manual"
        || !matches!(
            ctx.source_mode,
            crate::core::SourceMode::Auto | crate::core::SourceMode::Web
        )
        || ctx.manual_cookie_header.is_some()
        || ctx.manual_cookie_missing
        || ctx.token_account_isolated
    {
        return;
    }
    ctx.manual_cookie_header = load_cookies()
        .get(provider.cli_name())
        .filter(|cookie| !cookie.trim().is_empty())
        .map(str::to_owned);
}

/// Apply the selected labeled account to the same route used by the desktop
/// shell. The three v0.65 account-source ports require provider-specific route
/// selection in addition to the shared credential injection.
fn project_cli_account(
    provider: ProviderId,
    account_ref: Option<&str>,
    ctx: &mut crate::core::FetchContext,
) -> anyhow::Result<bool> {
    if !matches!(
        provider,
        ProviderId::Kimi | ProviderId::Doubao | ProviderId::OpenCodeGo
    ) {
        return Ok(false);
    }
    let store = TokenAccountStore::new();
    let data = match store.load_provider(provider) {
        Ok(data) => data,
        Err(error) if account_ref.is_some() => {
            return Err(error.into());
        }
        Err(_) => return Ok(false),
    };
    if data.accounts.is_empty() {
        if account_ref.is_some() {
            anyhow::bail!(
                "No token accounts configured for {}",
                provider.display_name()
            );
        }
        return Ok(false);
    }
    let account = if let Some(account_ref) = account_ref {
        find_token_account(&data, account_ref)?
    } else {
        data.active_account().ok_or_else(|| {
            anyhow::anyhow!("No active token account for {}", provider.display_name())
        })?
    }
    .clone();
    project_token_account(provider, &account, ctx);
    Ok(true)
}

pub(super) fn project_token_account(
    provider: ProviderId,
    account: &crate::core::TokenAccount,
    ctx: &mut crate::core::FetchContext,
) {
    let projected = TokenAccountOverride::from_account(provider, account.clone());
    let effective_source_mode = projected.effective_source_mode(ctx.source_mode);
    ctx.token_account_kind = Some(projected.kind);
    ctx.token_account_isolated = true;
    ctx.api_key = projected
        .env_override
        .as_ref()
        .and_then(|env| env.values().next().cloned());
    ctx.manual_cookie_header = projected.cookie_header;
    ctx.auto_prefer_web = projected.kind == TokenAccountKind::Cookie;

    if let Some(source_mode) = effective_source_mode {
        ctx.source_mode = source_mode;
    }
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
