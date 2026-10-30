//! Canonical provider factory.
//!
//! All call sites (CLI, Tauri desktop shell, legacy native UI) must construct
//! provider instances through [`instantiate`] so that adding a new provider
//! only requires editing `ProviderId`, the matching `providers/` module, and
//! this one match arm.

use super::{
    CostCliExtension, Provider, ProviderId, ProviderLimitResetPolicy, ProviderPaceCapability,
};
use crate::providers::AbliterationProvider;
use crate::providers::AerostackProvider;
use crate::providers::AtlasCloudProvider;
use crate::providers::ChatboxProvider;
use crate::providers::ClaudePlatformProvider;
use crate::providers::ClawRouterProvider;
use crate::providers::CosmicProvider;
use crate::providers::ExaProvider;
use crate::providers::FalProvider;
use crate::providers::FloraProvider;
use crate::providers::FutureAgiProvider;
use crate::providers::IBMBobProvider;
use crate::providers::LangdockProvider;
use crate::providers::LinkupProvider;
use crate::providers::LithosAIProvider;
use crate::providers::MunitoProvider;
use crate::providers::MuseAIProvider;
use crate::providers::ParallelProvider;
use crate::providers::SyntheticProvider;
use crate::providers::TavilyProvider;
use crate::providers::TraeCnProvider;
use crate::providers::WorkBuddyProvider;
use crate::providers::{
    AbacusProvider, AiAndProvider, AixyProvider, AlibabaProvider, AlibabaTokenPlanProvider,
    AmpProvider, AntigravityProvider, AugmentProvider, AzureOpenAIProvider, BedrockProvider,
    BifrostProvider, ChutesProvider, ClaudeProvider, ClinePassProvider, CodeBuddyProvider,
    CodeRabbitProvider, CodebuffProvider, CodexProvider, CommandCodeProvider, CopilotProvider,
    CrossModelProvider, CursorProvider, DeepInfraProvider, DeepSeekProvider, DeepgramProvider,
    DevPassProvider, DevinProvider, DoubaoProvider, ElevenLabsProvider, FactoryProvider,
    FireworksProvider, GeminiProvider, GitKrakenProvider, GrokProvider, GroqProvider,
    HelmcodeProvider, HuggingFaceProvider, HyperProvider, InfiniProvider, JetBrainsProvider,
    KiloProvider, KimiK2Provider, KimiProvider, KiroProvider, LLMManProvider, LLMProxyProvider,
    LiteLLMProvider, LongCatProvider, ManusProvider, MetaProvider, MiMoProvider, MiniMaxProvider,
    MistralProvider, MuseProvider, NanoGPTProvider, NeuralwattProvider, NotionProvider,
    NousProvider, OllamaProvider, OpenAIApiProvider, OpenCodeGoProvider, OpenCodeProvider,
    OpenRouterProvider, PerplexityProvider, PiProvider, PoeProvider, QoderProvider,
    QwenCloudProvider, RaycastProvider, ReplicateProvider, SailResearchProvider, SakanaProvider,
    SofyaProvider, SonioxProvider, StepFunProvider, Sub2ApiProvider, T3ChatProvider,
    TinyApiProvider, TypeSafeProvider, V0Provider, VeniceProvider, VercelProvider,
    VertexAIProvider, WarpProvider, WayfinderProvider, WindsurfProvider, XKiroProvider,
    XaiProvider, XapiProvider, ZaiProvider, ZedProvider, ZenMuxProvider, ZoomMateProvider,
};

/// Instantiate the concrete [`Provider`] implementation for a given [`ProviderId`].
///
/// Exhaustive over [`ProviderId`]: adding a new variant is a compile error until
/// the corresponding provider type is wired in below.
pub fn instantiate(id: ProviderId) -> Box<dyn Provider> {
    match id {
        ProviderId::Claude => Box::new(ClaudeProvider::new()),
        ProviderId::Codex => Box::new(CodexProvider::new()),
        ProviderId::Pi => Box::new(PiProvider::new()),
        ProviderId::Cursor => Box::new(CursorProvider::new()),
        ProviderId::Gemini => Box::new(GeminiProvider::new()),
        ProviderId::Copilot => Box::new(CopilotProvider::new()),
        ProviderId::Antigravity => Box::new(AntigravityProvider::new()),
        ProviderId::AtlasCloud => Box::new(AtlasCloudProvider::new()),
        ProviderId::Synthetic => Box::new(SyntheticProvider::new()),
        ProviderId::ClawRouter => Box::new(ClawRouterProvider::new()),
        ProviderId::IBMBob => Box::new(IBMBobProvider::new()),
        ProviderId::Langdock => Box::new(LangdockProvider::new()),
        ProviderId::Linkup => Box::new(LinkupProvider::new()),
        ProviderId::LithosAI => Box::new(LithosAIProvider::new()),
        ProviderId::MuseAI => Box::new(MuseAIProvider::new()),
        ProviderId::WorkBuddy => Box::new(WorkBuddyProvider::new()),
        ProviderId::Tavily => Box::new(TavilyProvider::new()),
        ProviderId::Exa => Box::new(ExaProvider::new()),
        ProviderId::Factory => Box::new(FactoryProvider::new()),
        ProviderId::Zai => Box::new(ZaiProvider::new()),
        ProviderId::Kiro => Box::new(KiroProvider::new()),
        ProviderId::VertexAI => Box::new(VertexAIProvider::new()),
        ProviderId::Augment => Box::new(AugmentProvider::new()),
        ProviderId::MiniMax => Box::new(MiniMaxProvider::new()),
        ProviderId::OpenCode => Box::new(OpenCodeProvider::new()),
        ProviderId::Kimi => Box::new(KimiProvider::new()),
        ProviderId::KimiK2 => Box::new(KimiK2Provider::new()),
        ProviderId::Amp => Box::new(AmpProvider::new()),
        ProviderId::Warp => Box::new(WarpProvider::new()),
        ProviderId::Ollama => Box::new(OllamaProvider::new()),
        ProviderId::AzureOpenAI => Box::new(AzureOpenAIProvider::new()),
        ProviderId::T3Chat => Box::new(T3ChatProvider::new()),
        ProviderId::OpenRouter => Box::new(OpenRouterProvider::new()),
        ProviderId::JetBrains => Box::new(JetBrainsProvider::new()),
        ProviderId::Alibaba => Box::new(AlibabaProvider::new()),
        ProviderId::AlibabaTokenPlan => Box::new(AlibabaTokenPlanProvider::new()),
        ProviderId::NanoGPT => Box::new(NanoGPTProvider::new()),
        ProviderId::Infini => Box::new(InfiniProvider::default()),
        ProviderId::Perplexity => Box::new(PerplexityProvider::new()),
        ProviderId::Abacus => Box::new(AbacusProvider::new()),
        ProviderId::Mistral => Box::new(MistralProvider::new()),
        ProviderId::OpenCodeGo => Box::new(OpenCodeGoProvider::new()),
        ProviderId::Kilo => Box::new(KiloProvider::new()),
        ProviderId::Bedrock => Box::new(BedrockProvider::new()),
        ProviderId::Codebuff => Box::new(CodebuffProvider::new()),
        ProviderId::CodeRabbit => Box::new(CodeRabbitProvider::new()),
        ProviderId::DeepSeek => Box::new(DeepSeekProvider::new()),
        ProviderId::DeepInfra => Box::new(DeepInfraProvider::new()),
        ProviderId::AiAnd => Box::new(AiAndProvider::new()),
        ProviderId::Windsurf => Box::new(WindsurfProvider::new()),
        ProviderId::Manus => Box::new(ManusProvider::new()),
        ProviderId::MiMo => Box::new(MiMoProvider::new()),
        ProviderId::Doubao => Box::new(DoubaoProvider::new()),
        ProviderId::CommandCode => Box::new(CommandCodeProvider::new()),
        ProviderId::StepFun => Box::new(StepFunProvider::new()),
        ProviderId::Venice => Box::new(VeniceProvider::new()),
        ProviderId::OpenAIApi => Box::new(OpenAIApiProvider::new()),
        ProviderId::Grok => Box::new(GrokProvider::new()),
        ProviderId::ElevenLabs => Box::new(ElevenLabsProvider::new()),
        ProviderId::Deepgram => Box::new(DeepgramProvider::new()),
        ProviderId::Groq => Box::new(GroqProvider::new()),
        ProviderId::HuggingFace => Box::new(HuggingFaceProvider::new()),
        ProviderId::Helmcode => Box::new(HelmcodeProvider::new()),
        ProviderId::V0 => Box::new(V0Provider::new()),
        ProviderId::TinyApi => Box::new(TinyApiProvider::new()),
        ProviderId::TypeSafe => Box::new(TypeSafeProvider::new()),
        ProviderId::LLMProxy => Box::new(LLMProxyProvider::new()),
        ProviderId::Chutes => Box::new(ChutesProvider::new()),
        ProviderId::LiteLLM => Box::new(LiteLLMProvider::new()),
        ProviderId::Poe => Box::new(PoeProvider::new()),
        ProviderId::Devin => Box::new(DevinProvider::new()),
        ProviderId::Zed => Box::new(ZedProvider::new()),
        ProviderId::CrossModel => Box::new(CrossModelProvider::new()),
        ProviderId::Qoder => Box::new(QoderProvider::new()),
        ProviderId::CodeBuddy => Box::new(CodeBuddyProvider::new()),
        ProviderId::Sakana => Box::new(SakanaProvider::new()),
        ProviderId::Sofya => Box::new(SofyaProvider::new()),
        ProviderId::Sub2Api => Box::new(Sub2ApiProvider::new()),
        ProviderId::Wayfinder => Box::new(WayfinderProvider::new()),
        ProviderId::ZenMux => Box::new(ZenMuxProvider::new()),
        ProviderId::ClinePass => Box::new(ClinePassProvider::new()),
        ProviderId::LongCat => Box::new(LongCatProvider::new()),
        ProviderId::Neuralwatt => Box::new(NeuralwattProvider::new()),
        ProviderId::ZoomMate => Box::new(ZoomMateProvider::new()),
        ProviderId::QwenCloud => Box::new(QwenCloudProvider::new()),
        ProviderId::Replicate => Box::new(ReplicateProvider::new()),
        ProviderId::SailResearch => Box::new(SailResearchProvider::new()),
        ProviderId::Notion => Box::new(NotionProvider::new()),
        ProviderId::Xai => Box::new(XaiProvider::new()),
        ProviderId::Xapi => Box::new(XapiProvider::new()),
        ProviderId::Fireworks => Box::new(FireworksProvider::new()),
        ProviderId::Meta => Box::new(MetaProvider::new()),
        ProviderId::Muse => Box::new(MuseProvider::new()),
        ProviderId::Nous => Box::new(NousProvider::new()),
        ProviderId::Hyper => Box::new(HyperProvider::new()),
        ProviderId::GitKraken => Box::new(GitKrakenProvider::new()),
        ProviderId::Bifrost => Box::new(BifrostProvider::new()),
        ProviderId::Aixy => Box::new(AixyProvider::new()),
        ProviderId::LLMMan => Box::new(LLMManProvider::new()),
        ProviderId::DevPass => Box::new(DevPassProvider::new()),
        ProviderId::XKiro => Box::new(XKiroProvider::new()),
        ProviderId::Raycast => Box::new(RaycastProvider::new()),
        ProviderId::Vercel => Box::new(VercelProvider::new()),
        ProviderId::Cosmic => Box::new(CosmicProvider::new()),
        ProviderId::Aerostack => Box::new(AerostackProvider::new()),
        ProviderId::Parallel => Box::new(ParallelProvider::new()),
        ProviderId::Fal => Box::new(FalProvider::new()),
        ProviderId::Flora => Box::new(FloraProvider::new()),
        ProviderId::FutureAgi => Box::new(FutureAgiProvider::new()),
        ProviderId::Munito => Box::new(MunitoProvider::new()),
        ProviderId::Soniox => Box::new(SonioxProvider::new()),
        ProviderId::ClaudePlatform => Box::new(ClaudePlatformProvider::new()),
        ProviderId::TraeCn => Box::new(TraeCnProvider::new()),
        ProviderId::Chatbox => Box::new(ChatboxProvider::new()),
        ProviderId::Abliteration => Box::new(AbliterationProvider::new()),
    }
}

/// Reset-window pace rule declared by each provider's module (upstream
/// `ProviderDescriptor.pace`, v0.70.0). Exhaustive over [`ProviderId`]: a new
/// variant is a compile error until it is listed here, and providers that
/// upstream does not opt in map to [`ProviderPaceCapability::UNSUPPORTED`].
pub fn pace_capability(id: ProviderId) -> ProviderPaceCapability {
    use crate::providers as p;
    match id {
        ProviderId::Copilot => p::copilot::PACE_CAPABILITY,
        ProviderId::Cursor => p::cursor::PACE_CAPABILITY,
        ProviderId::Grok => p::grok::PACE_CAPABILITY,
        ProviderId::Kimi => p::kimi::PACE_CAPABILITY,
        ProviderId::Zai => p::zai::PACE_CAPABILITY,
        ProviderId::Amp => p::amp::PACE_CAPABILITY,
        ProviderId::Alibaba => p::alibaba::PACE_CAPABILITY,
        ProviderId::AlibabaTokenPlan => p::alibabatokenplan::PACE_CAPABILITY,
        ProviderId::CommandCode => p::commandcode::PACE_CAPABILITY,
        ProviderId::Doubao => p::doubao::PACE_CAPABILITY,
        ProviderId::MiMo => p::mimo::PACE_CAPABILITY,
        ProviderId::StepFun => p::stepfun::PACE_CAPABILITY,
        ProviderId::Notion => p::notion::PACE_CAPABILITY,
        ProviderId::Ollama => p::ollama::PACE_CAPABILITY,
        ProviderId::OpenCodeGo => p::opencodego::PACE_CAPABILITY,
        ProviderId::Claude => p::claude::PACE_CAPABILITY,
        ProviderId::Kiro => p::kiro::PACE_CAPABILITY,
        ProviderId::Codex => p::codex::PACE_CAPABILITY,
        ProviderId::Pi
        | ProviderId::Gemini
        | ProviderId::Antigravity
        | ProviderId::AtlasCloud
        | ProviderId::Synthetic
        | ProviderId::ClawRouter
        | ProviderId::IBMBob
        | ProviderId::Langdock
        | ProviderId::Linkup
        | ProviderId::LithosAI
        | ProviderId::MuseAI
        | ProviderId::WorkBuddy
        | ProviderId::Tavily
        | ProviderId::Exa
        | ProviderId::Factory
        | ProviderId::VertexAI
        | ProviderId::Augment
        | ProviderId::MiniMax
        | ProviderId::OpenCode
        | ProviderId::KimiK2
        | ProviderId::Warp
        | ProviderId::AzureOpenAI
        | ProviderId::T3Chat
        | ProviderId::OpenRouter
        | ProviderId::JetBrains
        | ProviderId::NanoGPT
        | ProviderId::Infini
        | ProviderId::Perplexity
        | ProviderId::Abacus
        | ProviderId::Mistral
        | ProviderId::Kilo
        | ProviderId::Bedrock
        | ProviderId::Codebuff
        | ProviderId::CodeRabbit
        | ProviderId::DeepSeek
        | ProviderId::DeepInfra
        | ProviderId::AiAnd
        | ProviderId::Windsurf
        | ProviderId::Manus
        | ProviderId::Venice
        | ProviderId::OpenAIApi
        | ProviderId::ElevenLabs
        | ProviderId::Deepgram
        | ProviderId::Groq
        | ProviderId::HuggingFace
        | ProviderId::Helmcode
        | ProviderId::V0
        | ProviderId::TinyApi
        | ProviderId::TypeSafe
        | ProviderId::LLMProxy
        | ProviderId::Chutes
        | ProviderId::LiteLLM
        | ProviderId::Poe
        | ProviderId::Devin
        | ProviderId::Zed
        | ProviderId::CrossModel
        | ProviderId::Qoder
        | ProviderId::CodeBuddy
        | ProviderId::Sakana
        | ProviderId::Sofya
        | ProviderId::Sub2Api
        | ProviderId::Wayfinder
        | ProviderId::ZenMux
        | ProviderId::ClinePass
        | ProviderId::LongCat
        | ProviderId::Neuralwatt
        | ProviderId::ZoomMate
        | ProviderId::QwenCloud
        | ProviderId::Replicate
        | ProviderId::SailResearch
        | ProviderId::Xai
        | ProviderId::Xapi
        | ProviderId::Fireworks
        | ProviderId::Meta
        | ProviderId::Muse
        | ProviderId::Nous
        | ProviderId::Hyper
        | ProviderId::GitKraken
        | ProviderId::Bifrost
        | ProviderId::Aixy
        | ProviderId::LLMMan
        | ProviderId::DevPass
        | ProviderId::XKiro
        | ProviderId::Raycast
        | ProviderId::Vercel
        | ProviderId::Cosmic
        | ProviderId::Aerostack
        | ProviderId::Parallel
        | ProviderId::Fal
        | ProviderId::Flora
        | ProviderId::FutureAgi
        | ProviderId::Munito
        | ProviderId::Soniox
        | ProviderId::ClaudePlatform
        | ProviderId::TraeCn
        | ProviderId::Chatbox
        | ProviderId::Abliteration => ProviderPaceCapability::UNSUPPORTED,
    }
}

/// Limit reset detector rule declared by each provider's module. Exhaustive
/// over [`ProviderId`]: a new variant is a compile error until it is listed
/// here, and providers without a specific rule map to
/// [`ProviderLimitResetPolicy::DEFAULT`].
pub fn limit_reset_policy(id: ProviderId) -> ProviderLimitResetPolicy {
    use crate::providers as p;
    match id {
        ProviderId::Claude => p::claude::LIMIT_RESET_POLICY,
        ProviderId::Codex => p::codex::LIMIT_RESET_POLICY,
        ProviderId::TinyApi => p::tinyapi::LIMIT_RESET_POLICY,
        ProviderId::Xapi => p::xapi::LIMIT_RESET_POLICY,
        ProviderId::Langdock => p::langdock::LIMIT_RESET_POLICY,
        ProviderId::Exa => p::exa::LIMIT_RESET_POLICY,
        ProviderId::Copilot
        | ProviderId::Cursor
        | ProviderId::Grok
        | ProviderId::Kimi
        | ProviderId::Zai
        | ProviderId::Amp
        | ProviderId::Alibaba
        | ProviderId::AlibabaTokenPlan
        | ProviderId::CommandCode
        | ProviderId::Doubao
        | ProviderId::MiMo
        | ProviderId::StepFun
        | ProviderId::Notion
        | ProviderId::Ollama
        | ProviderId::OpenCodeGo
        | ProviderId::Pi
        | ProviderId::Gemini
        | ProviderId::Antigravity
        | ProviderId::AtlasCloud
        | ProviderId::Synthetic
        | ProviderId::ClawRouter
        | ProviderId::IBMBob
        | ProviderId::Linkup
        | ProviderId::LithosAI
        | ProviderId::MuseAI
        | ProviderId::WorkBuddy
        | ProviderId::Tavily
        | ProviderId::Factory
        | ProviderId::Kiro
        | ProviderId::VertexAI
        | ProviderId::Augment
        | ProviderId::MiniMax
        | ProviderId::OpenCode
        | ProviderId::KimiK2
        | ProviderId::Warp
        | ProviderId::AzureOpenAI
        | ProviderId::T3Chat
        | ProviderId::OpenRouter
        | ProviderId::JetBrains
        | ProviderId::NanoGPT
        | ProviderId::Infini
        | ProviderId::Perplexity
        | ProviderId::Abacus
        | ProviderId::Mistral
        | ProviderId::Kilo
        | ProviderId::Bedrock
        | ProviderId::Codebuff
        | ProviderId::CodeRabbit
        | ProviderId::DeepSeek
        | ProviderId::DeepInfra
        | ProviderId::AiAnd
        | ProviderId::Windsurf
        | ProviderId::Manus
        | ProviderId::Venice
        | ProviderId::OpenAIApi
        | ProviderId::ElevenLabs
        | ProviderId::Deepgram
        | ProviderId::Groq
        | ProviderId::HuggingFace
        | ProviderId::Helmcode
        | ProviderId::V0
        | ProviderId::TypeSafe
        | ProviderId::LLMProxy
        | ProviderId::Chutes
        | ProviderId::LiteLLM
        | ProviderId::Poe
        | ProviderId::Devin
        | ProviderId::Zed
        | ProviderId::CrossModel
        | ProviderId::Qoder
        | ProviderId::CodeBuddy
        | ProviderId::Sakana
        | ProviderId::Sofya
        | ProviderId::Sub2Api
        | ProviderId::Wayfinder
        | ProviderId::ZenMux
        | ProviderId::ClinePass
        | ProviderId::LongCat
        | ProviderId::Neuralwatt
        | ProviderId::ZoomMate
        | ProviderId::QwenCloud
        | ProviderId::Replicate
        | ProviderId::SailResearch
        | ProviderId::Xai
        | ProviderId::Fireworks
        | ProviderId::Meta
        | ProviderId::Muse
        | ProviderId::Nous
        | ProviderId::Hyper
        | ProviderId::GitKraken
        | ProviderId::Bifrost
        | ProviderId::Aixy
        | ProviderId::LLMMan
        | ProviderId::DevPass
        | ProviderId::XKiro
        | ProviderId::Raycast
        | ProviderId::Vercel
        | ProviderId::Cosmic
        | ProviderId::Aerostack
        | ProviderId::Parallel
        | ProviderId::Fal
        | ProviderId::Flora
        | ProviderId::FutureAgi
        | ProviderId::Munito
        | ProviderId::Soniox
        | ProviderId::ClaudePlatform
        | ProviderId::TraeCn
        | ProviderId::Chatbox
        | ProviderId::Abliteration => ProviderLimitResetPolicy::DEFAULT,
    }
}

/// `codexbar cost` extension declared by a provider's module. Exhaustive over
/// [`ProviderId`]: a new variant is a compile error until it is listed here,
/// and providers without extra cost modes map to `None`.
pub(crate) fn cost_cli_extension_for(id: ProviderId) -> Option<&'static dyn CostCliExtension> {
    match id {
        ProviderId::Codex => Some(&crate::codex_costs::CODEX_COST_CLI),
        ProviderId::OpenCodeGo => {
            Some(&crate::providers::opencodego::cost_cli::OPENCODEGO_COST_CLI)
        }
        ProviderId::Claude
        | ProviderId::TinyApi
        | ProviderId::Xapi
        | ProviderId::Langdock
        | ProviderId::Exa
        | ProviderId::Copilot
        | ProviderId::Cursor
        | ProviderId::Grok
        | ProviderId::Kimi
        | ProviderId::Zai
        | ProviderId::Amp
        | ProviderId::Alibaba
        | ProviderId::AlibabaTokenPlan
        | ProviderId::CommandCode
        | ProviderId::Doubao
        | ProviderId::MiMo
        | ProviderId::StepFun
        | ProviderId::Notion
        | ProviderId::Ollama
        | ProviderId::Pi
        | ProviderId::Gemini
        | ProviderId::Antigravity
        | ProviderId::AtlasCloud
        | ProviderId::Synthetic
        | ProviderId::ClawRouter
        | ProviderId::IBMBob
        | ProviderId::Linkup
        | ProviderId::LithosAI
        | ProviderId::MuseAI
        | ProviderId::WorkBuddy
        | ProviderId::Tavily
        | ProviderId::Factory
        | ProviderId::Kiro
        | ProviderId::VertexAI
        | ProviderId::Augment
        | ProviderId::MiniMax
        | ProviderId::OpenCode
        | ProviderId::KimiK2
        | ProviderId::Warp
        | ProviderId::AzureOpenAI
        | ProviderId::T3Chat
        | ProviderId::OpenRouter
        | ProviderId::JetBrains
        | ProviderId::NanoGPT
        | ProviderId::Infini
        | ProviderId::Perplexity
        | ProviderId::Abacus
        | ProviderId::Mistral
        | ProviderId::Kilo
        | ProviderId::Bedrock
        | ProviderId::Codebuff
        | ProviderId::CodeRabbit
        | ProviderId::DeepSeek
        | ProviderId::DeepInfra
        | ProviderId::AiAnd
        | ProviderId::Windsurf
        | ProviderId::Manus
        | ProviderId::Venice
        | ProviderId::OpenAIApi
        | ProviderId::ElevenLabs
        | ProviderId::Deepgram
        | ProviderId::Groq
        | ProviderId::HuggingFace
        | ProviderId::Helmcode
        | ProviderId::V0
        | ProviderId::TypeSafe
        | ProviderId::LLMProxy
        | ProviderId::Chutes
        | ProviderId::LiteLLM
        | ProviderId::Poe
        | ProviderId::Devin
        | ProviderId::Zed
        | ProviderId::CrossModel
        | ProviderId::Qoder
        | ProviderId::CodeBuddy
        | ProviderId::Sakana
        | ProviderId::Sofya
        | ProviderId::Sub2Api
        | ProviderId::Wayfinder
        | ProviderId::ZenMux
        | ProviderId::ClinePass
        | ProviderId::LongCat
        | ProviderId::Neuralwatt
        | ProviderId::ZoomMate
        | ProviderId::QwenCloud
        | ProviderId::Replicate
        | ProviderId::SailResearch
        | ProviderId::Xai
        | ProviderId::Fireworks
        | ProviderId::Meta
        | ProviderId::Muse
        | ProviderId::Nous
        | ProviderId::Hyper
        | ProviderId::GitKraken
        | ProviderId::Bifrost
        | ProviderId::Aixy
        | ProviderId::LLMMan
        | ProviderId::DevPass
        | ProviderId::XKiro
        | ProviderId::Raycast
        | ProviderId::Vercel
        | ProviderId::Cosmic
        | ProviderId::Aerostack
        | ProviderId::Parallel
        | ProviderId::Fal
        | ProviderId::Flora
        | ProviderId::FutureAgi
        | ProviderId::Munito
        | ProviderId::Soniox
        | ProviderId::ClaudePlatform
        | ProviderId::TraeCn
        | ProviderId::Chatbox
        | ProviderId::Abliteration => None,
    }
}

/// Every registered `codexbar cost` extension, in [`ProviderId::all`] order.
pub(crate) fn cost_cli_extensions() -> impl Iterator<Item = &'static dyn CostCliExtension> {
    ProviderId::all()
        .iter()
        .filter_map(|&id| cost_cli_extension_for(id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_provider_id_is_instantiable() {
        for &id in ProviderId::all() {
            let provider = instantiate(id);
            assert_eq!(
                provider.id(),
                id,
                "factory returned wrong provider for {id}"
            );
        }
    }

    #[test]
    fn limit_reset_policies_match_the_provider_rules() {
        use super::super::{MissingIdentityRule, WeeklyResetRule};

        assert_eq!(
            limit_reset_policy(ProviderId::Claude),
            ProviderLimitResetPolicy {
                supported: true,
                weekly: WeeklyResetRule::RecoveryObservations { count: 2 },
                missing_identity: MissingIdentityRule::Track {
                    requires_low_confirmation: true,
                },
            }
        );
        assert_eq!(
            limit_reset_policy(ProviderId::Codex),
            ProviderLimitResetPolicy {
                supported: true,
                weekly: WeeklyResetRule::AdvancedBoundary {
                    reset_threshold_percent: 5.0,
                    requires_previous_boundary: true,
                },
                missing_identity: MissingIdentityRule::Skip,
            }
        );
        let unsupported = [
            ProviderId::TinyApi,
            ProviderId::Xapi,
            ProviderId::Langdock,
            ProviderId::Exa,
        ];
        for id in unsupported {
            assert_eq!(
                limit_reset_policy(id),
                ProviderLimitResetPolicy::UNSUPPORTED,
                "{id}"
            );
        }
        for &id in ProviderId::all() {
            if !unsupported.contains(&id) && !matches!(id, ProviderId::Claude | ProviderId::Codex) {
                assert_eq!(
                    id.limit_reset_policy(),
                    ProviderLimitResetPolicy::DEFAULT,
                    "{id}"
                );
            }
        }
        assert_eq!(
            ProviderLimitResetPolicy::UNSUPPORTED,
            ProviderLimitResetPolicy {
                supported: false,
                ..ProviderLimitResetPolicy::DEFAULT
            }
        );
    }

    #[test]
    fn only_codex_and_opencodego_register_a_cost_cli_extension() {
        for &id in ProviderId::all() {
            assert_eq!(
                cost_cli_extension_for(id).is_some(),
                matches!(id, ProviderId::Codex | ProviderId::OpenCodeGo),
                "{id}"
            );
        }
        assert_eq!(cost_cli_extensions().count(), 2);
    }
}
