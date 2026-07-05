//! Usage command implementation

use clap::Args;
use serde::Serialize;

use crate::core::{
    CostSnapshot, FetchContext, ProviderFetchResult, ProviderId, RateWindow, SourceMode, UsagePace,
    UsageSnapshot, instantiate_provider,
};
use crate::status::{ProviderStatus as StatusInfo, StatusLevel, fetch_provider_status};

pub const PROVIDER_ARG_HELP: &str = "Provider to query (for example: codex, claude, gemini, nanogpt, deepseek, codebuff, windsurf, all, both)";

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

    /// Fetch all token accounts where supported
    #[arg(long = "all-accounts")]
    pub all_accounts: bool,

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

/// Run the usage command
pub async fn run(args: UsageArgs) -> anyhow::Result<()> {
    let command = UsageCommand::from_args(args)?;
    command.log();
    let output = collect_usage_output(&command).await;
    print_usage_output(output)
}

struct UsageCommand {
    format: OutputFormat,
    providers: Vec<ProviderId>,
    use_color: bool,
    fetch_status: bool,
    pretty: bool,
    ctx: FetchContext,
}

impl UsageCommand {
    fn from_args(args: UsageArgs) -> anyhow::Result<Self> {
        let format = effective_format(&args);
        let source_mode = SourceMode::parse(&args.source).unwrap_or(SourceMode::Auto);
        let providers = ProviderSelection::from_arg(args.provider.as_deref())?.as_list();

        Ok(Self {
            format,
            providers,
            use_color: !args.no_color && is_terminal(),
            fetch_status: args.status,
            pretty: args.pretty,
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

fn effective_format(args: &UsageArgs) -> OutputFormat {
    if args.json {
        OutputFormat::Json
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
        api_key: None,
        workspace_id: None,
        api_region: None,
    }
}

enum UsageOutput {
    Text(Vec<String>),
    Json {
        results: Vec<serde_json::Value>,
        pretty: bool,
    },
}

/// Cap on concurrent provider fetches for `-p all`, so results overlap (rather
/// than summing per-provider latency) without opening a connection to every
/// provider at once. `buffered` preserves input order, so output is unchanged.
const MAX_CONCURRENT_FETCHES: usize = 8;

async fn collect_usage_output(command: &UsageCommand) -> UsageOutput {
    use futures::stream::StreamExt;
    match command.format {
        OutputFormat::Text => {
            let sections = futures::stream::iter(
                command
                    .providers
                    .iter()
                    .map(|id| fetch_provider_text_output(*id, command)),
            )
            .buffered(MAX_CONCURRENT_FETCHES)
            .collect::<Vec<_>>()
            .await;
            UsageOutput::Text(sections)
        }
        OutputFormat::Json => {
            let results = futures::stream::iter(
                command
                    .providers
                    .iter()
                    .map(|id| fetch_provider_json_output(*id, command)),
            )
            .buffered(MAX_CONCURRENT_FETCHES)
            .collect::<Vec<_>>()
            .await;
            UsageOutput::Json {
                results,
                pretty: command.pretty,
            }
        }
    }
}

async fn fetch_provider_text_output(provider_id: ProviderId, command: &UsageCommand) -> String {
    match fetch_provider_result(provider_id, command).await {
        Ok((result, status)) => {
            render_text_with_status(provider_id, &result, status.as_ref(), command.use_color)
        }
        Err(e) => render_text_error(provider_id, &e.to_string(), command.use_color),
    }
}

async fn fetch_provider_json_output(
    provider_id: ProviderId,
    command: &UsageCommand,
) -> serde_json::Value {
    match fetch_provider_result(provider_id, command).await {
        Ok((result, status)) => render_json_result(provider_id, result, status.as_ref()),
        Err(e) => serde_json::json!({
            "provider": provider_id.cli_name(),
            "error": e.to_string(),
        }),
    }
}

/// Overall wall-clock bound for a single provider's fetch (+ optional status),
/// generous relative to the per-request web timeout. Without this a provider
/// with an untimed subprocess/network probe (e.g. a hung CLI) would stall the
/// whole `usage`/`serve` command indefinitely.
fn provider_fetch_timeout(command: &UsageCommand) -> std::time::Duration {
    std::time::Duration::from_secs(command.ctx.web_timeout.saturating_mul(2).saturating_add(20))
}

async fn fetch_provider_result(
    provider_id: ProviderId,
    command: &UsageCommand,
) -> anyhow::Result<(ProviderFetchResult, Option<StatusInfo>)> {
    let timeout = provider_fetch_timeout(command);
    let fetch = async {
        let provider = instantiate_provider(provider_id);
        let status_future = command
            .fetch_status
            .then(|| fetch_provider_status(provider_id.cli_name()));
        let result = provider.fetch_usage(&command.ctx).await?;
        let status = if let Some(fut) = status_future {
            fut.await
        } else {
            None
        };
        anyhow::Ok((result, status))
    };

    match tokio::time::timeout(timeout, fetch).await {
        Ok(inner) => inner,
        Err(_) => anyhow::bail!(
            "Timed out fetching {} after {}s",
            provider_id.cli_name(),
            timeout.as_secs()
        ),
    }
}

fn render_text_error(provider_id: ProviderId, error_msg: &str, use_color: bool) -> String {
    let header = if use_color {
        format!("\x1b[1m{}\x1b[0m", provider_id.display_name())
    } else {
        provider_id.display_name().to_string()
    };
    format!("{}  Error: {}", header, error_msg)
}

fn render_json_result(
    provider_id: ProviderId,
    result: ProviderFetchResult,
    status: Option<&StatusInfo>,
) -> serde_json::Value {
    let mut json_result = serde_json::json!({
        "provider": provider_id.cli_name(),
        "source": result.source_label,
        "usage": result.usage,
        "cost": result.cost,
    });

    if let Some(s) = status {
        json_result["status"] = serde_json::json!({
            "level": format!("{:?}", s.level).to_lowercase(),
            "description": s.description,
        });
    }

    json_result
}

fn print_usage_output(output: UsageOutput) -> anyhow::Result<()> {
    match output {
        UsageOutput::Text(sections) => {
            println!("{}", sections.join("\n\n"));
        }
        UsageOutput::Json { results, pretty } => {
            let output = if pretty {
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
    let metadata = instantiate_provider(provider).metadata().clone();

    lines.push(render_usage_header(provider, result, status, use_color));
    append_status_line(&mut lines, status);
    append_account_lines(&mut lines, &result.usage);
    append_usage_window_lines(&mut lines, &result.usage, &metadata, use_color);
    append_cost_line(&mut lines, result.cost.as_ref());

    lines.join("\n")
}

fn render_usage_header(
    provider: ProviderId,
    result: &ProviderFetchResult,
    status: Option<&StatusInfo>,
    use_color: bool,
) -> String {
    let status_indicator = render_status_indicator(status, use_color);
    if use_color {
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
    }
}

fn render_status_indicator(status: Option<&StatusInfo>, use_color: bool) -> String {
    let Some(status) = status else {
        return String::new();
    };

    let (symbol, color) = match status.level {
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
}

fn append_status_line(lines: &mut Vec<String>, status: Option<&StatusInfo>) {
    if let Some(s) = status
        && s.level != StatusLevel::Operational
        && s.level != StatusLevel::Unknown
    {
        lines.push(format!("  Status: {}", s.description));
    }
}

fn append_account_lines(lines: &mut Vec<String>, usage: &UsageSnapshot) {
    if let Some(ref email) = usage.account_email {
        lines.push(format!("  Account: {}", email));
    }
    if let Some(ref method) = usage.login_method {
        lines.push(format!("  Plan:    {}", method));
    }
}

fn append_usage_window_lines(
    lines: &mut Vec<String>,
    usage: &UsageSnapshot,
    metadata: &crate::core::ProviderMetadata,
    use_color: bool,
) {
    append_window_line(lines, metadata.session_label, &usage.primary, use_color);
    append_secondary_window_line(
        lines,
        usage.secondary.as_ref(),
        metadata.weekly_label,
        use_color,
    );
    append_model_specific_line(lines, usage.model_specific.as_ref(), use_color);
}

fn append_window_line(lines: &mut Vec<String>, label: &str, window: &RateWindow, use_color: bool) {
    let bar = render_progress_bar(window.used_percent, 20, use_color);
    let reset = window
        .format_countdown()
        .map(|c| format!(" (resets in {})", c))
        .unwrap_or_default();
    lines.push(format!(
        "  {:<8} {} {:.0}% used{}",
        format!("{}:", label),
        bar,
        window.used_percent,
        reset
    ));
}

fn append_secondary_window_line(
    lines: &mut Vec<String>,
    secondary: Option<&RateWindow>,
    label: &str,
    use_color: bool,
) {
    if let Some(secondary) = secondary {
        append_window_line(lines, label, secondary, use_color);
        let window_minutes = secondary.window_minutes.unwrap_or(10080);
        if let Some(pace) = UsagePace::weekly(secondary, None, window_minutes) {
            lines.push(format!(
                "  Pace:    {} {}",
                pace.stage.emoji(),
                pace.format_status()
            ));
        }
    }
}

fn append_model_specific_line(
    lines: &mut Vec<String>,
    model_specific: Option<&RateWindow>,
    use_color: bool,
) {
    if let Some(opus) = model_specific {
        let opus_bar = render_progress_bar(opus.used_percent, 20, use_color);
        lines.push(format!(
            "  Opus:    {} {:.0}% used",
            opus_bar, opus.used_percent
        ));
    }
}

fn append_cost_line(lines: &mut Vec<String>, cost: Option<&CostSnapshot>) {
    let Some(cost) = cost else {
        return;
    };

    if let Some(limit) = cost.format_limit() {
        lines.push(format!(
            "  Cost:    {} / {} ({})",
            cost.format_used(),
            limit,
            cost.period
        ));
    } else {
        lines.push(format!(
            "  Cost:    {} ({})",
            cost.format_used(),
            cost.period
        ));
    }
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
