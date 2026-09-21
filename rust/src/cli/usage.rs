//! Usage command implementation

use clap::Args;
use serde::Serialize;

use crate::core::{FetchContext, ProviderFetchResult, ProviderId, SourceMode};

mod claude_swap;
mod fetch_helpers;
mod render;

use fetch_helpers::{fetch_provider_json_output, fetch_provider_text_output};
pub(super) use render::{
    append_status_line, format_percent, render_status_indicator, render_text_error,
};
use render::{is_terminal, print_usage_output};
pub use render::{render_brief_text, render_text, render_text_with_status};

pub(super) enum UsageOutput {
    Text(Vec<String>),
    Json {
        results: Vec<serde_json::Value>,
        pretty: bool,
    },
    Toon(Vec<serde_json::Value>),
}

pub const PROVIDER_ARG_HELP: &str = "Provider to query (for example: codex, claude, gemini, antigravity/agy, nanogpt, deepseek, codebuff, windsurf, all, both)";

/// Arguments for the usage command
#[derive(Args, Debug, Default)]
pub struct UsageArgs {
    #[arg(short, long, help = PROVIDER_ARG_HELP)]
    pub provider: Option<String>,

    /// Output format: text, json, or toon
    #[arg(short, long, default_value = "text")]
    pub format: UsageOutputFormat,

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

    /// Fetch all token accounts where supported
    #[arg(long = "all-accounts")]
    pub all_accounts: bool,

    /// Token-account label or 1-based index (requires a single provider)
    #[arg(long = "account")]
    pub account: Option<String>,

    /// Data source: auto, oauth, web, cli
    #[arg(long, default_value = "auto", value_parser = ["auto", "web", "cli", "oauth"])]
    pub source: String,

    /// Web fetch timeout in seconds
    #[arg(long = "web-timeout", default_value = "60")]
    pub web_timeout: u64,

    /// Print one compact line per provider
    #[arg(long)]
    pub brief: bool,
}

/// Output format enum
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum OutputFormat {
    #[default]
    Text,
    Json,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum UsageOutputFormat {
    #[default]
    Text,
    Json,
    Toon,
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

impl std::str::FromStr for UsageOutputFormat {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "text" => Ok(UsageOutputFormat::Text),
            "json" => Ok(UsageOutputFormat::Json),
            "toon" => Ok(UsageOutputFormat::Toon),
            _ => Err(format!(
                "Invalid format: {}. Use 'text', 'json', or 'toon'",
                s
            )),
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

/// Run the usage command
pub async fn run(args: UsageArgs) -> anyhow::Result<()> {
    let command = UsageCommand::from_args(args)?;
    command.log();
    let output = claude_swap::collect_usage_output(&command).await;
    print_usage_output(output)
}

struct UsageCommand {
    format: UsageOutputFormat,
    providers: Vec<ProviderId>,
    use_color: bool,
    brief: bool,
    fetch_status: bool,
    pretty: bool,
    /// Optional token-account label/index for a single-provider fetch.
    account: Option<String>,
    /// Read every external claude-swap account for Claude (read-only).
    all_accounts: bool,
    ctx: FetchContext,
}

impl UsageCommand {
    fn from_args(args: UsageArgs) -> anyhow::Result<Self> {
        let format = effective_format(&args);
        let source_mode = SourceMode::parse(&args.source).unwrap_or(SourceMode::Auto);
        let providers = ProviderSelection::from_arg(args.provider.as_deref())?.as_list();
        if args.account.is_some() && providers.len() != 1 {
            anyhow::bail!("--account requires a single --provider (not all/both)");
        }
        if args.all_accounts && args.account.is_some() {
            anyhow::bail!("--all-accounts cannot be combined with --account");
        }

        Ok(Self {
            format,
            providers,
            use_color: !args.no_color && is_terminal(),
            brief: args.brief,
            fetch_status: args.status,
            pretty: args.pretty,
            account: args.account.clone(),
            all_accounts: args.all_accounts,
            ctx: build_usage_fetch_context(&args, source_mode),
        })
    }

    fn log(&self) {
        tracing::debug!(
            "Running usage command: providers={:?}, format={:?}, source={:?}, status={}",
            self.providers,
            self.format,
            self.ctx.source_mode,
            self.fetch_status
        );
    }
}

fn effective_format(args: &UsageArgs) -> UsageOutputFormat {
    if args.json {
        UsageOutputFormat::Json
    } else {
        args.format
    }
}

fn build_usage_fetch_context(args: &UsageArgs, source_mode: SourceMode) -> FetchContext {
    FetchContext {
        source_mode,
        include_credits: !args.no_credits,
        web_timeout: args.web_timeout,
        verbose: false,
        manual_cookie_header: None,
        manual_cookie_missing: false,
        api_key: None,
        workspace_id: None,
        seat_credit_entitlement: None,
        api_region: None,
        gateway_url: None,
        auto_prefer_web: false,
        // `codexbar usage` is a foreground read: optional enrichment (e.g. the
        // OpenCode Go Zen balance) is worth its full bounded wait (#2583).
        requires_optional_usage_completeness: true,
    }
}

#[cfg(test)]
#[path = "usage_tests.rs"]
mod tests;
