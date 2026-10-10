//! Run-key command shape and post-update repair.

use super::*;

#[test]
fn test_start_at_login_command_uses_only_the_executable_path() {
    let path = std::path::PathBuf::from(r"C:\Program Files\CodexBar\codexbar-desktop-tauri.exe");
    let command = Settings::start_at_login_command(&path);
    assert_eq!(
        command,
        "\"C:\\Program Files\\CodexBar\\codexbar-desktop-tauri.exe\""
    );
    assert!(!command.contains("menubar"));
}

#[test]
fn test_start_at_login_prefers_desktop_sibling_when_called_from_cli() {
    let temp = tempfile::tempdir().expect("temp dir");
    let cli_path = temp.path().join("codexbar-cli.exe");
    let desktop_path = temp.path().join("codexbar.exe");
    std::fs::write(&cli_path, b"cli").expect("write cli");
    std::fs::write(&desktop_path, b"desktop").expect("write desktop");

    let command = Settings::start_at_login_command(&cli_path);

    assert_eq!(command, format!("\"{}\"", desktop_path.display()));
}

#[test]
fn test_start_at_login_keeps_current_exe_when_desktop_sibling_missing() {
    let temp = tempfile::tempdir().expect("temp dir");
    let cli_path = temp.path().join("codexbar-cli.exe");
    std::fs::write(&cli_path, b"cli").expect("write cli");

    let command = Settings::start_at_login_command(&cli_path);

    assert_eq!(command, format!("\"{}\"", cli_path.display()));
}

#[test]
fn test_start_at_login_repairs_stale_cli_command_after_update() {
    let temp = tempfile::tempdir().expect("temp dir");
    let cli_path = temp.path().join("codexbar-cli.exe");
    let desktop_path = temp.path().join("codexbar.exe");
    std::fs::write(&cli_path, b"cli").expect("write cli");
    std::fs::write(&desktop_path, b"desktop").expect("write desktop");
    let stale_command = format!("\"{}\"", cli_path.display());

    assert!(Settings::start_at_login_command_needs_repair(
        &stale_command,
        &desktop_path
    ));
}

#[test]
fn test_start_at_login_keeps_current_desktop_command_after_update() {
    let temp = tempfile::tempdir().expect("temp dir");
    let desktop_path = temp.path().join("codexbar.exe");
    std::fs::write(&desktop_path, b"desktop").expect("write desktop");
    let current_command = format!("\"{}\"", desktop_path.display());

    assert!(!Settings::start_at_login_command_needs_repair(
        &current_command,
        &desktop_path
    ));
}

#[test]
fn test_start_at_login_repairs_legacy_desktop_command_after_update() {
    let temp = tempfile::tempdir().expect("temp dir");
    let desktop_path = temp.path().join("codexbar.exe");
    let legacy_desktop_path = temp.path().join("codexbar-desktop.exe");
    std::fs::write(&desktop_path, b"desktop").expect("write desktop");
    std::fs::write(&legacy_desktop_path, b"legacy desktop").expect("write legacy desktop");
    let stale_command = format!("\"{}\"", legacy_desktop_path.display());

    assert!(Settings::start_at_login_command_needs_repair(
        &stale_command,
        &legacy_desktop_path
    ));
}
