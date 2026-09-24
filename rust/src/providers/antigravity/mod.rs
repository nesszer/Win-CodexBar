//! Antigravity provider implementation
//!
//! Fetches usage data from Antigravity's local language server probe
//! Uses Windows process detection to find CSRF token

mod cli_fallback;
mod cli_resolution;
mod legacy_status;
mod local_proto;
pub mod local_sessions;
mod local_sqlite;
mod local_step_resolver;
mod quota_summary;

use legacy_status::{UserStatus, UserStatusResponse};

#[cfg(windows)]
use crate::managed_process::{ManagedProcess, ManagedProcessConfig, ManagedProcessError};
use async_trait::async_trait;
#[cfg(windows)]
use futures::{StreamExt, stream};
use regex_lite::Regex;
#[cfg(windows)]
use std::ffi::OsString;
use std::future::Future;
#[cfg(windows)]
use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::LazyLock;
#[cfg(windows)]
use std::time::Duration;
#[cfg(windows)]
use std::time::Instant;

use crate::core::{
    FetchContext, Provider, ProviderError, ProviderFetchResult, ProviderId, ProviderMetadata,
    RateWindow, SourceMode, UsageSnapshot,
};

const AGY_NOT_FOUND_MESSAGE: &str =
    "Antigravity is not running and the signed-in agy CLI was not found.";
#[cfg(windows)]
const AGY_ATTEMPT_TIMEOUT: Duration = Duration::from_secs(25);
#[cfg(windows)]
const AGY_CLEANUP_RESERVE: Duration = Duration::from_secs(2);
#[cfg(windows)]
const AGY_PROBE_TIMEOUT: Duration = Duration::from_millis(750);
#[cfg(windows)]
const AGY_READY_POLL_INTERVAL: Duration = Duration::from_millis(250);
const GET_USER_STATUS_PATH: &str = "/exa.language_server_pb.LanguageServerService/GetUserStatus";
const QUOTA_SUMMARY_PATH: &str =
    "/exa.language_server_pb.LanguageServerService/RetrieveUserQuotaSummary";
/// Serialize task-owned `agy` launches so concurrent app surfaces never start
/// multiple interactive CLI servers at the same time.
#[cfg(windows)]
static MANAGED_AGY_FETCH: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The managed-process owner stays provider-neutral; the provider only maps its
/// error surface into [`ProviderError`].
#[cfg(windows)]
impl From<ManagedProcessError> for ProviderError {
    fn from(error: ManagedProcessError) -> Self {
        ProviderError::Other(error.to_string())
    }
}

/// Antigravity provider
pub struct AntigravityProvider {
    metadata: ProviderMetadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AntigravityStrategyId {
    Local,
    Cli,
    Offline,
}

impl AntigravityStrategyId {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Cli => "cli",
            Self::Offline => "offline",
        }
    }
}

pub(crate) fn strategy_from_source_label(source_label: &str) -> Option<AntigravityStrategyId> {
    match source_label {
        "local" => Some(AntigravityStrategyId::Local),
        "cli" => Some(AntigravityStrategyId::Cli),
        "offline" => Some(AntigravityStrategyId::Offline),
        _ => None,
    }
}

/// Return a regex that matches `--<flag> <value>` or `--<flag>=<value>`.
fn flag_re(flag: &str) -> Regex {
    Regex::new(&format!("--{f}(?:\\s+|\\s*=\\s*)(\\S+)", f = flag)).expect("valid flag pattern")
}

/// The kind of local Antigravity process a `ProcessInfo` was derived from.
///
/// The desktop IDE/app language server authenticates local requests with a
/// `--csrf_token` flag and requires the `X-Codeium-Csrf-Token` header. The
/// `agy` CLI hosts the same language server in-process but launches it without
/// that flag and serves the quota endpoints with no CSRF header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProcessSource {
    /// Desktop IDE/app language server — requires a CSRF token.
    Ide,
    /// `agy` CLI language server — no CSRF token required.
    Cli,
}

/// True when `command_line` looks like the `agy` CLI language server process.
///
/// `agy.exe` (and `antigravity-cli` / `antigravity_cli`) hosts the same local
/// language server as the IDE but under a different process name and without a
/// `--csrf_token` flag. Match either the bare `agy` executable or the
/// `antigravity-cli` package name; a leading path separator prevents unrelated
/// names (e.g. `notantigravity-cli`) from matching.
fn is_agy_cli_command(command_line: &str) -> bool {
    static CLI_PATH_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(^|[\\/])(antigravity-cli|antigravity_cli)(?:"|[\s/\\]|$)"#)
            .expect("valid antigravity-cli pattern")
    });
    static AGY_RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(^|[\\/])agy(\.exe)?(?:"|\s|$)"#).expect("valid agy pattern")
    });
    let lower = command_line.to_ascii_lowercase();
    CLI_PATH_RE.is_match(&lower) || AGY_RE.is_match(&lower)
}

impl AntigravityProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::Antigravity,
                display_name: "Antigravity",
                session_label: "Claude",
                weekly_label: "Gemini Pro",
                supports_opus: true,
                supports_credits: false,
                default_enabled: false,
                is_primary: false,
                dashboard_url: None,
                status_page_url: None,
                tertiary_label_key: None,
            },
        }
    }

    /// Detect running Antigravity language server and extract connection info
    fn detect_process_info() -> Result<Option<ProcessInfo>, ProviderError> {
        // Use PowerShell to get process command lines
        #[cfg(windows)]
        const CREATE_NO_WINDOW: u32 = 0x08000000;

        let mut cmd = Command::new("powershell.exe");
        cmd.args([
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-ExecutionPolicy", "Bypass",
                "-Command",
                // Match the desktop IDE/app language server (language_server.exe /
                // language_server_windows*) and the `agy` CLI (agy / agy.exe), which
                // hosts the same language server in-process with no --csrf_token flag.
                "Get-CimInstance Win32_Process | Where-Object { $_.Name -like '*language_server_windows*' -or $_.Name -like 'language_server.exe' -or $_.Name -eq 'agy.exe' -or $_.Name -eq 'agy' } | ForEach-Object { \"$($_.ProcessId)`t$($_.CommandLine)\" }"
            ]);
        #[cfg(windows)]
        cmd.creation_flags(CREATE_NO_WINDOW);

        let output = cmd
            .output()
            .map_err(|e| ProviderError::Other(format!("Failed to run PowerShell: {}", e)))?;

        if !output.status.success() {
            return Err(ProviderError::NotInstalled(
                "Failed to detect Antigravity process".to_string(),
            ));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(Self::parse_process_info(&stdout))
    }

    fn parse_process_info(stdout: &str) -> Option<ProcessInfo> {
        // Shared argument parser: handles `--flag value` and `--flag=value` forms
        let csrf_re = flag_re("csrf_token");
        let ext_csrf_re = flag_re("extension_server_csrf_token");
        let port_re = flag_re("extension_server_port");
        let https_port_re = flag_re("https_server_port");

        // Prefer desktop IDE/app matches (which carry a --csrf_token) over the
        // tokenless `agy` CLI so the CSRF-protected endpoint is used when both
        // happen to be running. Only fall back to a CLI match when no IDE match
        // is found, mirroring upstream's process-kind precedence.
        let mut cli_match: Option<ProcessInfo> = None;

        for line in stdout.lines() {
            // Line is "<pid>\t<command line>"; split off the PID prefix we added so the
            // PID can be used to enumerate the process's real listening ports below.
            let (pid, line) = match line.split_once('\t') {
                Some((p, rest)) => (p.trim().parse::<u32>().ok(), rest),
                None => (None, line),
            };

            let csrf_token = csrf_re
                .captures(line)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string());

            let ext_csrf_token = ext_csrf_re
                .captures(line)
                .and_then(|c| c.get(1))
                .map(|m| m.as_str().to_string());

            let port = port_re
                .captures(line)
                .and_then(|c| c.get(1))
                .and_then(|m| m.as_str().parse::<u16>().ok())
                .or_else(|| {
                    https_port_re
                        .captures(line)
                        .and_then(|c| c.get(1))
                        .and_then(|m| m.as_str().parse::<u16>().ok())
                });

            // Desktop IDE/app language server: requires --csrf_token.
            if let Some(token) = csrf_token {
                return Some(ProcessInfo {
                    csrf_token: token,
                    extension_server_csrf_token: ext_csrf_token,
                    extension_port: port,
                    pid,
                    source: ProcessSource::Ide,
                });
            }

            // `agy` CLI: hosts the same language server without --csrf_token.
            // Allow an empty CSRF token; the CLI's quota endpoint requires none.
            if cli_match.is_none() && is_agy_cli_command(line) {
                cli_match = Some(ProcessInfo {
                    csrf_token: String::new(),
                    extension_server_csrf_token: None,
                    extension_port: port,
                    pid,
                    source: ProcessSource::Cli,
                });
            }
        }

        cli_match
    }

    /// Find the actual API port by probing the language server's candidate ports.
    async fn find_api_port(
        extension_port: Option<u16>,
        pid: Option<u32>,
    ) -> Result<u16, ProviderError> {
        // The language server binds a RANDOM localhost port at startup; --extension_server_port
        // is only a reference point (and belongs to a separate HTTP extension server), so the
        // real gRPC/Connect API port is not guaranteed to be within a small window above it.
        // Mirror the macOS/Linux probe (which uses `lsof`) by enumerating the language-server
        // process's own listening ports first, then fall back to a heuristic window above the
        // extension port and a few historically-seen ports.
        //
        // SECURITY: TLS verification is disabled because the local language server uses a
        // self-signed certificate. This is scoped to 127.0.0.1 only; we confirm a port by
        // checking that it answers the expected gRPC endpoint.
        // The language server is a local loopback endpoint. Do not route it
        // through the app-wide outbound proxy.
        let client = crate::core::credentialed_http_client_builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(2))
            .danger_accept_invalid_certs(true)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| ProviderError::Other(e.to_string()))?;

        // Ordered candidate ports: the process's real listening ports first (Windows
        // equivalent of `lsof`), then the heuristic window above the extension port, then a
        // few known ports as a last resort.
        let mut candidates: Vec<u16> = Vec::new();
        if let Some(pid) = pid
            && let Ok(ports) = Self::listening_ports_for_pid(pid)
        {
            candidates.extend(ports);
        }
        if let Some(ep) = extension_port.filter(|&p| p > 0) {
            candidates.extend((0..20u16).map(|offset| ep.saturating_add(offset)));
        }
        candidates.extend([53835, 53836, 53837, 53838, 53845, 53849]);

        let mut probed: Vec<u16> = Vec::new();
        for port in candidates {
            if probed.contains(&port) {
                continue; // probe each port at most once
            }
            probed.push(port);
            if Self::probe_api_port(&client, port).await {
                return Ok(port);
            }
        }

        Err(ProviderError::Other(
            "Could not find Antigravity API port".to_string(),
        ))
    }

    /// Probe a single candidate port. Returns true if it answers the language server's
    /// gRPC endpoint (HTTP 200 or 401).
    async fn probe_api_port(client: &reqwest::Client, port: u16) -> bool {
        let url = format!(
            "https://127.0.0.1:{}/exa.language_server_pb.LanguageServerService/GetUnleashData",
            port
        );
        match client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Connect-Protocol-Version", "1")
            .body("{}")
            .send()
            .await
        {
            Ok(resp) => {
                let code = resp.status().as_u16();
                code == 200 || code == 401
            }
            Err(_) => false,
        }
    }

    /// Enumerate IPv4 TCP listener ports for a PID through the Windows IP Helper API.
    /// This avoids starting PowerShell inside the managed readiness poll.
    ///
    /// The owner lives in the provider-neutral
    /// [`crate::managed_process::listening_ports_for_pid`]; this binding only
    /// maps its error into the provider surface.
    #[cfg(windows)]
    fn listening_ports_for_pid(pid: u32) -> Result<Vec<u16>, ProviderError> {
        crate::managed_process::listening_ports_for_pid(pid).map_err(ProviderError::from)
    }

    /// Non-Windows platforms have no `Get-NetTCPConnection`; return an empty list by design so
    /// the caller falls back to the heuristic candidate ports.
    #[cfg(not(windows))]
    fn listening_ports_for_pid(_pid: u32) -> Result<Vec<u16>, ProviderError> {
        Ok(Vec::new())
    }

    /// Fetch user status from Antigravity API.
    ///
    /// v0.56.0: prefer the quota-summary endpoint so the 5-hour and weekly
    /// lanes can be resolved independently across model families. The legacy
    /// model-quota payload remains the compatibility fallback.
    fn with_cadence_labels(mut usage: UsageSnapshot) -> UsageSnapshot {
        if usage
            .secondary
            .as_ref()
            .is_some_and(|window| window.window_minutes == Some(7 * 24 * 60))
            && usage.secondary_label.is_none()
        {
            usage.secondary_label = Some("Weekly".to_string());
        }
        usage
    }

    async fn fetch_user_status(&self) -> Result<Option<ProviderFetchResult>, ProviderError> {
        let process_info = tokio::task::spawn_blocking(Self::detect_process_info)
            .await
            .map_err(|error| {
                ProviderError::Other(format!(
                    "Failed to join the Antigravity process detector: {error}"
                ))
            })??;
        let Some(process_info) = process_info else {
            return Ok(None);
        };
        let api_port = Self::find_api_port(process_info.extension_port, process_info.pid).await?;
        self.fetch_user_status_at_port(&process_info, api_port)
            .await
            .map(Some)
    }

    async fn fetch_user_status_at_port(
        &self,
        process_info: &ProcessInfo,
        api_port: u16,
    ) -> Result<ProviderFetchResult, ProviderError> {
        // SECURITY: TLS verification disabled only for this loopback language server.
        let client = crate::core::credentialed_http_client_builder()
            .no_proxy()
            .timeout(std::time::Duration::from_secs(8))
            .danger_accept_invalid_certs(true)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|e| ProviderError::Other(e.to_string()))?;

        let quota_body = serde_json::json!({ "forceRefresh": true });
        match Self::fetch_local_payload(
            &client,
            process_info,
            api_port,
            QUOTA_SUMMARY_PATH,
            &quota_body,
            std::time::Duration::from_secs(4),
        )
        .await
        {
            Ok(bytes) => match quota_summary::parse_usage_snapshot(&bytes) {
                Ok(mut snapshot) => {
                    // Identity is best-effort enrichment and must not displace a
                    // successful quota-summary result.
                    let identity_body = serde_json::json!({
                        "metadata": {
                            "ideName": "antigravity",
                            "extensionName": "antigravity",
                            "ideVersion": "unknown",
                            "locale": "en"
                        }
                    });
                    if let Ok(identity_bytes) = Self::fetch_local_payload(
                        &client,
                        process_info,
                        api_port,
                        GET_USER_STATUS_PATH,
                        &identity_body,
                        std::time::Duration::from_secs(1),
                    )
                    .await
                        && let Ok(identity) =
                            serde_json::from_slice::<UserStatusResponse>(&identity_bytes)
                    {
                        legacy_status::apply_user_identity(&mut snapshot, &identity);
                    }
                    return Ok(Self::fetch_result(snapshot, AntigravityStrategyId::Local));
                }
                Err(error) => tracing::debug!(
                    %error,
                    "Antigravity quota summary unusable; falling back to model quotas"
                ),
            },
            Err(error) => tracing::debug!(
                %error,
                "Antigravity quota summary unavailable; falling back to model quotas"
            ),
        }

        let body = serde_json::json!({
            "metadata": {
                "ideName": "antigravity",
                "extensionName": "antigravity",
                "ideVersion": "unknown",
                "locale": "en"
            }
        });
        let bytes = Self::fetch_local_payload(
            &client,
            process_info,
            api_port,
            GET_USER_STATUS_PATH,
            &body,
            std::time::Duration::from_secs(8),
        )
        .await?;
        let response: UserStatusResponse = serde_json::from_slice(&bytes)
            .map_err(|e| ProviderError::Parse(format!("Failed to parse response: {e}")))?;
        self.parse_user_status(response)
            .map(|usage| Self::fetch_result(usage, AntigravityStrategyId::Local))
    }

    pub(super) fn fetch_result(
        usage: UsageSnapshot,
        strategy: AntigravityStrategyId,
    ) -> ProviderFetchResult {
        ProviderFetchResult::new(Self::with_cadence_labels(usage), strategy.as_str())
    }

    async fn try_print_usage_fallback(&self) -> Result<Option<ProviderFetchResult>, ProviderError> {
        cli_fallback::try_fetch(cli_resolution::locate_agy_binary()?).await
    }

    /// Start a short-lived, headless `agy` session when neither the Antigravity
    /// desktop app nor a user-owned CLI session is running. The deadline includes
    /// launch serialization, the after-lock recheck, startup, probing and cleanup.
    #[cfg(windows)]
    async fn fetch_with_managed_agy(&self) -> Result<ManagedAgyOutcome, ProviderError> {
        let deadline = Instant::now() + AGY_ATTEMPT_TIMEOUT;
        let lock_budget = deadline.saturating_duration_since(Instant::now());
        let _launch_guard = tokio::time::timeout(lock_budget, MANAGED_AGY_FETCH.lock())
            .await
            .map_err(|_| {
                ProviderError::Other(
                    "Timed out waiting for another managed agy refresh to finish".to_string(),
                )
            })?;

        // A desktop app or user-owned CLI may have appeared while this request
        // waited for the launch lock. Reuse it and never include it in our job.
        let recheck_budget = deadline.saturating_duration_since(Instant::now());
        if recheck_budget.is_zero() {
            return Err(Self::managed_agy_timeout());
        }
        match tokio::time::timeout(recheck_budget, self.fetch_user_status()).await {
            Ok(Ok(Some(usage))) => return Ok(ManagedAgyOutcome::Reused(usage)),
            Ok(Ok(None)) => {}
            Ok(Err(error)) => return Err(error),
            Err(_) => return Err(Self::managed_agy_timeout()),
        }

        let Some(binary) = cli_resolution::locate_agy_binary()? else {
            return Ok(ManagedAgyOutcome::Missing);
        };
        let probe_client = crate::core::credentialed_http_client_builder()
            .no_proxy()
            .timeout(AGY_PROBE_TIMEOUT)
            .danger_accept_invalid_certs(true)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|error| ProviderError::Other(error.to_string()))?;
        let config = ManagedProcessConfig {
            program: binary,
            args: Vec::new(),
            env: vec![
                (OsString::from("TERM"), OsString::from("xterm-256color")),
                (OsString::from("COLORTERM"), OsString::from("truecolor")),
            ],
            cwd: dirs::home_dir().filter(|path| path.is_dir()),
            pty_rows: 30,
            pty_cols: 120,
            label: "agy".to_string(),
        };
        let mut managed = ManagedProcess::spawn(&config)?;
        let pid = managed.pid();
        let process_info = ProcessInfo {
            csrf_token: String::new(),
            extension_server_csrf_token: None,
            extension_port: None,
            pid: Some(pid),
            source: ProcessSource::Cli,
        };

        let result = async {
            let work_deadline = deadline.checked_sub(AGY_CLEANUP_RESERVE).unwrap_or(deadline);
            let mut last_error = None;
            loop {
                if let Some(status) = managed.try_wait()? {
                    return Err(ProviderError::NotInstalled(format!(
                        "agy exited before its local quota service was ready ({status}). Open Antigravity or run agy and sign in, then retry."
                    )));
                }

                match Self::listening_ports_for_pid(pid) {
                    Ok(ports) => {
                        if let Some(port) = Self::first_ready_api_port(&probe_client, ports).await {
                            let remaining = work_deadline
                                .saturating_duration_since(Instant::now());
                            if remaining.is_zero() {
                                break;
                            }
                            match tokio::time::timeout(
                                remaining,
                                self.fetch_user_status_at_port(&process_info, port),
                            )
                            .await
                            {
                                Ok(Ok(usage)) => return Ok(ManagedAgyOutcome::Fetched(usage)),
                                Ok(Err(ProviderError::AuthRequired)) => {
                                    return Err(ProviderError::AuthRequired);
                                }
                                Ok(Err(error)) => last_error = Some(error),
                                Err(_) => break,
                            }
                        }
                    }
                    Err(error) => last_error = Some(error),
                }

                let remaining = work_deadline.saturating_duration_since(Instant::now());
                if remaining.is_zero() {
                    break;
                }
                tokio::time::sleep(AGY_READY_POLL_INTERVAL.min(remaining)).await;
            }

            if let Some(error) = last_error {
                tracing::debug!(%error, "managed agy quota service did not become ready");
            }
            Err(Self::managed_agy_timeout())
        }
        .await;

        managed.shutdown(AGY_CLEANUP_RESERVE).await;
        result
    }

    #[cfg(windows)]
    async fn first_ready_api_port(client: &reqwest::Client, ports: Vec<u16>) -> Option<u16> {
        let mut probes = stream::iter(
            ports
                .into_iter()
                .map(|port| async move { (port, Self::probe_api_port(client, port).await) }),
        )
        .buffer_unordered(4);
        while let Some((port, ready)) = probes.next().await {
            if ready {
                return Some(port);
            }
        }
        None
    }

    #[cfg(windows)]
    fn managed_agy_timeout() -> ProviderError {
        ProviderError::Other(
            "agy started but its quota service did not become ready before the managed refresh deadline. Open Antigravity or run agy and sign in, then retry."
                .to_string(),
        )
    }

    fn offline_usage_result() -> Option<ProviderFetchResult> {
        let count = local_sessions::offline_conversation_count();
        if count == 0 {
            return None;
        }
        let noun = if count == 1 {
            "conversation"
        } else {
            "conversations"
        };
        let usage = UsageSnapshot::new(RateWindow::informational(format!(
            "Offline · {count} {noun}"
        )))
        .with_login_method("offline");
        Some(ProviderFetchResult::new(
            usage,
            AntigravityStrategyId::Offline.as_str(),
        ))
    }

    /// Resolve a failure to obtain live usage.
    ///
    /// A failed sign-in is actionable, so it always surfaces. Every other
    /// failure means the runtime/CLI is unavailable or inconclusive, so an
    /// available offline conversation-history snapshot is preferred over
    /// discarding it for a transient error.
    fn resolve_probe_failure(
        error: ProviderError,
        offline: Option<ProviderFetchResult>,
    ) -> Result<ProviderFetchResult, ProviderError> {
        if matches!(error, ProviderError::AuthRequired) {
            return Err(error);
        }
        offline.ok_or(error)
    }

    /// Map a managed-lifecycle outcome onto provider policy.
    ///
    /// `Reused` means a user-owned runtime answered and the fetch stays local;
    /// `Fetched` means the task-owned CLI answered; `Missing` is a policy no-op
    /// so the caller can fall back to offline history; an error follows the
    /// same offline-preferred resolution as the local probe. This is the policy
    /// seam that the lifecycle owner deliberately does not own, so a fake
    /// lifecycle can be driven through it in tests.
    #[cfg(windows)]
    fn resolve_managed_outcome(
        outcome: Result<ManagedAgyOutcome, ProviderError>,
    ) -> Result<Option<ProviderFetchResult>, ProviderError> {
        match outcome {
            Ok(ManagedAgyOutcome::Reused(result)) => Ok(Some(result)),
            Ok(ManagedAgyOutcome::Fetched(mut result)) => {
                result.source_label = AntigravityStrategyId::Cli.as_str().to_string();
                Ok(Some(result))
            }
            Ok(ManagedAgyOutcome::Missing) => Ok(None),
            Err(error) => {
                if !matches!(error, ProviderError::AuthRequired) {
                    tracing::debug!(%error, "managed Antigravity CLI probe failed");
                }
                Self::resolve_probe_failure(error, Self::offline_usage_result()).map(Some)
            }
        }
    }

    /// Resolve the ordered fallback chain once per fetch.
    ///
    /// A successful local probe is terminal. A local probe failure is
    /// inconclusive, including the typed `AuthRequired` produced by the
    /// affected local API/CSRF path, so the structured CLI report may still
    /// recover live usage. When no local runtime was found, a managed runtime
    /// gets the first fallback attempt; its own `AuthRequired` remains
    /// terminal and never starts another CLI process.
    async fn resolve_runtime_fallback<F, Fut>(
        &self,
        local_result: Result<Option<ProviderFetchResult>, ProviderError>,
        cli_fallback: F,
    ) -> Result<ProviderFetchResult, ProviderError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Option<ProviderFetchResult>, ProviderError>>,
    {
        self.resolve_runtime_fallback_with_offline(
            local_result,
            cli_fallback,
            Self::offline_usage_result(),
        )
        .await
    }

    async fn resolve_runtime_fallback_with_offline<F, Fut>(
        &self,
        local_result: Result<Option<ProviderFetchResult>, ProviderError>,
        cli_fallback: F,
        offline: Option<ProviderFetchResult>,
    ) -> Result<ProviderFetchResult, ProviderError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Option<ProviderFetchResult>, ProviderError>>,
    {
        let (mut failure, allow_managed_runtime) = match local_result {
            Ok(Some(result)) => return Ok(result),
            Ok(None) => (None, true),
            Err(error) => {
                if !matches!(error, ProviderError::AuthRequired) {
                    tracing::debug!(%error, "Antigravity local probe failed");
                }
                (Some(error), false)
            }
        };

        #[cfg(windows)]
        if allow_managed_runtime {
            match cli_resolution::locate_agy_binary() {
                Ok(binary) => {
                    if cli_fallback::managed_spawn_is_csrf_gated(binary).await {
                        tracing::debug!(
                            "skipping managed agy readiness wait because the local server requires CSRF"
                        );
                    } else {
                        match self.fetch_with_managed_agy().await {
                            Ok(ManagedAgyOutcome::Reused(result)) => return Ok(result),
                            Ok(ManagedAgyOutcome::Fetched(mut result)) => {
                                result.source_label =
                                    AntigravityStrategyId::Cli.as_str().to_string();
                                return Ok(result);
                            }
                            Ok(ManagedAgyOutcome::Missing) => {}
                            Err(error) => {
                                if matches!(error, ProviderError::AuthRequired) {
                                    return Err(error);
                                }
                                tracing::debug!(%error, "managed Antigravity CLI probe failed");
                                failure = Some(error);
                            }
                        }
                    }
                }
                Err(error) => {
                    tracing::debug!(%error, "managed Antigravity CLI resolution failed");
                    failure = Some(error);
                }
            }
        }

        #[cfg(not(windows))]
        let _ = allow_managed_runtime;

        match cli_fallback().await {
            Ok(Some(result)) => return Ok(result),
            Ok(None) => {}
            Err(error) => {
                if !matches!(error, ProviderError::AuthRequired) {
                    tracing::debug!(%error, "structured Antigravity CLI fallback failed");
                }
                // A failed CLI attempt is an inconclusive runtime probe. Let
                // the same offline-history policy handle it rather than
                // returning the earlier local-probe error directly.
                failure = Some(error);
            }
        }

        match failure {
            Some(error) => Self::resolve_probe_failure(error, offline),
            None => {
                offline.ok_or_else(|| ProviderError::NotInstalled(AGY_NOT_FOUND_MESSAGE.into()))
            }
        }
    }

    async fn fetch_local_payload(
        client: &reqwest::Client,
        process_info: &ProcessInfo,
        api_port: u16,
        path: &str,
        body: &serde_json::Value,
        timeout: std::time::Duration,
    ) -> Result<Vec<u8>, ProviderError> {
        let url = format!("https://127.0.0.1:{api_port}{path}");
        let requires_csrf = process_info.source == ProcessSource::Ide;
        let csrf_token = process_info
            .extension_server_csrf_token
            .as_deref()
            .unwrap_or(&process_info.csrf_token);
        let mut request = client
            .post(&url)
            .header("Content-Type", "application/json")
            .header("Connect-Protocol-Version", "1")
            .timeout(timeout)
            .json(body);
        if requires_csrf {
            request = request.header("X-Codeium-Csrf-Token", csrf_token);
        }
        let response = request
            .send()
            .await
            .map_err(|e| ProviderError::Other(format!("API request failed: {e}")))?;
        if response.status().is_success() {
            return response
                .bytes()
                .await
                .map(|bytes| bytes.to_vec())
                .map_err(|e| ProviderError::Other(format!("Failed to read response: {e}")));
        }

        let status = response.status();
        let text = response.text().await.unwrap_or_default();
        if requires_csrf && process_info.extension_server_csrf_token.is_some() {
            let retry = client
                .post(&url)
                .header("Content-Type", "application/json")
                .header("Connect-Protocol-Version", "1")
                .header("X-Codeium-Csrf-Token", &process_info.csrf_token)
                .timeout(timeout)
                .json(body)
                .send()
                .await;
            if let Ok(retry) = retry
                && retry.status().is_success()
            {
                return retry
                    .bytes()
                    .await
                    .map(|bytes| bytes.to_vec())
                    .map_err(|e| ProviderError::Other(format!("Failed to read response: {e}")));
            }
        }

        if process_info.source == ProcessSource::Cli
            && (status == reqwest::StatusCode::UNAUTHORIZED
                || status == reqwest::StatusCode::FORBIDDEN
                || text.to_ascii_lowercase().contains("not logged")
                || text.to_ascii_lowercase().contains("login method")
                || text.to_ascii_lowercase().contains("keyring"))
        {
            return Err(ProviderError::AuthRequired);
        }
        Err(ProviderError::Other(format!("API error {status}: {text}")))
    }

    fn resolve_plan_name(status: &UserStatus) -> Option<String> {
        legacy_status::resolve_plan_name(status)
    }

    fn parse_user_status(
        &self,
        response: UserStatusResponse,
    ) -> Result<UsageSnapshot, ProviderError> {
        legacy_status::parse_user_status(response)
    }
}

impl Default for AntigravityProvider {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Provider for AntigravityProvider {
    fn automatic_metric_prioritizes_exhausted_window(&self) -> bool {
        false
    }

    fn id(&self) -> ProviderId {
        ProviderId::Antigravity
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        // `oauth` is not supported (no remote API path is ported yet); surface it
        // explicitly instead of silently probing locally. Both `auto` and `cli`
        // prefer an existing desktop/CLI language server. When neither is
        // running, start a task-owned `agy` session for this fetch only.
        if ctx.source_mode == SourceMode::OAuth {
            return Err(ProviderError::UnsupportedSource(ctx.source_mode));
        }

        tracing::debug!("Fetching Antigravity usage via local probe");

        let local_result = self.fetch_user_status().await;
        self.resolve_runtime_fallback(local_result, || self.try_print_usage_fallback())
            .await
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Cli]
    }

    fn supports_cli(&self) -> bool {
        true
    }

    /// Antigravity's `NotInstalled` reports the local language-server probe
    /// finding nothing to talk to — a runtime that is not running, not a
    /// credential problem — so it surfaces as an offline runtime.
    fn error_state_kind(&self, error: &ProviderError) -> crate::core::ProviderStateKind {
        match error {
            // Only the not-running marker proves the runtime is down; a failed
            // probe (PowerShell unavailable etc.) is inconclusive, not offline.
            ProviderError::NotInstalled(msg) if msg.contains("not running") => {
                crate::core::ProviderStateKind::LocalRuntimeOffline
            }
            ProviderError::NotInstalled(_) => crate::core::ProviderStateKind::Unknown,
            _ => error.state_kind(),
        }
    }
}

struct ProcessInfo {
    csrf_token: String,
    extension_server_csrf_token: Option<String>,
    extension_port: Option<u16>,
    pid: Option<u32>,
    /// Whether the process is the desktop IDE/app server (CSRF required) or the
    /// `agy` CLI (no CSRF). See [`ProcessSource`].
    source: ProcessSource,
}

#[cfg(windows)]
enum ManagedAgyOutcome {
    /// A user-owned desktop or CLI process appeared after the launch lock.
    Reused(ProviderFetchResult),
    /// Usage came from the short-lived process owned by this fetch.
    Fetched(ProviderFetchResult),
    /// No configured `agy` executable exists, so offline history may be used.
    Missing,
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
