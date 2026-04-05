//! Provider implementations

#![allow(dead_code)]

pub mod alibaba;
pub mod amp;
pub mod antigravity;
pub mod augment;
pub mod claude;
pub mod codex;
pub mod copilot;
pub mod cursor;
pub mod factory;
pub mod gemini;
pub mod infini;
pub mod jetbrains;
pub mod kimi;
pub mod kimik2;
pub mod kiro;
pub mod minimax;
pub mod nanogpt;
pub mod ollama;
pub mod openai;
pub mod opencode;
pub mod openrouter;
pub mod synthetic;
pub mod vertexai;
pub mod warp;
pub mod zai;

// Re-export provider implementations
pub use alibaba::AlibabaProvider;
pub use amp::AmpProvider;
pub use antigravity::AntigravityProvider;
pub use augment::AugmentProvider;
pub use claude::ClaudeProvider;
pub use codex::CodexProvider;
pub use copilot::CopilotProvider;
pub use cursor::CursorProvider;
pub use factory::FactoryProvider;
pub use gemini::GeminiProvider;
pub use infini::InfiniProvider;
pub use jetbrains::JetBrainsProvider;
pub use kimi::KimiProvider;
pub use kimik2::KimiK2Provider;
pub use kiro::KiroProvider;
pub use minimax::MiniMaxProvider;
pub use nanogpt::NanoGPTProvider;
pub use ollama::OllamaProvider;
pub use opencode::OpenCodeProvider;
pub use openrouter::OpenRouterProvider;
pub use synthetic::SyntheticProvider;
pub use vertexai::VertexAIProvider;
pub use warp::WarpProvider;
pub use zai::ZaiProvider;
