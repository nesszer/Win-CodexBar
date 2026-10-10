//! The provider registry: one row per [`ProviderId`], in declaration order,
//! holding its names, cookie domain, brand color, metadata and CLI aliases.

use super::ProviderId;
use super::ProviderId as P;

/// Metadata about a provider
#[derive(Debug, Clone)]
pub struct ProviderMetadata {
    pub id: ProviderId,
    pub display_name: &'static str,
    pub session_label: &'static str,
    pub weekly_label: &'static str,
    pub supports_opus: bool,
    pub supports_credits: bool,
    pub default_enabled: bool,
    pub is_primary: bool,
    pub dashboard_url: Option<&'static str>,
    pub status_page_url: Option<&'static str>,
    /// Locale key shown for the provider's tertiary metric lane in settings
    /// pickers when the lane carries a semantic identity beyond "Tertiary"
    /// (upstream F5). `None` renders the generic tertiary label.
    pub tertiary_label_key: Option<&'static str>,
}

/// Static registry row for one provider.
#[derive(Debug)]
pub(crate) struct ProviderSpec {
    pub(crate) id: ProviderId,
    pub(crate) cli_name: &'static str,
    pub(crate) display_name: &'static str,
    pub(crate) cookie_domain: Option<&'static str>,
    pub(crate) brand_color: &'static str,
    pub(crate) metadata: ProviderMetadata,
    /// Accepted by `from_cli_name` and listed in `cli_name_map`.
    pub(crate) aliases: &'static [&'static str],
    /// Accepted by `from_cli_name` only.
    pub(crate) parse_aliases: &'static [&'static str],
}

impl ProviderSpec {
    /// Whether an already-lowercased CLI argument names this provider.
    pub(crate) fn accepts(&self, name: &str) -> bool {
        self.cli_name == name || self.aliases.contains(&name) || self.parse_aliases.contains(&name)
    }

    const fn labels(mut self, session: &'static str, weekly: &'static str) -> Self {
        self.metadata.session_label = session;
        self.metadata.weekly_label = weekly;
        self
    }

    const fn cookie(mut self, domain: &'static str) -> Self {
        self.cookie_domain = Some(domain);
        self
    }

    /// Metadata display name, where it differs from [`ProviderId::display_name`].
    const fn metadata_name(mut self, name: &'static str) -> Self {
        self.metadata.display_name = name;
        self
    }

    const fn opus(mut self) -> Self {
        self.metadata.supports_opus = true;
        self
    }

    const fn credits(mut self) -> Self {
        self.metadata.supports_credits = true;
        self
    }

    const fn default_enabled(mut self) -> Self {
        self.metadata.default_enabled = true;
        self
    }

    const fn primary(mut self) -> Self {
        self.metadata.is_primary = true;
        self
    }

    const fn dashboard(mut self, url: &'static str) -> Self {
        self.metadata.dashboard_url = Some(url);
        self
    }

    const fn status(mut self, url: &'static str) -> Self {
        self.metadata.status_page_url = Some(url);
        self
    }

    const fn tertiary(mut self, key: &'static str) -> Self {
        self.metadata.tertiary_label_key = Some(key);
        self
    }

    const fn aliases(mut self, aliases: &'static [&'static str]) -> Self {
        self.aliases = aliases;
        self
    }

    const fn parse_aliases(mut self, aliases: &'static [&'static str]) -> Self {
        self.parse_aliases = aliases;
        self
    }
}

const fn spec(
    id: ProviderId,
    cli_name: &'static str,
    display_name: &'static str,
    brand_color: &'static str,
) -> ProviderSpec {
    ProviderSpec {
        id,
        cli_name,
        display_name,
        cookie_domain: None,
        brand_color,
        metadata: ProviderMetadata {
            id,
            display_name,
            session_label: "",
            weekly_label: "",
            supports_opus: false,
            supports_credits: false,
            default_enabled: false,
            is_primary: false,
            dashboard_url: None,
            status_page_url: None,
            tertiary_label_key: None,
        },
        aliases: &[],
        parse_aliases: &[],
    }
}

const COUNT: usize = 89;

pub(crate) static ALL: [ProviderId; COUNT] = {
    // Indexing SPECS by discriminant relies on rows following declaration order.
    assert!(P::Vercel as usize + 1 == COUNT);
    let mut ids = [P::Codex; COUNT];
    let mut i = 0;
    while i < COUNT {
        assert!(SPECS[i].id as usize == i);
        ids[i] = SPECS[i].id;
        i += 1;
    }
    ids
};

pub(crate) static SPECS: [ProviderSpec; COUNT] = [
    spec(P::Codex, "codex", "Codex", "#49A3B0")
        .labels("Session", "Weekly")
        .cookie("chatgpt.com")
        .credits()
        .default_enabled()
        .primary()
        .dashboard("https://chatgpt.com/codex/cloud/settings/analytics#usage")
        .status("https://status.openai.com")
        .aliases(&["openai"]),
    spec(P::Claude, "claude", "Claude", "#CC7C5E")
        .labels("Session (5h)", "Weekly")
        .cookie("claude.ai")
        .opus()
        .credits()
        .default_enabled()
        .primary()
        .dashboard("https://claude.ai/settings/usage")
        .status("https://status.claude.com/")
        .aliases(&["anthropic"]),
    spec(P::Pi, "pi", "Pi", "#7C3AED")
        .labels("Session", "Weekly")
        .dashboard("https://github.com/badlogic/pi-mono")
        .parse_aliases(&["pi-mono"]),
    // Upstream #2338: Cursor has no account credit balance to advertise.
    spec(P::Cursor, "cursor", "Cursor", "#F54E00")
        .labels("Plan", "Cursor")
        .cookie("cursor.com")
        .default_enabled()
        .dashboard("https://cursor.com/dashboard/usage"),
    spec(P::Factory, "factory", "Factory", "#FF6B35")
        .labels("Standard", "Premium")
        .cookie("app.factory.ai")
        .metadata_name("Droid")
        .credits()
        .dashboard("https://app.factory.ai")
        .status("https://status.factory.ai")
        .parse_aliases(&["droid"]),
    spec(P::Gemini, "gemini", "Gemini", "#AB87EA")
        .labels("Daily", "Daily")
        .cookie("aistudio.google.com")
        .default_enabled()
        .dashboard("https://aistudio.google.com")
        .status("https://status.cloud.google.com")
        .aliases(&["google"]),
    spec(P::Antigravity, "antigravity", "Antigravity", "#60BA7E")
        .labels("Claude", "Gemini Pro")
        .cookie("antigravity.ai")
        .opus()
        .aliases(&["agy"]),
    spec(P::Copilot, "copilot", "Copilot", "#A855F7")
        .labels("Premium", "Chat")
        .metadata_name("GitHub Copilot")
        .default_enabled()
        .dashboard("https://github.com/settings/copilot")
        .status("https://www.githubstatus.com/")
        .aliases(&["github"]),
    spec(P::Zai, "zai", "z.ai", "#E85A6A")
        .labels("Tokens", "MCP")
        .credits()
        .dashboard("https://z.ai/manage-apikey/coding-plan/personal/my-plan")
        .parse_aliases(&["z.ai"]),
    spec(P::MiniMax, "minimax", "MiniMax", "#FE603C")
        .labels("Usage", "Monthly")
        .cookie("platform.minimax.io")
        .credits()
        .dashboard("https://platform.minimax.io/user-center/payment/coding-plan?cycle_type=3"),
    spec(P::Kiro, "kiro", "Kiro", "#9046FF")
        .labels("Session", "Monthly")
        .cookie("kiro.dev")
        .credits()
        .dashboard("https://kiro.dev/account")
        .status("https://health.aws.amazon.com")
        .aliases(&["aws"]),
    spec(P::VertexAI, "vertexai", "Vertex AI", "#4285F4")
        .labels("Usage", "Monthly")
        .credits()
        .dashboard("https://console.cloud.google.com/vertex-ai")
        .status("https://status.cloud.google.com")
        .aliases(&["vertex"])
        .parse_aliases(&["vertex ai"]),
    spec(P::Augment, "augment", "Augment", "#1AA049")
        .labels("Session", "Monthly")
        .cookie("app.augmentcode.com")
        .credits()
        .dashboard("https://app.augmentcode.com/account")
        .status("https://status.augmentcode.com"),
    spec(P::OpenCode, "opencode", "OpenCode", "#3B82F6")
        .labels("5-hour", "Weekly")
        .cookie("opencode.ai")
        .dashboard("https://opencode.ai"),
    spec(P::Kimi, "kimi", "Kimi", "#FE603C")
        .labels("Weekly", "Rate Limit")
        .cookie("kimi.moonshot.cn")
        .dashboard("https://kimi.moonshot.cn")
        .parse_aliases(&["moonshot"]),
    // Soft-removed (upstream #2254); still resolvable via CLI for legacy configs.
    spec(P::KimiK2, "kimik2", "Kimi K2 (removed)", "#4C00FF")
        .labels("Balance", "Cash")
        .cookie("platform.moonshot.cn")
        .metadata_name("Moonshot / Kimi Open Platform")
        .credits()
        .dashboard("https://platform.moonshot.ai/console/account")
        .parse_aliases(&["kimi-k2", "kimi k2", "k2", "kimi k2 (removed)"]),
    spec(P::Amp, "amp", "Amp", "#F34E3F")
        .labels("Usage", "Monthly")
        .cookie("sourcegraph.com")
        .credits()
        .dashboard("https://ampcode.com/settings/usage")
        .status("https://sourcegraphstatus.com")
        .aliases(&["sourcegraph"]),
    spec(P::Warp, "warp", "Warp", "#6366F1")
        .labels("Credits", "Add-on credits")
        .dashboard("https://docs.warp.dev/reference/cli/api-keys")
        .aliases(&["warp-ai", "warp-terminal"]),
    spec(P::Ollama, "ollama", "Ollama", "#8B95B0")
        .labels("Session", "Weekly")
        .cookie("ollama.com")
        .dashboard("https://ollama.com/settings"),
    spec(P::AzureOpenAI, "azureopenai", "Azure OpenAI", "#0078D4")
        .labels("Deployment", "Status")
        .dashboard("https://ai.azure.com")
        .status("https://status.azure.com")
        .parse_aliases(&["azure-openai", "azure openai"]),
    spec(P::T3Chat, "t3chat", "T3 Chat", "#8B5CF6")
        .labels("Base", "Overage")
        .cookie("t3.chat")
        .dashboard("https://t3.chat/settings/customization")
        .parse_aliases(&["t3-chat", "t3 chat"]),
    spec(P::OpenRouter, "openrouter", "OpenRouter", "#6B7280")
        .labels("Credits", "API key limit")
        .credits()
        .dashboard("https://openrouter.ai/activity")
        .status("https://status.openrouter.ai")
        .aliases(&["or"]),
    spec(P::JetBrains, "jetbrains", "JetBrains AI", "#FF3399")
        .labels("Credits", "Monthly")
        .credits()
        .dashboard("https://www.jetbrains.com/ai/")
        .parse_aliases(&["jetbrains-ai", "jetbrains ai", "intellij"]),
    spec(P::Alibaba, "alibaba", "Alibaba", "#FF6A00")
        .labels("5-Hour", "Weekly")
        .cookie("modelstudio.console.alibabacloud.com")
        .dashboard("https://modelstudio.console.alibabacloud.com")
        .aliases(&["tongyi", "qianwen"]),
    spec(
        P::AlibabaTokenPlan,
        "alibabatokenplan",
        "Alibaba Token Plan",
        "#FF6A00",
    )
    .labels("Credits", "Usage")
    .cookie("bailian.console.aliyun.com")
    .dashboard(
        "https://bailian.console.aliyun.com/cn-beijing?tab=plan#/efm/subscription/token-plan",
    )
    .status("https://status.aliyun.com")
    .parse_aliases(&[
        "alibaba-token-plan",
        "alibaba token plan",
        "alibaba-token",
        "bailian-token-plan",
    ]),
    spec(P::NanoGPT, "nanogpt", "NanoGPT", "#687FA1")
        .labels("Daily", "Monthly")
        .dashboard("https://nano-gpt.com/usage")
        .parse_aliases(&["nano-gpt"]),
    spec(P::Infini, "infini", "Infini", "#687FA1")
        .labels("5-Hour", "7-Day")
        .dashboard("https://cloud.infini-ai.com")
        .aliases(&["infini-ai"]),
    spec(P::Perplexity, "perplexity", "Perplexity", "#1FB8CD")
        .labels("Credits", "Bonus credits")
        .cookie("perplexity.ai")
        .credits()
        .dashboard("https://www.perplexity.ai/account/usage")
        .aliases(&["pplx"]),
    spec(P::Abacus, "abacus", "Abacus AI", "#814EE8")
        .labels(crate::providers::abacus::CREDITS_LABEL, "")
        .cookie("apps.abacus.ai")
        .credits()
        .dashboard("https://apps.abacus.ai/app/billing")
        .aliases(&["abacus-ai"])
        .parse_aliases(&["abacus ai"]),
    spec(P::Mistral, "mistral", "Mistral", "#FF5229")
        .labels("Monthly", "")
        .cookie("admin.mistral.ai")
        .credits()
        .dashboard("https://admin.mistral.ai/organization/usage")
        .status("https://status.mistral.ai")
        .aliases(&["mistral-ai"])
        .parse_aliases(&["mistral ai"]),
    spec(P::OpenCodeGo, "opencodego", "OpenCode Go", "#3B82F6")
        .labels("5-hour", "Weekly")
        .cookie("opencode.ai")
        .opus()
        .dashboard("https://opencode.ai")
        .tertiary("ProviderMonthly")
        .aliases(&["opencode-go"])
        .parse_aliases(&["opencode go"]),
    spec(P::Kilo, "kilo", "Kilo", "#5D87FF")
        .labels("Credits", "Pass")
        .credits()
        .dashboard("https://app.kilo.ai/usage"),
    spec(P::Bedrock, "bedrock", "AWS Bedrock", "#01A88D")
        .labels("Budget", "Cost")
        .dashboard("https://console.aws.amazon.com/bedrock")
        .status("https://health.aws.amazon.com/health/status")
        .aliases(&["aws-bedrock", "aws bedrock"]),
    spec(P::Codebuff, "codebuff", "Codebuff", "#00FF95")
        .labels("Credits", "Weekly")
        .credits()
        .dashboard("https://www.codebuff.com/usage")
        .aliases(&["manicode"]),
    spec(P::CodeRabbit, "coderabbit", "CodeRabbit", "#FF5C35")
        .labels("Reviews", "Billing")
        .dashboard("https://app.coderabbit.ai")
        .status("https://status.coderabbit.ai")
        .parse_aliases(&["code-rabbit", "code rabbit"]),
    spec(P::DeepSeek, "deepseek", "DeepSeek", "#4D6BFE")
        .labels("Balance", "Balance")
        .credits()
        .dashboard("https://platform.deepseek.com/usage")
        .status("https://status.deepseek.com")
        .aliases(&["deep-seek", "ds"]),
    // Upstream marks supportsCredits=false; balance is shown via primary window text.
    spec(P::DeepInfra, "deepinfra", "DeepInfra", "#2A3275")
        .labels("Balance", "Balance")
        .dashboard("https://deepinfra.com/dash")
        .status("https://status.deepinfra.com")
        .aliases(&["deep-infra", "di"]),
    spec(P::AiAnd, "aiand", "ai&", "#E25C2B")
        .labels("Spend", "Spend")
        .dashboard("https://console.aiand.com")
        .aliases(&["ai&", "ai-and"])
        .parse_aliases(&["ai and"]),
    spec(P::Windsurf, "windsurf", "Windsurf", "#22C55E")
        .labels("Daily", "Weekly")
        .dashboard("https://windsurf.com/subscription")
        .status("https://status.windsurf.com")
        .aliases(&["codeium"]),
    spec(P::Manus, "manus", "Manus", "#34322D")
        .labels("Credits", "Refresh")
        .cookie("manus.im")
        .credits()
        .dashboard("https://manus.im"),
    spec(P::MiMo, "mimo", "Xiaomi MiMo", "#FF6900")
        .labels("Tokens", "Balance")
        .cookie("platform.xiaomimimo.com")
        .credits()
        .dashboard("https://platform.xiaomimimo.com/#/console/balance")
        .aliases(&["xiaomimimo", "xiaomi-mimo"])
        .parse_aliases(&["xiaomi", "xiaomi mimo"]),
    spec(P::Doubao, "doubao", "Doubao", "#2563EB")
        .labels("Requests", "Usage")
        .dashboard("https://console.volcengine.com/ark/region:ark+cn-beijing/usage")
        .aliases(&["ark", "volcengine"]),
    spec(P::CommandCode, "commandcode", "Command Code", "#8C4EDD")
        .labels("5-hour", "Weekly")
        .cookie("commandcode.ai")
        .credits()
        .dashboard("https://commandcode.ai")
        .aliases(&["command-code"])
        .parse_aliases(&["command code"]),
    spec(P::StepFun, "stepfun", "StepFun", "#999999")
        .labels("5-hour", "Weekly")
        .dashboard("https://platform.stepfun.com/dashboard")
        .aliases(&["step-fun"])
        .parse_aliases(&["step fun"]),
    spec(P::Venice, "venice", "Venice", "#3C8FDD")
        .labels("Balance", "DIEM")
        .cookie("venice.ai")
        .credits()
        .dashboard("https://venice.ai/settings/api"),
    spec(P::OpenAIApi, "openaiapi", "OpenAI API", "#10A37F")
        .labels("Spend", "Requests")
        .metadata_name("OpenAI")
        .dashboard("https://platform.openai.com/usage")
        .status("https://status.openai.com")
        .aliases(&["openai-api", "openai-balance"])
        .parse_aliases(&["openai api"]),
    spec(P::Grok, "grok", "Grok", "#111827")
        .labels("Credits", "On-demand")
        .cookie("grok.com")
        .dashboard("https://grok.com/?_s=usage")
        .status("https://status.x.ai")
        .aliases(&["supergrok"])
        .parse_aliases(&["super-grok"]),
    spec(P::ElevenLabs, "elevenlabs", "ElevenLabs", "#111827")
        .labels("Credits", "Voices")
        .credits()
        .dashboard("https://elevenlabs.io/app/settings/api-keys")
        .status("https://status.elevenlabs.io")
        .aliases(&["eleven-labs", "11labs"]),
    spec(P::Deepgram, "deepgram", "Deepgram", "#13EF93")
        .labels("Requests", "Usage")
        .credits()
        .dashboard("https://console.deepgram.com/usage")
        .status("https://status.deepgram.com")
        .aliases(&["dg"]),
    spec(P::Groq, "groq", "Groq", "#F55036")
        .labels("Requests", "Tokens")
        .credits()
        .dashboard("https://console.groq.com/settings/metrics")
        .status("https://status.groq.com")
        .aliases(&["groqcloud", "groq-cloud"])
        .parse_aliases(&["groq cloud"]),
    spec(P::HuggingFace, "huggingface", "Hugging Face", "#FFD21E")
        .labels("Credits", "ZeroGPU")
        .dashboard("https://huggingface.co/settings/billing")
        .status("https://status.huggingface.co")
        .aliases(&["hugging-face", "hf"])
        .parse_aliases(&["hugging face"]),
    spec(P::Helmcode, "helmcode", "Helmcode", "#4F46E5")
        .labels("Quota", "Quota")
        .cookie("helmcode.com")
        .credits()
        .dashboard("https://cloud.helmcode.com/dashboard")
        .aliases(&["nan-builders"])
        .parse_aliases(&["nan builders"]),
    spec(P::V0, "v0", "v0", "#111827")
        .labels("Billing", "Rate limit")
        .credits()
        .dashboard("https://v0.app/settings/billing")
        .status("https://www.vercel-status.com/")
        .aliases(&["v0-dev"])
        .parse_aliases(&["v0.dev"]),
    spec(P::TypeSafe, "typesafe", "TypeSafe", "#2563EB")
        .labels("Balance", "Spend")
        .cookie("typesafe.ai")
        .credits()
        .dashboard(crate::providers::typesafe::BILLING_URL)
        .aliases(&["type-safe"])
        .parse_aliases(&["type safe"]),
    spec(P::LLMProxy, "llmproxy", "LLM Proxy", "#4F46E5")
        .labels("Quota", "Requests")
        .credits()
        .aliases(&["llm-proxy"])
        .parse_aliases(&["llm proxy"]),
    spec(P::Chutes, "chutes", "Chutes", "#FF5C35")
        .labels("4-hour quota", "Monthly quota")
        .credits()
        .dashboard("https://chutes.ai")
        .aliases(&["chutes-ai"])
        .parse_aliases(&["chutes ai"]),
    spec(P::LiteLLM, "litellm", "LiteLLM", "#0EA5E9")
        .labels("Personal budget", "Team budget")
        .aliases(&["lite-llm"])
        .parse_aliases(&["lite llm"]),
    spec(P::Poe, "poe", "Poe", "#5D5FEF")
        .labels("Balance", "Points")
        .credits()
        .dashboard("https://poe.com/settings/subscription"),
    spec(P::Devin, "devin", "Devin", "#317CFF")
        .labels("Daily", "Weekly")
        .credits()
        .dashboard("https://app.devin.ai/settings/billing"),
    spec(P::Zed, "zed", "Zed", "#084CCF")
        .labels("Edits", "Cycle")
        .cookie("zed.dev")
        .credits()
        .dashboard("https://zed.dev/account")
        .aliases(&["zed-ai"]),
    // Soft-removed (upstream #2254); still resolvable via CLI for legacy configs.
    spec(
        P::CrossModel,
        "crossmodel",
        "CrossModel (removed)",
        "#C084FC",
    )
    .labels("Daily cost", "Weekly cost")
    .metadata_name("CrossModel")
    .credits()
    .dashboard("https://crossmodel.ai")
    .aliases(&["cross-model"])
    .parse_aliases(&["cross model", "crossmodel (removed)"]),
    spec(P::Qoder, "qoder", "Qoder", "#2563EB")
        .labels("Credits", "Shared credits")
        .cookie("qoder.com")
        .credits()
        .dashboard("https://qoder.com/account/usage"),
    spec(P::CodeBuddy, "codebuddy", "CodeBuddy", "#0052D9")
        .labels("Credits", "Packages")
        .cookie("codebuddy.cn")
        .credits()
        .dashboard("https://www.codebuddy.cn/profile/plans-usage")
        .parse_aliases(&["code-buddy", "codebuddy-cn", "codebuddycn", "腾讯codebuddy"]),
    spec(P::Sakana, "sakana", "Sakana AI", "#0EA5E9")
        .labels("5-hour", "Weekly")
        .cookie("console.sakana.ai")
        .dashboard(crate::providers::sakana::BILLING_URL)
        .aliases(&["sakana-ai"])
        .parse_aliases(&["sakana ai"]),
    spec(P::Sub2Api, "sub2api", "sub2api", "#14B8A6")
        .labels("Quota", "Weekly quota")
        .aliases(&["sub-2-api"])
        .parse_aliases(&["sub 2 api"]),
    spec(P::Wayfinder, "wayfinder", "Wayfinder", "#14B8A6").labels("Gateway", "Savings"),
    spec(P::ZenMux, "zenmux", "ZenMux", "#6C5CE7")
        .labels("5-hour quota", "Weekly quota")
        .dashboard("https://zenmux.ai/platform/management")
        .parse_aliases(&["zen-mux"]),
    spec(P::ClinePass, "clinepass", "ClinePass", "#5487C8")
        .labels("5-hour", "Weekly")
        .opus()
        .dashboard("https://app.cline.bot/dashboard/subscription?personal=true")
        .parse_aliases(&["cline-pass", "cline"]),
    spec(P::LongCat, "longcat", "LongCat", "#29E154")
        .labels("Quota", "Fuel Pack")
        .cookie("longcat.chat")
        .dashboard("https://longcat.chat/platform/")
        .parse_aliases(&["long-cat", "lc"]),
    spec(P::Neuralwatt, "neuralwatt", "Neuralwatt", "#D55934")
        .labels("Subscription", "Key allowance")
        .dashboard("https://portal.neuralwatt.com/dashboard")
        .parse_aliases(&["neural-watt", "nw", "neural"]),
    spec(P::ZoomMate, "zoommate", "ZoomMate", "#0B5CFF")
        .labels("Credits", "Credits")
        .cookie("zoommate.zoom.us")
        .credits()
        .dashboard("https://zoommate.zoom.us/#/?settings=credit-usage")
        .status("https://www.zoomstatus.com/")
        .parse_aliases(&["zoom-mate", "zoom mate"]),
    spec(P::QwenCloud, "qwen-cloud", "Qwen Cloud", "#615CED")
        .labels("5-hour", "Weekly")
        .cookie("qwencloud.com")
        .dashboard(crate::providers::qwencloud::DASHBOARD_URL)
        .status("https://status.alibabacloud.com")
        .aliases(&["qwencloud", "qwen", "qwen-token-plan"])
        .parse_aliases(&["qwen cloud"]),
    spec(P::Notion, "notion", "Notion AI", "#337EA9")
        .labels("Rolling", "Monthly")
        .cookie("app.notion.com")
        .dashboard(crate::providers::notion::DASHBOARD_URL)
        .status("https://status.notion.so")
        .aliases(&["notion-ai", "notionai"])
        .parse_aliases(&["notion ai"]),
    spec(P::Xai, "xai", "xAI", "#8E8E93")
        .labels("Spend", "Spend")
        .dashboard("https://console.x.ai")
        .status("https://status.x.ai")
        .aliases(&["x.ai"])
        .parse_aliases(&["x-ai"]),
    spec(P::Fireworks, "fireworks", "Fireworks", "#F25B1C")
        .labels("Spend", "Spend")
        .dashboard("https://app.fireworks.ai")
        .aliases(&["fireworks-ai", "fw"]),
    spec(P::AtlasCloud, "atlascloud", "Atlas Cloud", "#5975F5")
        .labels("Balance", "Balance")
        .dashboard(crate::providers::atlascloud::DASHBOARD_URL)
        .parse_aliases(&["atlas-cloud", "atlas cloud"]),
    spec(P::Meta, "meta", "Meta", "#0467DF")
        .labels("Status", "Models")
        .dashboard("https://dev.meta.ai/docs")
        .aliases(&[
            "metaspark",
            "meta-spark",
            "muse-spark",
            "musespark",
            "muse spark",
            "meta muse spark",
        ]),
    spec(P::Muse, "muse", "Muse Code", "#0668E1")
        .labels("5 hours", "Weekly")
        .cookie("dev.meta.ai")
        .dashboard("https://dev.meta.ai")
        .aliases(&["muse-code", "muse code"]),
    spec(P::Replicate, "replicate", "Replicate", "#000000")
        .labels("Spend", "Spend")
        .cookie("replicate.com")
        .credits()
        .dashboard(crate::providers::replicate::BILLING_URL)
        .parse_aliases(&["r8"]),
    spec(P::Nous, "nous", "Nous Portal", "#D6A55C")
        .labels("Monthly credits", "Weekly")
        .dashboard("https://portal.nousresearch.com/usage")
        .aliases(&["nous-portal", "nous portal", "hermes"]),
    spec(P::Hyper, "hyper", "Charm Hyper", "#FF60FF")
        .labels("Balance", "Balance")
        .cookie("hyper.charm.land")
        .dashboard("https://hyper.charm.land")
        .parse_aliases(&["charm-hyper", "charm hyper"]),
    spec(P::GitKraken, "gitkraken", "GitKraken AI", "#179287")
        .labels("Personal", "Shared pool")
        .dashboard("https://gitkraken.dev/account#ai-usage")
        .parse_aliases(&["gitkraken-ai", "gitkraken ai"]),
    spec(P::Bifrost, "bifrost", "Bifrost", "#33C09E")
        .labels("Budget", "Spend")
        .credits()
        .parse_aliases(&["bifrost-gateway", "bifrost gateway"]),
    spec(P::Aixy, "aixy", "Aixy", "#123650")
        .labels("Budget", "Secondary budget")
        .dashboard("https://dash.aixy-gateway.com")
        .parse_aliases(&["aixy-gateway", "aixy gateway"]),
    spec(P::LLMMan, "llmman", "llmman", "#6CC5B0")
        .labels("Memory", "Models")
        .dashboard(crate::providers::llmman::DEFAULT_BASE_URL),
    spec(P::DevPass, "devpass", "DevPass", "#2563EB")
        .labels("Plan credits", "Premium weekly")
        .dashboard("https://devpass.llmgateway.io/dashboard"),
    spec(P::XKiro, "xkiro", "xKiro", "#52C99B")
        .labels("Daily free tokens", "Weekly")
        .dashboard("https://xkiro.com")
        .aliases(&["x-kiro"]),
    spec(P::Raycast, "raycast", "Raycast", "#FF6363")
        .labels("Credits", "Plan")
        .cookie("www.raycast.com")
        .dashboard(crate::providers::raycast::SETTINGS_URL)
        .parse_aliases(&["raycast-ai"]),
    // Upstream uses white; a mid neutral keeps contrast on light and dark surfaces.
    spec(P::Vercel, "vercel", "Vercel AI Gateway", "#737373")
        .labels("Balance", "Balance")
        .dashboard("https://vercel.com/d?to=%2F%5Bteam%5D%2F%7E%2Fai-gateway")
        .aliases(&[
            "vercel-ai-gateway",
            "vercel ai gateway",
            "ai-gateway",
            "ai gateway",
        ]),
];
