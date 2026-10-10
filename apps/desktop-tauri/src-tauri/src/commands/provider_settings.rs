use super::*;

// ── Provider summaries + ordering ─────────────────────────────────────

/// Lightweight provider entry returned to the UI after a reorder.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSummary {
    pub id: String,
    pub display_name: String,
    pub enabled: bool,
    pub order: u32,
}

/// Build `ProviderSummary` list honouring the persisted `provider_order`.
pub(crate) fn build_provider_summaries(settings: &Settings) -> Vec<ProviderSummary> {
    let order = settings.provider_display_order_names();

    let by_id: std::collections::HashMap<String, &ProviderId> = ProviderId::all()
        .iter()
        .map(|p| (p.cli_name().to_string(), p))
        .collect();

    order
        .iter()
        .filter_map(|id| {
            by_id.get(id).and_then(|p| {
                // Soft-removed providers (upstream #2254) stay hidden unless already enabled.
                if !settings.is_provider_listed(**p) {
                    return None;
                }
                let enabled = settings.is_provider_enabled(**p);
                Some(ProviderSummary {
                    id: id.clone(),
                    display_name: p.display_name().to_string(),
                    enabled,
                    // `order` is assigned below, over the emitted (post-filter) list,
                    // so deprecated gaps never leave holes in the display indices.
                    order: 0,
                })
            })
        })
        .enumerate()
        .map(|(idx, mut s)| {
            s.order = idx as u32;
            s
        })
        .collect()
}

#[tauri::command]
pub fn reorder_providers(
    app: tauri::AppHandle,
    ids: Vec<String>,
) -> Result<Vec<ProviderSummary>, String> {
    let mut settings = Settings::load();
    settings.provider_order = codexbar::settings::normalize_provider_order(&ids);
    settings.save().map_err(|e| e.to_string())?;
    crate::tray_bridge::refresh_tray_presentation(&app);
    // Notify open surfaces (tray flyout, pop-out window) so their provider grid
    // and cards re-render in the new order immediately after a drag-reorder.
    crate::events::emit_settings_changed(&app);
    Ok(build_provider_summaries(&settings))
}

// ── Per-provider usage source ─────────────────────────────────────────

pub(crate) fn provider_usage_source_lookup(
    settings: &Settings,
    provider_id: &str,
) -> Option<String> {
    parse_provider_arg(provider_id)
        .ok()
        .map(|id| settings.usage_source(id).to_string())
}

#[tauri::command]
pub fn set_provider_usage_source(provider_id: String, source: String) -> Result<(), String> {
    let id = parse_provider_arg(&provider_id)?;
    let mode = SourceMode::parse(source.trim())
        .ok_or_else(|| format!("Invalid usage source '{source}' for provider '{provider_id}'"))?;
    let provider = instantiate_provider(id);
    if !provider.available_sources().contains(&mode) {
        return Err(format!(
            "Usage source '{source}' is unavailable for provider '{provider_id}'"
        ));
    }
    let value = match mode {
        SourceMode::Auto => "auto",
        SourceMode::Cli => "cli",
        SourceMode::OAuth => "oauth",
        SourceMode::Web => "web",
    };
    let mut settings = Settings::load();
    settings.set_usage_source(id, value);
    settings.save().map_err(|e| e.to_string())
}

fn auto_resume_provider(provider_id: &str) -> Result<ProviderId, String> {
    let id = parse_provider_arg(provider_id)?;
    if crate::auto_resume::supports_auto_resume(id) {
        Ok(id)
    } else {
        Err(format!(
            "Provider '{provider_id}' does not support automatic session resume"
        ))
    }
}

/// Persist the explicit Codex/Claude opt-in for reopening an exact CLI session
/// after its quota becomes available again.
#[tauri::command]
pub fn set_provider_auto_resume_after_quota_reset(
    app: tauri::AppHandle,
    provider_id: String,
    enabled: bool,
) -> Result<(), String> {
    let id = auto_resume_provider(&provider_id)?;
    if enabled && !super::provider_detail::auto_resume_supported(id) {
        return Err(
            "Automatic session resume is unavailable while a managed token account is active"
                .to_string(),
        );
    }
    let mut settings = Settings::load();
    settings.set_auto_resume_after_quota_reset(id, enabled);
    settings.save().map_err(|e| e.to_string())?;
    if !enabled {
        crate::auto_resume::clear(&app, id);
    }
    crate::events::emit_settings_changed(&app);
    Ok(())
}

/// Persist the opt-in for a provider's optional extra breakdown (LiteLLM model
/// activity, Claude workspace spend). Takes effect on the next refresh.
#[tauri::command]
pub fn set_provider_optional_details(
    app: tauri::AppHandle,
    provider_id: String,
    enabled: bool,
) -> Result<(), String> {
    let id = parse_provider_arg(&provider_id)?;
    if !codexbar::settings::provider_has_optional_details(id) {
        return Err(format!(
            "Provider '{provider_id}' has no optional detail breakdown"
        ));
    }
    let mut settings = Settings::load();
    settings.set_optional_details_enabled(id, enabled);
    settings.save().map_err(|e| e.to_string())?;
    crate::events::emit_settings_changed(&app);
    Ok(())
}

// ── OpenRouter Management API key ────────────────────────────────────

#[tauri::command]
pub fn has_openrouter_management_api_key() -> bool {
    Settings::load()
        .management_api_token(ProviderId::OpenRouter)
        .is_some()
}

#[tauri::command]
pub fn set_openrouter_management_api_key(api_key: String) -> Result<(), String> {
    let trimmed = api_key.trim();
    if trimmed.is_empty() {
        return Err("Management API key must not be empty".to_string());
    }
    validate_single_line_secret(trimmed, "Management API key", MAX_API_KEY_LEN)?;
    let mut settings = Settings::load();
    settings.set_management_api_token(ProviderId::OpenRouter, Some(trimmed.to_string()));
    settings.save().map_err(|error| error.to_string())
}

#[tauri::command]
pub fn remove_openrouter_management_api_key() -> Result<(), String> {
    let mut settings = Settings::load();
    settings.set_management_api_token(ProviderId::OpenRouter, None);
    settings.save().map_err(|error| error.to_string())
}

// ── Azure OpenAI API version ─────────────────────────────────────────

fn azure_openai_provider(provider_id: &str) -> Result<codexbar::core::ProviderId, String> {
    let id = parse_provider_arg(provider_id)?;
    if id != codexbar::core::ProviderId::AzureOpenAI {
        return Err(format!(
            "Provider '{provider_id}' does not expose an Azure OpenAI API-version picker"
        ));
    }
    Ok(id)
}

#[tauri::command]
pub fn get_provider_azure_api_version(provider_id: String) -> Result<Option<String>, String> {
    let id = azure_openai_provider(&provider_id)?;
    Ok(ApiKeys::load()
        .api_version(id.cli_name())
        .map(ToOwned::to_owned))
}

#[tauri::command]
pub fn set_provider_azure_api_version(
    provider_id: String,
    api_version: String,
) -> Result<(), String> {
    let id = azure_openai_provider(&provider_id)?;
    let value = api_version.trim();
    if value.len() > 128 || value.chars().any(char::is_control) {
        return Err("Azure OpenAI API version is invalid".to_string());
    }
    let mut keys = ApiKeys::load();
    keys.set_api_version(
        id.cli_name(),
        (!value.is_empty()).then_some(value.to_string()),
    );
    keys.save().map_err(|error| error.to_string())
}

// ── Per-provider cookie source + region ───────────────────────────────

/// Map a CLI-name string to a `ProviderId` whose cookie source is exposed in
/// the UI. Returns `None` for providers without a user-facing cookie source.
fn cookie_source_provider(provider_id: &str) -> Option<codexbar::core::ProviderId> {
    use codexbar::core::ProviderId;
    Some(match provider_id {
        "codex" => ProviderId::Codex,
        "claude" => ProviderId::Claude,
        "cursor" => ProviderId::Cursor,
        "opencode" => ProviderId::OpenCode,
        "factory" => ProviderId::Factory,
        "alibaba" => ProviderId::Alibaba,
        "alibabatokenplan" => ProviderId::AlibabaTokenPlan,
        "kimi" | "kimik2" => ProviderId::Kimi,
        "minimax" => ProviderId::MiniMax,
        "augment" => ProviderId::Augment,
        "amp" => ProviderId::Amp,
        "ollama" => ProviderId::Ollama,
        "mistral" => ProviderId::Mistral,
        "qoder" => ProviderId::Qoder,
        "codebuddy" => ProviderId::CodeBuddy,
        "sakana" => ProviderId::Sakana,
        "notion" => ProviderId::Notion,
        "grok" => ProviderId::Grok,
        "muse" => ProviderId::Muse,
        "replicate" => ProviderId::Replicate,
        "raycast" => ProviderId::Raycast,
        "helmcode" => ProviderId::Helmcode,
        "typesafe" => ProviderId::TypeSafe,
        "hyper" => ProviderId::Hyper,
        "groq" => ProviderId::Groq,
        _ => return None,
    })
}

pub(crate) fn provider_cookie_source_lookup(
    settings: &Settings,
    provider_id: &str,
) -> Option<String> {
    cookie_source_provider(provider_id).map(|id| settings.cookie_source(id).to_string())
}

pub(crate) fn provider_cookie_source_set(
    settings: &mut Settings,
    provider_id: &str,
    source: String,
) -> Result<(), String> {
    let id = cookie_source_provider(provider_id)
        .ok_or_else(|| format!("Provider '{provider_id}' does not expose a cookie source"))?;
    settings.set_cookie_source(id, source);
    Ok(())
}

#[tauri::command]
pub fn set_provider_cookie_source(provider_id: String, source: String) -> Result<(), String> {
    let source = source.trim();
    if source.is_empty()
        || !cookie_source_options_for(&provider_id, Language::English)
            .iter()
            .any(|option| option.value == source)
    {
        return Err(format!(
            "Invalid cookie source '{source}' for provider '{provider_id}'"
        ));
    }
    let mut settings = Settings::load();
    provider_cookie_source_set(&mut settings, &provider_id, source.to_string())?;
    settings.save().map_err(|e| e.to_string())
}

fn region_provider(provider_id: &str) -> Option<codexbar::core::ProviderId> {
    use codexbar::core::ProviderId;
    Some(match provider_id {
        "alibaba" => ProviderId::Alibaba,
        "alibabatokenplan" => ProviderId::AlibabaTokenPlan,
        "zai" => ProviderId::Zai,
        "minimax" => ProviderId::MiniMax,
        "kimi" => ProviderId::Kimi,
        _ => return None,
    })
}

pub(crate) fn provider_region_lookup(settings: &Settings, provider_id: &str) -> Option<String> {
    region_provider(provider_id).map(|id| {
        if id == codexbar::core::ProviderId::MiniMax {
            codexbar::providers::MiniMaxProvider::region_from_settings(Some(
                settings.api_region(id),
            ))
            .settings_value()
            .to_string()
        } else if id == codexbar::core::ProviderId::Kimi {
            codexbar::providers::KimiRegion::from_settings(Some(settings.api_region(id)))
                .settings_value()
                .to_string()
        } else {
            settings.api_region(id).to_string()
        }
    })
}

pub(crate) fn provider_region_set(
    settings: &mut Settings,
    provider_id: &str,
    region: String,
) -> Result<(), String> {
    let id = region_provider(provider_id)
        .ok_or_else(|| format!("Provider '{provider_id}' does not have a region picker"))?;
    settings.set_api_region(id, region);
    Ok(())
}

#[tauri::command]
pub fn set_provider_region(provider_id: String, region: String) -> Result<(), String> {
    let region = region.trim();
    if region.is_empty()
        || !region_options_for(&provider_id, Language::English)
            .iter()
            .any(|option| option.value == region)
    {
        return Err(format!(
            "Invalid region '{region}' for provider '{provider_id}'"
        ));
    }
    let mut settings = Settings::load();
    provider_region_set(&mut settings, &provider_id, region.to_string())?;
    settings.save().map_err(|e| e.to_string())
}

fn workspace_provider(provider_id: &str) -> Option<codexbar::core::ProviderId> {
    use codexbar::core::ProviderId;
    Some(match provider_id {
        "openaiapi" => ProviderId::OpenAIApi,
        "litellm" => ProviderId::LiteLLM,
        "llmman" => ProviderId::LLMMan,
        "devin" => ProviderId::Devin,
        "opencodego" => ProviderId::OpenCodeGo,
        "zed" => ProviderId::Zed,
        "llmproxy" => ProviderId::LLMProxy,
        "xai" => ProviderId::Xai,
        "v0" => ProviderId::V0,
        "helmcode" => ProviderId::Helmcode,
        "gitkraken" => ProviderId::GitKraken,
        "muse" => ProviderId::Muse,
        _ => return None,
    })
}

#[tauri::command]
pub fn set_provider_workspace_id(provider_id: String, workspace_id: String) -> Result<(), String> {
    let id = workspace_provider(&provider_id).ok_or_else(|| {
        format!("Provider '{provider_id}' does not expose a workspace/project id")
    })?;
    let workspace_id = codexbar::settings::validate_provider_workspace_value(id, &workspace_id)?;
    let mut settings = Settings::load();
    prevent_litellm_key_retargeting(id, settings.workspace_id(id), &workspace_id)?;
    settings.set_workspace_id(id, workspace_id);
    settings.save().map_err(|e| e.to_string())
}

fn prevent_litellm_key_retargeting(
    id: codexbar::core::ProviderId,
    current_workspace_id: &str,
    next_workspace_id: &str,
) -> Result<(), String> {
    litellm_workspace_change_allowed(
        id,
        current_workspace_id,
        next_workspace_id,
        ApiKeys::load().has_key(id.cli_name()),
    )
}

fn litellm_workspace_change_allowed(
    id: codexbar::core::ProviderId,
    current_workspace_id: &str,
    next_workspace_id: &str,
    has_saved_api_key: bool,
) -> Result<(), String> {
    if id != codexbar::core::ProviderId::LiteLLM || next_workspace_id.trim().is_empty() {
        return Ok(());
    }

    let current = current_workspace_id.trim().trim_end_matches('/');
    let next = next_workspace_id.trim().trim_end_matches('/');
    if current.eq_ignore_ascii_case(next) || !has_saved_api_key {
        return Ok(());
    }

    Err(
        "Remove the saved LiteLLM API key before changing the LiteLLM base URL, then save the key again for the new endpoint."
            .to_string(),
    )
}

#[cfg(test)]
mod tests {
    use codexbar::core::ProviderId;

    use super::{gateway_provider, litellm_workspace_change_allowed, workspace_provider};

    #[test]
    fn muse_exposes_cookie_source_and_browser_team_settings() {
        assert_eq!(workspace_provider("muse"), Some(ProviderId::Muse));
        assert_eq!(
            super::cookie_source_provider("muse"),
            Some(ProviderId::Muse)
        );
        let values: Vec<String> =
            super::cookie_source_options_for("muse", codexbar::settings::Language::English)
                .into_iter()
                .map(|option| option.value)
                .collect();
        assert_eq!(values, ["auto", "manual", "off"]);
    }

    #[test]
    fn maps_opencode_go_workspace_provider() {
        assert_eq!(
            workspace_provider("opencodego"),
            Some(ProviderId::OpenCodeGo)
        );
    }

    #[test]
    fn fetch_context_carries_saved_gateway_urls_for_every_gateway_provider() {
        use codexbar::settings::{ApiKeys, ManualCookies, Settings};
        use std::collections::HashMap;

        let mut settings = Settings::default();
        for (id, url) in [
            (ProviderId::Wayfinder, "http://localhost:8787"),
            (ProviderId::Bifrost, "https://bifrost.example.com"),
            (ProviderId::Aixy, "https://aixy.example.com/prefix"),
        ] {
            settings.set_gateway_url(id, url);
            let ctx = super::super::providers::build_fetch_context(
                id,
                &settings,
                &ManualCookies::default(),
                &ApiKeys::default(),
                &HashMap::new(),
            );
            assert_eq!(ctx.gateway_url.as_deref(), Some(url), "{id:?}");
        }

        let ctx = super::super::providers::build_fetch_context(
            ProviderId::Codex,
            &settings,
            &ManualCookies::default(),
            &ApiKeys::default(),
            &HashMap::new(),
        );
        assert_eq!(ctx.gateway_url, None);
    }

    #[test]
    fn maps_gitkraken_organization_provider() {
        assert_eq!(workspace_provider("gitkraken"), Some(ProviderId::GitKraken));
    }

    #[test]
    fn gateway_provider_exposes_gateway_providers_only() {
        assert_eq!(gateway_provider("wayfinder"), Some(ProviderId::Wayfinder));
        assert_eq!(gateway_provider("bifrost"), Some(ProviderId::Bifrost));
        assert_eq!(gateway_provider("aixy"), Some(ProviderId::Aixy));
        assert_eq!(gateway_provider("codex"), None);
    }

    #[test]
    fn maps_llmman_workspace_provider() {
        assert_eq!(workspace_provider("llmman"), Some(ProviderId::LLMMan));
    }

    #[test]
    fn litellm_endpoint_change_requires_reentering_saved_key() {
        assert!(
            litellm_workspace_change_allowed(
                ProviderId::LiteLLM,
                "https://old.example.com",
                "https://new.example.com",
                true,
            )
            .is_err()
        );
        assert!(
            litellm_workspace_change_allowed(
                ProviderId::LiteLLM,
                "https://old.example.com",
                "https://old.example.com/",
                true,
            )
            .is_ok()
        );
        assert!(
            litellm_workspace_change_allowed(
                ProviderId::LiteLLM,
                "https://old.example.com",
                "",
                true,
            )
            .is_ok()
        );
        assert!(
            litellm_workspace_change_allowed(
                ProviderId::LiteLLM,
                "https://old.example.com",
                "https://new.example.com",
                false,
            )
            .is_ok()
        );
    }
}

#[tauri::command]
pub fn get_provider_workspace_id(provider_id: String) -> Result<Option<String>, String> {
    let Some(id) = workspace_provider(&provider_id) else {
        return Ok(None);
    };
    let value = Settings::load().workspace_id(id).trim().to_string();
    Ok((!value.is_empty()).then_some(value))
}

fn gateway_provider(provider_id: &str) -> Option<codexbar::core::ProviderId> {
    match provider_id {
        "wayfinder" => Some(codexbar::core::ProviderId::Wayfinder),
        "bifrost" => Some(codexbar::core::ProviderId::Bifrost),
        "aixy" => Some(codexbar::core::ProviderId::Aixy),
        _ => None,
    }
}

#[tauri::command]
pub fn get_provider_gateway_url(provider_id: String) -> Result<String, String> {
    let id = gateway_provider(&provider_id)
        .ok_or_else(|| format!("Provider '{provider_id}' does not expose a gateway URL"))?;
    Ok(Settings::load().gateway_url(id).to_string())
}

#[tauri::command]
pub fn set_provider_gateway_url(provider_id: String, gateway_url: String) -> Result<(), String> {
    let id = gateway_provider(&provider_id)
        .ok_or_else(|| format!("Provider '{provider_id}' does not expose a gateway URL"))?;
    let gateway_url = gateway_url.trim();
    match id {
        codexbar::core::ProviderId::Wayfinder => {
            codexbar::providers::wayfinder::parse_gateway_url(gateway_url)
                .map_err(|error| error.to_string())?;
        }
        codexbar::core::ProviderId::Bifrost => {
            codexbar::providers::bifrost::validate_gateway_url(gateway_url)
                .map_err(|error| error.to_string())?;
        }
        codexbar::core::ProviderId::Aixy => {
            codexbar::providers::aixy::validate_gateway_url(gateway_url)
                .map_err(|error| error.to_string())?;
        }
        _ => unreachable!("gateway_provider only returns gateway providers"),
    }

    let mut settings = Settings::load();
    settings.set_gateway_url(id, gateway_url.to_string());
    settings.save().map_err(|error| error.to_string())
}

// ── Phase 6c — cookie source & region option catalogs ────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CookieSourceOption {
    pub value: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RegionOption {
    pub value: String,
    pub label: String,
}

fn cookie_option(
    lang: Language,
    value: &str,
    desc_key: Option<locale::LocaleKey>,
) -> CookieSourceOption {
    let label = match value {
        "auto" => locale::get_text(lang, locale::LocaleKey::Automatic),
        "manual" => locale::get_text(lang, locale::LocaleKey::CookieSourceManual),
        "off" => locale::get_text(lang, locale::LocaleKey::ProviderDisabled),
        other => other.to_string(),
    };
    CookieSourceOption {
        value: value.to_string(),
        label,
        description: desc_key
            .map(|key| locale::get_text(lang, key))
            .filter(|text| !text.is_empty()),
    }
}

/// Returns the catalog of cookie source options for a given provider,
/// mirroring the `egui` ComboBox choices in `preferences.rs`.
/// Empty vec means the provider does not expose a cookie-source picker.
pub fn cookie_source_options_for(provider_id: &str, lang: Language) -> Vec<CookieSourceOption> {
    match provider_id {
        "codex" => vec![
            cookie_option(
                lang,
                "auto",
                Some(locale::LocaleKey::ProviderCodexAutoImportHelp),
            ),
            cookie_option(
                lang,
                "manual",
                Some(locale::LocaleKey::CookieHelpCodexManual),
            ),
            cookie_option(lang, "off", Some(locale::LocaleKey::CookieHelpCodexOff)),
        ],
        "claude" => vec![
            cookie_option(
                lang,
                "auto",
                Some(locale::LocaleKey::ProviderClaudeCookiesHelp),
            ),
            cookie_option(
                lang,
                "manual",
                Some(locale::LocaleKey::ProviderClaudeCookiesHelp),
            ),
        ],
        "cursor" => vec![
            cookie_option(
                lang,
                "auto",
                Some(locale::LocaleKey::ProviderCursorCookieSourceHelp),
            ),
            cookie_option(
                lang,
                "manual",
                Some(locale::LocaleKey::CookieHelpCursorManual),
            ),
        ],
        "grok" => vec![
            cookie_option(lang, "auto", Some(locale::LocaleKey::CookieHelpGrokAuto)),
            cookie_option(
                lang,
                "manual",
                Some(locale::LocaleKey::CookieHelpGrokManual),
            ),
            cookie_option(lang, "off", Some(locale::LocaleKey::CookieHelpGrokOff)),
        ],
        "opencode" => vec![
            cookie_option(
                lang,
                "auto",
                Some(locale::LocaleKey::CookieHelpOpenCodeAuto),
            ),
            cookie_option(
                lang,
                "manual",
                Some(locale::LocaleKey::CookieHelpOpenCodeManual),
            ),
        ],
        "factory" => vec![
            cookie_option(lang, "auto", Some(locale::LocaleKey::CookieHelpFactoryAuto)),
            cookie_option(
                lang,
                "manual",
                Some(locale::LocaleKey::CookieHelpFactoryManual),
            ),
        ],
        "alibaba" | "alibabatokenplan" => vec![
            cookie_option(lang, "auto", Some(locale::LocaleKey::CookieHelpAlibabaAuto)),
            cookie_option(
                lang,
                "manual",
                Some(locale::LocaleKey::CookieHelpAlibabaManual),
            ),
        ],
        "kimi" | "kimik2" => vec![
            cookie_option(
                lang,
                "auto",
                Some(locale::LocaleKey::CookieHelpAutoBrowserCookies),
            ),
            cookie_option(
                lang,
                "manual",
                Some(locale::LocaleKey::CookieHelpKimiManual),
            ),
            cookie_option(lang, "off", Some(locale::LocaleKey::CookieHelpKimiOff)),
        ],
        "minimax" => vec![
            cookie_option(lang, "auto", Some(locale::LocaleKey::CookieHelpMiniMaxAuto)),
            cookie_option(
                lang,
                "manual",
                Some(locale::LocaleKey::CookieHelpMiniMaxManual),
            ),
        ],
        "augment" => vec![
            cookie_option(
                lang,
                "auto",
                Some(locale::LocaleKey::CookieHelpAutoBrowserCookies),
            ),
            cookie_option(
                lang,
                "manual",
                Some(locale::LocaleKey::CookieHelpAugmentManual),
            ),
        ],
        "amp" => vec![
            cookie_option(
                lang,
                "auto",
                Some(locale::LocaleKey::CookieHelpAutoBrowserCookies),
            ),
            cookie_option(lang, "manual", Some(locale::LocaleKey::CookieHelpAmpManual)),
        ],
        "ollama" => vec![
            cookie_option(
                lang,
                "auto",
                Some(locale::LocaleKey::CookieHelpAutoBrowserCookies),
            ),
            cookie_option(
                lang,
                "manual",
                Some(locale::LocaleKey::CookieHelpOllamaManual),
            ),
        ],
        "mistral" => vec![
            cookie_option(lang, "auto", Some(locale::LocaleKey::CookieHelpMistralAuto)),
            cookie_option(
                lang,
                "manual",
                Some(locale::LocaleKey::CookieHelpMistralManual),
            ),
        ],
        "notion" => vec![
            cookie_option(lang, "auto", Some(locale::LocaleKey::CookieHelpNotionAuto)),
            cookie_option(
                lang,
                "manual",
                Some(locale::LocaleKey::CookieHelpNotionManual),
            ),
            cookie_option(lang, "off", Some(locale::LocaleKey::CookieHelpNotionOff)),
        ],
        "muse" => vec![
            cookie_option(lang, "auto", Some(locale::LocaleKey::CookieHelpMuseAuto)),
            cookie_option(
                lang,
                "manual",
                Some(locale::LocaleKey::CookieHelpMuseManual),
            ),
            cookie_option(lang, "off", Some(locale::LocaleKey::CookieHelpMuseOff)),
        ],
        "replicate" => vec![
            cookie_option(
                lang,
                "auto",
                Some(locale::LocaleKey::CookieHelpReplicateAuto),
            ),
            cookie_option(
                lang,
                "manual",
                Some(locale::LocaleKey::CookieHelpReplicateManual),
            ),
        ],
        "raycast" => vec![
            cookie_option(
                lang,
                "auto",
                Some(locale::LocaleKey::ProviderRaycastAutoImportHelp),
            ),
            cookie_option(
                lang,
                "manual",
                Some(locale::LocaleKey::ProviderRaycastManualCookieHelp),
            ),
            cookie_option(
                lang,
                "off",
                Some(locale::LocaleKey::ProviderRaycastCookiesDisabled),
            ),
        ],
        "helmcode" => vec![
            cookie_option(
                lang,
                "auto",
                Some(locale::LocaleKey::CookieHelpHelmcodeAuto),
            ),
            cookie_option(
                lang,
                "manual",
                Some(locale::LocaleKey::CookieHelpHelmcodeManual),
            ),
        ],
        "typesafe" => vec![
            cookie_option(
                lang,
                "auto",
                Some(locale::LocaleKey::CookieHelpTypeSafeAuto),
            ),
            cookie_option(
                lang,
                "manual",
                Some(locale::LocaleKey::CookieHelpTypeSafeManual),
            ),
        ],
        // Upstream's Hyper picker; the session can come from any selected
        // browser here, not only Chrome.
        "hyper" => vec![
            cookie_option(lang, "auto", Some(locale::LocaleKey::CookieHelpHyperAuto)),
            cookie_option(
                lang,
                "manual",
                Some(locale::LocaleKey::CookieHelpHyperManual),
            ),
            cookie_option(lang, "off", Some(locale::LocaleKey::CookieHelpHyperOff)),
        ],
        // Upstream reads the console session from the browser only; manual
        // keeps a pasted `stytch_session` header for browsers whose cookies
        // Windows cannot decrypt.
        "groq" => vec![
            cookie_option(lang, "auto", None),
            cookie_option(lang, "manual", None),
            cookie_option(lang, "off", None),
        ],
        _ => Vec::new(),
    }
}

/// Returns the API region options for a given provider, labelled in `lang`.
/// Empty vec means the provider has no region picker.
pub fn region_options_for(provider_id: &str, lang: Language) -> Vec<RegionOption> {
    use codexbar::providers::{AlibabaRegion, AlibabaTokenPlanRegion, KimiRegion};
    use locale::LocaleKey as K;
    let option = |value: &str, key: K| RegionOption {
        value: value.to_string(),
        label: locale::get_text(lang, key),
    };
    match provider_id {
        "alibaba" => AlibabaRegion::ALL
            .iter()
            .map(|region| {
                let key = match region {
                    AlibabaRegion::Singapore => K::RegionAlibabaSingapore,
                    AlibabaRegion::UsEast => K::RegionAlibabaUsEast,
                    AlibabaRegion::Germany => K::RegionAlibabaGermany,
                    AlibabaRegion::HongKong => K::RegionAlibabaHongKong,
                    AlibabaRegion::ChinaMainland => K::RegionAlibabaChinaMainland,
                };
                option(region.settings_value(), key)
            })
            .collect(),
        "zai" => vec![
            option("global", K::RegionZaiGlobal),
            option("china", K::RegionZaiChinaMainland),
        ],
        "minimax" => vec![
            option("global", K::RegionMiniMaxGlobal),
            option("cn", K::RegionMiniMaxChinaMainland),
        ],
        "kimi" => KimiRegion::ALL
            .iter()
            .copied()
            .map(|region| {
                let key = match region {
                    KimiRegion::China => K::RegionKimiChina,
                    KimiRegion::International => K::RegionKimiInternational,
                };
                option(region.settings_value(), key)
            })
            .collect(),
        "alibabatokenplan" => AlibabaTokenPlanRegion::ALL
            .iter()
            .copied()
            .map(|region| {
                let key = match region {
                    AlibabaTokenPlanRegion::Cn => K::RegionAlibabaTokenPlanChinaTeam,
                    AlibabaTokenPlanRegion::Intl => K::RegionAlibabaTokenPlanInternationalTeam,
                    AlibabaTokenPlanRegion::CnPersonal => K::RegionAlibabaTokenPlanChinaPersonal,
                    AlibabaTokenPlanRegion::IntlPersonal => {
                        K::RegionAlibabaTokenPlanInternationalPersonal
                    }
                };
                option(region.as_str(), key)
            })
            .collect(),
        _ => Vec::new(),
    }
}

#[tauri::command]
pub fn get_provider_cookie_source_options(
    provider_id: String,
) -> Result<Vec<CookieSourceOption>, String> {
    let lang = Settings::load().ui_language;
    Ok(cookie_source_options_for(&provider_id, lang))
}

#[tauri::command]
pub fn get_provider_region_options(provider_id: String) -> Result<Vec<RegionOption>, String> {
    let lang = Settings::load().ui_language;
    Ok(region_options_for(&provider_id, lang))
}
