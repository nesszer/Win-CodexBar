// Hide console window on Windows
#[cfg(windows)]
#![windows_subsystem = "windows"]

//! CodexBar - Windows/WSL system tray app for monitoring AI provider usage limits
//!
//! This is a Rust port of the macOS CodexBar application, providing:
//! - System tray icon with usage status (default when double-clicking)
//! - CLI for querying usage from terminal (`codexbar usage`)
//! - Support for multiple AI providers (Claude, Codex, Gemini, etc.)
//! - Native WSL support for Linux environments on Windows

mod browser;
mod cli;
mod core;
mod cost_scanner;
mod host;
mod logging;
mod login;
mod native_ui;
mod notifications;
mod providers;
mod settings;
mod shortcuts;
mod single_instance;
mod sound;
mod status;
mod tray;
mod updater;
mod wsl;

use clap::Parser;
use cli::{exit_codes, Cli, Commands};

/// Redact sensitive CLI arguments (tokens, keys, cookies) from log output
fn redact_sensitive_args(args: &[String]) -> Vec<String> {
    let sensitive_flags = ["--token", "--api-key", "--key", "--cookie", "--password"];
    let mut result = Vec::with_capacity(args.len());
    let mut redact_next = false;
    for arg in args {
        if redact_next {
            result.push("[REDACTED]".to_string());
            redact_next = false;
        } else if sensitive_flags.iter().any(|f| arg.starts_with(f)) {
            if arg.contains('=') {
                let prefix = arg.split('=').next().unwrap_or(arg);
                result.push(format!("{}=[REDACTED]", prefix));
            } else {
                result.push(arg.clone());
                redact_next = true;
            }
        } else {
            result.push(arg.clone());
        }
    }
    result
}

fn main() {
    // Log immediately at program start (redact sensitive args)
    let log_path = std::env::temp_dir().join("codexbar_launch.log");
    let args: Vec<String> = std::env::args().collect();
    let redacted_args = redact_sensitive_args(&args);
    let _ = std::fs::write(
        &log_path,
        format!(
            "main() started at {:?}\nArgs: {:?}\n",
            std::time::SystemTime::now(),
            redacted_args
        ),
    );

    let exit_code = run();

    let _ = std::fs::OpenOptions::new()
        .append(true)
        .open(&log_path)
        .and_then(|mut f| {
            use std::io::Write;
            writeln!(f, "Exiting with code: {}", exit_code)
        });

    std::process::exit(exit_code);
}

fn run() -> i32 {
    // Log to file immediately for debugging
    let log_path = std::env::temp_dir().join("codexbar_launch.log");
    let mut log = String::new();
    log.push_str(&format!("Starting at {:?}\n", std::time::SystemTime::now()));

    // Log WSL environment info
    if wsl::is_wsl() {
        log.push_str("Running inside WSL\n");
        if let Some(info) = wsl::get_wsl_info() {
            log.push_str(&format!("  Distro: {}\n", info.distro_name));
            log.push_str(&format!(
                "  Windows user: {}\n",
                info.windows_username.as_deref().unwrap_or("(unknown)")
            ));
            log.push_str(&format!("  Drive mount: {:?}\n", info.drive_mount));
        }
    }

    let args: Vec<String> = std::env::args().collect();
    log.push_str(&format!("Args: {:?}\n", redact_sensitive_args(&args)));
    let _ = std::fs::write(&log_path, &log);

    let cli = Cli::parse();

    // Initialize logging
    if let Err(e) = logging::init(cli.verbose, cli.json_output) {
        eprintln!("Failed to initialize logging: {}", e);
        return exit_codes::UNEXPECTED_FAILURE;
    }

    // Create tokio runtime for async commands
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("Failed to create runtime: {}", e);
            return exit_codes::UNEXPECTED_FAILURE;
        }
    };

    match cli.command {
        Some(Commands::Usage(args)) => rt.block_on(async {
            match cli::usage::run(args).await {
                Ok(()) => exit_codes::SUCCESS,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    categorize_error(&e)
                }
            }
        }),
        Some(Commands::Cost(args)) => rt.block_on(async {
            match cli::cost::run(args).await {
                Ok(()) => exit_codes::SUCCESS,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    categorize_error(&e)
                }
            }
        }),
        Some(Commands::Menubar) => {
            // Hide the console window for GUI mode (Windows only)
            #[cfg(windows)]
            hide_console_window();

            // Log WSL GUI availability
            if wsl::is_wsl() {
                tracing::info!(
                    "Running in WSL — GUI requires WSLg or an X server. \
                     If the window doesn't appear, use CLI commands instead: \
                     codexbar usage -p <provider>"
                );
            }

            // Check for existing instance
            let _guard = match single_instance::SingleInstanceGuard::try_acquire() {
                Some(guard) => guard,
                None => {
                    // Can't print to console anymore, just exit
                    return exit_codes::SUCCESS;
                }
            };

            match native_ui::run() {
                Ok(()) => exit_codes::SUCCESS,
                Err(_) => exit_codes::UNEXPECTED_FAILURE,
            }
        }
        Some(Commands::Autostart(args)) => rt.block_on(async {
            match cli::autostart::run(args).await {
                Ok(()) => exit_codes::SUCCESS,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    exit_codes::UNEXPECTED_FAILURE
                }
            }
        }),
        Some(Commands::Account(args)) => rt.block_on(async {
            match cli::account::run(args).await {
                Ok(()) => exit_codes::SUCCESS,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    exit_codes::UNEXPECTED_FAILURE
                }
            }
        }),
        Some(Commands::Config(args)) => rt.block_on(async {
            match cli::config::run(args).await {
                Ok(()) => exit_codes::SUCCESS,
                Err(e) => {
                    eprintln!("Error: {}", e);
                    exit_codes::UNEXPECTED_FAILURE
                }
            }
        }),
        None => {
            // Default: launch menubar GUI
            // Log to file since we can't see console output
            let log_path = std::env::temp_dir().join("codexbar_launch.log");
            let _ = std::fs::write(
                &log_path,
                format!("Starting at {:?}\n", std::time::SystemTime::now()),
            );

            let _guard = match single_instance::SingleInstanceGuard::try_acquire() {
                Some(guard) => guard,
                None => {
                    let _ = std::fs::write(&log_path, "Already running, exiting\n");
                    return exit_codes::SUCCESS;
                }
            };

            let _ = std::fs::write(&log_path, "Launching native_ui::run()\n");
            match native_ui::run() {
                Ok(()) => exit_codes::SUCCESS,
                Err(e) => {
                    let _ = std::fs::write(&log_path, format!("native_ui error: {:?}\n", e));
                    exit_codes::UNEXPECTED_FAILURE
                }
            }
        }
    }
}

/// Categorize an error into the appropriate exit code
fn categorize_error(e: &anyhow::Error) -> i32 {
    let msg = e.to_string().to_lowercase();

    if msg.contains("not installed") || msg.contains("not found") || msg.contains("binary") {
        exit_codes::PROVIDER_MISSING
    } else if msg.contains("parse") || msg.contains("format") || msg.contains("invalid") {
        exit_codes::PARSE_ERROR
    } else if msg.contains("timeout") || msg.contains("timed out") {
        exit_codes::CLI_TIMEOUT
    } else {
        exit_codes::UNEXPECTED_FAILURE
    }
}

/// Hide the console window on Windows (for GUI mode)
#[cfg(windows)]
fn hide_console_window() {
    use windows::Win32::System::Console::GetConsoleWindow;
    use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE};

    unsafe {
        let console = GetConsoleWindow();
        if !console.is_invalid() {
            let _ = ShowWindow(console, SW_HIDE);
        }
    }
}
