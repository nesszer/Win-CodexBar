//! Usage command implementation

use clap::Args;
use serde::Serialize;

use crate::core::{FetchContext, Provider, ProviderFetchResult, ProviderId, SourceMode};
use crate::providers::{
    AlibabaProvider, AmpProvider, AntigravityProvider, AugmentProvider, ClaudeProvider,
    CodexProvider, CopilotProvider, CursorProvider, FactoryProvider, GeminiProvider,
    InfiniProvider, JetBrainsProvider, KimiK2Provider, KimiProvider, KiroProvider, MiniMaxProvider,
    NanoGPTProvider, OllamaProvider, OpenCodeProvider, OpenRouterProvider, SyntheticProvider,
    VertexAIProvider, WarpProvider, ZaiProvider,
};
use crate::status::{ProviderStatus as StatusInfo, StatusLevel, fetch_provider_status};

pub const PROVIDER_ARG_HELP: &str =
    "Provider to query (for example: codex, claude, gemini, zai, nanogpt, all, both)";

/// Arguments for the usage command
#[derive(Args, Debug, Default)]
pub struct UsageArgs {
    #[arg(short, long, help = PROVIDER_ARG_HELP)]
    pub provider: Option<String>,

    /// Output format: text or json
    #[arg(short, long, default_value = "text")]
    pub format: OutputFormat,

    /// Shorthand for --format json
    #[arg(long)]
    pub json: bool,

    /// Skip credits line in output
    #[arg(long = "no-credits")]
    pub no_credits: bool,

    /// Disable ANSI colors in text output
    #[arg(long = "no-color")]
    pub no_color: bool,

    /// Pretty-print JSON output
    #[arg(long)]
    pub pretty: bool,

    /// Fetch and include provider status pages
    #[arg(long)]
    pub status: bool,

    /// Data source: auto, oauth, web, cli
    #[arg(long, default_value = "auto", value_parser = ["auto", "web", "cli", "oauth"])]
    pub source: String,

    /// Web fetch timeout in seconds
    #[arg(long = "web-timeout", default_value = "60")]
    pub web_timeout: u64,

    /// Save HTML snapshots to temp dir when data is missing (debug)
    #[arg(long = "web-debug-dump-html")]
    pub web_debug_dump_html: bool,

    /// Send Antigravity planInfo fields to stderr (debug)
    #[arg(long = "antigravity-plan-debug")]
    pub antigravity_plan_debug: bool,
}

/// Output format enum
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
}

impl std::str::FromStr for OutputFormat {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "text" => Ok(OutputFormat::Text),
            "json" => Ok(OutputFormat::Json),
            _ => Err(format!("Invalid format: {}. Use 'text' or 'json'", s)),
        }
    }
}

/// Provider selection from CLI args
#[derive(Debug, Clone)]
pub enum ProviderSelection {
    Single(ProviderId),
    Both,
    All,
}

impl ProviderSelection {
    pub fn from_arg(arg: Option<&str>) -> anyhow::Result<Self> {
        match arg.map(|s| s.to_lowercase()).as_deref() {
            Some("all") => Ok(ProviderSelection::All),
            Some("both") => Ok(ProviderSelection::Both),
            Some(name) => {
                if let Some(id) = ProviderId::from_cli_name(name) {
                    Ok(ProviderSelection::Single(id))
                } else {
                    anyhow::bail!(
                        "Unknown provider: '{}'. Use --help to see available providers.",
                        name
                    )
                }
            }
            None => Ok(ProviderSelection::Single(ProviderId::Claude)), // Default to Claude
        }
    }

    pub fn as_list(&self) -> Vec<ProviderId> {
        match self {
            ProviderSelection::Single(id) => vec![*id],
            ProviderSelection::Both => vec![ProviderId::Codex, ProviderId::Claude],
            ProviderSelection::All => ProviderId::all().to_vec(),
        }
    }
}

/// JSON output payload
#[derive(Debug, Serialize)]
pub struct ProviderPayload {
    pub provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub source: String,
    #[serde(flatten)]
    pub result: ProviderFetchResult,
}

/// Error payload for JSON output
#[derive(Debug, Serialize)]
struct ErrorPayload {
    provider: String,
    error: String,
}

/// Create a provider instance by ID
fn create_provider(id: ProviderId) -> Box<dyn Provider> {
    match id {
        ProviderId::Claude => Box::new(ClaudeProvider::new()),
        ProviderId::Codex => Box::new(CodexProvider::new()),
        ProviderId::Cursor => Box::new(CursorProvider::new()),
        ProviderId::Gemini => Box::new(GeminiProvider::new()),
        ProviderId::Copilot => Box::new(CopilotProvider::new()),
        ProviderId::Antigravity => Box::new(AntigravityProvider::new()),
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
        ProviderId::OpenRouter => Box::new(OpenRouterProvider::new()),
        ProviderId::Synthetic => Box::new(SyntheticProvider::new()),
        ProviderId::JetBrains => Box::new(JetBrainsProvider::new()),
        ProviderId::Alibaba => Box::new(AlibabaProvider::new()),
        ProviderId::NanoGPT => Box::new(NanoGPTProvider::new()),
        ProviderId::Infini => Box::new(InfiniProvider::default()),
    }
}

/// Run the usage command
pub async fn run(args: UsageArgs) -> anyhow::Result<()> {
    let format = if args.json {
        OutputFormat::Json
    } else {
        args.format
    };

    let source_mode = SourceMode::from_str(&args.source).unwrap_or(SourceMode::Auto);
    let providers = ProviderSelection::from_arg(args.provider.as_deref())?;
    let use_color = !args.no_color && is_terminal();
    let fetch_status = args.status;

    tracing::debug!(
        "Running usage command: providers={:?}, format={:?}, source={:?}, status={}",
        providers.as_list(),
        format,
        source_mode,
        fetch_status
    );

    let ctx = FetchContext {
        source_mode,
        include_credits: !args.no_credits,
        web_timeout: args.web_timeout,
        verbose: false,
        manual_cookie_header: None,
        api_key: None,
    };

    let mut results: Vec<serde_json::Value> = Vec::new();
    let mut text_sections: Vec<String> = Vec::new();

    for provider_id in providers.as_list() {
        let provider = create_provider(provider_id);

        // Optionally fetch status in parallel with usage
        let status_future = if fetch_status {
            Some(fetch_provider_status(provider_id.cli_name()))
        } else {
            None
        };

        match provider.fetch_usage(&ctx).await {
            Ok(result) => {
                let status = if let Some(fut) = status_future {
                    fut.await
                } else {
                    None
                };

                if format == OutputFormat::Text {
                    text_sections.push(render_text_with_status(
                        provider_id,
                        &result,
                        status.as_ref(),
                        use_color,
                    ));
                } else {
                    let mut json_result = serde_json::json!({
                        "provider": provider_id.cli_name(),
                        "source": result.source_label,
                        "usage": result.usage,
                        "cost": result.cost,
                    });

                    if let Some(ref s) = status {
                        json_result["status"] = serde_json::json!({
                            "level": format!("{:?}", s.level).to_lowercase(),
                            "description": s.description,
                        });
                    }

                    results.push(json_result);
                }
            }
            Err(e) => {
                let error_msg = e.to_string();
                if format == OutputFormat::Text {
                    let header = if use_color {
                        format!("\x1b[1m{}\x1b[0m", provider_id.display_name())
                    } else {
                        provider_id.display_name().to_string()
                    };
                    text_sections.push(format!("{}  Error: {}", header, error_msg));
                } else {
                    results.push(serde_json::json!({
                        "provider": provider_id.cli_name(),
                        "error": error_msg,
                    }));
                }
            }
        }
    }

    match format {
        OutputFormat::Text => {
            println!("{}", text_sections.join("\n\n"));
        }
        OutputFormat::Json => {
            let output = if args.pretty {
                serde_json::to_string_pretty(&results)?
            } else {
                serde_json::to_string(&results)?
            };
            println!("{}", output);
        }
    }

    Ok(())
}

/// Check if stdout is a terminal
fn is_terminal() -> bool {
    use std::io::IsTerminal;
    std::io::stdout().is_terminal()
}

/// Render usage as text with optional status
pub fn render_text_with_status(
    provider: ProviderId,
    result: &ProviderFetchResult,
    status: Option<&StatusInfo>,
    use_color: bool,
) -> String {
    let mut lines = Vec::new();
    let metadata = create_provider(provider).metadata().clone();

    // Header with optional status indicator
    let status_indicator = if let Some(s) = status {
        let (symbol, color) = match s.level {
            StatusLevel::Operational => ("●", "\x1b[32m"), // Green
            StatusLevel::Degraded => ("◐", "\x1b[33m"),    // Yellow
            StatusLevel::Partial => ("◑", "\x1b[33m"),     // Yellow
            StatusLevel::Major => ("○", "\x1b[31m"),       // Red
            StatusLevel::Unknown => ("?", "\x1b[90m"),     // Gray
        };
        if use_color {
            format!(" {}{}\x1b[0m", color, symbol)
        } else {
            format!(" {}", symbol)
        }
    } else {
        String::new()
    };

    let header = if use_color {
        format!(
            "\x1b[1m{}\x1b[0m ({}){}",
            provider.display_name(),
            result.source_label,
            status_indicator
        )
    } else {
        format!(
            "{} ({}){}",
            provider.display_name(),
            result.source_label,
            status_indicator
        )
    };
    lines.push(header);

    // Status description if available
    if let Some(s) = status
        && s.level != StatusLevel::Operational
        && s.level != StatusLevel::Unknown
    {
        lines.push(format!("  Status: {}", s.description));
    }

    // Account info
    if let Some(ref email) = result.usage.account_email {
        lines.push(format!("  Account: {}", email));
    }
    if let Some(ref method) = result.usage.login_method {
        lines.push(format!("  Plan:    {}", method));
    }

    // Primary window
    let primary = &result.usage.primary;
    let session_bar = render_progress_bar(primary.used_percent, 20, use_color);
    let session_reset = primary
        .format_countdown()
        .map(|c| format!(" (resets in {})", c))
        .unwrap_or_default();
    lines.push(format!(
        "  {:<8} {} {:.0}% used{}",
        format!("{}:", metadata.session_label),
        session_bar,
        primary.used_percent,
        session_reset
    ));

    // Secondary window
    if let Some(ref secondary) = result.usage.secondary {
        let weekly_bar = render_progress_bar(secondary.used_percent, 20, use_color);
        let weekly_reset = secondary
            .format_countdown()
            .map(|c| format!(" (resets in {})", c))
            .unwrap_or_default();
        lines.push(format!(
            "  {:<8} {} {:.0}% used{}",
            format!("{}:", metadata.weekly_label),
            weekly_bar,
            secondary.used_percent,
            weekly_reset
        ));
    }

    // Model-specific window
    if let Some(ref opus) = result.usage.model_specific {
        let opus_bar = render_progress_bar(opus.used_percent, 20, use_color);
        lines.push(format!(
            "  Opus:    {} {:.0}% used",
            opus_bar, opus.used_percent
        ));
    }

    // Cost info
    if let Some(ref cost) = result.cost {
        let cost_line = if let Some(limit) = cost.format_limit() {
            format!(
                "  Cost:    {} / {} ({})",
                cost.format_used(),
                limit,
                cost.period
            )
        } else {
            format!("  Cost:    {} ({})", cost.format_used(), cost.period)
        };
        lines.push(cost_line);
    }

    lines.join("\n")
}

/// Render usage as text (backwards compatible version)
pub fn render_text(provider: ProviderId, result: &ProviderFetchResult, use_color: bool) -> String {
    render_text_with_status(provider, result, None, use_color)
}

/// Render a text-based progress bar
fn render_progress_bar(percent: f64, width: usize, use_color: bool) -> String {
    let filled = ((percent / 100.0) * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);

    let bar = format!("[{}{}]", "█".repeat(filled), "░".repeat(empty));

    if use_color {
        let color = if percent >= 90.0 {
            "\x1b[31m" // Red
        } else if percent >= 70.0 {
            "\x1b[33m" // Yellow
        } else {
            "\x1b[32m" // Green
        };
        format!("{}{}\x1b[0m", color, bar)
    } else {
        bar
    }
}
