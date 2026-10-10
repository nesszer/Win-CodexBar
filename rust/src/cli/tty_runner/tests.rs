
use super::*;

#[test]
fn script_accepted_matches_markers_case_insensitively_through_ansi() {
    let opts = TtyCommandOptions::new().with_script_retries(
        vec![1.0],
        vec!["Current session".to_string()],
        vec!["/usage".to_string()],
    );
    assert!(script_accepted(
        "\x1b[1mCURRENT\x1b[0m session 12% used",
        &opts
    ));
    assert!(!script_accepted("Try \"fix lint errors\"", &opts));
    assert!(!script_accepted(
        "Current session",
        &TtyCommandOptions::new()
    ));
    assert!(script_echoed("❯ /usa\x1b[0mge", &opts));
    assert!(!script_echoed("❯ Try \"fix lint errors\"", &opts));
    assert!(script_echoed("❯ /usage", &opts));
}

/// The trigger text is printed at startup, inside the initial delay, and
/// nothing is printed afterwards: the key must still be sent.
#[cfg(windows)]
#[test]
fn trigger_printed_inside_initial_delay_is_still_sent() {
    let opts = TtyCommandOptions::new()
        .with_timeout(10.0)
        .with_idle_timeout(3.0)
        .with_initial_delay(1.5)
        .with_send_on_substring("Microsoft Windows", "echo LATE_%OS%\nexit\n");
    let result = TtyCommandRunner::new()
        .run("cmd", "", opts)
        .expect("pty command should run");
    assert!(result.text.contains("LATE_Windows_NT"), "{}", result.text);
}

/// Same startup timing for the screen responder: the banner is drawn inside
/// the initial delay and nothing is printed afterwards.
#[cfg(windows)]
#[test]
fn screen_responder_fires_on_output_buffered_during_initial_delay() {
    use crate::cli::tty_responder::ScreenReading;

    fn on_banner(screen: &str) -> ScreenReading {
        if screen.contains("Microsoft Windows") {
            ScreenReading::Answer(vec!["echo LATE_%OS%\r", "exit\r"])
        } else {
            ScreenReading::Absent
        }
    }
    let opts = TtyCommandOptions::new()
        .with_timeout(10.0)
        .with_idle_timeout(3.0)
        .with_initial_delay(1.5)
        .with_screen_responder(ScreenResponder {
            read: on_banner,
            after_dialog: &[],
        });
    let result = TtyCommandRunner::new()
        .run("cmd", "", opts)
        .expect("pty command should run");
    assert!(result.text.contains("LATE_Windows_NT"), "{}", result.text);
}

#[test]
fn substring_triggers_fire_once_on_already_buffered_output() {
    let options = TtyCommandOptions::new().with_send_on_substring("Enter", "go\n");
    let mut triggered = std::collections::HashSet::new();
    let mut sent = Vec::new();
    fire_substring_triggers(&options, "Enter to confirm", &mut triggered, &mut sent);
    fire_substring_triggers(&options, "Enter to confirm", &mut triggered, &mut sent);
    assert_eq!(sent, b"go\r\n");
}

#[test]
fn test_tty_options_builder() {
    let opts = TtyCommandOptions::new()
        .with_timeout(30.0)
        .with_idle_timeout(5.0)
        .with_stop_on_url(true)
        .with_stop_on_substring("error");

    assert_eq!(opts.timeout_secs, 30.0);
    assert_eq!(opts.idle_timeout_secs, Some(5.0));
    assert!(opts.stop_on_url);
    assert!(opts.stop_on_substrings.contains(&"error".to_string()));
}

#[test]
fn test_tty_result_first_url() {
    let result = TtyCommandResult {
        text: "Visit https://example.com for more info".to_string(),
        stopped_early: false,
        detected_urls: vec!["https://example.com".to_string()],
    };

    assert_eq!(result.first_url(), Some("https://example.com"));
}

#[test]
fn test_run_sends_script_through_pty() {
    let runner = TtyCommandRunner::new();
    let opts = TtyCommandOptions::new()
        .with_timeout(15.0)
        .with_idle_timeout(6.0)
        .with_initial_delay(1.0)
        .with_script_line_delay(0.1);

    #[cfg(windows)]
    let result = runner.run("cmd", "echo CODEXBAR_PTY_OK\nexit", opts);
    #[cfg(not(windows))]
    let result = runner.run("sh", "echo CODEXBAR_PTY_OK\nexit", opts);

    let result = result.expect("pty command should run");
    assert!(result.text.contains("CODEXBAR_PTY_OK"), "{}", result.text);
}

/// A retry is input like the first attempt: a child that stays silent
/// (here `ping` with its output discarded, which never reads or echoes
/// the typed text) must not hit the idle timeout before the last retry.
#[cfg(windows)]
#[test]
fn script_retry_restarts_the_idle_window() {
    let runner = TtyCommandRunner::new();
    let opts = TtyCommandOptions::new()
        .with_timeout(8.0)
        .with_idle_timeout(1.5)
        .with_initial_delay(0.3)
        .with_extra_args(
            ["/d", "/c", "ping -n 10 127.0.0.1 >nul"]
                .map(String::from)
                .to_vec(),
        )
        .with_script_retries(
            vec![0.8, 1.4],
            vec!["marker that never appears".to_string()],
            Vec::new(),
        );

    let started = Instant::now();
    let result = runner.run("cmd", "x", opts);
    let elapsed = started.elapsed();

    let result = result.expect("silent pty command should still finish");
    assert!(result.stopped_early, "the idle timeout should end the run");
    // Without the restart the window closes 1.5 s after the first
    // attempt (about 1.8 s); with it, no earlier than 1.4 s + 1.5 s.
    assert!(
        elapsed >= Duration::from_secs_f64(2.8),
        "idle timeout fired {elapsed:?} after launch, inside the last retry's idle window"
    );
}

/// Ending the session ends every process the PTY child started (as with a
/// `claude.cmd` launcher and its `node` child), not only the child. The
/// grandchild runs on its own hidden console, so closing the pseudoconsole
/// does not end it; only the session's job does. It holds an exclusive
/// handle on a file, so its exit is observed through that handle instead
/// of a PID that could be reused.
#[cfg(windows)]
#[test]
fn session_end_terminates_processes_started_by_the_child() {
    use base64::Engine as _;
    use std::os::windows::fs::OpenOptionsExt as _;

    fn encoded(command: &str) -> String {
        let utf16: Vec<u8> = command
            .encode_utf16()
            .flat_map(|unit| unit.to_le_bytes())
            .collect();
        base64::engine::general_purpose::STANDARD.encode(utf16)
    }
    fn quoted(path: &std::path::Path) -> String {
        format!("'{}'", path.display().to_string().replace('\'', "''"))
    }

    let dir = tempfile::tempdir().expect("temp dir");
    let held = dir.path().join("held-by-grandchild.lock");
    let ready = dir.path().join("grandchild-ready");
    let grandchild = format!(
        "$f = [IO.File]::Open({held}, 'OpenOrCreate', 'ReadWrite', 'None'); \
             [IO.File]::WriteAllText({ready}, 'ready'); Start-Sleep -Seconds 30",
        held = quoted(&held),
        ready = quoted(&ready),
    );
    let child = format!(
        "$psi = New-Object System.Diagnostics.ProcessStartInfo -ArgumentList \
             'powershell.exe', '-NoLogo -NoProfile -NonInteractive -EncodedCommand {command}'; \
             $psi.UseShellExecute = $false; $psi.CreateNoWindow = $true; \
             [void][System.Diagnostics.Process]::Start($psi); \
             while (-not (Test-Path -LiteralPath {ready})) {{ Start-Sleep -Milliseconds 50 }}; \
             Write-Output 'grandchild-holds-lock'; Start-Sleep -Seconds 30",
        command = encoded(&grandchild),
        ready = quoted(&ready),
    );
    let opts = TtyCommandOptions::new()
        .with_timeout(30.0)
        .with_initial_delay(0.2)
        .with_stop_on_substring("grandchild-holds-lock")
        .with_extra_args(
            [
                "-NoLogo",
                "-NoProfile",
                "-NonInteractive",
                "-EncodedCommand",
                &encoded(&child),
            ]
            .map(String::from)
            .to_vec(),
        );

    let result = TtyCommandRunner::new()
        .run("powershell", "", opts)
        .expect("pty command should run");
    assert!(
        result.text.contains("grandchild-holds-lock"),
        "grandchild never started: {}",
        result.text
    );

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let exclusive = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .share_mode(0)
            .open(&held);
        if exclusive.is_ok() {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the grandchild still holds its file after the session ended"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn test_enriched_path() {
    let path = TtyCommandRunner::enriched_path();
    assert!(!path.is_empty());
    // Should contain path separator or at least a non-empty path string.
    assert!(path.contains(';') || !path.is_empty());
}
