//! `codexbar hooks` — list / enable / disable / test external hook rules.

use clap::{Args, Subcommand};
use serde::Serialize;

use crate::core::{HookEvent, HookEventType, HookRunner, HooksConfig, ProviderId};
use crate::settings::Settings;

#[derive(Args, Debug, Clone)]
pub struct HooksArgs {
    #[command(subcommand)]
    pub command: HooksCommand,
}

#[derive(Subcommand, Debug, Clone)]
pub enum HooksCommand {
    /// Print configured hook rules
    List(HooksListArgs),
    /// Enable hooks in hooks.json (master switch)
    Enable(HooksToggleArgs),
    /// Disable hooks in hooks.json (master switch)
    Disable(HooksToggleArgs),
    /// Run matching rules for a sample event
    Test(HooksTestArgs),
}

#[derive(Args, Debug, Clone)]
pub struct HooksListArgs {
    /// Emit JSON
    #[arg(long)]
    pub json: bool,
    /// Pretty-print JSON
    #[arg(long)]
    pub pretty: bool,
}

#[derive(Args, Debug, Clone)]
pub struct HooksToggleArgs {
    /// Emit JSON
    #[arg(long)]
    pub json: bool,
    /// Pretty-print JSON
    #[arg(long)]
    pub pretty: bool,
}

#[derive(Args, Debug, Clone)]
pub struct HooksTestArgs {
    /// Event name (quota_low, quota_reached, quota_reset, provider_unavailable, provider_recovered, refresh_failed)
    pub event: String,
    /// Provider CLI name
    #[arg(long)]
    pub provider: String,
    /// Emit JSON
    #[arg(long)]
    pub json: bool,
    /// Pretty-print JSON
    #[arg(long)]
    pub pretty: bool,
}

#[derive(Debug, Serialize)]
struct HooksListJson {
    enabled: bool,
    settings_hooks_enabled: bool,
    path: Option<String>,
    rules: Vec<HookRuleListItem>,
}

#[derive(Debug, Serialize)]
struct HookRuleListItem {
    enabled: bool,
    event: Option<String>,
    events: Vec<String>,
    provider: Option<String>,
    executable: String,
    arguments: Vec<String>,
    timeout_secs: u64,
}

#[derive(Debug, Serialize)]
struct HookTestResult {
    executable: String,
    event: String,
    provider: String,
    ok: bool,
    error: Option<String>,
}

pub async fn run(args: HooksArgs) -> anyhow::Result<()> {
    match args.command {
        HooksCommand::List(a) => run_list(a),
        HooksCommand::Enable(a) => run_set_enabled(true, a),
        HooksCommand::Disable(a) => run_set_enabled(false, a),
        HooksCommand::Test(a) => run_test(a),
    }
}

fn run_list(args: HooksListArgs) -> anyhow::Result<()> {
    let config = HooksConfig::load();
    let settings = Settings::load();
    let path = HooksConfig::path().map(|p| p.display().to_string());

    if args.json {
        let payload = HooksListJson {
            enabled: config.enabled,
            settings_hooks_enabled: settings.hooks_enabled,
            path,
            rules: config
                .events
                .iter()
                .map(|r| HookRuleListItem {
                    enabled: r.enabled,
                    event: r.event.map(|e| e.as_str().to_string()),
                    events: r.events.iter().map(|e| e.as_str().to_string()).collect(),
                    provider: r.provider.clone(),
                    executable: r.executable.display().to_string(),
                    arguments: r.arguments.clone(),
                    timeout_secs: r.timeout_secs,
                })
                .collect(),
        };
        print_json(&payload, args.pretty)?;
        return Ok(());
    }

    println!(
        "Hooks: {} (settings toggle: {})",
        if config.enabled {
            "enabled"
        } else {
            "disabled"
        },
        if settings.hooks_enabled { "on" } else { "off" }
    );
    if let Some(path) = path {
        println!("Config: {path}");
    }
    if config.events.is_empty() {
        println!("No rules configured.");
        return Ok(());
    }
    for rule in &config.events {
        let state = if rule.enabled { "on" } else { "off" };
        let event = rule
            .event
            .map(|e| e.as_str().to_string())
            .or_else(|| {
                (!rule.events.is_empty()).then(|| {
                    rule.events
                        .iter()
                        .map(|e| e.as_str())
                        .collect::<Vec<_>>()
                        .join(",")
                })
            })
            .unwrap_or_else(|| "any".into());
        let provider = rule.provider.as_deref().unwrap_or("any");
        let command = std::iter::once(rule.executable.display().to_string())
            .chain(rule.arguments.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ");
        println!("[{state}] {event} provider={provider}: {command}");
    }
    Ok(())
}

fn run_set_enabled(enabled: bool, args: HooksToggleArgs) -> anyhow::Result<()> {
    let mut config = HooksConfig::load();
    config.enabled = enabled;
    let path = config.save().map_err(anyhow::Error::msg)?;
    if args.json {
        print_json(
            &serde_json::json!({
                "enabled": config.enabled,
                "path": path.display().to_string(),
            }),
            args.pretty,
        )?;
    } else {
        println!(
            "Hooks: {} ({})",
            if enabled { "enabled" } else { "disabled" },
            path.display()
        );
    }
    Ok(())
}

fn run_test(args: HooksTestArgs) -> anyhow::Result<()> {
    let event_type = parse_event(&args.event)?;
    let provider = ProviderId::from_cli_name(&args.provider).ok_or_else(|| {
        anyhow::anyhow!(
            "Unknown provider '{}'. Use a CLI name such as codex or claude.",
            args.provider
        )
    })?;

    let event = sample_event(event_type, provider.cli_name());
    let config = HooksConfig::load();
    if !config.enabled {
        anyhow::bail!("Hooks are disabled. Run `codexbar hooks enable` first.");
    }
    let settings = Settings::load();
    if !settings.hooks_enabled {
        anyhow::bail!(
            "Hooks are disabled in Settings (hooks_enabled=false). Enable them in Advanced."
        );
    }

    let rules = config.matching_rules(&event);
    if rules.is_empty() {
        anyhow::bail!(
            "No hook rule matches {} for {}.",
            event_type.as_str(),
            provider.cli_name()
        );
    }

    let base_env = std::env::vars().collect();
    let mut results = Vec::new();
    for rule in rules {
        match HookRunner::run(rule, &event, &base_env) {
            Ok(()) => results.push(HookTestResult {
                executable: rule.executable.display().to_string(),
                event: event_type.as_str().into(),
                provider: provider.cli_name().into(),
                ok: true,
                error: None,
            }),
            Err(err) => results.push(HookTestResult {
                executable: rule.executable.display().to_string(),
                event: event_type.as_str().into(),
                provider: provider.cli_name().into(),
                ok: false,
                error: Some(err),
            }),
        }
    }

    if args.json {
        print_json(&results, args.pretty)?;
    } else {
        for r in &results {
            if r.ok {
                println!("ok  {} ({})", r.executable, r.event);
            } else {
                println!(
                    "err {} ({}) — {}",
                    r.executable,
                    r.event,
                    r.error.as_deref().unwrap_or("error")
                );
            }
        }
    }

    if results.iter().any(|r| !r.ok) {
        anyhow::bail!("one or more hooks failed");
    }
    Ok(())
}

fn sample_event(event: HookEventType, provider: &str) -> HookEvent {
    let mut e = HookEvent::new(event, provider).with_window("session");
    match event {
        HookEventType::QuotaLow => e = e.with_used_percent(85.0),
        HookEventType::QuotaReached => e = e.with_used_percent(100.0),
        HookEventType::QuotaReset => e = e.with_used_percent(5.0),
        HookEventType::ProviderUnavailable => e = e.with_status("unavailable"),
        HookEventType::ProviderRecovered => e = e.with_status("ok"),
        HookEventType::RefreshFailed => e = e.with_status("refresh_failed"),
    }
    e
}

fn parse_event(raw: &str) -> anyhow::Result<HookEventType> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "quota_low" => Ok(HookEventType::QuotaLow),
        "quota_reached" => Ok(HookEventType::QuotaReached),
        "quota_reset" => Ok(HookEventType::QuotaReset),
        "provider_unavailable" => Ok(HookEventType::ProviderUnavailable),
        "provider_recovered" => Ok(HookEventType::ProviderRecovered),
        "refresh_failed" => Ok(HookEventType::RefreshFailed),
        other => anyhow::bail!(
            "Unknown event '{other}'. Use one of: quota_low, quota_reached, quota_reset, provider_unavailable, provider_recovered, refresh_failed."
        ),
    }
}

fn print_json<T: Serialize>(value: &T, pretty: bool) -> anyhow::Result<()> {
    if pretty {
        println!("{}", serde_json::to_string_pretty(value)?);
    } else {
        println!("{}", serde_json::to_string(value)?);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_event_names() {
        assert!(matches!(
            parse_event("quota_low").unwrap(),
            HookEventType::QuotaLow
        ));
        assert!(parse_event("nope").is_err());
    }

    #[test]
    fn sample_quota_low_has_remaining() {
        let e = sample_event(HookEventType::QuotaLow, "claude");
        assert!(e.remaining_percent.unwrap() < 20.0);
        assert_eq!(e.provider, "claude");
        assert!(e.environment_variables().contains_key("CODEXBAR_PROVIDER"));
    }
}
