use crate::core::{FetchContext, ProviderId};
use crate::settings::Settings;

/// Fill a missing API region from the saved provider settings.
///
/// An explicitly supplied fetch-context value takes precedence over settings.
pub(crate) fn populate_api_region_from_settings(
    provider_id: ProviderId,
    settings: &Settings,
    ctx: &mut FetchContext,
) {
    if ctx.api_region.is_none() {
        let region = settings.api_region(provider_id);
        ctx.api_region = (!region.is_empty()).then(|| region.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::kimi::KimiRegion;

    fn fetch_context(api_region: Option<&str>) -> FetchContext {
        FetchContext {
            source_mode: crate::core::SourceMode::Auto,
            include_credits: true,
            web_timeout: 60,
            verbose: false,
            manual_cookie_header: None,
            manual_cookie_missing: false,
            api_key: None,
            token_account_kind: None,
            token_account_isolated: false,
            workspace_id: None,
            seat_credit_entitlement: None,
            api_region: api_region.map(str::to_string),
            gateway_url: None,
            auto_prefer_web: false,
            browser_cookie_import: false,
            requires_optional_usage_completeness: false,
            optional_details_enabled: false,
            live_usage_enabled: false,
        }
    }

    #[test]
    fn saved_kimi_international_region_populates_fetch_context() {
        let mut settings = Settings::default();
        settings.set_api_region(ProviderId::Kimi, "international");
        let mut ctx = fetch_context(None);

        populate_api_region_from_settings(ProviderId::Kimi, &settings, &mut ctx);

        assert_eq!(ctx.api_region.as_deref(), Some("international"));
    }

    #[test]
    fn empty_or_unset_saved_region_leaves_context_region_missing() {
        for configured_region in [None, Some("")] {
            let mut settings = Settings::default();
            if let Some(region) = configured_region {
                settings.set_api_region(ProviderId::Kimi, region);
            }
            let mut ctx = fetch_context(None);

            populate_api_region_from_settings(ProviderId::Kimi, &settings, &mut ctx);

            assert_eq!(ctx.api_region, None);
        }
    }

    #[test]
    fn explicit_fetch_context_region_is_preserved() {
        let mut settings = Settings::default();
        settings.set_api_region(ProviderId::Kimi, "international");
        let mut ctx = fetch_context(Some("caller-region"));

        populate_api_region_from_settings(ProviderId::Kimi, &settings, &mut ctx);

        assert_eq!(ctx.api_region.as_deref(), Some("caller-region"));
    }

    #[test]
    fn saved_international_region_selects_kimi_ai_domains() {
        let mut settings = Settings::default();
        settings.set_api_region(ProviderId::Kimi, "international");
        let mut ctx = fetch_context(None);

        populate_api_region_from_settings(ProviderId::Kimi, &settings, &mut ctx);

        let region = KimiRegion::from_settings(ctx.api_region.as_deref());
        assert_eq!(region, KimiRegion::International);
        assert_eq!(region.code_api_base_url(), "https://api.kimi.ai");
        assert_eq!(region.web_base_url(), "https://www.kimi.ai");
        assert_eq!(region.cookie_domains(), &["www.kimi.ai", "kimi.ai"][..]);
    }
}
