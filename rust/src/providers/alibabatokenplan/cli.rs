use std::collections::HashMap;
use std::time::Duration;

use serde_json::Value;

use super::personal::{WindowStrictness, personal_usage_snapshot};
use super::{AlibabaTokenPlanRegion, PERSONAL_USAGE_API, TokenPlanSnapshot, expand_json_strings};
use crate::core::ProviderError;
use crate::host::{CommandError, CommandOptions, CommandRunner};

const CLI_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_OUTPUT_BYTES: usize = 64 * 1024;
const CHILD_ENV_ALLOWLIST: &[&str] = &[
    "PATH",
    "PATHEXT",
    "HOME",
    "USERPROFILE",
    "HOMEDRIVE",
    "HOMEPATH",
    "APPDATA",
    "LOCALAPPDATA",
    "TEMP",
    "TMP",
    "SystemRoot",
    "SYSTEMROOT",
    "ComSpec",
    "COMSPEC",
    "LANG",
    "LC_ALL",
    "LC_CTYPE",
    "TZ",
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "ALL_PROXY",
    "NO_PROXY",
    "http_proxy",
    "https_proxy",
    "all_proxy",
    "no_proxy",
];

pub(super) async fn fetch_cli_usage(
    region: AlibabaTokenPlanRegion,
) -> Result<TokenPlanSnapshot, ProviderError> {
    let environment = sanitized_child_environment(std::env::vars());
    let runner = environment.into_iter().fold(
        CommandRunner::new().without_inherited_env(),
        |runner, (key, value)| runner.with_env(key, value),
    );
    let options = CommandOptions {
        timeout: CLI_TIMEOUT,
        initial_delay: Duration::ZERO,
        extra_args: cli_arguments(region),
        ..CommandOptions::default()
    };

    let result = runner
        .run_async("bl", None, &options)
        .await
        .map_err(map_command_error)?;
    if result.timed_out {
        return Err(ProviderError::Timeout);
    }
    if result.exit_code != Some(0) {
        return Err(ProviderError::Other(
            "Bailian CLI could not load Token Plan usage. Sign in with 'bl' and try again."
                .to_string(),
        ));
    }
    if result.text.len() > MAX_OUTPUT_BYTES {
        return Err(ProviderError::Parse(
            "Bailian CLI Token Plan usage response was too large".to_string(),
        ));
    }
    parse_cli_usage(&result.text)
}

fn map_command_error(error: CommandError) -> ProviderError {
    match error {
        CommandError::BinaryNotFound(_) => ProviderError::NotInstalled(
            "Bailian CLI 'bl' is not installed or not on PATH.".to_string(),
        ),
        CommandError::TimedOut => ProviderError::Timeout,
        CommandError::LaunchFailed(_) | CommandError::IoError(_) => ProviderError::Other(
            "Bailian CLI could not load Token Plan usage. Sign in with 'bl' and try again."
                .to_string(),
        ),
    }
}

pub(super) fn cli_arguments(region: AlibabaTokenPlanRegion) -> Vec<String> {
    // Personal/Solo reads the raw usage endpoint: `bl usage token-plan` omits the
    // monthly window. Team keeps the dedicated subcommand.
    let command: &[&str] = if region.uses_personal_api() {
        &[
            "console",
            "call",
            "--api",
            PERSONAL_USAGE_API,
            "--data",
            "{}",
        ]
    } else {
        &["usage", "token-plan"]
    };
    command
        .iter()
        .copied()
        .chain([
            "--console-region",
            region.current_region_id(),
            "--console-site",
            region.cli_console_site(),
            "--output",
            "json",
        ])
        .map(str::to_string)
        .collect()
}

pub(super) fn sanitized_child_environment(
    environment: impl IntoIterator<Item = (String, String)>,
) -> HashMap<String, String> {
    environment
        .into_iter()
        .filter(|(key, _)| {
            CHILD_ENV_ALLOWLIST
                .iter()
                .any(|allowed| key.eq_ignore_ascii_case(allowed))
        })
        .collect()
}

pub(super) fn parse_cli_usage(text: &str) -> Result<TokenPlanSnapshot, ProviderError> {
    let unsupported = || {
        ProviderError::Parse("Bailian CLI returned an unsupported Token Plan usage response".into())
    };
    let value: Value = serde_json::from_str(text).map_err(|_| unsupported())?;
    if !value.is_object() {
        return Err(unsupported());
    }
    // `bl console call` wraps the usage object in the gateway envelope
    // (`data.DataV2.data.data`); `bl usage token-plan` prints it flat.
    personal_usage_snapshot(
        &expand_json_strings(value),
        None,
        None,
        "Token Plan",
        WindowStrictness::Cli,
    )
    .ok_or_else(unsupported)
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};

    use super::*;

    #[test]
    fn parses_both_cli_windows_and_millisecond_resets() {
        let parsed = parse_cli_usage(
            r#"{"per5HourPercentage":0.25,"per5HourResetTime":1787000400000,"per1WeekPercentage":0.7,"per1WeekResetTime":1787001180000}"#,
        )
        .unwrap();
        assert_eq!(parsed.five_hour_used_percent, Some(25.0));
        assert_eq!(
            parsed.five_hour_resets_at,
            Utc.timestamp_millis_opt(1_787_000_400_000).single()
        );
        assert_eq!(parsed.weekly_used_percent, Some(70.0));
        assert_eq!(
            parsed.weekly_resets_at,
            Utc.timestamp_millis_opt(1_787_001_180_000).single()
        );
    }

    #[test]
    fn accepts_either_valid_window_and_rejects_no_valid_window() {
        let weekly =
            parse_cli_usage(r#"{"per5HourPercentage":"bad","per1WeekPercentage":0.7}"#).unwrap();
        assert_eq!(weekly.five_hour_used_percent, None);
        assert_eq!(weekly.weekly_used_percent, Some(70.0));

        assert!(
            parse_cli_usage(
                r#"{"per5HourPercentage":true,"per1WeekPercentage":-0.1,"percentage":0.5}"#
            )
            .is_err()
        );
    }

    #[test]
    fn regional_cli_arguments_match_bailian_contract() {
        use AlibabaTokenPlanRegion::*;
        const TEAM: &[&str] = &["usage", "token-plan"];
        const PERSONAL: &[&str] = &[
            "console",
            "call",
            "--api",
            "zeldaHttp.apikeyMgr./tokenplan/personal/api/v2/usage",
            "--data",
            "{}",
        ];
        for (region, head, console_region, console_site) in [
            (Cn, TEAM, "cn-beijing", "domestic"),
            (CnPersonal, PERSONAL, "cn-beijing", "domestic"),
            (IntlPersonal, PERSONAL, "ap-southeast-1", "international"),
            (Intl, TEAM, "ap-southeast-1", "international"),
        ] {
            let mut expected = head.to_vec();
            expected.extend([
                "--console-region",
                console_region,
                "--console-site",
                console_site,
                "--output",
                "json",
            ]);
            assert_eq!(cli_arguments(region), expected, "{region:?}");
        }
    }

    #[test]
    fn child_environment_drops_unrelated_secrets() {
        let sanitized = sanitized_child_environment([
            ("PATH".to_string(), "fixture".to_string()),
            ("USERPROFILE".to_string(), "C:\\Users\\fixture".to_string()),
            ("HTTPS_PROXY".to_string(), "http://proxy".to_string()),
            ("AWS_SECRET_ACCESS_KEY".to_string(), "secret".to_string()),
            (
                "ALIBABA_TOKEN_PLAN_COOKIE".to_string(),
                "cookie".to_string(),
            ),
            ("SSH_AUTH_SOCK".to_string(), "socket".to_string()),
        ]);
        assert_eq!(sanitized.get("PATH").map(String::as_str), Some("fixture"));
        assert!(sanitized.contains_key("USERPROFILE"));
        assert!(sanitized.contains_key("HTTPS_PROXY"));
        assert!(!sanitized.contains_key("AWS_SECRET_ACCESS_KEY"));
        assert!(!sanitized.contains_key("ALIBABA_TOKEN_PLAN_COOKIE"));
        assert!(!sanitized.contains_key("SSH_AUTH_SOCK"));
    }
}
