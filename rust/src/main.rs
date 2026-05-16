use clap::Parser;
use codexbar::{
    cli::{self, Cli, Commands, exit_codes},
    logging, wsl,
};
use std::path::{Path, PathBuf};

fn launch_log_path() -> PathBuf {
    std::env::temp_dir().join(format!("codexbar_launch_{}.log", std::process::id()))
}

fn append_launch_log(log_path: &Path, message: &str) {
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
        .and_then(|mut f| {
            use std::io::Write;
            f.write_all(message.as_bytes())
        });
}

fn launch_arg_summary() -> String {
    let arg_count = std::env::args().count().saturating_sub(1);
    format!("{} CLI argument value(s) omitted", arg_count)
}

fn main() {
    let log_path = launch_log_path();
    append_launch_log(
        &log_path,
        &format!(
            "main() started at {:?}\nArgs: {:?}\n",
            std::time::SystemTime::now(),
            launch_arg_summary()
        ),
    );

    let exit_code = run(&log_path);

    append_launch_log(&log_path, &format!("Exiting with code: {}\n", exit_code));

    std::process::exit(exit_code);
}

fn run(log_path: &Path) -> i32 {
    let mut log = String::new();
    log.push_str(&format!("Starting at {:?}\n", std::time::SystemTime::now()));

    if wsl::is_wsl() {
        log.push_str("Running inside WSL\n");
        if let Some(info) = wsl::get_wsl_info() {
            log.push_str(&format!("  Distro: {}\n", info.distro_name));
            log.push_str(&format!("  Drive mount: {:?}\n", info.drive_mount));
        }
    }

    log.push_str(&format!("Args: {:?}\n", launch_arg_summary()));
    append_launch_log(log_path, &log);

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
            // The egui menubar shell has been retired; the desktop UI lives in
            // apps/desktop-tauri. The CLI binary now requires an explicit subcommand.
            eprintln!(
                "codexbar is now CLI-only. Run a subcommand (e.g. `codexbar usage -p claude`) \
                 or launch the Tauri desktop shell via `apps/desktop-tauri`.\n\
                 Use `codexbar --help` for the full list of subcommands."
            );
            exit_codes::USAGE_ERROR
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_log_path_is_process_scoped() {
        let path = launch_log_path();
        let file_name = path.file_name().and_then(|name| name.to_str()).unwrap();

        assert!(file_name.starts_with("codexbar_launch_"));
        assert!(file_name.ends_with(".log"));
        assert!(file_name.contains(&std::process::id().to_string()));
    }
}
