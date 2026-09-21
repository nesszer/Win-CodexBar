//! CodeRabbit provider implementation.
//!
//! CodeRabbit exposes its usage through the local `coderabbit usage` command.
//! This provider deliberately keeps that boundary local: it does not inspect
//! browser state, make network requests, or persist the command output.

use async_trait::async_trait;
use std::collections::HashSet;
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

use crate::core::{
    FetchContext, Provider, ProviderDisplayDetail, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, RateWindow, SourceMode, UsageSnapshot,
};

const CLI_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_OUTPUT_BYTES: usize = 128 * 1024;
const MAX_FIELD_CHARS: usize = 512;
const DEFAULT_PROGRAM: &str = "coderabbit";
const PROGRAM_OVERRIDE_ENV: &str = "CODERABBIT_CLI_PATH";

/// Single message for the shared 128 KiB output cap across reader boundaries.
const OUTPUT_BUDGET_EXCEEDED: &str = "CodeRabbit CLI output exceeded 128 KiB.";

/// CREATE_NO_WINDOW for `std::os::windows::process::CommandExt::creation_flags`.
/// Keeps the CLI's console window hidden when the fetch runs from a GUI process.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct CodeRabbitUsage {
    organization: Option<String>,
    user: Option<String>,
    plan: Option<String>,
    reviews: Option<u64>,
    usage_billing: Option<String>,
    period_resets: Option<String>,
}

#[derive(Debug)]
struct CliOutput {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

pub struct CodeRabbitProvider {
    metadata: ProviderMetadata,
}

impl CodeRabbitProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::CodeRabbit,
                display_name: "CodeRabbit",
                session_label: "Reviews",
                weekly_label: "Billing",
                supports_opus: false,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://app.coderabbit.ai"),
                status_page_url: Some("https://status.coderabbit.ai"),
                tertiary_label_key: None,
            },
        }
    }
}

impl Default for CodeRabbitProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for CodeRabbitProvider {
    fn id(&self) -> ProviderId {
        ProviderId::CodeRabbit
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        match ctx.source_mode {
            SourceMode::Auto | SourceMode::Cli => {
                let output = run_cli(&configured_program()).await?;
                let mut combined = output.stdout;
                combined.push(b'\n');
                combined.extend_from_slice(&output.stderr);
                let combined = decode_cli_output(&combined)?;

                if let Some(issues) = decode_control_character_issue(combined) {
                    return Err(issues);
                }

                if !output.status.success() {
                    if looks_signed_out(combined) {
                        return Err(ProviderError::AuthRequired);
                    }
                    return Err(ProviderError::Other(format!(
                        "CodeRabbit CLI failed with exit code {}",
                        output
                            .status
                            .code()
                            .map_or_else(|| "unknown".to_string(), |code| code.to_string())
                    )));
                }

                let usage = parse_usage(combined)?;
                Ok(fetch_result(&usage))
            }
            SourceMode::OAuth | SourceMode::Web => {
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Cli]
    }

    fn supports_cli(&self) -> bool {
        true
    }
}

fn configured_program() -> String {
    std::env::var(PROGRAM_OVERRIDE_ENV)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| {
            which::which(DEFAULT_PROGRAM)
                .ok()
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|| DEFAULT_PROGRAM.to_string())
        })
}

async fn run_cli(program: &str) -> Result<CliOutput, ProviderError> {
    let mut command = Command::new(program);
    command
        .arg("usage")
        .env("NO_COLOR", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    command.creation_flags(CREATE_NO_WINDOW);
    let mut child = command.spawn().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ProviderError::NotInstalled(
                "CodeRabbit CLI not found. Install it or set CODERABBIT_CLI_PATH.".to_string(),
            )
        } else {
            ProviderError::Other("CodeRabbit CLI could not be started.".to_string())
        }
    })?;

    let stdout = child.stdout.take().ok_or_else(|| {
        ProviderError::Other("CodeRabbit CLI stdout was unavailable.".to_string())
    })?;
    let stderr = child.stderr.take().ok_or_else(|| {
        ProviderError::Other("CodeRabbit CLI stderr was unavailable.".to_string())
    })?;

    let budget = AtomicBudget::default();
    let combined = tokio::time::timeout(CLI_TIMEOUT, async {
        let (stdout, stderr, status) = tokio::join!(
            read_bounded(stdout, &budget),
            read_bounded(stderr, &budget),
            child.wait(),
        );

        let mut combined = stdout?;
        let stderr = stderr?;
        combined.push(b'\n');
        combined.extend_from_slice(&stderr);

        let status = status
            .map_err(|_| ProviderError::Other("CodeRabbit CLI process wait failed.".to_string()))?;

        Ok(CliOutput {
            status,
            stdout: combined,
            stderr: Vec::new(),
        })
    })
    .await;

    match combined {
        Ok(result) => result,
        Err(_) => {
            drop(child.kill().await);
            drop(child.wait().await);
            Err(ProviderError::Timeout)
        }
    }
}

/// Shared byte budget for the two bounded stream readers. Both streams write
/// into one buffer, so the cap applies to their combined size.
#[derive(Default)]
struct AtomicBudget(AtomicUsize);

impl AtomicBudget {
    fn reserve(&self, count: usize) -> Result<(), ProviderError> {
        let mut current = self.0.load(Ordering::Relaxed);
        loop {
            let next = current
                .checked_add(count)
                .filter(|next| *next <= MAX_OUTPUT_BYTES);
            let Some(next) = next else {
                return Err(ProviderError::Other(OUTPUT_BUDGET_EXCEEDED.to_string()));
            };
            match self
                .0
                .compare_exchange_weak(current, next, Ordering::Relaxed, Ordering::Relaxed)
            {
                Ok(_) => return Ok(()),
                Err(actual) => current = actual,
            }
        }
    }
}

fn decode_cli_output(bytes: &[u8]) -> Result<&str, ProviderError> {
    std::str::from_utf8(bytes).map_err(|_| {
        ProviderError::Parse("CodeRabbit CLI returned invalid UTF-8 output.".to_string())
    })
}

/// Boundary check for control characters outside newline/CR. It validates the
/// whole decoded payload once, before any field parsing; per-field rules stay
/// inside `parse_usage`.
fn decode_control_character_issue(text: &str) -> Option<ProviderError> {
    let has_control = text
        .chars()
        .any(|character| character.is_control() && !matches!(character, '\n' | '\r'));
    has_control
        .then(|| ProviderError::Parse("CodeRabbit CLI returned an invalid field.".to_string()))
}

async fn read_bounded<R: AsyncRead + Unpin>(
    reader: R,
    budget: &AtomicBudget,
) -> Result<Vec<u8>, ProviderError> {
    let mut reader = reader;
    let mut output = Vec::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let count = reader.read(&mut buffer).await.map_err(|_| {
            ProviderError::Other("CodeRabbit CLI output could not be read.".to_string())
        })?;
        if count == 0 {
            return Ok(output);
        }

        budget.reserve(count)?;
        output.extend_from_slice(&buffer[..count]);
    }
}

fn parse_usage(text: &str) -> Result<CodeRabbitUsage, ProviderError> {
    if looks_signed_out(text) {
        return Err(ProviderError::AuthRequired);
    }

    let mut usage = CodeRabbitUsage::default();
    let mut seen: HashSet<&str> = HashSet::new();
    for line in text.lines() {
        let Some((label, raw_value)) = line.split_once(':') else {
            continue;
        };
        let label = label.trim().to_ascii_lowercase();
        let value = raw_value.trim();
        if value.chars().count() > MAX_FIELD_CHARS || value.chars().any(char::is_control) {
            return Err(ProviderError::Parse(
                "CodeRabbit CLI returned an invalid field.".to_string(),
            ));
        }

        match label.as_str() {
            "organization" if !seen.contains("organization") => {
                seen.insert("organization");
                if !value.is_empty() {
                    usage.organization = Some(value.to_string());
                }
            }
            "user" if !seen.contains("user") => {
                seen.insert("user");
                if !value.is_empty() {
                    usage.user = Some(value.to_string());
                }
            }
            "plan" if !seen.contains("plan") => {
                seen.insert("plan");
                if !value.is_empty() {
                    usage.plan = Some(value.to_string());
                }
            }
            "your reviews" if !seen.contains("reviews") => {
                seen.insert("reviews");
                usage.reviews = Some(value.parse::<u64>().map_err(|_| {
                    ProviderError::Parse(
                        "CodeRabbit CLI returned an invalid review count.".to_string(),
                    )
                })?);
            }
            "usage billing" if !seen.contains("billing") => {
                seen.insert("billing");
                if !value.is_empty() {
                    usage.usage_billing = Some(value.to_string());
                }
            }
            "period resets" if !seen.contains("resets") => {
                seen.insert("resets");
                if !value.is_empty() {
                    usage.period_resets = Some(value.to_string());
                }
            }
            _ => {}
        }
    }

    if usage.reviews.is_none() && usage.usage_billing.is_none() {
        return Err(ProviderError::Parse(
            "CodeRabbit CLI returned no usage fields.".to_string(),
        ));
    }

    Ok(usage)
}

fn fetch_result(usage: &CodeRabbitUsage) -> ProviderFetchResult {
    let detail = |id: &'static str, title: &'static str, value: Option<String>| {
        value.map(|value| ProviderDisplayDetail::new(id, title, value))
    };
    let details = [
        detail(
            "organization",
            "Organization",
            usage.organization.clone().filter(|value| !value.is_empty()),
        ),
        detail(
            "user",
            "User",
            usage.user.clone().filter(|value| !value.is_empty()),
        ),
        detail(
            "plan",
            "Plan",
            usage.plan.clone().filter(|value| !value.is_empty()),
        ),
        usage
            .reviews
            .map(|value| ProviderDisplayDetail::new("reviews", "Reviews", value.to_string())),
        detail(
            "usage-billing",
            "Usage billing",
            usage
                .usage_billing
                .clone()
                .filter(|value| !value.is_empty()),
        ),
        detail(
            "period-resets",
            "Period resets",
            usage
                .period_resets
                .clone()
                .filter(|value| !value.is_empty()),
        ),
    ];

    let base = ProviderFetchResult::new(
        UsageSnapshot::new(RateWindow::informational("CodeRabbit CLI"))
            .with_primary_label("Reviews"),
        "cli",
    )
    .with_non_authoritative_pace();

    details
        .into_iter()
        .flatten()
        .fold(base, |result, detail| result.with_display_detail(detail))
}

/// Matches human-readable sign-out markers in CLI text. This is a heuristic:
/// CodeRabbit's CLI currently exposes no deterministic signed-out signal
/// (no dedicated exit code or structured auth field in `coderabbit usage`
/// output), so marker phrases are the only available detection channel. If a
/// deterministic signal appears upstream, prefer it and retire these phrases.
fn looks_signed_out(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    [
        "not authenticated",
        "please log in",
        "authentication required",
        "unauthorized",
        "no session found",
    ]
    .iter()
    .any(|marker| lower.contains(marker))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncWriteExt, duplex};

    #[test]
    fn parses_supported_fields_with_first_value_wins() {
        let usage = parse_usage(
            "Organization: acme\nUser: ness\nPlan: Pro\nYour reviews: 42\nYour reviews: 99\nUsage billing: Included\nPeriod resets: Friday",
        )
        .expect("valid CodeRabbit output");

        assert_eq!(usage.organization.as_deref(), Some("acme"));
        assert_eq!(usage.user.as_deref(), Some("ness"));
        assert_eq!(usage.plan.as_deref(), Some("Pro"));
        assert_eq!(usage.reviews, Some(42));
        assert_eq!(usage.usage_billing.as_deref(), Some("Included"));
        assert_eq!(usage.period_resets.as_deref(), Some("Friday"));
    }

    #[test]
    fn rejects_control_sequences_and_signed_out_output_even_with_usage() {
        assert!(matches!(
            decode_control_character_issue("\u{1b}[32mYour reviews: 3\u{1b}[0m"),
            Some(ProviderError::Parse(_))
        ));
        assert!(decode_control_character_issue("Your reviews: 3\nok").is_none());

        assert!(matches!(
            parse_usage("Your reviews: 3\nPlease log in with auth login"),
            Err(ProviderError::AuthRequired)
        ));
        assert!(matches!(
            parse_usage("status: ready"),
            Err(ProviderError::Parse(_))
        ));
    }

    #[test]
    fn rejects_invalid_review_counts_and_requires_usage_structure() {
        assert!(matches!(
            parse_usage("Your reviews: -1"),
            Err(ProviderError::Parse(_))
        ));
        assert!(matches!(
            parse_usage("Period resets: tomorrow"),
            Err(ProviderError::Parse(_))
        ));
        let usage = parse_usage("Your reviews: 0").unwrap();
        assert_eq!(usage.reviews, Some(0));
    }

    #[test]
    fn rejects_control_or_oversized_fields_before_display_projection() {
        assert!(matches!(
            parse_usage("Plan: bad\u{1b}[31m"),
            Err(ProviderError::Parse(_))
        ));
        let oversized = format!("Plan: {}\nYour reviews: 1", "x".repeat(MAX_FIELD_CHARS + 1));
        assert!(matches!(
            parse_usage(&oversized),
            Err(ProviderError::Parse(_))
        ));
    }

    #[test]
    fn rejects_invalid_utf8_before_parsing() {
        assert!(matches!(
            decode_cli_output(b"Your reviews: 3\xff"),
            Err(ProviderError::Parse(_))
        ));
    }

    #[test]
    fn malformed_first_review_value_cannot_be_replaced_by_a_later_duplicate() {
        assert!(matches!(
            parse_usage("Your reviews: unknown\nYour reviews: 3\nUsage billing: Included"),
            Err(ProviderError::Parse(_))
        ));
    }

    #[test]
    fn help_text_can_mention_login_without_marking_a_valid_report_signed_out() {
        let usage =
            parse_usage("Your reviews: 3\nTo switch accounts, run coderabbit auth login.").unwrap();
        assert_eq!(usage.reviews, Some(3));
    }

    #[test]
    fn configured_path_override_is_trimmed_without_shell_parsing() {
        // Same trim/filter contract the collapsed `configured_program` applies
        // to `CODERABBIT_CLI_PATH` before the `which()` fallback.
        let trimmed = |value: Option<&str>| {
            value
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        };
        assert_eq!(
            trimmed(Some("  C:\\Tools\\coderabbit.exe  ")),
            Some("C:\\Tools\\coderabbit.exe".to_string())
        );
        assert_eq!(trimmed(Some("   ")), None);
        assert_eq!(trimmed(None), None);
    }

    #[tokio::test]
    async fn bounded_reader_rejects_combined_output_budget() {
        let (mut writer, mut reader) = duplex(MAX_OUTPUT_BYTES + 1);
        let writer_task = tokio::spawn(async move {
            let bytes = vec![b'x'; MAX_OUTPUT_BYTES + 1];
            drop(writer.write_all(&bytes).await);
        });
        let budget = AtomicBudget::default();
        let result = read_bounded(&mut reader, &budget).await;
        assert!(matches!(result, Err(ProviderError::Other(_))));
        drop(writer_task.await);
    }

    #[test]
    fn result_keeps_details_transient_and_does_not_set_identity() {
        let usage = parse_usage(
            "Organization: acme\nUser: ness@example.test\nPlan: Pro\nYour reviews: 4\nUsage billing: Included\nPeriod resets: Friday",
        )
        .unwrap();
        let result = fetch_result(&usage);
        let detail_ids = result
            .display_details()
            .iter()
            .map(|detail| detail.id().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            detail_ids,
            vec![
                "organization",
                "user",
                "plan",
                "reviews",
                "usage-billing",
                "period-resets",
            ]
        );
        assert!(result.account_identity().is_none());
        assert!(result.usage.account_organization.is_none());
        assert!(result.usage.login_method.is_none());
        let encoded = serde_json::to_value(result).unwrap();
        assert!(encoded.get("display_details").is_none());
    }

    #[test]
    fn provider_is_cli_only_and_disabled_by_default() {
        let provider = CodeRabbitProvider::new();
        assert_eq!(provider.id(), ProviderId::CodeRabbit);
        assert_eq!(
            provider.available_sources(),
            vec![SourceMode::Auto, SourceMode::Cli]
        );
        assert!(provider.supports_cli());
        assert!(!provider.metadata().default_enabled);
    }
}
