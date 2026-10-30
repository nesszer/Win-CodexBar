//! Codex local Workspaces indexing foundation.
//!
//! Attributes local Codex rollout usage to projects, sessions, models, and days.
//! Codex-only; never writes the Codex catalog database.

mod freshness;
mod indexer;
mod project_names;
mod projectless;
mod session_details;
mod sidecar;
mod thread_names;
mod tool_activity;
mod turn_performance;
mod types;

#[cfg(test)]
mod tests;

pub use indexer::{CodexLocalDataScope, CodexWorkspacesIndex, IndexError, project_identity};
pub use session_details::{
    CodexSessionDetails, CodexSessionRequest, CodexSessionTurn, SESSION_DETAIL_MAX_BYTES,
    SESSION_DETAIL_MAX_REQUESTS, SessionDetailCheckpoint, SessionDetailError,
};
pub use sidecar::{PAYLOAD_FORMAT_VERSION, SCHEMA_VERSION, SidecarError, WorkspaceUsageSidecar};
pub use tool_activity::{
    CodexSessionToolActivity, CodexToolOperation, CodexToolOperationKind,
    CodexToolOperationOutcome, CodexToolOperationTiming,
};
pub use types::{
    CodexLocalProjectUsageSnapshot, CostEstimate, DailyPoint, Progress, ProgressPhase,
    ProjectUsage, SessionUsage, SourceStatus, UsageTotals, short_session_id,
    untitled_session_label,
};

pub const CHATS_PROJECT_ID: &str = "chats";
pub const CHATS_DISPLAY_NAME: &str = "Chats";
