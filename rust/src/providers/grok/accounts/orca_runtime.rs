use crate::core::ProviderError;
use serde_json::Value;
use std::future::Future;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

#[derive(Clone, Copy)]
enum Outcome {
    Complete,
    RuntimeUnavailable,
    Rejected,
}

pub(super) async fn run(binary: &Path, args: &[&str]) -> Result<(), ProviderError> {
    let operation = run_with(
        args,
        |args| invoke(binary, args),
        Duration::from_millis(500),
    );
    tokio::time::timeout(Duration::from_secs(25), operation)
        .await
        .map_err(|_| {
            ProviderError::Other("Orca did not respond. Open Orca and try again.".into())
        })?
}

async fn invoke(binary: &Path, args: Vec<String>) -> Result<Outcome, ProviderError> {
    let mut command = tokio::process::Command::new(binary);
    command
        .args(&args)
        .arg("--json")
        .env("ORCA_BACKGROUND_LAUNCH", "1")
        .kill_on_drop(true);
    #[cfg(windows)]
    command.creation_flags(0x0800_0000);
    if args.as_slice() == ["open"] {
        // The GUI inherits open's handles; pipes would wait for the GUI to exit.
        let status = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .map_err(|_| {
                ProviderError::Other("Could not start Orca. Open Orca and try again.".into())
            })?;
        return Ok(if status.success() {
            Outcome::Complete
        } else {
            Outcome::Rejected
        });
    }
    let output = command.output().await.map_err(|_| {
        ProviderError::Other("Could not reach Orca. Open Orca and try again.".into())
    })?;
    let reply: Value = serde_json::from_slice(&output.stdout)
        .map_err(|_| ProviderError::Other("Orca returned an unreadable account result.".into()))?;
    Ok(classify_reply(output.status.success(), &reply))
}

fn classify_reply(success: bool, reply: &Value) -> Outcome {
    if success && reply.get("ok").and_then(Value::as_bool) == Some(true) {
        Outcome::Complete
    } else if reply.get("ok").and_then(Value::as_bool) == Some(false)
        && reply.pointer("/error/code").and_then(Value::as_str) == Some("runtime_unavailable")
    {
        Outcome::RuntimeUnavailable
    } else {
        Outcome::Rejected
    }
}

async fn run_with<F, R>(args: &[&str], mut invoke: F, delay: Duration) -> Result<(), ProviderError>
where
    F: FnMut(Vec<String>) -> R,
    R: Future<Output = Result<Outcome, ProviderError>>,
{
    let arguments = || args.iter().map(|arg| (*arg).to_owned()).collect();
    match invoke(arguments()).await? {
        Outcome::Complete => return Ok(()),
        Outcome::Rejected => return Err(rejected()),
        Outcome::RuntimeUnavailable => {}
    }
    // Account commands do not start Orca; only its explicit open command does.
    if !matches!(invoke(vec!["open".into()]).await?, Outcome::Complete) {
        return Err(rejected());
    }
    for attempt in 0..10 {
        match invoke(arguments()).await? {
            Outcome::Complete => return Ok(()),
            Outcome::Rejected => return Err(rejected()),
            Outcome::RuntimeUnavailable => {}
        }
        if attempt < 9 {
            tokio::time::sleep(delay).await;
        }
    }
    Err(rejected())
}

fn rejected() -> ProviderError {
    ProviderError::Other(
        "Orca could not complete the account change. Check the account in Orca.".into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    #[cfg(windows)]
    #[tokio::test]
    async fn startup_finishes_while_its_background_child_is_still_running() {
        let directory = tempfile::tempdir().unwrap();
        let binary = directory.path().join("startup.cmd");
        std::fs::write(
            &binary,
            "@echo off\r\nstart \"\" /b cmd.exe /d /c \"ping -n 6 127.0.0.1 >nul\"\r\necho {\"ok\":true}\r\nexit /b 0\r\n",
        )
        .unwrap();
        let result =
            tokio::time::timeout(Duration::from_secs(3), invoke(&binary, vec!["open".into()]))
                .await
                .expect("Startup waited for its background child")
                .unwrap();
        assert!(matches!(result, Outcome::Complete));
    }

    async fn replay(outcomes: &[Outcome]) -> (bool, Vec<Vec<String>>) {
        let mut replies: VecDeque<_> = outcomes.iter().copied().collect();
        let mut calls = Vec::new();
        let result = run_with(
            &["account", "usage", "--agent", "grok"],
            |args| {
                calls.push(args);
                std::future::ready(Ok(replies.pop_front().expect("Unexpected command")))
            },
            Duration::ZERO,
        )
        .await;
        (result.is_ok(), calls)
    }

    #[tokio::test]
    async fn closed_runtime_is_opened_once_then_the_original_command_is_retried() {
        let (ok, calls) = replay(&[
            Outcome::RuntimeUnavailable,
            Outcome::Complete,
            Outcome::RuntimeUnavailable,
            Outcome::Complete,
        ])
        .await;
        assert!(ok);
        assert_eq!(calls.len(), 4);
        assert_eq!(calls[1], ["open"]);
        assert_eq!(calls[0], calls[2]);
        assert_eq!(calls[0], calls[3]);
    }

    #[tokio::test]
    async fn running_runtime_and_rejected_account_changes_are_never_reopened() {
        for outcome in [Outcome::Complete, Outcome::Rejected] {
            let (ok, calls) = replay(&[outcome]).await;
            assert_eq!(ok, matches!(outcome, Outcome::Complete));
            assert_eq!(calls.len(), 1);
        }
        let (ok, calls) = replay(&[Outcome::RuntimeUnavailable, Outcome::Rejected]).await;
        assert!(!ok);
        assert_eq!(calls.len(), 2);
    }

    #[tokio::test]
    async fn unavailable_runtime_has_bounded_retries() {
        let mut outcomes = vec![Outcome::RuntimeUnavailable, Outcome::Complete];
        outcomes.extend([Outcome::RuntimeUnavailable; 10]);
        let (ok, calls) = replay(&outcomes).await;
        assert!(!ok);
        assert_eq!(calls.len(), 12);
        assert_eq!(
            calls
                .iter()
                .filter(|args| args.as_slice() == ["open"])
                .count(),
            1
        );
    }

    #[test]
    fn only_an_explicit_runtime_unavailable_error_permits_recovery() {
        for reply in [
            serde_json::json!({"ok":false,"error":{"code":"permission_denied"}}),
            serde_json::json!({"ok":false,"error":{"code":"invalid_argument"}}),
            serde_json::json!({"ok":false,"error":{"message":"runtime_unavailable"}}),
            serde_json::json!({"ok":true,"error":{"code":"runtime_unavailable"}}),
        ] {
            assert!(matches!(classify_reply(false, &reply), Outcome::Rejected));
        }
        assert!(matches!(
            classify_reply(
                false,
                &serde_json::json!({"ok":false,"error":{"code":"runtime_unavailable"}})
            ),
            Outcome::RuntimeUnavailable
        ));
    }
}
