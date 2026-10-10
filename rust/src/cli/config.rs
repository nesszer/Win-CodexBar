//! Config command implementation
//!
//! Utilities for validating and inspecting configuration.

use clap::{Parser, Subcommand};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::path::Path;

use super::usage::OutputFormat;
use crate::core::{ProviderId, TokenAccountStore, instantiate_provider};
use crate::settings::{ApiKeys, ManualCookies, PreferencesDocument, Settings};

/// Arguments for the config command
#[derive(Parser, Debug)]
pub struct ConfigArgs {
    #[command(subcommand)]
    pub command: ConfigCommand,
}

#[derive(Subcommand, Debug)]
pub enum ConfigCommand {
    /// Validate configuration files
    Validate,
    /// Dump configuration to stdout
    Dump {
        /// Output format: json or toml
        #[arg(short, long, default_value = "json")]
        format: String,
        /// Include raw secrets (default: redact)
        #[arg(long = "show-secrets", default_value_t = false)]
        show_secrets: bool,
    },
    /// List providers and enabled state
    Providers {
        #[command(flatten)]
        output: ConfigOutputArgs,
    },
    /// Enable a provider
    Enable {
        #[command(flatten)]
        target: ConfigProviderArg,
        #[command(flatten)]
        output: ConfigOutputArgs,
    },
    /// Disable a provider
    Disable {
        #[command(flatten)]
        target: ConfigProviderArg,
        #[command(flatten)]
        output: ConfigOutputArgs,
    },
    /// Store an API key for a provider
    SetApiKey {
        #[command(flatten)]
        target: ConfigProviderArg,
        /// API key to store
        #[arg(long = "api-key")]
        api_key: Option<String>,
        /// Read API key from stdin
        #[arg(long)]
        stdin: bool,
        /// Store the key without enabling the provider
        #[arg(long = "no-enable")]
        no_enable: bool,
    },
    /// Show configuration file paths
    Path,
    /// Export or import portable preferences (no secrets, no machine state)
    Preferences {
        #[command(subcommand)]
        action: PreferencesAction,
    },
    /// Allow or deny reading (and refreshing) Claude Code's own credentials
    /// (~/.claude/.credentials.json or Credential Manager), or show the
    /// current choice. Off by default; without it Claude Auto falls back to
    /// reduced-fidelity CLI usage.
    ClaudeCodeCredentials {
        /// allow, deny, or status
        #[arg(value_enum)]
        action: ConsentAction,
        #[command(flatten)]
        output: ConfigOutputArgs,
    },
}

#[derive(Subcommand, Debug)]
pub enum PreferencesAction {
    /// Write portable preferences as JSON (to stdout unless --file is given)
    Export {
        /// Destination file
        #[arg(long)]
        file: Option<std::path::PathBuf>,
    },
    /// Apply a preferences file; restart a running CodexBar afterwards
    Import {
        /// Preferences file to read
        #[arg(long)]
        file: std::path::PathBuf,
    },
}

/// `config claude-code-credentials` action.
#[derive(clap::ValueEnum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsentAction {
    /// Let CodexBar read and refresh Claude Code's credentials
    Allow,
    /// Keep Claude Code's credentials closed (the default)
    Deny,
    /// Show the current choice without changing it
    Status,
}

/// `--format`, `--json`, and `--pretty` for the config subcommands that
/// report a result (upstream `CLICommonOptions`).
#[derive(clap::Args, Debug, Clone, Copy, Default)]
pub struct ConfigOutputArgs {
    /// Output format: text or json
    #[arg(short, long, default_value = "text")]
    pub format: OutputFormat,

    /// Shorthand for --format json
    #[arg(long)]
    pub json: bool,

    /// Pretty-print JSON output
    #[arg(long)]
    pub pretty: bool,
}

impl ConfigOutputArgs {
    fn is_json(self) -> bool {
        self.json || self.format == OutputFormat::Json
    }

    fn print_json<T: Serialize>(self, value: &T) -> anyhow::Result<()> {
        super::print_json(value, self.pretty)
    }
}

/// The provider a config subcommand acts on: positional, or `-p/--provider`
/// as upstream spells it.
#[derive(clap::Args, Debug, Clone, Default)]
pub struct ConfigProviderArg {
    /// Provider CLI name or alias
    #[arg(value_name = "PROVIDER", required_unless_present = "provider_option")]
    pub provider: Option<String>,

    /// Provider CLI name or alias (same as the positional PROVIDER)
    #[arg(
        id = "provider_option",
        short = 'p',
        long = "provider",
        value_name = "PROVIDER",
        conflicts_with = "provider"
    )]
    pub provider_option: Option<String>,
}

impl ConfigProviderArg {
    fn name(&self) -> &str {
        self.provider_option
            .as_deref()
            .or(self.provider.as_deref())
            .unwrap_or_default()
    }
}

/// Run the config command
pub async fn run(args: ConfigArgs) -> anyhow::Result<()> {
    match args.command {
        ConfigCommand::Validate => validate_config().await,
        ConfigCommand::Dump {
            format,
            show_secrets,
        } => dump_config(&format, show_secrets).await,
        ConfigCommand::Providers { output } => list_providers(output).await,
        ConfigCommand::Enable { target, output } => {
            set_provider_enabled(target.name(), true, output).await
        }
        ConfigCommand::Disable { target, output } => {
            set_provider_enabled(target.name(), false, output).await
        }
        ConfigCommand::SetApiKey {
            target,
            api_key,
            stdin,
            no_enable,
        } => set_api_key(target.name(), api_key.as_deref(), stdin, !no_enable).await,
        ConfigCommand::Path => show_paths().await,
        ConfigCommand::Preferences { action } => transfer_preferences(action),
        ConfigCommand::ClaudeCodeCredentials { action, output } => {
            claude_code_credentials(action, output).await
        }
    }
}

/// Export or import the portable preferences document.
fn transfer_preferences(action: PreferencesAction) -> anyhow::Result<()> {
    match action {
        PreferencesAction::Export { file } => {
            let document = PreferencesDocument::from_settings(&Settings::load())?;
            match file {
                Some(path) => {
                    document.write_file(&path)?;
                    println!(
                        "Config: exported {} preferences to {}",
                        document.len(),
                        path.display()
                    );
                }
                None => print!("{}", document.to_json()),
            }
        }
        PreferencesAction::Import { file: path } => {
            let document = PreferencesDocument::read_file(&path)?;
            let mut settings = Settings::load();
            let applied = document.apply_to(&mut settings)?;
            settings.save()?;
            println!(
                "Config: imported {applied} preferences from {}. Restart CodexBar to apply them to a running app.",
                path.display()
            );
        }
    }
    Ok(())
}

/// Validate configuration files
async fn validate_config() -> anyhow::Result<()> {
    let mut report = ValidationReport::default();
    validate_settings_config(&mut report);
    validate_manual_cookies_config(&mut report);
    validate_token_accounts_config(&mut report);
    report.finish()
}

#[derive(Default)]
struct ValidationReport {
    errors: Vec<String>,
    warnings: Vec<String>,
}

impl ValidationReport {
    fn error(&mut self, message: impl Into<String>) {
        self.errors.push(message.into());
    }

    fn warning(&mut self, message: impl Into<String>) {
        self.warnings.push(message.into());
    }

    fn finish(self) -> anyhow::Result<()> {
        print_validation_summary(&self.errors, &self.warnings)
    }
}

fn validate_settings_config(report: &mut ValidationReport) {
    print!("Checking settings.json... ");
    let Some(path) = Settings::settings_path() else {
        println!("ERROR");
        report.error("settings.json: Could not determine config path");
        return;
    };

    if validate_optional_json_file::<Settings>(&path, "settings.json", report) {
        return;
    }

    println!("NOT FOUND (using defaults)");
    report.warning("settings.json: File does not exist, using defaults");
}

fn validate_manual_cookies_config(report: &mut ValidationReport) {
    print!("Checking manual_cookies.json... ");
    let Some(path) = ManualCookies::cookies_path() else {
        println!("SKIP");
        return;
    };

    if !validate_optional_json_file::<ManualCookies>(&path, "manual_cookies.json", report) {
        println!("NOT FOUND (none configured)");
    }
}

fn validate_optional_json_file<T>(path: &Path, label: &str, report: &mut ValidationReport) -> bool
where
    T: DeserializeOwned,
{
    if !path.exists() {
        return false;
    }

    match read_json_config::<T>(path) {
        Ok(()) => println!("OK"),
        Err(ConfigFileError::Read(e)) => {
            println!("ERROR");
            report.error(format!("{label}: Could not read file: {e}"));
        }
        Err(ConfigFileError::Parse(e)) => {
            println!("INVALID");
            report.error(format!("{label}: {e}"));
        }
    }

    true
}

fn read_json_config<T>(path: &Path) -> Result<(), ConfigFileError>
where
    T: DeserializeOwned,
{
    // Match the application loaders so validation supports DPAPI-protected
    // config files without materializing plaintext on disk.
    let content = crate::secure_file::read_string(path).map_err(ConfigFileError::Read)?;
    serde_json::from_str::<T>(&content)
        .map(|_| ())
        .map_err(ConfigFileError::Parse)
}

enum ConfigFileError {
    Read(std::io::Error),
    Parse(serde_json::Error),
}

fn validate_token_accounts_config(report: &mut ValidationReport) {
    print!("Checking token-accounts.json... ");
    let path = TokenAccountStore::default_path();
    if !path.exists() {
        println!("NOT FOUND (none configured)");
        return;
    }

    match TokenAccountStore::new().load() {
        Ok(_) => println!("OK"),
        Err(e) => {
            println!("INVALID");
            report.error(format!("token-accounts.json: {e}"));
        }
    }
}

fn print_validation_summary(errors: &[String], warnings: &[String]) -> anyhow::Result<()> {
    println!();
    if errors.is_empty() && warnings.is_empty() {
        println!("Configuration is valid.");
    } else {
        if !warnings.is_empty() {
            println!("Warnings:");
            for w in warnings {
                println!("  - {}", w);
            }
        }
        if !errors.is_empty() {
            println!("Errors:");
            for e in errors {
                println!("  - {}", e);
            }
            anyhow::bail!(
                "Configuration validation failed with {} error(s).",
                errors.len()
            );
        }
    }

    Ok(())
}

/// Dump configuration to stdout
async fn dump_config(format: &str, show_secrets: bool) -> anyhow::Result<()> {
    let value = build_dump_value()?;
    let value = sanitize_settings_for_dump(value, show_secrets);

    match format.to_lowercase().as_str() {
        "json" => {
            let json = serde_json::to_string_pretty(&value)?;
            println!("{}", json);
        }
        "toml" => {
            let toml = toml::to_string_pretty(&value)
                .map_err(|e| anyhow::anyhow!("Failed to convert dump to TOML: {e}"))?;
            println!("{}", toml);
        }
        _ => {
            anyhow::bail!("Unknown format '{}'. Supported formats: json, toml", format);
        }
    }

    Ok(())
}

fn build_dump_value() -> anyhow::Result<serde_json::Value> {
    let settings = Settings::load();
    let mut root = serde_json::to_value(&settings)?;

    if let Some(obj) = root.as_object_mut() {
        obj.insert(
            "api_keys".to_string(),
            serde_json::to_value(ApiKeys::load())?,
        );
        obj.insert(
            "manual_cookies".to_string(),
            serde_json::to_value(ManualCookies::load())?,
        );

        let token_accounts = TokenAccountStore::new()
            .load()
            .unwrap_or_default()
            .into_iter()
            .map(|(id, data)| (id.cli_name().to_string(), data))
            .collect::<std::collections::BTreeMap<_, _>>();
        obj.insert(
            "token_accounts".to_string(),
            serde_json::to_value(token_accounts)?,
        );
    }

    Ok(root)
}

/// Recursively redact secret-shaped fields for `config dump`.
///
/// When `show_secrets` is true the value is returned unchanged.
fn sanitize_settings_for_dump(value: serde_json::Value, show_secrets: bool) -> serde_json::Value {
    if show_secrets {
        return value;
    }
    redact_secrets_value(value)
}

fn is_secret_field_name(key: &str) -> bool {
    let normalized: String = key
        .chars()
        .filter(|c| *c != '_')
        .flat_map(|c| c.to_lowercase())
        .collect();
    matches!(
        normalized.as_str(),
        "apikey"
            | "secretkey"
            | "cookieheader"
            | "manualcookieheader"
            | "token"
            | "apitoken"
            | "managementapitoken"
            | "httpproxypassword"
            | "password"
    )
}

fn redact_secrets_value(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (key, child) in map {
                if is_secret_field_name(&key) {
                    out.insert(key, serde_json::Value::String("[REDACTED]".to_string()));
                } else {
                    out.insert(key, redact_secrets_value(child));
                }
            }
            serde_json::Value::Object(out)
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(redact_secrets_value).collect())
        }
        other => other,
    }
}

/// One `config providers` row; the JSON shape matches upstream's
/// `ConfigProviderStatusResult`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfigProviderStatus {
    provider: String,
    display_name: String,
    enabled: bool,
    default_enabled: bool,
}

impl ConfigProviderStatus {
    fn text_line(&self) -> String {
        let state = if self.enabled { "enabled" } else { "disabled" };
        let default_marker = if self.default_enabled { " default" } else { "" };
        format!(
            "{}: {state}{default_marker} ({})",
            self.provider, self.display_name
        )
    }
}

fn provider_statuses(settings: &Settings) -> Vec<ConfigProviderStatus> {
    ProviderId::all()
        .iter()
        .filter(|id| settings.is_provider_listed(**id))
        .map(|id| ConfigProviderStatus {
            provider: id.cli_name().to_string(),
            display_name: id.display_name().to_string(),
            enabled: settings.is_provider_enabled(*id),
            default_enabled: instantiate_provider(*id).metadata().default_enabled,
        })
        .collect()
}

/// List provider enabled state.
async fn list_providers(output: ConfigOutputArgs) -> anyhow::Result<()> {
    let statuses = provider_statuses(&Settings::load());
    if output.is_json() {
        return output.print_json(&statuses);
    }
    for status in &statuses {
        println!("{}", status.text_line());
    }
    Ok(())
}

/// `config enable|disable` JSON result (upstream `ConfigProviderToggleResult`).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfigProviderToggle {
    provider: String,
    display_name: String,
    enabled: bool,
    config_path: Option<String>,
}

/// Enable or disable a provider by CLI name.
async fn set_provider_enabled(
    provider: &str,
    enabled: bool,
    output: ConfigOutputArgs,
) -> anyhow::Result<()> {
    let id = parse_provider(provider)?;
    let mut settings = Settings::load();
    if enabled {
        settings.enable_provider(id);
    } else {
        settings.disable_provider(id);
    }
    settings.save()?;
    if output.is_json() {
        return output.print_json(&ConfigProviderToggle {
            provider: id.cli_name().to_string(),
            display_name: id.display_name().to_string(),
            enabled,
            config_path: Settings::settings_path().map(|path| path.display().to_string()),
        });
    }
    let state = if enabled { "enabled" } else { "disabled" };
    println!("Config: {state} {}", id.display_name());
    Ok(())
}

/// `config claude-code-credentials` JSON result.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeCodeCredentialsConsent {
    allowed: bool,
    config_path: Option<String>,
}

/// Apply a consent action to the settings `claude_allow_reading_claude_code_credentials`
/// flag; returns whether it changed (and so needs saving).
fn apply_consent_action(settings: &mut Settings, action: ConsentAction) -> bool {
    let allowed = match action {
        ConsentAction::Allow => true,
        ConsentAction::Deny => false,
        ConsentAction::Status => return false,
    };
    let changed = settings.claude_allow_reading_claude_code_credentials != allowed;
    settings.claude_allow_reading_claude_code_credentials = allowed;
    changed
}

fn consent_text(allowed: bool) -> &'static str {
    if allowed {
        "Claude Code credentials: allowed (CodexBar may read and refresh Claude Code's OAuth credentials)"
    } else {
        "Claude Code credentials: not allowed (Claude Auto falls back to reduced-fidelity CLI usage)"
    }
}

/// Allow, deny, or show consent to read Claude Code's own credentials.
async fn claude_code_credentials(
    action: ConsentAction,
    output: ConfigOutputArgs,
) -> anyhow::Result<()> {
    let mut settings = Settings::load();
    if apply_consent_action(&mut settings, action) {
        settings.save()?;
    }
    let allowed = settings.claude_allow_reading_claude_code_credentials;
    if output.is_json() {
        return output.print_json(&ClaudeCodeCredentialsConsent {
            allowed,
            config_path: Settings::settings_path().map(|path| path.display().to_string()),
        });
    }
    println!("{}", consent_text(allowed));
    Ok(())
}

/// Store an API key and optionally enable the provider.
async fn set_api_key(
    provider: &str,
    api_key: Option<&str>,
    read_from_stdin: bool,
    enable_provider: bool,
) -> anyhow::Result<()> {
    let id = parse_provider(provider)?;
    ensure_provider_accepts_api_key(id)?;
    let api_key = resolve_api_key_input(api_key, read_from_stdin)?;

    let mut keys = ApiKeys::load();
    keys.set(id.cli_name(), &api_key, None);
    keys.save()?;

    if enable_provider {
        let mut settings = Settings::load();
        settings.enable_provider(id);
        settings.save()?;
    }

    let suffix = if enable_provider { " and enabled" } else { "" };
    println!("Config: stored API key for {}{suffix}", id.display_name());
    Ok(())
}

fn parse_provider(raw: &str) -> anyhow::Result<ProviderId> {
    ProviderId::from_cli_name(raw).ok_or_else(|| {
        anyhow::anyhow!(
            "Unknown provider '{}'. Run `codexbar config providers` to list providers.",
            raw
        )
    })
}

fn ensure_provider_accepts_api_key(id: ProviderId) -> anyhow::Result<()> {
    if crate::settings::get_api_key_providers()
        .iter()
        .any(|provider| provider.id == id)
    {
        return Ok(());
    }
    anyhow::bail!("{} does not support stored API keys.", id.display_name())
}

fn resolve_api_key_input(api_key: Option<&str>, read_from_stdin: bool) -> anyhow::Result<String> {
    if api_key.is_some() && read_from_stdin {
        anyhow::bail!("Use either --api-key or --stdin, not both.");
    }

    let raw = if read_from_stdin {
        let mut buffer = String::new();
        use std::io::Read;
        std::io::stdin().read_to_string(&mut buffer)?;
        Some(buffer)
    } else {
        api_key.map(ToString::to_string)
    };

    let mut value = raw
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Missing API key. Pass --api-key <key> or use --stdin."))?;

    if (value.starts_with('"') && value.ends_with('"'))
        || (value.starts_with('\'') && value.ends_with('\''))
    {
        value.remove(0);
        value.pop();
    }

    let value = value.trim().to_string();
    if value.is_empty() {
        anyhow::bail!("Missing API key. Pass --api-key <key> or use --stdin.");
    }
    Ok(value)
}

/// Show configuration file paths
async fn show_paths() -> anyhow::Result<()> {
    println!("Configuration paths:");

    if let Some(path) = Settings::settings_path() {
        let exists = if path.exists() { "" } else { " (not found)" };
        println!("  Settings:       {}{}", path.display(), exists);
    } else {
        println!("  Settings:       (could not determine path)");
    }

    if let Some(path) = ManualCookies::cookies_path() {
        let exists = if path.exists() { "" } else { " (not found)" };
        println!("  Manual cookies: {}{}", path.display(), exists);
    } else {
        println!("  Manual cookies: (could not determine path)");
    }

    let token_path = TokenAccountStore::default_path();
    let exists = if token_path.exists() {
        ""
    } else {
        " (not found)"
    };
    println!("  Token accounts: {}{}", token_path.display(), exists);

    if crate::settings::settings_file_override().is_some() {
        println!();
        println!(
            "Settings file from {}; the stores above sit beside it.",
            crate::settings::CONFIG_PATH_ENV
        );
    }

    // Show config directory
    if let Some(codexbar_dir) = crate::settings::config_store_dir() {
        println!();
        println!("Config directory: {}", codexbar_dir.display());
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        ClaudeCodeCredentialsConsent, ConfigCommand, ConfigFileError, ConfigOutputArgs,
        ConsentAction, apply_consent_action, consent_text, provider_statuses, read_json_config,
        sanitize_settings_for_dump,
    };
    use crate::cli::{Cli, Commands};
    use crate::core::ProviderId;
    #[cfg(windows)]
    use crate::secure_file;
    use crate::settings::{ManualCookies, Settings};
    use clap::Parser;
    use serde_json::json;

    #[test]
    fn preferences_subcommands_parse_a_path() {
        use super::{ConfigArgs, ConfigCommand, PreferencesAction};
        use clap::Parser;

        let export =
            ConfigArgs::try_parse_from(["config", "preferences", "export", "--file", "p.json"])
                .expect("export parses");
        assert!(matches!(
            export.command,
            ConfigCommand::Preferences {
                action: PreferencesAction::Export { file: Some(_) }
            }
        ));
        let stdout = ConfigArgs::try_parse_from(["config", "preferences", "export"])
            .expect("export without --file parses");
        assert!(matches!(
            stdout.command,
            ConfigCommand::Preferences {
                action: PreferencesAction::Export { file: None }
            }
        ));
        let import =
            ConfigArgs::try_parse_from(["config", "preferences", "import", "--file", "p.json"])
                .expect("import parses");
        assert!(matches!(
            import.command,
            ConfigCommand::Preferences {
                action: PreferencesAction::Import { .. }
            }
        ));
        assert!(ConfigArgs::try_parse_from(["config", "preferences", "import"]).is_err());
    }

    fn parse_config(argv: &[&str]) -> Result<ConfigCommand, clap::Error> {
        let cli = Cli::try_parse_from(["codexbar", "config"].iter().chain(argv.iter()))?;
        match cli.command {
            Some(Commands::Config(args)) => Ok(args.command),
            other => panic!("expected the config command, got {other:?}"),
        }
    }

    #[test]
    fn config_providers_accepts_json_output_flags() {
        let ConfigCommand::Providers { output } = parse_config(&["providers"]).unwrap() else {
            panic!("expected providers");
        };
        assert!(!output.is_json());
        for argv in [
            &["providers", "--json"][..],
            &["providers", "--format", "json"][..],
            &["providers", "-f", "json", "--pretty"][..],
        ] {
            let ConfigCommand::Providers { output } = parse_config(argv).unwrap() else {
                panic!("expected providers for {argv:?}");
            };
            assert!(output.is_json(), "{argv:?}");
        }
        assert!(parse_config(&["providers", "--format", "toml"]).is_err());
    }

    #[test]
    fn provider_status_json_matches_the_upstream_shape() {
        let mut settings = Settings::default();
        settings.enabled_providers.clear();
        settings.enable_provider(ProviderId::Cursor);

        let statuses = provider_statuses(&settings);

        assert_eq!(
            statuses.len(),
            ProviderId::all()
                .iter()
                .filter(|p| !p.is_deprecated())
                .count()
        );
        let cursor = statuses
            .iter()
            .find(|status| status.provider == "cursor")
            .expect("cursor row");
        assert!(cursor.enabled);
        let codex = statuses
            .iter()
            .find(|status| status.provider == "codex")
            .expect("codex row");
        assert!(!codex.enabled);

        let value = serde_json::to_value(codex).unwrap();
        let mut keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            ["defaultEnabled", "displayName", "enabled", "provider"]
        );
        assert_eq!(value["displayName"], ProviderId::Codex.display_name());
        assert_eq!(value["enabled"], false);
        assert_eq!(value["defaultEnabled"], codex.default_enabled);
    }

    #[test]
    fn deprecated_providers_are_hidden_until_enabled_then_listed() {
        let mut settings = Settings::default();
        settings.enabled_providers.clear();
        let statuses = provider_statuses(&settings);
        for retired in [ProviderId::KimiK2, ProviderId::CrossModel] {
            assert!(retired.is_deprecated());
            assert!(statuses.iter().all(|s| s.provider != retired.cli_name()));
        }

        settings.enable_provider(ProviderId::KimiK2);
        let statuses = provider_statuses(&settings);
        let kimi = statuses
            .iter()
            .find(|s| s.provider == "kimik2")
            .expect("enabled deprecated provider is listed");
        assert!(kimi.enabled);
        assert!(statuses.iter().all(|s| s.provider != "crossmodel"));
    }

    #[test]
    fn provider_status_text_lines_keep_the_existing_format() {
        let mut settings = Settings::default();
        settings.enabled_providers.clear();
        settings.enable_provider(ProviderId::Codex);
        let statuses = provider_statuses(&settings);
        let codex = statuses.iter().find(|s| s.provider == "codex").unwrap();
        let claude = statuses.iter().find(|s| s.provider == "claude").unwrap();

        let marker = |default: bool| if default { " default" } else { "" };
        assert_eq!(
            codex.text_line(),
            format!("codex: enabled{} (Codex)", marker(codex.default_enabled))
        );
        assert_eq!(
            claude.text_line(),
            format!(
                "claude: disabled{} (Claude)",
                marker(claude.default_enabled)
            )
        );
    }

    #[test]
    fn provider_subcommands_accept_the_positional_or_provider_option() {
        for argv in [
            &["enable", "cursor"][..],
            &["enable", "-p", "cursor"][..],
            &["enable", "--provider", "cursor", "--json"][..],
        ] {
            let ConfigCommand::Enable { target, .. } = parse_config(argv).unwrap() else {
                panic!("expected enable for {argv:?}");
            };
            assert_eq!(target.name(), "cursor", "{argv:?}");
        }
        let ConfigCommand::Disable { target, output } =
            parse_config(&["disable", "--provider", "cursor", "--format", "json"]).unwrap()
        else {
            panic!("expected disable");
        };
        assert_eq!(target.name(), "cursor");
        assert!(output.is_json());
        let ConfigCommand::SetApiKey { target, stdin, .. } =
            parse_config(&["set-api-key", "-p", "openrouter", "--stdin"]).unwrap()
        else {
            panic!("expected set-api-key");
        };
        assert_eq!(target.name(), "openrouter");
        assert!(stdin);

        assert!(parse_config(&["enable"]).is_err(), "provider is required");
        assert!(
            parse_config(&["enable", "cursor", "--provider", "codex"]).is_err(),
            "positional and --provider conflict"
        );
    }

    #[test]
    fn default_output_args_are_text() {
        assert!(!ConfigOutputArgs::default().is_json());
    }

    #[test]
    fn claude_code_credentials_parses_each_action() {
        for (word, expected) in [
            ("allow", ConsentAction::Allow),
            ("deny", ConsentAction::Deny),
            ("status", ConsentAction::Status),
        ] {
            let ConfigCommand::ClaudeCodeCredentials { action, output } =
                parse_config(&["claude-code-credentials", word, "--json"]).unwrap()
            else {
                panic!("expected claude-code-credentials for {word}");
            };
            assert_eq!(action, expected);
            assert!(output.is_json());
        }
        assert!(parse_config(&["claude-code-credentials"]).is_err());
        assert!(parse_config(&["claude-code-credentials", "maybe"]).is_err());
    }

    #[test]
    fn consent_actions_toggle_only_the_claude_code_flag() {
        let mut settings = Settings::default();
        assert!(!settings.claude_allow_reading_claude_code_credentials);
        let before = serde_json::to_value(&settings).unwrap();

        assert!(!apply_consent_action(&mut settings, ConsentAction::Status));
        assert!(!apply_consent_action(&mut settings, ConsentAction::Deny));
        assert!(apply_consent_action(&mut settings, ConsentAction::Allow));
        assert!(settings.claude_allow_reading_claude_code_credentials);
        assert!(!apply_consent_action(&mut settings, ConsentAction::Allow));
        assert!(!apply_consent_action(&mut settings, ConsentAction::Status));
        assert!(settings.claude_allow_reading_claude_code_credentials);

        let mut after = serde_json::to_value(&settings).unwrap();
        after["claude_allow_reading_claude_code_credentials"] = json!(false);
        assert_eq!(after, before, "no other setting changes");

        assert!(apply_consent_action(&mut settings, ConsentAction::Deny));
        assert!(!settings.claude_allow_reading_claude_code_credentials);
    }

    #[test]
    fn consent_output_reports_the_choice() {
        let value = serde_json::to_value(ClaudeCodeCredentialsConsent {
            allowed: true,
            config_path: Some("settings.json".to_string()),
        })
        .unwrap();
        assert_eq!(
            value,
            json!({ "allowed": true, "configPath": "settings.json" })
        );
        assert!(consent_text(true).contains("allowed"));
        assert!(consent_text(false).contains("not allowed"));
    }

    #[cfg(windows)]
    #[test]
    fn validates_dpapi_protected_manual_cookies_without_plaintext() {
        let dir = tempfile::tempdir().expect("create temporary directory");
        let path = dir.path().join("manual_cookies.json");
        let expected = r#"{"cookies":{}}"#;

        secure_file::write_string(&path, expected).expect("write protected cookies");

        let raw = std::fs::read_to_string(&path).expect("read protected wrapper");
        assert!(raw.contains("\"format\": \"codexbar.secure-file\""));
        assert!(!raw.contains(expected));
        assert!(
            read_json_config::<ManualCookies>(&path).is_ok(),
            "valid protected cookies should validate"
        );
    }

    #[test]
    fn rejects_malformed_secure_wrapper_without_echoing_payload() {
        let dir = tempfile::tempdir().expect("create temporary directory");
        let path = dir.path().join("manual_cookies.json");
        let payload = "AA==";
        let malformed = format!(
            r#"{{"format":"codexbar.secure-file","version":1,"protection":"unsupported","payload":"{payload}"}}"#
        );
        std::fs::write(&path, malformed).expect("write malformed wrapper");

        let error = read_json_config::<ManualCookies>(&path)
            .expect_err("reject malformed protected wrapper");
        let ConfigFileError::Read(error) = error else {
            panic!("malformed protected wrapper must be a read error");
        };
        let message = error.to_string();
        assert!(message.contains("unsupported secure file protection"));
        assert!(!message.contains(payload));
    }

    #[test]
    fn sanitize_settings_for_dump_redacts_secret_fields() {
        let raw = json!({
            "http_proxy_password": "proxy-secret",
            "provider_configs": {
                "claude": {
                    "manual_cookie_header": "sessionKey=abc",
                    "api_token": "tok-123",
                    "management_api_token": "management-secret"
                }
            },
            "api_keys": {
                "keys": {
                    "zai": {
                        "api_key": "cb_test_api_key_456",
                        "label": "Work",
                        "saved_at": "2026-01-01"
                    }
                }
            },
            "manual_cookies": {
                "cookies": {
                    "claude": {
                        "cookie_header": "sessionKey=secret-cookie",
                        "saved_at": "2026-01-01"
                    }
                }
            },
            "token_accounts": {
                "factory": {
                    "accounts": [{
                        "id": "11111111-1111-1111-1111-111111111111",
                        "label": "Team",
                        "token": "raw-token-value",
                        "added_at": 1
                    }]
                }
            },
            "secret_key": "top-secret"
        });

        let redacted = sanitize_settings_for_dump(raw, false);
        let text = serde_json::to_string(&redacted).expect("serialize");

        assert!(text.contains("[REDACTED]"));
        assert!(!text.contains("proxy-secret"));
        assert!(!text.contains("sessionKey=abc"));
        assert!(!text.contains("tok-123"));
        assert!(!text.contains("management-secret"));
        assert!(!text.contains("cb_test_api_key_456"));
        assert!(!text.contains("secret-cookie"));
        assert!(!text.contains("raw-token-value"));
        assert!(!text.contains("top-secret"));
        // Non-secret identity fields stay.
        assert!(text.contains("Work"));
        assert!(text.contains("Team"));
        assert!(text.contains("11111111-1111-1111-1111-111111111111"));
    }

    #[test]
    fn sanitize_settings_for_dump_show_secrets_keeps_raw() {
        let raw = json!({
            "api_key": "keep-me",
            "nested": { "token": "also-keep", "label": "Team" }
        });

        let out = sanitize_settings_for_dump(raw.clone(), true);
        assert_eq!(out, raw);
        assert_eq!(out["api_key"], "keep-me");
        assert_eq!(out["nested"]["token"], "also-keep");
    }
}
