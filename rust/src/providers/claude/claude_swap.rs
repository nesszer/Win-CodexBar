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

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use serde::Serialize;
use serde_json::Value;

/// Upstream rejects list output larger than 256 KiB before parsing.
pub const MAX_OUTPUT_BYTES: usize = 262_144;
/// Default read-only probe timeout.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30);
/// Long upper bound for credential switches so a stalled helper cannot block forever.
pub const SWITCH_TIMEOUT: Duration = Duration::from_secs(300);
/// Bound on display-only label fields copied from cswap (upstream uses 256 scalars).
pub const MAX_LABEL_CHARS: usize = 256;
/// Bound on diagnostic strings copied from a cswap error envelope (upstream: 512).
pub const MAX_DIAGNOSTIC_CHARS: usize = 512;

/// Sanitize an external display string: strip ANSI/VT escape sequences and
/// control/format characters, collapse line breaks to spaces, and bound length.
///
/// cswap output is attacker-influenced only in the sense that a compromised or
/// unusual executable controls it, so nothing copied into UI/logs may carry
/// terminal escapes or unbounded text. Raw subprocess stdout is never surfaced.
pub fn sanitize_display(text: &str, max_chars: usize) -> String {
    let mut out = String::with_capacity(text.len().min(max_chars));
    let mut count = 0usize;
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\u{1b}' => match chars.next() {
                Some('[') => {
                    // CSI ... final byte in 0x40..=0x7e.
                    for c in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    // OSC ... terminated by BEL or ST.
                    let mut previous = '\0';
                    for c in chars.by_ref() {
                        if c == '\u{07}' || (previous == '\u{1b}' && c == '\\') {
                            break;
                        }
                        previous = c;
                    }
                }
                Some(_) | None => {}
            },
            '\r' | '\n' | '\u{2028}' | '\u{2029}' => {
                // Collapse runs of line breaks (and adjacent literal spaces)
                // into a single separating space.
                if !out.ends_with(' ') {
                    out.push(' ');
                    count += 1;
                }
            }
            ' ' => {
                if !out.ends_with(' ') {
                    out.push(' ');
                    count += 1;
                }
            }
            _ if ch.is_control() => {}
            _ => {
                out.push(ch);
                count += 1;
            }
        }
        if count >= max_chars {
            break;
        }
    }
    out.trim().to_string()
}

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

/// Bridge-facing external account row. Identity is the source-issued numeric
/// slot (`claude-swap:<slot>`), never email or credential-derived values.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeSwapAccount {
    pub id: String,
    pub slot: u32,
    pub label: String,
    pub email: Option<String>,
    pub organization: Option<String>,
    pub alias: Option<String>,
    pub is_active: bool,
    pub can_activate: bool,
    pub status: String,
    pub error: Option<String>,
    pub five_hour: Option<ClaudeSwapUsageWindowDto>,
    pub seven_day: Option<ClaudeSwapUsageWindowDto>,
    pub scoped: Vec<ClaudeSwapScopedWindowDto>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeSwapUsageWindowDto {
    pub used_percent: f64,
    pub resets_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeSwapScopedWindowDto {
    pub name: String,
    pub used_percent: f64,
    pub resets_at: Option<DateTime<Utc>>,
}

/// Exact argument arrays. Pure so the no-shell contract is testable.
pub fn list_arguments() -> Vec<String> {
    vec!["--list".to_string(), "--json".to_string()]
}

pub fn switch_arguments(slot: u32) -> Vec<String> {
    vec![
        "--switch-to".to_string(),
        slot.to_string(),
        "--json".to_string(),
    ]
}

/// Trim and expand a leading `~` exactly like upstream; no shell expansion.
pub fn resolve_executable_path(configured: &str) -> Result<PathBuf, ClaudeSwapError> {
    let trimmed = configured.trim();
    if trimmed.is_empty() {
        return Err(ClaudeSwapError::ExecutablePathNotConfigured);
    }
    if trimmed == "~" {
        return dirs::home_dir()
            .ok_or_else(|| ClaudeSwapError::Process("Home directory not found.".to_string()));
    }
    if let Some(rest) = trimmed
        .strip_prefix("~/")
        .or_else(|| trimmed.strip_prefix("~\\"))
    {
        return dirs::home_dir()
            .map(|home| home.join(rest))
            .ok_or_else(|| ClaudeSwapError::Process("Home directory not found.".to_string()));
    }
    Ok(PathBuf::from(trimmed))
}

/// A configured bare command name is resolved on `PATH` by the OS; an explicit
/// path must exist so a typo fails with a clear message instead of a spawn error.
fn validate_executable_path(path: &Path) -> Result<(), ClaudeSwapError> {
    let looks_like_path = path.is_absolute()
        || path
            .parent()
            .is_some_and(|parent| !parent.as_os_str().is_empty());
    if looks_like_path && !path.is_file() {
        return Err(ClaudeSwapError::ExecutableNotFound);
    }
    Ok(())
}

#[derive(Debug, Default)]
struct RunOutcome {
    stdout: Vec<u8>,
    total_stdout_bytes: usize,
}

fn read_bounded(mut reader: impl std::io::Read) -> RunOutcome {
    let mut outcome = RunOutcome::default();
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => break,
            Ok(read) => {
                outcome.total_stdout_bytes += read;
                if outcome.stdout.len() < MAX_OUTPUT_BYTES + 1 {
                    let remaining = (MAX_OUTPUT_BYTES + 1) - outcome.stdout.len();
                    let take = remaining.min(read);
                    outcome.stdout.extend_from_slice(&chunk[..take]);
                }
            }
            Err(_) => break,
        }
    }
    outcome
}

/// Run a fixed argument array with an optional deadline. Production list and switch
/// operations always pass finite timeouts so a stalled helper cannot block indefinitely.
fn run_bounded(
    program: &Path,
    arguments: &[String],
    timeout: Option<Duration>,
) -> Result<String, ClaudeSwapError> {
    let mut command = Command::new(program);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = command
        .spawn()
        .map_err(|e| ClaudeSwapError::Process(e.to_string()))?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| ClaudeSwapError::Process("Failed to capture stdout.".to_string()))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| ClaudeSwapError::Process("Failed to capture stderr.".to_string()))?;
    let stdout_reader = std::thread::spawn(move || read_bounded(stdout));
    let stderr_reader = std::thread::spawn(move || read_bounded(stderr));

    let deadline = timeout.map(|timeout| Instant::now() + timeout);
    loop {
        match child.try_wait() {
            Ok(Some(_status)) => break,
            Ok(None) => {
                if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                    let _ = child.kill();
                    let _ = child.wait();
                    let _ = stdout_reader.join();
                    let _ = stderr_reader.join();
                    let seconds = timeout.map(|t| t.as_secs()).unwrap_or_default();
                    return Err(ClaudeSwapError::TimedOut(seconds));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                let _ = stdout_reader.join();
                let _ = stderr_reader.join();
                return Err(ClaudeSwapError::Process(e.to_string()));
            }
        }
    }

    let stdout = stdout_reader
        .join()
        .map_err(|_| ClaudeSwapError::Process("stdout reader panicked.".to_string()))?;
    let _stderr = stderr_reader.join();
    if stdout.total_stdout_bytes > MAX_OUTPUT_BYTES {
        return Err(ClaudeSwapError::OutputTooLarge {
            actual: stdout.total_stdout_bytes,
            limit: MAX_OUTPUT_BYTES,
        });
    }
    Ok(String::from_utf8_lossy(&stdout.stdout).into_owned())
}

/// Read-only `cswap --list --json`.
pub fn read_account_list(configured_path: &str) -> Result<ClaudeSwapAccountList, ClaudeSwapError> {
    read_account_list_with_timeout(configured_path, DEFAULT_TIMEOUT)
}

pub fn read_account_list_with_timeout(
    configured_path: &str,
    timeout: Duration,
) -> Result<ClaudeSwapAccountList, ClaudeSwapError> {
    let program = resolve_executable_path(configured_path)?;
    validate_executable_path(&program)?;
    // Handled cswap failures print a schema-v1 error envelope to stdout and exit
    // non-zero, so parse stdout regardless of the exit status.
    let output = run_bounded(&program, &list_arguments(), Some(timeout))?;
    parse_account_list(&output)
}

/// Explicit `cswap --switch-to <slot> --json`, bounded by [`SWITCH_TIMEOUT`].
pub fn switch_account(
    configured_path: &str,
    slot: u32,
) -> Result<ClaudeSwapSwitchResult, ClaudeSwapError> {
    if slot == 0 {
        return Err(ClaudeSwapError::MalformedShape(
            "requested account slot must be positive".to_string(),
        ));
    }
    let program = resolve_executable_path(configured_path)?;
    validate_executable_path(&program)?;
    let output = run_bounded(&program, &switch_arguments(slot), Some(SWITCH_TIMEOUT))?;
    let parsed = parse_switch_result(&output)?;
    validate_switch_target(slot, &parsed)?;
    Ok(parsed)
}

/// The switch envelope must confirm the exact slot CodexBar requested.
pub fn validate_switch_target(
    requested: u32,
    parsed: &ClaudeSwapSwitchResult,
) -> Result<(), ClaudeSwapError> {
    if parsed.to_account_number != requested {
        return Err(ClaudeSwapError::MismatchedTarget {
            expected: requested,
            actual: parsed.to_account_number,
        });
    }
    Ok(())
}

fn as_object(value: &Value) -> Option<&serde_json::Map<String, Value>> {
    value.as_object()
}

fn finite_number(value: &Value) -> Option<f64> {
    value.as_f64().filter(|number| number.is_finite())
}

fn non_negative_slot(value: &Value) -> Option<u32> {
    value.as_u64().and_then(|number| {
        if number == 0 {
            None
        } else {
            u32::try_from(number).ok()
        }
    })
}

fn parse_timestamp(text: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|value| value.with_timezone(&Utc))
}

fn trimmed_non_empty(value: Option<&Value>) -> String {
    value
        .and_then(Value::as_str)
        .map(|text| sanitize_display(text, MAX_LABEL_CHARS))
        .unwrap_or_default()
}

fn non_empty_display_string(value: Option<&Value>) -> Option<String> {
    let text = trimmed_non_empty(value);
    if text.is_empty() { None } else { Some(text) }
}

fn parse_window(
    raw: Option<&Value>,
    slot: u32,
    name: &str,
) -> Result<Option<ClaudeSwapUsageWindow>, ClaudeSwapError> {
    let Some(raw) = raw else {
        return Ok(None);
    };
    let object = as_object(raw).ok_or_else(|| {
        ClaudeSwapError::MalformedShape(format!("slot {slot} {name} window is not an object"))
    })?;
    let percent = object.get("pct").and_then(finite_number).ok_or_else(|| {
        ClaudeSwapError::MalformedShape(format!(
            "slot {slot} {name} percent is not a finite number"
        ))
    })?;
    let resets_at = match object.get("resetsAt") {
        None | Some(Value::Null) => None,
        Some(Value::String(text)) => Some(parse_timestamp(text).ok_or_else(|| {
            ClaudeSwapError::MalformedShape(format!(
                "slot {slot} {name} resetsAt is not a timestamp"
            ))
        })?),
        Some(_) => {
            return Err(ClaudeSwapError::MalformedShape(format!(
                "slot {slot} {name} resetsAt is not a timestamp"
            )));
        }
    };
    Ok(Some(ClaudeSwapUsageWindow {
        used_percent: percent.clamp(0.0, 100.0),
        resets_at,
    }))
}

/// `usage.scoped` is additive schema-v1 data. Malformed or future scope rows are
/// ignored so they cannot suppress otherwise valid account-wide usage.
fn parse_scoped(raw: Option<&Value>) -> Vec<ClaudeSwapScopedWindow> {
    let Some(rows) = raw.and_then(Value::as_array) else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|row| {
            let object = row.as_object()?;
            let name = object
                .get("name")
                .map(|value| trimmed_non_empty(Some(value)))?;
            if name.is_empty() {
                return None;
            }
            let percent = object.get("pct").and_then(finite_number)?;
            let resets_at = match object.get("resetsAt") {
                None | Some(Value::Null) => None,
                Some(Value::String(text)) => Some(parse_timestamp(text)?),
                Some(_) => return None,
            };
            Some(ClaudeSwapScopedWindow {
                name,
                used_percent: percent.clamp(0.0, 100.0),
                resets_at,
            })
        })
        .collect()
}

fn parse_row(
    object: &serde_json::Map<String, Value>,
) -> Result<ClaudeSwapAccountRow, ClaudeSwapError> {
    let number = object
        .get("number")
        .and_then(non_negative_slot)
        .ok_or_else(|| {
            ClaudeSwapError::MalformedShape("account row has no numeric slot".to_string())
        })?;
    let is_active = object
        .get("active")
        .and_then(Value::as_bool)
        .ok_or_else(|| {
            ClaudeSwapError::MalformedShape(format!("slot {number} has no active flag"))
        })?;
    let raw_status = object
        .get("usageStatus")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ClaudeSwapError::MalformedShape(format!("slot {number} has no usageStatus"))
        })?;
    let usage = object.get("usage").and_then(Value::as_object);
    Ok(ClaudeSwapAccountRow {
        number,
        email: trimmed_non_empty(object.get("email")),
        organization_name: trimmed_non_empty(object.get("organizationName")),
        alias: non_empty_display_string(object.get("alias")),
        is_active,
        usage_status: ClaudeSwapUsageStatus::from_raw(raw_status),
        five_hour: parse_window(usage.and_then(|u| u.get("fiveHour")), number, "fiveHour")?,
        seven_day: parse_window(usage.and_then(|u| u.get("sevenDay")), number, "sevenDay")?,
        scoped: parse_scoped(usage.and_then(|u| u.get("scoped"))),
        usage_fetched_at: object
            .get("usageFetchedAt")
            .and_then(Value::as_str)
            .and_then(parse_timestamp),
    })
}

/// Strictly parse the schema-v1 `cswap --list --json` envelope.
pub fn parse_account_list(raw: &str) -> Result<ClaudeSwapAccountList, ClaudeSwapError> {
    let value: Value = serde_json::from_str(raw).map_err(|_| ClaudeSwapError::NotJsonObject)?;
    let object = value.as_object().ok_or(ClaudeSwapError::NotJsonObject)?;

    let schema_version = object
        .get("schemaVersion")
        .and_then(Value::as_i64)
        .ok_or(ClaudeSwapError::MissingSchemaVersion)?;
    if schema_version != 1 {
        return Err(ClaudeSwapError::UnsupportedSchemaVersion(schema_version));
    }
    if let Some(error) = object.get("error").and_then(Value::as_object) {
        let kind = sanitize_display(
            error.get("type").and_then(Value::as_str).unwrap_or("Error"),
            MAX_LABEL_CHARS,
        );
        let message = sanitize_display(
            error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown error"),
            MAX_DIAGNOSTIC_CHARS,
        );
        return Err(ClaudeSwapError::ReportedError {
            kind: if kind.is_empty() {
                "Error".to_string()
            } else {
                kind
            },
            message: if message.is_empty() {
                "unknown error".to_string()
            } else {
                message
            },
        });
    }

    let raw_accounts = object
        .get("accounts")
        .and_then(Value::as_array)
        .ok_or_else(|| ClaudeSwapError::MalformedShape("missing accounts array".to_string()))?;
    let active_field = object.get("activeAccountNumber").ok_or_else(|| {
        ClaudeSwapError::MalformedShape("missing activeAccountNumber".to_string())
    })?;
    let active_account_number = match active_field {
        Value::Null => None,
        Value::Number(_) => Some(non_negative_slot(active_field).ok_or_else(|| {
            ClaudeSwapError::MalformedShape(
                "activeAccountNumber is not a numeric slot or null".to_string(),
            )
        })?),
        _ => {
            return Err(ClaudeSwapError::MalformedShape(
                "activeAccountNumber is not a numeric slot or null".to_string(),
            ));
        }
    };

    let mut seen = std::collections::HashSet::new();
    let mut accounts = Vec::with_capacity(raw_accounts.len());
    for raw_row in raw_accounts {
        let object = raw_row.as_object().ok_or_else(|| {
            ClaudeSwapError::MalformedShape("account row is not an object".to_string())
        })?;
        let account = parse_row(object)?;
        if !seen.insert(account.number) {
            return Err(ClaudeSwapError::MalformedShape(format!(
                "duplicate account slot {}",
                account.number
            )));
        }
        accounts.push(account);
    }

    let active_slots = accounts
        .iter()
        .filter(|account| account.is_active)
        .map(|account| account.number)
        .collect::<Vec<_>>();
    let expected_active = active_account_number
        .map(|slot| vec![slot])
        .unwrap_or_default();
    if active_slots != expected_active {
        return Err(ClaudeSwapError::MalformedShape(
            "active account fields disagree".to_string(),
        ));
    }

    Ok(ClaudeSwapAccountList {
        active_account_number,
        accounts,
    })
}

/// Strictly parse the schema-v1 `cswap --switch-to <slot> --json` envelope.
pub fn parse_switch_result(raw: &str) -> Result<ClaudeSwapSwitchResult, ClaudeSwapError> {
    let value: Value = serde_json::from_str(raw).map_err(|_| ClaudeSwapError::NotJsonObject)?;
    let object = value.as_object().ok_or(ClaudeSwapError::NotJsonObject)?;

    let schema_version = object
        .get("schemaVersion")
        .and_then(Value::as_i64)
        .ok_or(ClaudeSwapError::MissingSchemaVersion)?;
    if schema_version != 1 {
        return Err(ClaudeSwapError::UnsupportedSchemaVersion(schema_version));
    }
    if let Some(error) = object.get("error").and_then(Value::as_object) {
        let kind = sanitize_display(
            error.get("type").and_then(Value::as_str).unwrap_or("Error"),
            MAX_LABEL_CHARS,
        );
        let message = sanitize_display(
            error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown error"),
            MAX_DIAGNOSTIC_CHARS,
        );
        return Err(ClaudeSwapError::ReportedError {
            kind: if kind.is_empty() {
                "Error".to_string()
            } else {
                kind
            },
            message: if message.is_empty() {
                "unknown error".to_string()
            } else {
                message
            },
        });
    }

    let switched = object
        .get("switched")
        .and_then(Value::as_bool)
        .ok_or_else(|| ClaudeSwapError::MalformedShape("missing switched flag".to_string()))?;
    let reason = object
        .get("reason")
        .and_then(Value::as_str)
        .map(|text| sanitize_display(text, MAX_DIAGNOSTIC_CHARS))
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ClaudeSwapError::MalformedShape("missing reason".to_string()))?;

    let from_account_number = parse_switch_slot(object.get("from"), "from", true)?;
    let to_account_number = parse_switch_slot(object.get("to"), "to", false)?.ok_or_else(|| {
        ClaudeSwapError::MalformedShape("to account has no numeric slot".to_string())
    })?;

    Ok(ClaudeSwapSwitchResult {
        switched,
        from_account_number,
        to_account_number,
        reason,
    })
}

fn parse_switch_slot(
    raw: Option<&Value>,
    field: &str,
    allows_null: bool,
) -> Result<Option<u32>, ClaudeSwapError> {
    match raw {
        Some(Value::Null) if allows_null => Ok(None),
        Some(value) => {
            let object = value.as_object().ok_or_else(|| {
                ClaudeSwapError::MalformedShape(format!("missing {field} account"))
            })?;
            match object.get("number") {
                Some(Value::Null) if allows_null => Ok(None),
                Some(number) => non_negative_slot(number).map(Some).ok_or_else(|| {
                    ClaudeSwapError::MalformedShape(format!(
                        "{field} account number is not a positive slot"
                    ))
                }),
                None => Err(ClaudeSwapError::MalformedShape(format!(
                    "{field} account has no number"
                ))),
            }
        }
        None => {
            if allows_null {
                Ok(None)
            } else {
                Err(ClaudeSwapError::MalformedShape(format!(
                    "missing {field} account"
                )))
            }
        }
    }
}

fn normalized_email(email: &str) -> String {
    email.trim().to_lowercase()
}

fn collision_labels(rows: &[&ClaudeSwapAccountRow]) -> Vec<String> {
    let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for email in rows.iter().map(|row| normalized_email(&row.email)) {
        if !email.is_empty() {
            *counts.entry(email).or_default() += 1;
        }
    }
    let duplicates = counts
        .into_iter()
        .filter(|(_, count)| *count > 1)
        .map(|(email, _)| email)
        .collect::<std::collections::HashSet<_>>();

    let labels = rows
        .iter()
        .map(|row| candidate_label(row, &duplicates))
        .collect::<Vec<_>>();
    let mut label_counts: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    for (row, label) in rows.iter().zip(labels.iter()) {
        if row.alias.is_none() && duplicates.contains(&normalized_email(&row.email)) {
            *label_counts.entry(label.to_lowercase()).or_default() += 1;
        }
    }
    rows.iter()
        .zip(labels)
        .map(|(row, label)| {
            let colliding = row.alias.is_none()
                && duplicates.contains(&normalized_email(&row.email))
                && label_counts
                    .get(&label.to_lowercase())
                    .is_some_and(|count| *count > 1);
            if colliding {
                format!("{label} · Account {}", row.number)
            } else {
                label
            }
        })
        .collect()
}

fn candidate_label(
    row: &ClaudeSwapAccountRow,
    duplicates: &std::collections::HashSet<String>,
) -> String {
    if let Some(alias) = &row.alias {
        return alias.clone();
    }
    if row.email.is_empty() {
        return format!("Account {}", row.number);
    }
    if !duplicates.contains(&normalized_email(&row.email)) {
        return row.email.clone();
    }
    if !row.organization_name.is_empty() {
        return format!("{} · {}", row.email, row.organization_name);
    }
    format!("{} · Account {}", row.email, row.number)
}

fn error_text_for(row: &ClaudeSwapAccountRow) -> Option<String> {
    let has_windows = row.five_hour.is_some() || row.seven_day.is_some() || !row.scoped.is_empty();
    match row.usage_status {
        ClaudeSwapUsageStatus::Ok => {
            if has_windows {
                None
            } else {
                Some("No usage windows reported.".to_string())
            }
        }
        ClaudeSwapUsageStatus::TokenExpired => {
            Some("Token expired. Switch to this account in claude-swap to refresh it.".to_string())
        }
        ClaudeSwapUsageStatus::ReloginRequired => {
            Some("Re-login required. Re-authenticate this account in claude-swap.".to_string())
        }
        ClaudeSwapUsageStatus::ApiKey => {
            Some("API-key account; subscription usage is unavailable.".to_string())
        }
        ClaudeSwapUsageStatus::KeychainUnavailable => {
            Some("claude-swap could not read the active account's Keychain entry.".to_string())
        }
        ClaudeSwapUsageStatus::NoCredentials => {
            Some("No stored credentials for this account slot.".to_string())
        }
        ClaudeSwapUsageStatus::Unavailable => {
            Some("Polling deferred until a limit resets.".to_string())
        }
        ClaudeSwapUsageStatus::Unknown => Some("Unrecognized claude-swap status.".to_string()),
    }
}

/// Project parsed rows into the provider-neutral account snapshot consumed by
/// the settings UI. When personal information is hidden, labels collapse to
/// stable `Account N` ordinals and identity is omitted from the bridge payload.
pub fn project_accounts(
    list: &ClaudeSwapAccountList,
    hide_personal_info: bool,
) -> Vec<ClaudeSwapAccount> {
    let mut ordered = list.accounts.iter().collect::<Vec<_>>();
    ordered.sort_by(|lhs, rhs| {
        if lhs.is_active != rhs.is_active {
            return rhs.is_active.cmp(&lhs.is_active);
        }
        lhs.number.cmp(&rhs.number)
    });
    let labels = collision_labels(&ordered);

    ordered
        .into_iter()
        .zip(labels)
        .map(|(row, label)| {
            let display_label = if hide_personal_info {
                format!("Account {}", row.number)
            } else {
                label
            };
            let to_window = |window: &Option<ClaudeSwapUsageWindow>| {
                window.as_ref().map(|window| ClaudeSwapUsageWindowDto {
                    used_percent: window.used_percent,
                    resets_at: window.resets_at,
                })
            };
            ClaudeSwapAccount {
                id: format!("claude-swap:{}", row.number),
                slot: row.number,
                label: display_label,
                email: if hide_personal_info || row.email.is_empty() {
                    None
                } else {
                    Some(row.email.clone())
                },
                organization: if hide_personal_info {
                    None
                } else if row.organization_name.is_empty() {
                    None
                } else {
                    Some(row.organization_name.clone())
                },
                alias: if hide_personal_info {
                    None
                } else {
                    row.alias.clone()
                },
                is_active: row.is_active,
                can_activate: !row.is_active && row.usage_status.can_activate(),
                status: row.usage_status.as_label().to_string(),
                error: error_text_for(row),
                five_hour: to_window(&row.five_hour),
                seven_day: to_window(&row.seven_day),
                scoped: row
                    .scoped
                    .iter()
                    .map(|window| ClaudeSwapScopedWindowDto {
                        name: window.name.clone(),
                        used_percent: window.used_percent,
                        resets_at: window.resets_at,
                    })
                    .collect(),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn list_fixture() -> Value {
        json!({
            "schemaVersion": 1,
            "activeAccountNumber": 2,
            "accounts": [
                {
                    "number": 1,
                    "email": "same@example.com",
                    "organizationName": "Work",
                    "active": false,
                    "usageStatus": "ok",
                    "usage": {
                        "fiveHour": { "pct": 120.0, "resetsAt": "2026-09-12T01:00:00Z" },
                        "sevenDay": { "pct": 18.0 },
                        "scoped": [{ "name": "Fable only", "pct": 4.0 }]
                    },
                    "usageFetchedAt": "2026-09-12T00:30:00.000Z"
                },
                {
                    "number": 2,
                    "email": "same@example.com",
                    "organizationName": "Personal",
                    "active": true,
                    "usageStatus": "ok",
                    "usage": { "fiveHour": { "pct": 81.0 }, "sevenDay": { "pct": 18.0 } }
                },
                {
                    "number": 3,
                    "email": "expired@example.com",
                    "organizationName": "",
                    "alias": "Backup",
                    "active": false,
                    "usageStatus": "token_expired"
                }
            ]
        })
    }

    #[test]
    fn parses_schema_v1_and_normalizes_windows() {
        let parsed = parse_account_list(&list_fixture().to_string()).unwrap();
        assert_eq!(parsed.active_account_number, Some(2));
        assert_eq!(parsed.accounts.len(), 3);
        let first = &parsed.accounts[0];
        assert_eq!(first.number, 1);
        assert_eq!(first.organization_name, "Work");
        // Out-of-range percentages are clamped like upstream.
        assert_eq!(first.five_hour.as_ref().unwrap().used_percent, 100.0);
        assert!(first.five_hour.as_ref().unwrap().resets_at.is_some());
        assert_eq!(first.scoped[0].name, "Fable only");
        assert_eq!(first.usage_status, ClaudeSwapUsageStatus::Ok);
    }

    #[test]
    fn rejects_unknown_and_missing_schema_versions() {
        let mut unknown = list_fixture();
        unknown["schemaVersion"] = json!(2);
        assert!(matches!(
            parse_account_list(&unknown.to_string()),
            Err(ClaudeSwapError::UnsupportedSchemaVersion(2))
        ));

        let mut missing = list_fixture();
        missing.as_object_mut().unwrap().remove("schemaVersion");
        assert!(matches!(
            parse_account_list(&missing.to_string()),
            Err(ClaudeSwapError::MissingSchemaVersion)
        ));

        assert!(matches!(
            parse_account_list("not json"),
            Err(ClaudeSwapError::NotJsonObject)
        ));
        assert!(matches!(
            parse_account_list("[]"),
            Err(ClaudeSwapError::NotJsonObject)
        ));
    }

    #[test]
    fn sanitize_display_strips_terminal_escapes_and_bounds_length() {
        let csi = "\u{1b}[31mred\u{1b}[0m";
        assert_eq!(sanitize_display(csi, MAX_LABEL_CHARS), "red");

        let osc = "a\u{1b}]0;ignored\u{07}b";
        assert_eq!(sanitize_display(osc, MAX_LABEL_CHARS), "ab");

        let multiline = "one\r\ntwo\u{2028}three";
        assert_eq!(
            sanitize_display(multiline, MAX_LABEL_CHARS),
            "one two three"
        );

        let long = "x".repeat(MAX_LABEL_CHARS + 50);
        assert_eq!(
            sanitize_display(&long, MAX_LABEL_CHARS).chars().count(),
            MAX_LABEL_CHARS
        );
    }

    #[test]
    fn unknown_status_is_neither_echoed_nor_actionable() {
        let mut fixture = list_fixture();
        fixture["accounts"][2]["usageStatus"] = json!("super_secret_token\u{1b}]0;leak\u{07}");
        let parsed = parse_account_list(&fixture.to_string()).unwrap();
        let row = parsed.accounts.iter().find(|a| a.number == 3).unwrap();
        assert_eq!(row.usage_status, ClaudeSwapUsageStatus::Unknown);

        let projected = project_accounts(&parsed, false);
        let account = projected.iter().find(|a| a.slot == 3).unwrap();
        assert_eq!(account.status, "unknown");
        assert!(!account.can_activate);
        let error = account.error.as_deref().unwrap();
        assert!(!error.contains("super_secret_token"));
        assert!(!error.contains('\u{1b}'));
    }

    #[test]
    fn external_labels_strip_escapes_and_respect_length_bounds() {
        let hostile = format!("\u{1b}[31mEvil\u{1b}[0m\n{}", "x".repeat(400));
        let mut fixture = list_fixture();
        fixture["accounts"][2]["alias"] = json!(hostile);
        let parsed = parse_account_list(&fixture.to_string()).unwrap();
        let alias = parsed
            .accounts
            .iter()
            .find(|a| a.number == 3)
            .unwrap()
            .alias
            .as_deref()
            .unwrap();
        assert!(!alias.contains('\u{1b}'));
        assert!(alias.contains("Evil"));
        assert!(alias.chars().count() <= MAX_LABEL_CHARS);
    }

    #[test]
    fn reported_error_envelope_is_sanitized_and_bounded() {
        let raw = json!({
            "schemaVersion": 1,
            "error": {
                "type": "\u{1b}[31mBad\u{07}",
                "message": format!("\u{1b}]0;leak\u{07}{}", "y".repeat(900))
            }
        });
        match parse_account_list(&raw.to_string()) {
            Err(ClaudeSwapError::ReportedError { kind, message }) => {
                assert!(!kind.contains('\u{1b}'));
                assert!(!message.contains('\u{1b}'));
                assert!(message.chars().count() <= MAX_DIAGNOSTIC_CHARS);
            }
            other => panic!("expected reported error, got {other:?}"),
        }
    }

    #[test]
    fn surfaces_error_envelope_instead_of_partial_accounts() {
        let raw = json!({
            "schemaVersion": 1,
            "error": { "type": "LockHeld", "message": "another cswap is running" }
        });
        match parse_account_list(&raw.to_string()) {
            Err(ClaudeSwapError::ReportedError { kind, message }) => {
                assert_eq!(kind, "LockHeld");
                assert!(message.contains("another cswap"));
            }
            other => panic!("expected reported error, got {other:?}"),
        }
    }

    #[test]
    fn rejects_disagreeing_active_fields_and_duplicate_slots() {
        let mut disagree = list_fixture();
        disagree["activeAccountNumber"] = json!(1);
        assert!(matches!(
            parse_account_list(&disagree.to_string()),
            Err(ClaudeSwapError::MalformedShape(_))
        ));

        let mut duplicate = list_fixture();
        {
            let accounts = duplicate["accounts"].as_array_mut().unwrap();
            accounts[1]["number"] = json!(1);
            accounts[0]["active"] = json!(false);
        }
        duplicate["activeAccountNumber"] = json!(null);
        assert!(matches!(
            parse_account_list(&duplicate.to_string()),
            Err(ClaudeSwapError::MalformedShape(_))
        ));
    }

    #[test]
    fn same_email_accounts_get_distinct_stable_ids_and_labels() {
        let parsed = parse_account_list(&list_fixture().to_string()).unwrap();
        let projected = project_accounts(&parsed, false);
        // Active row sorts first.
        assert_eq!(projected[0].id, "claude-swap:2");
        let work = projected.iter().find(|a| a.slot == 1).unwrap();
        let personal = projected.iter().find(|a| a.slot == 2).unwrap();
        assert_ne!(work.id, personal.id);
        assert_eq!(work.label, "same@example.com · Work");
        assert_eq!(personal.label, "same@example.com · Personal");
        // Alias wins over email and expired slots are not actionable.
        let backup = projected.iter().find(|a| a.slot == 3).unwrap();
        assert_eq!(backup.label, "Backup");
        assert!(!backup.can_activate);
        assert_eq!(personal.status, "ok");
        assert!(projected.iter().find(|a| a.slot == 1).unwrap().can_activate);
    }

    #[test]
    fn hiding_personal_info_collapses_to_ordinals() {
        let parsed = parse_account_list(&list_fixture().to_string()).unwrap();
        let projected = project_accounts(&parsed, true);
        for account in &projected {
            assert_eq!(account.label, format!("Account {}", account.slot));
            assert!(account.email.is_none());
            assert!(account.organization.is_none());
            assert!(account.alias.is_none());
        }
    }

    #[test]
    fn ignores_malformed_scoped_rows_without_dropping_valid_windows() {
        let mut fixture = list_fixture();
        fixture["accounts"][0]["usage"]["scoped"] = json!([
            { "name": "Fable only", "pct": 4.0 },
            { "name": "", "pct": 9.0 },
            { "pct": 3.0 },
            "nonsense",
            { "name": "Broken reset", "pct": 2.0, "resetsAt": "not-a-date" }
        ]);
        let parsed = parse_account_list(&fixture.to_string()).unwrap();
        let scoped = &parsed.accounts[0].scoped;
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].name, "Fable only");
        assert!(parsed.accounts[0].five_hour.is_some());
    }

    #[test]
    fn exact_argument_arrays_never_use_a_shell() {
        assert_eq!(list_arguments(), vec!["--list", "--json"]);
        assert_eq!(switch_arguments(7), vec!["--switch-to", "7", "--json"]);
        // A slot is rendered as a plain decimal token, so it cannot smuggle
        // extra arguments or shell metacharacters.
        assert_eq!(list_arguments().len(), 2);
        assert_eq!(switch_arguments(1).len(), 3);
    }

    #[test]
    fn executable_path_requires_a_non_empty_value_and_expands_tilde() {
        assert!(matches!(
            resolve_executable_path("   "),
            Err(ClaudeSwapError::ExecutablePathNotConfigured)
        ));
        let expanded = resolve_executable_path("~/bin/cswap").unwrap();
        assert!(expanded.ends_with("bin/cswap") || expanded.ends_with("bin\\cswap"));
        assert!(!expanded.to_string_lossy().starts_with('~'));
        let explicit = resolve_executable_path("C:/tools/cswap.exe").unwrap();
        assert_eq!(explicit, PathBuf::from("C:/tools/cswap.exe"));
    }

    #[test]
    fn missing_explicit_executable_path_fails_before_spawn() {
        let missing = if cfg!(windows) {
            "C:/definitely/not/here/cswap.exe"
        } else {
            "/definitely/not/here/cswap"
        };
        assert!(matches!(
            read_account_list(missing),
            Err(ClaudeSwapError::ExecutableNotFound)
        ));
    }

    #[test]
    fn switch_result_requires_matching_target_slot() {
        let raw = json!({
            "schemaVersion": 1,
            "switched": true,
            "from": { "number": 2 },
            "to": { "number": 3 },
            "reason": "switched"
        });
        let parsed = parse_switch_result(&raw.to_string()).unwrap();
        assert!(parsed.switched);
        assert_eq!(parsed.from_account_number, Some(2));
        assert_eq!(parsed.to_account_number, 3);
        assert!(validate_switch_target(3, &parsed).is_ok());

        let wrong = json!({
            "schemaVersion": 1,
            "switched": true,
            "from": { "number": 1 },
            "to": { "number": 2 },
            "reason": "switched"
        });
        let wrong = parse_switch_result(&wrong.to_string()).unwrap();
        assert!(matches!(
            validate_switch_target(3, &wrong),
            Err(ClaudeSwapError::MismatchedTarget {
                expected: 3,
                actual: 2
            })
        ));
    }

    #[test]
    fn switch_result_rejects_missing_reason_and_bad_schema() {
        let missing_reason = json!({
            "schemaVersion": 1,
            "switched": true,
            "from": { "number": 1 },
            "to": { "number": 2 }
        });
        assert!(matches!(
            parse_switch_result(&missing_reason.to_string()),
            Err(ClaudeSwapError::MalformedShape(_))
        ));

        let bad_schema = json!({
            "schemaVersion": 9,
            "switched": true,
            "from": { "number": 1 },
            "to": { "number": 2 },
            "reason": "switched"
        });
        assert!(matches!(
            parse_switch_result(&bad_schema.to_string()),
            Err(ClaudeSwapError::UnsupportedSchemaVersion(9))
        ));
    }
}
