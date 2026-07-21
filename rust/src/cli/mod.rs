//! CLI module - command-line interface
//!
//! Matches the original CodexBar CLI structure:
//! - `codexbar` - launches the menu bar GUI app (default)
//! - `codexbar usage` - print usage from providers
//! - `codexbar cost` - print local token cost usage
//! - `codexbar autostart` - manage Windows auto-start

#![allow(dead_code)]

pub mod account;
pub mod autostart;
pub mod config;
pub mod cost;
pub mod diagnose;
pub mod guard;
pub mod serve;
pub mod sessions;
pub mod tty_runner;
pub mod usage;

use clap::{Parser, Subcommand};

/// Exit codes matching original CodexBar
pub mod exit_codes {
    pub const SUCCESS: i32 = 0;
    pub const UNEXPECTED_FAILURE: i32 = 1;
    /// Guard: remaining quota below `--min-remaining` threshold.
    pub const GUARD_BLOCKED: i32 = 1;
    pub const PROVIDER_MISSING: i32 = 2;
    pub const PARSE_ERROR: i32 = 3;
    pub const CLI_TIMEOUT: i32 = 4;
    /// Invalid CLI arguments (`EX_USAGE`).
    pub const USAGE_ERROR: i32 = 64;
    /// Quota could not be checked (`EX_UNAVAILABLE`); used by `codexbar guard`.
    pub const UNAVAILABLE: i32 = 69;
}

/// CodexBar - Monitor AI provider usage limits
///
/// CLI for inspecting provider usage and managing local config. The desktop
/// menubar shell now lives in `apps/desktop-tauri/`; this binary is CLI-only.
#[derive(Parser, Debug)]
#[command(name = "codexbar")]
#[command(author, version, about, long_about = None)]
pub struct Cli {
    // === Global flags ===
    /// Enable verbose logging
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Emit machine-readable logs (JSON) to stderr
    #[arg(long = "json-output", global = true)]
    pub json_output: bool,

    /// Set log level (trace, debug, info, warn, error)
    #[arg(long = "log-level", global = true, value_parser = ["trace", "verbose", "debug", "info", "warning", "warn", "error", "critical"])]
    pub log_level: Option<String>,

    /// Disable ANSI colors in output
    #[arg(long = "no-color", global = true)]
    pub no_color: bool,

    #[command(subcommand)]
    pub command: Option<Commands>,

    // === Top-level args for the default usage command ===
    #[arg(short, long, help = usage::PROVIDER_ARG_HELP)]
    pub provider: Option<String>,

    /// Output format: text or json
    #[arg(short, long, value_parser = ["text", "json"])]
    pub format: Option<String>,

    /// Shorthand for --format json
    #[arg(long)]
    pub json: bool,

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

    /// Skip credits line in output
    #[arg(long = "no-credits")]
    pub no_credits: bool,

    /// Data source: auto, web, cli, oauth
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

    /// Print one compact usage line per provider
    #[arg(long)]
    pub brief: bool,
}

#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Print usage from enabled providers as text or JSON (default command)
    Usage(usage::UsageArgs),

    /// Print local token cost usage (Claude + Codex) without web/CLI access
    Cost(cost::CostArgs),

    /// Gate automation on one provider's remaining quota
    Guard(guard::GuardArgs),

    /// Export safe provider diagnostics as JSON
    Diagnose(diagnose::DiagnoseArgs),

    /// List or focus local and configured remote agent sessions
    Sessions(sessions::SessionsArgs),

    /// Serve usage and cost JSON on 127.0.0.1
    Serve(serve::ServeArgs),

    /// Manage auto-start on Windows boot
    Autostart(autostart::AutostartArgs),

    /// Manage token accounts for providers
    Account(account::AccountArgs),

    /// Configuration utilities
    Config(config::ConfigArgs),
}

impl Cli {
    /// Convert top-level args to UsageArgs for default command
    pub fn to_usage_args(&self) -> usage::UsageArgs {
        usage::UsageArgs {
            provider: self.provider.clone(),
            format: if self.json {
                usage::OutputFormat::Json
            } else if let Some(ref f) = self.format {
                f.parse().unwrap_or_default()
            } else {
                usage::OutputFormat::Text
            },
            json: self.json,
            no_credits: self.no_credits,
            no_color: self.no_color,
            pretty: self.pretty,
            status: self.status,
            all_accounts: self.all_accounts,
            account: self.account.clone(),
            source: self.source.clone(),
            web_timeout: self.web_timeout,
            web_debug_dump_html: self.web_debug_dump_html,
            antigravity_plan_debug: self.antigravity_plan_debug,
            brief: self.brief,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn top_level_help_mentions_nanogpt_provider() {
        let mut command = Cli::command();
        let mut output = Vec::new();
        command
            .write_long_help(&mut output)
            .expect("top-level help should render");

        let help = String::from_utf8(output).expect("help should be valid utf-8");
        assert!(help.contains("nanogpt"));
    }

    #[test]
    fn usage_subcommand_help_mentions_nanogpt_provider() {
        let mut command = Cli::command();
        let usage = command
            .find_subcommand_mut("usage")
            .expect("usage subcommand should exist");
        let mut output = Vec::new();
        usage
            .write_long_help(&mut output)
            .expect("usage help should render");

        let help = String::from_utf8(output).expect("help should be valid utf-8");
        assert!(help.contains("nanogpt"));
    }
}
