//! Multi-account Codex support for CodexBar.
//!
//! This module is a Rust port of the Windows core of
//! [`ademisler/codexcontrol`](https://github.com/ademisler/codexcontrol) (MIT),
//! which manages multiple Codex accounts through isolated `CODEX_HOME`
//! directories and switches the ambient Codex identity. See `NOTICE`/LICENSE
//! for the upstream MIT attribution.
//!
//! The model is deliberately mirror-shaped: a `CodexAccount` lives in either the
//! ambient home (`~/.codex`) or an app-managed home (`managed-homes/<uuid>`),
//! quota snapshots are fetched per account, and switching swaps the ambient
//! `auth.json` plus the Codex Desktop MSIX session state.

pub mod account_manager;
pub mod api;
pub mod codex_desktop;
pub(crate) mod credentials;
mod extra_usage;
mod fetch_coordination;
pub mod file_locations;
mod labels;
pub mod login_runner;
pub mod models;
pub mod stores;
mod switch_runtime;

// Refreshes may rotate auth.json. Keep account replacement exclusive with
// those reads/writes while allowing different account lanes to fetch together.
pub(crate) static CREDENTIAL_OPERATIONS: tokio::sync::RwLock<()> =
    tokio::sync::RwLock::const_new(());

pub use account_manager::{CodexAccountManager, CodexAccountManagerError, CodexSwitchResult};
pub use api::{CodexAccountApi, CodexApiError};
pub use codex_desktop::{
    CodexDesktopControlError, build_restart_command, build_restart_script,
    encode_powershell_script, restart_codex_desktop,
};
pub use credentials::{AuthBackedIdentity, AuthCredentials, load_identity};
pub use labels::{display_names_by_id, ordinals_by_id};
pub use login_runner::{CodexLoginOutcome, CodexLoginResult, ManagedLoginProcess};
pub use models::{
    AccountUsageSnapshot, CodexAccount, CodexAccountSource, CreditsBalanceSnapshot,
    RemovedAccountIdentity, UsageWindowSnapshot, utc_now,
};
pub use stores::{AccountStore, SnapshotStore};
pub use switch_runtime::CodexAccountRuntime;
