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
    /// `display_details` ids that carry this provider's balance, in preference
    /// order (upstream #4319). The tray tooltip shows the first reported one
    /// when the headline window is informational, so a balance-only account
    /// never reads as a generic label. Empty for providers without a balance.
    pub(crate) balance_detail_ids: &'static [&'static str],
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

    const fn balance_details(mut self, ids: &'static [&'static str]) -> Self {
        self.balance_detail_ids = ids;
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
        balance_detail_ids: &[],
    }
}

const COUNT: usize = 115;

pub(crate) static ALL: [ProviderId; COUNT] = {
    // Indexing SPECS by discriminant relies on rows following declaration order.
    assert!(P::Abliteration as usize + 1 == COUNT);
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
        .dashboard("https://chatgpt.com/settings/usage?tab=overview")
        .status("https://status.openai.com")
        .aliases(&["openai"]),
    spec(P::Claude, "claude", "Claude", "#CC7C5E")
        .labels("Session (5h)", "Weekly")
        .cookie("claude.ai")
        .opus()
        .credits()
        .default_enabled()
        .primary()
        .dashboard("https://platform.claude.com/usage")
        .status("https://status.claude.com/")
        .aliases(&["anthropic"]),
    spec(P::Pi, "pi", "Pi", "#4D9ABF")
        .labels("Session", "Weekly")
        .dashboard("https://github.com/earendil-works/pi")
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
        .cookie("gemini.google.com")
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
        .labels("Amp Free", "Balance")
        .cookie("sourcegraph.com")
        .credits()
        .dashboard("https://ampcode.com/settings/usage")
        .aliases(&["sourcegraph"]),
    spec(P::Warp, "warp", "Warp", "#938BB4")
        .labels("Credits", "Add-on credits")
        .dashboard("https://docs.warp.dev/reference/cli/api-keys")
        .aliases(&["warp-ai", "warp-terminal"]),
    spec(P::Ollama, "ollama", "Ollama", "#888888")
        .labels("Session", "Weekly")
        .cookie("ollama.com")
        .dashboard("https://ollama.com/settings"),
    spec(P::AzureOpenAI, "azureopenai", "Azure OpenAI", "#0078D4")
        .labels("Deployment", "Status")
        .dashboard("https://ai.azure.com")
        .status("https://status.azure.com")
        .parse_aliases(&["azure-openai", "azure openai"]),
    spec(P::T3Chat, "t3chat", "T3 Chat", "#F56647")
        .labels("Base", "Overage")
        .cookie("t3.chat")
        .dashboard("https://t3.chat/settings/customization")
        .parse_aliases(&["t3-chat", "t3 chat"]),
    spec(P::OpenRouter, "openrouter", "OpenRouter", "#6467F2")
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
    spec(P::Perplexity, "perplexity", "Perplexity", "#20B2AA")
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
    spec(P::Kilo, "kilo", "Kilo", "#F27027")
        .labels("Credits", "Kilo Pass")
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
    spec(P::Windsurf, "windsurf", "Windsurf", "#34E8BB")
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
        .labels("Credits", "Window")
        .cookie("platform.xiaomimimo.com")
        .credits()
        .dashboard("https://platform.xiaomimimo.com/#/console/balance")
        .aliases(&["xiaomimimo", "xiaomi-mimo"])
        .parse_aliases(&["xiaomi", "xiaomi mimo"]),
    spec(P::Doubao, "doubao", "Doubao", "#3370FF")
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
    spec(P::StepFun, "stepfun", "StepFun", "#2196F2")
        .labels("5-hour", "Weekly")
        .dashboard("https://platform.stepfun.com/dashboard")
        .aliases(&["step-fun"])
        .parse_aliases(&["step fun"]),
    spec(P::Venice, "venice", "Venice", "#3C8FDD")
        .labels("Balance", "DIEM")
        .cookie("venice.ai")
        .credits()
        .dashboard("https://venice.ai/settings/api"),
    spec(P::OpenAIApi, "openaiapi", "OpenAI API", "#0F826E")
        .labels("Spend", "Requests")
        .metadata_name("OpenAI")
        .dashboard("https://platform.openai.com/usage")
        .status("https://status.openai.com")
        .aliases(&["openai-api", "openai-balance"])
        .parse_aliases(&["openai api"]),
    spec(P::Grok, "grok", "Grok", "#10A37F")
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
    spec(P::Deepgram, "deepgram", "Deepgram", "#6467F2")
        .labels("Requests", "Usage")
        .credits()
        .dashboard("https://console.deepgram.com/usage")
        .status("https://status.deepgram.com")
        .aliases(&["dg"]),
    spec(P::Groq, "groq", "Groq", "#F56844")
        .labels("Requests", "Tokens")
        // The console session cookies live on `.groq.com` and
        // `console.groq.com`; the parent domain matches both.
        .cookie("groq.com")
        .credits()
        .dashboard("https://console.groq.com/dashboard/usage")
        .status("https://status.groq.com")
        .aliases(&["groqcloud", "groq-cloud"])
        .parse_aliases(&["groq cloud"]),
    spec(P::HuggingFace, "huggingface", "Hugging Face", "#FFD21E")
        .labels("Credits", "ZeroGPU")
        .dashboard("https://huggingface.co/settings/billing")
        .status("https://status.huggingface.co")
        .aliases(&["hugging-face", "hf"])
        .parse_aliases(&["hugging face"]),
    spec(P::Helmcode, "helmcode", "Helmcode", "#4934E1")
        .labels("Quota", "Quota")
        .cookie("helmcode.com")
        .credits()
        .dashboard("https://cloud.helmcode.com/dashboard")
        .aliases(&["nan-builders"])
        .parse_aliases(&["nan builders"]),
    spec(P::V0, "v0", "v0", "#111111")
        .labels("Billing", "Rate limit")
        .credits()
        .dashboard("https://v0.app/settings/billing")
        .status("https://www.vercel-status.com/")
        .aliases(&["v0-dev"])
        .parse_aliases(&["v0.dev"]),
    spec(P::TinyApi, "tinyapi", "TinyApi", "#F97316")
        .labels("Credits", "Credits")
        .cookie("tinyapi.rest")
        .dashboard("https://tinyapi.rest/dashboard")
        .parse_aliases(&["tiny-api"]),
    spec(P::TypeSafe, "typesafe", "TypeSafe", "#111111")
        .labels("Balance", "Spend")
        .cookie("typesafe.ai")
        .credits()
        .dashboard(crate::providers::typesafe::BILLING_URL)
        .aliases(&["type-safe"])
        .parse_aliases(&["type safe"]),
    spec(P::LLMProxy, "llmproxy", "LLM Proxy", "#24B47E")
        .labels("Quota", "Requests")
        .credits()
        .aliases(&["llm-proxy"])
        .parse_aliases(&["llm proxy"]),
    spec(P::Chutes, "chutes", "Chutes", "#3184FF")
        .labels("4-hour quota", "Monthly quota")
        .credits()
        .dashboard("https://chutes.ai")
        .aliases(&["chutes-ai"])
        .parse_aliases(&["chutes ai"]),
    spec(P::LiteLLM, "litellm", "LiteLLM", "#4C89F0")
        .labels("Personal budget", "Team budget")
        .aliases(&["lite-llm"])
        .parse_aliases(&["lite llm"]),
    spec(P::Poe, "poe", "Poe", "#5D5CDE")
        .labels("Balance", "Points")
        .credits()
        .dashboard("https://poe.com/settings/subscription"),
    spec(P::Devin, "devin", "Devin", "#317CFF")
        .labels("Daily", "Weekly")
        .credits()
        .dashboard("https://app.devin.ai/settings/billing"),
    spec(P::Zed, "zed", "Zed", "#084EFF")
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
    spec(P::Qoder, "qoder", "Qoder", "#10B981")
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
    spec(P::Sakana, "sakana", "Sakana AI", "#2975DB")
        .labels("5-hour", "Weekly")
        .cookie("console.sakana.ai")
        .dashboard(crate::providers::sakana::BILLING_URL)
        .aliases(&["sakana-ai"])
        .parse_aliases(&["sakana ai"]),
    spec(P::Sofya, "sofya", "Sofya", "#B0B820")
        .labels("Credits", "Credits")
        .dashboard("https://sofya.co/dashboard"),
    spec(P::Sub2Api, "sub2api", "sub2api", "#14B8A6")
        .labels("Quota", "Weekly quota")
        .aliases(&["sub-2-api"])
        .parse_aliases(&["sub 2 api"]),
    spec(P::Wayfinder, "wayfinder", "Wayfinder", "#10A37F").labels("Gateway", "Savings"),
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
    spec(P::Xapi, "xapi", "X API", "#71767B")
        .labels("Balance", "Balance")
        .cookie("x.com")
        .dashboard("https://console.x.com")
        .parse_aliases(&["x-api", "x api"]),
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
        .aliases(&["nous-portal", "nous portal", "hermes"])
        .balance_details(&["total-usable", "top-up-credits"]),
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
    // Mac uses white, which is invisible on the light bar track; a mid neutral
    // keeps contrast on light and dark surfaces (user decision 2026-10-10).
    spec(P::Vercel, "vercel", "Vercel AI Gateway", "#737373")
        .labels("Balance", "Balance")
        .dashboard("https://vercel.com/d?to=%2F%5Bteam%5D%2F%7E%2Fai-gateway")
        .aliases(&[
            "vercel-ai-gateway",
            "vercel ai gateway",
            "ai-gateway",
            "ai gateway",
        ]),
    spec(P::Synthetic, "synthetic", "Synthetic", "#141414")
        .labels("Five-hour quota", "Weekly tokens")
        .aliases(&["synthetic.new"]),
    spec(P::ClawRouter, "clawrouter", "ClawRouter", "#596EF6")
        .labels("Monthly budget", "Requests")
        .dashboard("https://clawrouter.openclaw.ai/dashboard/access")
        .aliases(&["claw-router"]),
    spec(P::IBMBob, "ibmbob", "IBM Bob", "#0E61FA")
        .labels("Monthly Bobcoins", "Monthly Bobcoins")
        .dashboard("https://bob.ibm.com")
        .status("https://status.bob.ibm.com")
        .aliases(&["ibm-bob", "ibm bob", "bob", "bobshell"]),
    spec(P::Langdock, "langdock", "Langdock", "#5A4AE7")
        .labels("Session", "Weekly")
        .cookie("langdock.com")
        .dashboard("https://app.langdock.com/settings/account/usage"),
    spec(P::LithosAI, "lithosai", "LithosAI", "#6B7280")
        .labels("Balance", "Spend")
        .cookie("console.lithosai.cloud")
        .dashboard("https://console.lithosai.cloud"),
    spec(P::Linkup, "linkup", "Linkup", "#202020")
        .labels("Balance", "Balance")
        .dashboard("https://app.linkup.so/organization/billing"),
    spec(P::MuseAI, "museai", "Muse (muse.ai)", "#0668E1")
        .labels("Weekly", "Plan")
        .cookie("muse.ai")
        .dashboard("https://muse.ai/?settings_tab=general")
        .aliases(&["muse (muse.ai)"]),
    spec(P::WorkBuddy, "workbuddy", "WorkBuddy", "#0DC8A6")
        .labels("Credits", "Plan")
        .cookie("workbuddy.cn")
        .dashboard("https://www.workbuddy.cn/profile/plans-usage"),
    spec(P::SailResearch, "sailresearch", "Sail Research", "#2C4681")
        .labels("Balance", "Spend")
        .dashboard("https://app.sailresearch.com")
        .parse_aliases(&["sail research"]),
    spec(P::Tavily, "tavily", "Tavily", "#0F766E")
        .labels("Plan credits", "API key")
        .dashboard("https://app.tavily.com"),
    spec(P::Exa, "exa", "Exa", "#0143D9")
        .labels("Spend", "Spend")
        .dashboard("https://dashboard.exa.ai/api-keys"),
    spec(P::Cosmic, "cosmic", "Cosmic AI", "#6366F1")
        .labels("Input tokens", "Output tokens")
        .dashboard("https://app.cosmicjs.com")
        .parse_aliases(&["cosmic ai"]),
    spec(P::Aerostack, "aerostack", "Aerostack", "#6366F1")
        .labels("Monthly AI tokens", "Weekly")
        .dashboard("https://app.aerostack.dev/settings/billing"),
    spec(P::Parallel, "parallel", "Parallel", "#1D1C1A")
        .labels("Balance", "Balance")
        .cookie("platform.parallel.ai")
        .dashboard("https://platform.parallel.ai")
        .parse_aliases(&["parallel.ai"]),
    spec(P::Fal, "fal", "fal", "#7C3AED")
        .labels("Credit balance", "Credit balance")
        .dashboard("https://fal.ai/dashboard/billing"),
    spec(P::Flora, "flora", "FLORA", "#111111")
        .labels("Balance", "Balance")
        .dashboard("https://app.flora.ai"),
    spec(P::FutureAgi, "futureagi", "Future AGI", "#7857FC")
        .labels("AI credits", "Gateway requests")
        .dashboard("https://app.futureagi.com/dashboard/settings/usage-summary")
        .parse_aliases(&["future agi"]),
    spec(P::Munito, "munito", "Munito", "#702CF6")
        .labels("Today", "This month")
        .dashboard("https://console.munito.ai"),
    spec(
        P::ClaudePlatform,
        "claudeplatform",
        "Claude Platform",
        "#CC7C5E",
    )
    .labels("API credit", "Purchased credit")
    .cookie("platform.claude.com")
    .dashboard("https://platform.claude.com/settings/billing")
    .status("https://status.claude.com/")
    .aliases(&["claude-platform"])
    .parse_aliases(&["claude platform"]),
    spec(P::Soniox, "soniox", "Soniox", "#253772")
        .labels("Monthly budget", "Usage")
        .dashboard("https://console.soniox.com")
        .status("https://status.soniox.com"),
    spec(P::TraeCn, "traecn", "Trae CN", "#4D7CFE")
        .labels("Usage", "Window")
        .cookie("api.trae.cn")
        .dashboard("https://www.trae.cn/dashboard#usage")
        .aliases(&["trae-cn"])
        .parse_aliases(&["trae cn"]),
    spec(P::Chatbox, "chatbox", "Chatbox AI", "#228BE6")
        .labels("Plan", "Top-up credits")
        .dashboard("https://chatboxai.app")
        .parse_aliases(&["chatbox ai"]),
    spec(
        P::Abliteration,
        "abliteration",
        "abliteration.ai",
        "#1B1B1B",
    )
    .labels("Credits", "Usage")
    .dashboard("https://abliteration.ai")
    .parse_aliases(&["abliteration.ai"]),
];
