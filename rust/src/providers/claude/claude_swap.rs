//! Read-only adapter over the external `claude-swap` (`cswap`) executable.
//!
//! Port of upstream CodexBar's claude-swap Phase 1–2 contract
//! (`docs/claude-multi-account-and-status-items.md`). CodexBar never reads or
//! stores claude-swap (or Claude Code) credentials: the subprocess owns its own
//! credential access, and this module copies only allow-listed usage/identity
//! fields into a provider-neutral account snapshot.
//!
//! Only two fixed argument arrays are ever executed — `cswap --list --json` and
//! `cswap --switch-to <slot> --json` — never a shell and never config-defined
//! passthrough arguments. Output and runtime are bounded, and schema version 1
//! is required.
//!
//! Responsibilities are split into sibling modules so each can be tested in
//! isolation: [`sanitize`] owns hostile-text handling, [`runner`] owns the
//! bounded subprocess, [`parser`] owns schema-v1 validation, and [`projection`]
//! owns the display-only snapshot.

mod parser;
mod projection;
mod runner;
mod sanitize;

use chrono::{DateTime, Utc};
use serde::Serialize;

pub use parser::{parse_account_list, parse_switch_result, validate_switch_target};
pub use projection::{
    ClaudeSwapAccount, ClaudeSwapScopedWindowDto, ClaudeSwapUsageWindowDto, project_accounts,
};
pub use runner::{
    DEFAULT_TIMEOUT, MAX_OUTPUT_BYTES, SWITCH_TIMEOUT, list_arguments, read_account_list,
    read_account_list_with_timeout, resolve_executable_path, switch_account, switch_arguments,
};
pub use sanitize::{MAX_DIAGNOSTIC_CHARS, MAX_LABEL_CHARS, sanitize_display};

#[derive(Debug, thiserror::Error)]
pub enum ClaudeSwapError {
    #[error("No claude-swap executable path is configured.")]
    ExecutablePathNotConfigured,
    #[error("claude-swap executable was not found at the configured path.")]
    ExecutableNotFound,
    #[error("claude-swap produced {actual} bytes of output; refusing to parse more than {limit}.")]
    OutputTooLarge { actual: usize, limit: usize },
    #[error("claude-swap did not respond within {0} seconds.")]
    TimedOut(u64),
    #[error("Failed to run claude-swap: {0}")]
    Process(String),
    #[error("claude-swap returned output that is not a JSON object.")]
    NotJsonObject,
    #[error("claude-swap output has no schemaVersion field.")]
    MissingSchemaVersion,
    #[error("claude-swap output uses unsupported schema version {0}; CodexBar supports version 1.")]
    UnsupportedSchemaVersion(i64),
    #[error("claude-swap reported {kind}: {message}")]
    ReportedError { kind: String, message: String },
    #[error("claude-swap output is malformed: {0}")]
    MalformedShape(String),
    #[error("claude-swap reported account slot {actual} after CodexBar requested slot {expected}.")]
    MismatchedTarget { expected: u32, actual: u32 },
}

/// Sentinel `usageStatus` values emitted by cswap. Unknown values from newer
/// releases are preserved rather than failing the whole payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ClaudeSwapUsageStatus {
    Ok,
    TokenExpired,
    ReloginRequired,
    ApiKey,
    KeychainUnavailable,
    NoCredentials,
    Unavailable,
    /// A status CodexBar does not recognize. The raw value is deliberately not
    /// retained: unknown external strings are never echoed to UI or logs.
    Unknown,
}

impl ClaudeSwapUsageStatus {
    pub fn from_raw(raw: &str) -> Self {
        match raw {
            "ok" => Self::Ok,
            "token_expired" => Self::TokenExpired,
            "relogin_required" => Self::ReloginRequired,
            "api_key" => Self::ApiKey,
            "keychain_unavailable" => Self::KeychainUnavailable,
            "no_credentials" => Self::NoCredentials,
            "unavailable" => Self::Unavailable,
            _ => Self::Unknown,
        }
    }

    pub fn as_label(&self) -> &str {
        match self {
            Self::Ok => "ok",
            Self::TokenExpired => "token_expired",
            Self::ReloginRequired => "relogin_required",
            Self::ApiKey => "api_key",
            Self::KeychainUnavailable => "keychain_unavailable",
            Self::NoCredentials => "no_credentials",
            Self::Unavailable => "unavailable",
            Self::Unknown => "unknown",
        }
    }

    /// Upstream keeps expired / missing / inaccessible slots non-actionable.
    pub fn can_activate(&self) -> bool {
        matches!(self, Self::Ok | Self::ApiKey | Self::Unavailable)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeSwapUsageWindow {
    pub used_percent: f64,
    pub resets_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeSwapScopedWindow {
    pub name: String,
    pub used_percent: f64,
    pub resets_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeSwapAccountRow {
    pub number: u32,
    pub email: String,
    pub organization_name: String,
    pub alias: Option<String>,
    pub is_active: bool,
    pub usage_status: ClaudeSwapUsageStatus,
    pub five_hour: Option<ClaudeSwapUsageWindow>,
    pub seven_day: Option<ClaudeSwapUsageWindow>,
    pub scoped: Vec<ClaudeSwapScopedWindow>,
    pub usage_fetched_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeSwapAccountList {
    pub active_account_number: Option<u32>,
    pub accounts: Vec<ClaudeSwapAccountRow>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClaudeSwapSwitchResult {
    pub switched: bool,
    pub from_account_number: Option<u32>,
    pub to_account_number: u32,
    pub reason: String,
}
