//! Provider identity: the [`ProviderId`] enum and its registry lookups.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use super::spec::{ALL, ProviderMetadata, ProviderSpec, SPECS};

/// Unique identifier for a provider
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    Codex,
    Claude,
    Pi,
    Cursor,
    Factory,
    Gemini,
    Antigravity,
    Copilot,
    Zai,
    MiniMax,
    Kiro,
    VertexAI,
    Augment,
    OpenCode,
    Kimi,
    KimiK2,
    Amp,
    Warp,
    Ollama,
    AzureOpenAI,
    T3Chat,
    OpenRouter,
    JetBrains,
    Alibaba,
    AlibabaTokenPlan,
    NanoGPT,
    Infini,
    Perplexity,
    Abacus,
    Mistral,
    OpenCodeGo,
    Kilo,
    Bedrock,
    Codebuff,
    CodeRabbit,
    DeepSeek,
    DeepInfra,
    AiAnd,
    Windsurf,
    Manus,
    MiMo,
    Doubao,
    CommandCode,
    StepFun,
    Venice,
    OpenAIApi,
    Grok,
    ElevenLabs,
    Deepgram,
    Groq,
    HuggingFace,
    Helmcode,
    V0,
    TypeSafe,
    LLMProxy,
    Chutes,
    LiteLLM,
    Poe,
    Devin,
    Zed,
    CrossModel,
    Qoder,
    CodeBuddy,
    Sakana,
    Sub2Api,
    Wayfinder,
    ZenMux,
    ClinePass,
    LongCat,
    Neuralwatt,
    ZoomMate,
    QwenCloud,
    Notion,
    Xai,
    Fireworks,
    AtlasCloud,
    #[serde(alias = "metaspark")]
    Meta,
    Muse,
    Replicate,
    Nous,
    Hyper,
    GitKraken,
    Bifrost,
    Aixy,
    LLMMan,
    DevPass,
    XKiro,
    Raycast,
    Vercel,
}

impl ProviderId {
    /// Get all provider IDs
    pub fn all() -> &'static [ProviderId] {
        &ALL
    }

    /// The registry row for this provider.
    pub(crate) fn spec(self) -> &'static ProviderSpec {
        &SPECS[self as usize]
    }

    /// Static metadata for this provider.
    pub fn metadata(self) -> &'static ProviderMetadata {
        &self.spec().metadata
    }

    /// Get the CLI name for this provider
    pub fn cli_name(&self) -> &'static str {
        self.spec().cli_name
    }

    /// Get the display name for this provider
    pub fn display_name(&self) -> &'static str {
        self.spec().display_name
    }

    /// Get the cookie domain for this provider.
    /// Returns the domain used for cookie extraction, or None if the provider
    /// doesn't use cookies for authentication.
    pub fn cookie_domain(&self) -> Option<&'static str> {
        self.spec().cookie_domain
    }

    /// Id of the longer pool that blocks this provider's shorter windows once
    /// exhausted (upstream 0.69.0 #4091). See [`crate::core::BlockedWindows`].
    pub fn blocking_quota_window_id(&self) -> Option<&'static str> {
        match self {
            ProviderId::Kimi => Some(crate::providers::kimi::MONTHLY_WINDOW_ID),
            _ => None,
        }
    }

    /// Parse from CLI name string
    pub fn from_cli_name(name: &str) -> Option<Self> {
        let name = name.to_lowercase();
        SPECS
            .iter()
            .find(|spec| spec.accepts(&name))
            .map(|spec| spec.id)
    }

    /// Soft-removed providers (upstream #2254: Kimi K2 + CrossModel).
    ///
    /// Modules and CLI resolution stay so existing configs and `--provider kimik2`
    /// still work. Settings UI hides them unless already enabled in settings.
    pub fn is_deprecated(&self) -> bool {
        matches!(self, ProviderId::KimiK2 | ProviderId::CrossModel)
    }
}

impl std::fmt::Display for ProviderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.cli_name())
    }
}

/// Get the CLI name map for argument parsing
pub fn cli_name_map() -> HashMap<&'static str, ProviderId> {
    let mut map = HashMap::new();
    for spec in &SPECS {
        map.insert(spec.cli_name, spec.id);
        for alias in spec.aliases {
            map.insert(*alias, spec.id);
        }
    }
    map
}

/// The shipped brand color (hex) for a provider, mirroring the frontend
/// `PROVIDER_ICON_REGISTRY` in `providerIcons.ts` and the `--chart-<id>`
/// tokens in `styles.css` (`providerIcons.test.ts` checks both). Used as the
/// default accent color before any per-provider override (#2972).
///
/// Upstream 0.70.0 audited the palette (#4075, `docs/provider-palette.md`).
/// The 16 accents it adopted are pinned in the tests below; changing one
/// must not materially reduce contrast on white or `#222222`.
pub fn brand_color(id: ProviderId) -> &'static str {
    id.spec().brand_color
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::instantiate_provider;

    #[test]
    fn test_provider_id_all() {
        let all = ProviderId::all();
        assert_eq!(all.len(), 89);
        assert!(all.contains(&ProviderId::Claude));
        assert!(all.contains(&ProviderId::Codex));
        assert!(all.contains(&ProviderId::Pi));
        assert!(all.contains(&ProviderId::Fireworks));
        assert!(all.contains(&ProviderId::Kimi));
        assert!(all.contains(&ProviderId::KimiK2));
        assert!(all.contains(&ProviderId::Amp));
        assert!(all.contains(&ProviderId::AzureOpenAI));
        assert!(all.contains(&ProviderId::T3Chat));
        assert!(all.contains(&ProviderId::JetBrains));
        assert!(all.contains(&ProviderId::AlibabaTokenPlan));
        assert!(all.contains(&ProviderId::NanoGPT));
        assert!(all.contains(&ProviderId::Infini));
        assert!(all.contains(&ProviderId::Bedrock));
        assert!(all.contains(&ProviderId::Codebuff));
        assert!(all.contains(&ProviderId::CodeRabbit));
        assert!(all.contains(&ProviderId::DeepSeek));
        assert!(all.contains(&ProviderId::DeepInfra));
        assert!(all.contains(&ProviderId::AiAnd));
        assert!(all.contains(&ProviderId::Windsurf));
        assert!(all.contains(&ProviderId::Manus));
        assert!(all.contains(&ProviderId::MiMo));
        assert!(all.contains(&ProviderId::Doubao));
        assert!(all.contains(&ProviderId::CommandCode));
        assert!(all.contains(&ProviderId::StepFun));
        assert!(all.contains(&ProviderId::Venice));
        assert!(all.contains(&ProviderId::OpenAIApi));
        assert!(all.contains(&ProviderId::Grok));
        assert!(all.contains(&ProviderId::ElevenLabs));
        assert!(all.contains(&ProviderId::Deepgram));
        assert!(all.contains(&ProviderId::Groq));
        assert!(all.contains(&ProviderId::HuggingFace));
        assert!(all.contains(&ProviderId::Helmcode));
        assert!(all.contains(&ProviderId::V0));
        assert!(all.contains(&ProviderId::TypeSafe));
        assert!(all.contains(&ProviderId::LLMProxy));
        assert!(all.contains(&ProviderId::Chutes));
        assert!(all.contains(&ProviderId::LiteLLM));
        assert!(all.contains(&ProviderId::Poe));
        assert!(all.contains(&ProviderId::Devin));
        assert!(all.contains(&ProviderId::Zed));
        assert!(all.contains(&ProviderId::CrossModel));
        assert!(all.contains(&ProviderId::Qoder));
        assert!(all.contains(&ProviderId::CodeBuddy));
        assert!(all.contains(&ProviderId::Sakana));
        assert!(all.contains(&ProviderId::Sub2Api));
        assert!(all.contains(&ProviderId::Wayfinder));
        assert!(all.contains(&ProviderId::ZenMux));
        assert!(all.contains(&ProviderId::ClinePass));
        assert!(all.contains(&ProviderId::LongCat));
        assert!(all.contains(&ProviderId::Neuralwatt));
        assert!(all.contains(&ProviderId::ZoomMate));
        assert!(all.contains(&ProviderId::QwenCloud));
        assert!(all.contains(&ProviderId::Notion));
        assert!(all.contains(&ProviderId::Xai));
        assert!(all.contains(&ProviderId::Meta));
        assert!(all.contains(&ProviderId::Replicate));
        assert!(all.contains(&ProviderId::Muse));
        assert!(all.contains(&ProviderId::Nous));
        assert!(all.contains(&ProviderId::AtlasCloud));
        assert!(all.contains(&ProviderId::Hyper));
        assert!(all.contains(&ProviderId::GitKraken));
        assert!(all.contains(&ProviderId::Bifrost));
        assert!(all.contains(&ProviderId::Aixy));
        assert!(all.contains(&ProviderId::LLMMan));
        assert!(all.contains(&ProviderId::DevPass));
        assert!(all.contains(&ProviderId::XKiro));
        assert!(all.contains(&ProviderId::Raycast));
        assert!(all.contains(&ProviderId::Vercel));
    }

    #[test]
    fn deprecated_providers_are_kimik2_and_crossmodel() {
        assert!(ProviderId::KimiK2.is_deprecated());
        assert!(ProviderId::CrossModel.is_deprecated());
        assert!(!ProviderId::Kimi.is_deprecated());
        assert!(!ProviderId::AiAnd.is_deprecated());
        assert!(ProviderId::KimiK2.display_name().contains("(removed)"));
        assert!(ProviderId::CrossModel.display_name().contains("(removed)"));
        assert_eq!(
            ProviderId::from_cli_name("kimik2"),
            Some(ProviderId::KimiK2)
        );
        assert_eq!(
            ProviderId::from_cli_name("crossmodel"),
            Some(ProviderId::CrossModel)
        );
    }

    #[test]
    fn aiand_cli_aliases_resolve() {
        assert_eq!(ProviderId::from_cli_name("aiand"), Some(ProviderId::AiAnd));
        assert_eq!(ProviderId::from_cli_name("ai&"), Some(ProviderId::AiAnd));
        assert_eq!(ProviderId::from_cli_name("ai-and"), Some(ProviderId::AiAnd));
        assert_eq!(ProviderId::AiAnd.cli_name(), "aiand");
        assert_eq!(ProviderId::AiAnd.display_name(), "ai&");
    }

    #[test]
    fn test_provider_id_cli_name() {
        assert_eq!(ProviderId::Claude.cli_name(), "claude");
        assert_eq!(ProviderId::Codex.cli_name(), "codex");
        assert_eq!(ProviderId::Factory.cli_name(), "factory");
        assert_eq!(ProviderId::Zai.cli_name(), "zai");
        assert_eq!(ProviderId::HuggingFace.cli_name(), "huggingface");
        assert_eq!(ProviderId::CodeRabbit.cli_name(), "coderabbit");
    }

    #[test]
    fn test_provider_id_display_name() {
        assert_eq!(ProviderId::Claude.display_name(), "Claude");
        assert_eq!(ProviderId::Factory.display_name(), "Factory");
        assert_eq!(ProviderId::Zai.display_name(), "z.ai");
        assert_eq!(ProviderId::HuggingFace.display_name(), "Hugging Face");
        assert_eq!(ProviderId::CodeRabbit.display_name(), "CodeRabbit");
    }

    #[test]
    fn test_provider_id_from_cli_name() {
        assert_eq!(
            ProviderId::from_cli_name("claude"),
            Some(ProviderId::Claude)
        );
        assert_eq!(
            ProviderId::from_cli_name("anthropic"),
            Some(ProviderId::Claude)
        );
        assert_eq!(
            ProviderId::from_cli_name("CLAUDE"),
            Some(ProviderId::Claude)
        );
        assert_eq!(ProviderId::from_cli_name("codex"), Some(ProviderId::Codex));
        assert_eq!(
            ProviderId::from_cli_name("hf"),
            Some(ProviderId::HuggingFace)
        );
        assert_eq!(ProviderId::from_cli_name("openai"), Some(ProviderId::Codex));
        assert_eq!(
            ProviderId::from_cli_name("factory"),
            Some(ProviderId::Factory)
        );
        assert_eq!(
            ProviderId::from_cli_name("agy"),
            Some(ProviderId::Antigravity)
        );
        assert_eq!(ProviderId::from_cli_name("zed"), Some(ProviderId::Zed));
        assert_eq!(ProviderId::from_cli_name("crof"), None);
        assert_eq!(ProviderId::from_cli_name("unknown"), None);
        assert_eq!(
            ProviderId::from_cli_name("code-rabbit"),
            Some(ProviderId::CodeRabbit)
        );
    }

    #[test]
    fn test_provider_id_from_display_name_aliases() {
        for provider_id in ProviderId::all() {
            assert_eq!(
                ProviderId::from_cli_name(provider_id.display_name()),
                Some(*provider_id),
                "display name should round-trip for {}",
                provider_id.display_name()
            );
        }
    }

    #[test]
    fn test_provider_id_display() {
        assert_eq!(format!("{}", ProviderId::Claude), "claude");
        assert_eq!(format!("{}", ProviderId::Codex), "codex");
    }

    #[test]
    fn test_cli_name_map() {
        let map = cli_name_map();
        assert_eq!(map.get("claude"), Some(&ProviderId::Claude));
        assert_eq!(map.get("anthropic"), Some(&ProviderId::Claude));
        assert_eq!(map.get("codex"), Some(&ProviderId::Codex));
        assert_eq!(map.get("openai"), Some(&ProviderId::Codex));
        assert_eq!(map.get("agy"), Some(&ProviderId::Antigravity));
    }

    #[test]
    fn test_provider_id_cookie_domain() {
        // Cookie-based providers
        assert_eq!(ProviderId::Claude.cookie_domain(), Some("claude.ai"));
        assert_eq!(ProviderId::Cursor.cookie_domain(), Some("cursor.com"));
        assert_eq!(ProviderId::Factory.cookie_domain(), Some("app.factory.ai"));
        assert_eq!(ProviderId::Codex.cookie_domain(), Some("chatgpt.com"));
        assert_eq!(
            ProviderId::Gemini.cookie_domain(),
            Some("aistudio.google.com")
        );
        assert_eq!(ProviderId::Kiro.cookie_domain(), Some("kiro.dev"));
        assert_eq!(ProviderId::Zed.cookie_domain(), Some("zed.dev"));
        assert_eq!(ProviderId::Kimi.cookie_domain(), Some("kimi.moonshot.cn"));
        assert_eq!(ProviderId::OpenCode.cookie_domain(), Some("opencode.ai"));
        assert_eq!(ProviderId::Venice.cookie_domain(), Some("venice.ai"));
        assert_eq!(ProviderId::Groq.cookie_domain(), Some("groq.com"));

        // Token-based providers (no cookies)
        assert_eq!(ProviderId::Copilot.cookie_domain(), None);
        assert_eq!(ProviderId::Zai.cookie_domain(), None);
        assert_eq!(ProviderId::VertexAI.cookie_domain(), None);
        assert_eq!(ProviderId::JetBrains.cookie_domain(), None);
        assert_eq!(ProviderId::HuggingFace.cookie_domain(), None);
        assert_eq!(ProviderId::CodeRabbit.cookie_domain(), None);
    }

    #[test]
    fn test_provider_id_alibaba() {
        assert_eq!(ProviderId::Alibaba.cli_name(), "alibaba");
        assert_eq!(ProviderId::Alibaba.display_name(), "Alibaba");
        assert_eq!(
            ProviderId::Alibaba.cookie_domain(),
            Some("modelstudio.console.alibabacloud.com")
        );
        assert_eq!(
            ProviderId::from_cli_name("alibaba"),
            Some(ProviderId::Alibaba)
        );
        assert_eq!(
            ProviderId::from_cli_name("tongyi"),
            Some(ProviderId::Alibaba)
        );
        assert_eq!(
            ProviderId::from_cli_name("qianwen"),
            Some(ProviderId::Alibaba)
        );
    }

    #[test]
    fn test_provider_id_qwen_cloud() {
        assert_eq!(ProviderId::QwenCloud.cli_name(), "qwen-cloud");
        assert_eq!(ProviderId::QwenCloud.display_name(), "Qwen Cloud");
        assert_eq!(ProviderId::QwenCloud.cookie_domain(), Some("qwencloud.com"));
        assert_eq!(
            ProviderId::from_cli_name("qwen-cloud"),
            Some(ProviderId::QwenCloud)
        );
        assert_eq!(
            ProviderId::from_cli_name("qwencloud"),
            Some(ProviderId::QwenCloud)
        );
        assert_eq!(
            ProviderId::from_cli_name("qwen"),
            Some(ProviderId::QwenCloud)
        );
        assert_eq!(
            ProviderId::from_cli_name("qwen-token-plan"),
            Some(ProviderId::QwenCloud)
        );
        assert_eq!(
            ProviderId::from_cli_name("qwen cloud"),
            Some(ProviderId::QwenCloud)
        );
        // Bare "qwen" must not resolve to Alibaba Coding Plan.
        assert_ne!(ProviderId::from_cli_name("qwen"), Some(ProviderId::Alibaba));
    }

    #[test]
    fn test_provider_id_notion() {
        assert_eq!(ProviderId::Notion.cli_name(), "notion");
        assert_eq!(ProviderId::Notion.display_name(), "Notion AI");
        assert_eq!(ProviderId::Notion.cookie_domain(), Some("app.notion.com"));
        assert_eq!(
            ProviderId::from_cli_name("notion"),
            Some(ProviderId::Notion)
        );
        assert_eq!(
            ProviderId::from_cli_name("notion-ai"),
            Some(ProviderId::Notion)
        );
        assert_eq!(
            ProviderId::from_cli_name("notionai"),
            Some(ProviderId::Notion)
        );
        assert_eq!(
            ProviderId::from_cli_name("notion ai"),
            Some(ProviderId::Notion)
        );
    }

    #[test]
    fn test_provider_id_meta() {
        assert_eq!(ProviderId::Meta.cli_name(), "meta");
        assert_eq!(ProviderId::Meta.display_name(), "Meta");
        assert_eq!(ProviderId::Meta.cookie_domain(), None);
        assert_eq!(ProviderId::from_cli_name("meta"), Some(ProviderId::Meta));
        assert_eq!(
            ProviderId::from_cli_name("metaspark"),
            Some(ProviderId::Meta)
        );
        assert_eq!(
            ProviderId::from_cli_name("meta-spark"),
            Some(ProviderId::Meta)
        );
        assert_eq!(ProviderId::from_cli_name("meta"), Some(ProviderId::Meta));
        assert_eq!(
            ProviderId::from_cli_name("muse-spark"),
            Some(ProviderId::Meta)
        );
        assert_eq!(
            ProviderId::from_cli_name("musespark"),
            Some(ProviderId::Meta)
        );
        // Display name round-trips (also covered by the generic alias test).
        assert_eq!(ProviderId::from_cli_name("Meta"), Some(ProviderId::Meta));
        // Backwards-compat aliases still resolve.
        assert_eq!(
            ProviderId::from_cli_name("Meta Muse Spark"),
            Some(ProviderId::Meta)
        );
        assert_eq!(brand_color(ProviderId::Meta), "#0467DF");
    }

    #[test]
    fn test_provider_id_muse() {
        assert_eq!(ProviderId::Muse.cli_name(), "muse");
        assert_eq!(ProviderId::Muse.display_name(), "Muse Code");
        assert_eq!(ProviderId::Muse.cookie_domain(), Some("dev.meta.ai"));
        assert_eq!(ProviderId::from_cli_name("muse"), Some(ProviderId::Muse));
        assert_eq!(
            ProviderId::from_cli_name("muse-code"),
            Some(ProviderId::Muse)
        );
        assert_eq!(brand_color(ProviderId::Muse), "#0668E1");
    }

    // The "muse *" alias family spans two providers: bare `muse` / `muse code`
    // are the Muse Code CLI, while `muse spark` belongs to Meta (Meta Muse
    // Spark). Pin the boundary so a future alias edit cannot silently re-route
    // either side (review finding: latent UX/triage trap).
    #[test]
    fn muse_alias_family_boundary() {
        assert_eq!(
            ProviderId::from_cli_name("muse code"),
            Some(ProviderId::Muse)
        );
        assert_eq!(ProviderId::from_cli_name("muse"), Some(ProviderId::Muse));
        assert_eq!(
            ProviderId::from_cli_name("muse spark"),
            Some(ProviderId::Meta)
        );
        assert_eq!(
            ProviderId::from_cli_name("muse-spark"),
            Some(ProviderId::Meta)
        );
        assert_eq!(
            ProviderId::from_cli_name("musespark"),
            Some(ProviderId::Meta)
        );
        assert_eq!(
            ProviderId::from_cli_name("meta muse spark"),
            Some(ProviderId::Meta)
        );
    }

    #[test]
    fn test_provider_id_xai() {
        assert_eq!(ProviderId::Xai.cli_name(), "xai");
        assert_eq!(ProviderId::Xai.display_name(), "xAI");
        assert_eq!(ProviderId::Xai.cookie_domain(), None);
        assert_eq!(ProviderId::from_cli_name("xai"), Some(ProviderId::Xai));
        assert_eq!(ProviderId::from_cli_name("x.ai"), Some(ProviderId::Xai));
        assert_eq!(ProviderId::from_cli_name("x-ai"), Some(ProviderId::Xai));
        // Grok keeps consumer aliases; xai is the developer-platform provider.
        assert_eq!(ProviderId::from_cli_name("grok"), Some(ProviderId::Grok));
        assert_eq!(
            ProviderId::from_cli_name("supergrok"),
            Some(ProviderId::Grok)
        );
    }

    /// The 16 accents upstream 0.70.0 adopted in its palette audit (#4075),
    /// each with the accent Windows shipped before this port.
    const ADOPTED_ACCENTS: [(ProviderId, &str, &str); 16] = [
        (ProviderId::Abacus, "#7C3AED", "#814EE8"),
        (ProviderId::Amp, "#DC2626", "#F34E3F"),
        (ProviderId::Augment, "#6366F1", "#1AA049"),
        (ProviderId::Bedrock, "#FF9900", "#01A88D"),
        (ProviderId::ClinePass, "#61A3FA", "#5487C8"),
        (ProviderId::Codebuff, "#44FF00", "#00FF95"),
        (ProviderId::CommandCode, "#44FF00", "#8C4EDD"),
        (ProviderId::Cursor, "#00BFA5", "#F54E00"),
        (ProviderId::DeepSeek, "#527DF0", "#4D6BFE"),
        (ProviderId::Devin, "#111827", "#317CFF"),
        (ProviderId::Kiro, "#FF9900", "#9046FF"),
        (ProviderId::LongCat, "#FFD100", "#29E154"),
        (ProviderId::Mistral, "#FF500F", "#FF5229"),
        (ProviderId::Neuralwatt, "#38D98C", "#D55934"),
        (ProviderId::Sub2Api, "#2DC6D8", "#14B8A6"),
        (ProviderId::Venice, "#111827", "#3C8FDD"),
    ];

    /// Audited providers whose accent upstream kept, where the Windows accent
    /// already equals upstream's final value. Chutes, Deepgram, Doubao, Groq,
    /// Kilo, LiteLLM, Perplexity, Qoder, Sakana, T3 Chat and Warp keep older
    /// Windows accents that differ from upstream; aligning them is out of
    /// scope for the 0.70.0 port.
    const RETAINED_ACCENTS: [(ProviderId, &str); 7] = [
        (ProviderId::AiAnd, "#E25C2B"),
        (ProviderId::Copilot, "#A855F7"),
        (ProviderId::Fireworks, "#F25B1C"),
        (ProviderId::JetBrains, "#FF3399"),
        (ProviderId::Kimi, "#FE603C"),
        (ProviderId::Notion, "#337EA9"),
        (ProviderId::OpenCode, "#3B82F6"),
    ];

    /// WCAG relative luminance of a `#RRGGBB` color, as upstream's
    /// `ProviderPaletteRegressionTests` computes it.
    fn relative_luminance(hex: &str) -> f64 {
        let digits = hex.strip_prefix('#').expect("hex color starts with #");
        assert_eq!(digits.len(), 6, "{hex} is not #RRGGBB");
        let linear = |offset: usize| {
            let channel =
                f64::from(u8::from_str_radix(&digits[offset..offset + 2], 16).expect("hex digits"))
                    / 255.0;
            if channel <= 0.04045 {
                channel / 12.92
            } else {
                ((channel + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * linear(0) + 0.7152 * linear(2) + 0.0722 * linear(4)
    }

    fn contrast_ratio(color: &str, background: &str) -> f64 {
        let foreground = relative_luminance(color);
        let backdrop = relative_luminance(background);
        (foreground.max(backdrop) + 0.05) / (foreground.min(backdrop) + 0.05)
    }

    #[test]
    fn contrast_ratio_matches_upstream_audit_values() {
        // Rows of upstream docs/provider-palette.md, rounded to two decimals.
        let rounded = |value: f64| (value * 100.0).round() / 100.0;
        assert_eq!(rounded(contrast_ratio("#814EE8", "#FFFFFF")), 5.01);
        assert_eq!(rounded(contrast_ratio("#814EE8", "#222222")), 3.17);
        assert_eq!(rounded(contrast_ratio("#FF9900", "#FFFFFF")), 2.14);
        assert_eq!(rounded(contrast_ratio("#01A88D", "#222222")), 5.29);
        assert_eq!(rounded(contrast_ratio("#00FF95", "#FFFFFF")), 1.33);
        assert_eq!(rounded(contrast_ratio("#FFFFFF", "#FFFFFF")), 1.0);
    }

    #[test]
    fn adopted_upstream_palette_accents_are_pinned() {
        for (id, _, adopted) in ADOPTED_ACCENTS {
            assert_eq!(brand_color(id), adopted, "{id:?}");
        }
        for (id, retained) in RETAINED_ACCENTS {
            assert_eq!(brand_color(id), retained, "{id:?}");
        }
    }

    #[test]
    fn adopted_palette_accents_do_not_materially_regress_contrast() {
        // Upstream's controlled light and dark menu surfaces. A material
        // regression falls below 3:1 while losing at least 0.5 of contrast
        // against the accent Windows shipped before.
        for (id, previous, _) in ADOPTED_ACCENTS {
            for background in ["#FFFFFF", "#222222"] {
                let current = contrast_ratio(brand_color(id), background);
                let before = contrast_ratio(previous, background);
                assert!(
                    current >= 3.0 || before - current < 0.5,
                    "{id:?} on {background}: {before:.2} -> {current:.2}"
                );
            }
        }
    }

    #[test]
    fn brand_colors_are_uppercase_hex() {
        for id in ProviderId::all() {
            let color = brand_color(*id);
            assert!(
                color.len() == 7
                    && color.starts_with('#')
                    && color[1..]
                        .chars()
                        .all(|c| c.is_ascii_digit() || matches!(c, 'A'..='F')),
                "{id:?} has {color}"
            );
        }
    }

    #[test]
    fn test_provider_id_xkiro() {
        assert_eq!(ProviderId::XKiro.cli_name(), "xkiro");
        assert_eq!(ProviderId::XKiro.display_name(), "xKiro");
        assert_eq!(ProviderId::XKiro.cookie_domain(), None);
        assert_eq!(ProviderId::from_cli_name("xkiro"), Some(ProviderId::XKiro));
        assert_eq!(ProviderId::from_cli_name("x-kiro"), Some(ProviderId::XKiro));
        assert_eq!(cli_name_map().get("xkiro"), Some(&ProviderId::XKiro));
        assert_eq!(cli_name_map().get("x-kiro"), Some(&ProviderId::XKiro));
        // xKiro is a separate provider from Kiro and shares none of its aliases.
        assert_eq!(ProviderId::from_cli_name("kiro"), Some(ProviderId::Kiro));
        assert_eq!(ProviderId::from_cli_name("aws"), Some(ProviderId::Kiro));
    }

    #[test]
    fn alibaba_cookie_domain_matches_its_default_region() {
        assert_eq!(
            ProviderId::Alibaba.cookie_domain(),
            Some(crate::providers::AlibabaRegion::Singapore.primary_cookie_domain())
        );
    }

    #[test]
    fn instantiated_provider_metadata_matches_the_registry() {
        for &id in ProviderId::all() {
            let provider = instantiate_provider(id);
            assert_eq!(provider.id(), id);
            assert_eq!(
                format!("{:?}", provider.metadata()),
                format!("{:?}", id.metadata()),
                "{id:?}"
            );
        }
    }
}
