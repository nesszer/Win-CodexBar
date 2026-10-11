//! Claude transcript JSONL rows and the Vertex AI metadata classifier.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::Value;

use super::claude_usage::session_id_from_entries;

/// JSONL event structures for Claude transcripts.
///
/// The flattened values retain otherwise-unknown metadata long enough to
/// distinguish Anthropic rows from Vertex AI rows. Claude's local transcript
/// format can contain both shapes, and counting Vertex rows with Anthropic
/// pricing would misstate both cost and token history.
#[derive(Debug, Deserialize)]
pub(super) struct ClaudeEvent {
    #[serde(rename = "type")]
    pub(super) event_type: Option<String>,
    pub(super) timestamp: Option<String>,
    #[serde(rename = "requestId", alias = "request_id")]
    pub(super) request_id: Option<String>,
    #[serde(rename = "sessionId", alias = "session_id")]
    pub(super) session_id: Option<String>,
    pub(super) message: Option<ClaudeMessage>,
    #[serde(flatten)]
    pub(super) extra: HashMap<String, Value>,
}

impl ClaudeEvent {
    pub(super) fn parsed_timestamp(&self) -> Option<DateTime<Utc>> {
        let timestamp = self.timestamp.as_deref()?;
        DateTime::parse_from_rfc3339(timestamp)
            .ok()
            .map(|ts| ts.with_timezone(&Utc))
    }

    pub(super) fn session_id(&self) -> Option<&str> {
        self.session_id
            .as_deref()
            .map(str::trim)
            .filter(|session_id| !session_id.is_empty())
            .or_else(|| session_id_from_entries(self.extra.iter()))
            .or_else(|| {
                self.message
                    .as_ref()
                    .and_then(|message| session_id_from_entries(message.extra.iter()))
            })
    }

    pub(super) fn is_vertex_ai_usage_entry(&self) -> bool {
        // Vertex AI message/request identifiers use the `_vrtx_` marker.
        if self
            .message
            .as_ref()
            .and_then(|message| message.id.as_deref())
            .is_some_and(|id| id.contains("_vrtx_"))
            || self
                .request_id
                .as_deref()
                .is_some_and(|request_id| request_id.contains("_vrtx_"))
        {
            return true;
        }

        // Vertex AI model names use `@` as the version separator.
        if self
            .message
            .as_ref()
            .and_then(|message| message.model.as_deref())
            .is_some_and(model_name_looks_vertex)
        {
            return true;
        }

        if contains_claude_vertex_metadata_entries(self.extra.iter()) {
            return true;
        }
        self.message
            .as_ref()
            .is_some_and(ClaudeMessage::contains_vertex_metadata)
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct ClaudeMessage {
    pub(super) id: Option<String>,
    pub(super) model: Option<String>,
    pub(super) usage: Option<ClaudeUsage>,
    #[serde(flatten)]
    pub(super) extra: HashMap<String, Value>,
}

impl ClaudeMessage {
    pub(super) fn contains_vertex_metadata(&self) -> bool {
        if contains_claude_vertex_metadata_entries(self.extra.iter()) {
            return true;
        }
        self.usage
            .as_ref()
            .is_some_and(ClaudeUsage::contains_vertex_metadata)
    }
}

/// Claude Code proxies can emit a cache-unaware `message_start` estimate
/// before the final assistant response. It has a null stop reason, input
/// tokens, no output, and no cache breakdown; pricing that row would count a
/// preliminary estimate alongside the eventual final usage.
pub(super) fn is_preliminary_claude_usage(event: &ClaudeEvent) -> bool {
    if event.event_type.as_deref() != Some("assistant") {
        return false;
    }
    let Some(message) = event.message.as_ref() else {
        return false;
    };
    let Some(usage) = message.usage.as_ref() else {
        return false;
    };

    message.extra.get("stop_reason").is_some_and(Value::is_null)
        && usage.input_tokens.unwrap_or(0) > 0
        && usage.output_tokens.unwrap_or(0) == 0
        && usage.cache_read_input_tokens.is_none()
        && usage.cache_creation_input_tokens.is_none()
}

#[derive(Debug, Deserialize)]
pub(super) struct ClaudeUsage {
    pub(super) input_tokens: Option<u64>,
    pub(super) output_tokens: Option<u64>,
    pub(super) cache_creation_input_tokens: Option<u64>,
    pub(super) cache_read_input_tokens: Option<u64>,
    pub(super) cache_creation: Option<ClaudeCacheCreation>,
    #[serde(flatten)]
    pub(super) extra: HashMap<String, Value>,
}

impl ClaudeUsage {
    pub(super) fn contains_vertex_metadata(&self) -> bool {
        if contains_claude_vertex_metadata_entries(self.extra.iter()) {
            return true;
        }
        self.cache_creation
            .as_ref()
            .is_some_and(ClaudeCacheCreation::contains_vertex_metadata)
    }
}

impl ClaudeUsage {
    /// One-hour cache-write tokens, clamped to the total cache-write count.
    pub(super) fn one_hour_cache_creation_tokens(&self, total: u64) -> u64 {
        self.cache_creation
            .as_ref()
            .and_then(|cache_creation| cache_creation.ephemeral_1h_input_tokens)
            .unwrap_or(0)
            .min(total)
    }
}

/// TTL breakdown of cache writes reported by the API.
#[derive(Debug, Deserialize)]
pub(super) struct ClaudeCacheCreation {
    pub(super) ephemeral_1h_input_tokens: Option<u64>,
    #[serde(flatten)]
    pub(super) extra: HashMap<String, Value>,
}

impl ClaudeCacheCreation {
    pub(super) fn contains_vertex_metadata(&self) -> bool {
        contains_claude_vertex_metadata_entries(self.extra.iter())
    }
}

pub(super) const CLAUDE_VERTEX_PROVIDER_KEYS: &[&str] = &[
    "provider",
    "platform",
    "backend",
    "api_provider",
    "apiprovider",
    "api_type",
    "apitype",
    "source",
    "vendor",
    "client",
];

pub(super) fn model_name_looks_vertex(model: &str) -> bool {
    model.starts_with("claude-") && model.contains('@')
}

/// Match the upstream Claude classifier's recursive metadata rules. Marker
/// keys (`vertex`/`gcp`) classify regardless of value; provider-key values
/// classify only when their text contains `vertex` (not merely `gcp`).
pub(super) fn contains_claude_vertex_metadata(value: &Value) -> bool {
    match value {
        Value::Object(object) => contains_claude_vertex_metadata_entries(object.iter()),
        Value::Array(array) => array.iter().any(contains_claude_vertex_metadata),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => false,
    }
}

pub(super) fn contains_claude_vertex_metadata_entries<'a, I>(entries: I) -> bool
where
    I: IntoIterator<Item = (&'a String, &'a Value)>,
{
    entries.into_iter().any(|(key, value)| {
        contains_claude_vertex_marker(key, true)
            || (CLAUDE_VERTEX_PROVIDER_KEYS
                .iter()
                .any(|candidate| key.eq_ignore_ascii_case(candidate))
                && value
                    .as_str()
                    .is_some_and(|text| contains_claude_vertex_marker(text, false)))
            || contains_claude_vertex_metadata(value)
    })
}

pub(super) fn contains_claude_vertex_marker(value: &str, include_gcp: bool) -> bool {
    let bytes = value.as_bytes();
    let has_marker = |marker: &[u8]| {
        bytes.windows(marker.len()).any(|window| {
            window
                .iter()
                .zip(marker)
                .all(|(byte, expected)| byte.to_ascii_lowercase() == *expected)
        })
    };

    if has_marker(b"vertex") || (include_gcp && has_marker(b"gcp")) {
        return true;
    }

    // ASCII folding above is enough for the common path. Unicode lowercasing
    // preserves the historical classifier's behavior for non-ASCII strings.
    if value.is_ascii() {
        return false;
    }
    let lower = value.to_lowercase();
    lower.contains("vertex") || (include_gcp && lower.contains("gcp"))
}
