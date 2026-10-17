//! Codex local Workspaces indexing foundation.
//!
//! Attributes local Codex rollout usage to projects, sessions, models, and days.
//! Codex-only; never writes the Codex catalog database.

mod freshness;
mod indexer;
mod project_names;
mod projectless;
mod sidecar;
mod thread_names;
mod turn_performance;
mod types;

#[cfg(test)]
mod tests;

pub use indexer::{CodexLocalDataScope, CodexWorkspacesIndex, IndexError, project_identity};
pub use sidecar::{PAYLOAD_FORMAT_VERSION, SCHEMA_VERSION, SidecarError, WorkspaceUsageSidecar};
pub use types::{
    CodexLocalProjectUsageSnapshot, CostEstimate, DailyPoint, Progress, ProgressPhase,
    ProjectUsage, SessionUsage, SourceStatus, UsageTotals, short_session_id,
    untitled_session_label,
};

pub const CHATS_PROJECT_ID: &str = "chats";
pub const CHATS_DISPLAY_NAME: &str = "Chats";
