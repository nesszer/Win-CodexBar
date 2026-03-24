//! Browser detection and cookie extraction for Windows and WSL

pub mod cookie_cache;
pub mod cookies;
pub mod detection;
pub mod watchdog;
pub mod wsl_paths;

// Re-exports for future UI integration
#[allow(unused_imports)]
pub use watchdog::{global_watchdog, WatchdogConfig, WatchdogError, WebProbeWatchdog};
