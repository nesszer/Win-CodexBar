//! Codex source-row evidence: request-row retention, prefix validation, and
//! cached-pricing recovery for complete JSONL files.

use super::*;
use std::io::Read;

/// Build source-row evidence from the aggregate records produced by the
/// native Windows parser.  Rows start with the source model as their
/// pricing evidence; recovery may later replace that evidence with a
/// retained cached value or clear it when the match is ambiguous.
pub(crate) fn rows_from_records(records: &[(CodexUsageRecord, i64)]) -> Vec<CodexSourceUsageRow> {
    records
        .iter()
        .map(|(record, offset)| CodexSourceUsageRow {
            day_key: record.day_key.clone(),
            model: record.model.clone(),
            input: record.input.max(0),
            cached: record.cached.max(0).min(record.input.max(0)),
            output: record.output.max(0),
            reasoning: record.reasoning.map(|value| value.max(0)),
            source_end_offset: *offset,
            pricing: pricing_evidence(&record.model),
        })
        .collect()
}

/// Pricing evidence implied by a parsed model attribution.
pub(crate) fn pricing_evidence(model: &str) -> CodexSourcePricingEvidence {
    CodexSourcePricingEvidence {
        pricing_model: Some(model.to_string()),
        pricing_mode: Some(pricing_mode_of_model(model).to_string()),
    }
}

/// The pricing mode encoded by a model-name suffix (`-priority` or standard).
pub(crate) fn pricing_mode_of_model(model: &str) -> &'static str {
    if model.ends_with("-priority") {
        "priority"
    } else {
        "standard"
    }
}

/// The model name that a pricing mode implies for a base model.
pub(crate) fn model_of_pricing_mode(model: &str) -> String {
    match pricing_mode_of_model(model) {
        "priority" => model.strip_suffix("-priority").unwrap_or(model).to_string(),
        _ => model.to_string(),
    }
}

/// Re-read the bounded reporting partition to obtain request-row order.
/// The normal scanner still owns aggregate parsing and its byte budget;
/// this path is used only after a complete file pass has established that
/// source evidence is safe to retain.
pub(crate) fn read_source_rows(
    file_path: &Path,
    range: &CostUsageDayRange,
) -> std::io::Result<Vec<CodexSourceUsageRow>> {
    let source_range = CostUsageDayRange {
        since_key: range.scan_since_key.clone(),
        until_key: range.scan_until_key.clone(),
        scan_since_key: range.scan_since_key.clone(),
        scan_until_key: range.scan_until_key.clone(),
    };
    let parsed = JsonlScanner::parse_codex_file(file_path, &source_range, 0, None, None)?;
    Ok(rows_from_records(&parsed.records))
}

/// Recover cached per-request pricing only when the source row match is
/// unique or every matching cached row carries the same evidence.  Native
/// JSONL proves the request sequence, but it cannot choose between
/// conflicting prices for repeated identical requests.
pub(crate) fn recover_rows(
    cached: &[CodexSourceUsageRow],
    source: &[CodexSourceUsageRow],
    cached_size: i64,
) -> Vec<CodexSourceUsageRow> {
    // Valid cached rows keyed by source offset, plus the pricing consensus
    // for each request shape (`None` once two cached rows disagree).
    let mut candidates: HashMap<i64, &CodexSourceUsageRow> = HashMap::new();
    let mut consensus: HashMap<CodexSourceRowKey, Option<CodexSourcePricingEvidence>> =
        HashMap::new();
    for row in cached {
        if !offset_is_within_cached_prefix(row.source_end_offset, cached_size) {
            continue;
        }
        candidates.insert(row.source_end_offset, row);
        let key = CodexSourceRowKey::from(row);
        match consensus.get_mut(&key) {
            Some(slot @ Some(_)) if slot.as_ref() != Some(&row.pricing) => {
                *slot = None;
            }
            Some(_) => {}
            None => {
                consensus.insert(key, Some(row.pricing.clone()));
            }
        }
    }

    source
        .iter()
        .map(|row| {
            let mut recovered = row.clone();
            recovered.pricing = match candidates.get(&row.source_end_offset) {
                Some(candidate)
                    if offset_is_within_cached_prefix(row.source_end_offset, cached_size)
                        && CodexSourceRowKey::from(*candidate) == CodexSourceRowKey::from(row) =>
                {
                    consensus
                        .get(&CodexSourceRowKey::from(row))
                        .cloned()
                        .flatten()
                        .unwrap_or_default()
                }
                _ => CodexSourcePricingEvidence::default(),
            };
            recovered
        })
        .collect()
}

/// Construct a cache entry for source rows after a complete read. The cache
/// exists only when file identity and the prefix hash can both be computed,
/// so every retained cache carries a usable freshness receipt.
pub(crate) fn row_cache(
    file_path: &Path,
    metadata: &fs::Metadata,
    rows: Vec<CodexSourceUsageRow>,
) -> Option<CodexSourceRowCache> {
    let file_identity = JsonlScanner::codex_file_identity(file_path, metadata)?;
    Some(CodexSourceRowCache {
        file_identity,
        size: i64::try_from(metadata.len()).ok()?,
        mtime_unix_ms: source_mtime_unix_ms(metadata.modified().ok()),
        prefix_hash: source_prefix_hash(file_path, metadata.len())?,
        rows,
    })
}

/// Verify the retained source prefix before using cached request pricing.
/// A same-size rewrite is rejected when its modification time changes;
/// append-only growth is accepted only when the indexed prefix is intact.
pub(crate) fn row_cache_matches(
    file_path: &Path,
    metadata: &fs::Metadata,
    cached: &CodexSourceRowCache,
) -> bool {
    let Ok(size) = i64::try_from(metadata.len()) else {
        return false;
    };
    if size < cached.size
        || cached.size < 0
        || Some(&cached.file_identity)
            != JsonlScanner::codex_file_identity(file_path, metadata).as_ref()
    {
        return false;
    }
    if cached
        .rows
        .iter()
        .any(|row| !offset_is_within_cached_prefix(row.source_end_offset, cached.size))
    {
        return false;
    }
    if cached.prefix_hash
        != source_prefix_hash(file_path, u64::try_from(cached.size.max(0)).unwrap_or(0))
            .unwrap_or(0)
    {
        return false;
    }
    size > cached.size || cached.mtime_unix_ms == source_mtime_unix_ms(metadata.modified().ok())
}

/// Whether the cached prefix needs source-row pricing recovery.
///
/// A normal append keeps the native parser's model attribution. Recovery
/// is reserved for a cached prefix whose pricing evidence no longer agrees
/// with the current source; only that path leaves appended rows
/// intentionally unattributed until their pricing is validated.
pub(crate) fn row_cache_needs_recovery(
    cached: &CodexSourceRowCache,
    source: &[CodexSourceUsageRow],
) -> bool {
    let source_prefix: Vec<_> = source
        .iter()
        .filter(|row| offset_is_within_cached_prefix(row.source_end_offset, cached.size))
        .collect();
    source_prefix.len() != cached.rows.len()
        || cached
            .rows
            .iter()
            .zip(source_prefix)
            .any(|(cached_row, source_row)| {
                cached_row.source_end_offset != source_row.source_end_offset
                    || CodexSourceRowKey::from(cached_row) != CodexSourceRowKey::from(source_row)
                    || cached_row.pricing != source_row.pricing
            })
}

fn offset_is_within_cached_prefix(offset: i64, cached_size: i64) -> bool {
    offset > 0 && offset <= cached_size
}

fn source_mtime_unix_ms(time: Option<std::time::SystemTime>) -> i64 {
    time.and_then(|value| value.duration_since(std::time::UNIX_EPOCH).ok())
        .and_then(|value| i64::try_from(value.as_millis()).ok())
        .unwrap_or(0)
}

/// Content hash of a byte prefix. `DefaultHasher` is not guaranteed stable
/// across Rust releases, but the hash is only ever compared against a cache
/// written by the same binary, so an upgrade invalidates the retained rows
/// once and the cache self-heals on the next pass.
fn source_prefix_hash(file_path: &Path, size: u64) -> Option<u64> {
    let file = File::open(file_path).ok()?;
    let mut reader = file.take(size);
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut remaining = size;
    while remaining > 0 {
        let read = reader.read(&mut buffer).ok()?;
        if read == 0 {
            return None;
        }
        hasher.write(&buffer[..read]);
        remaining = remaining.saturating_sub(read as u64);
    }
    Some(hasher.finish())
}

/// Request-shape key used to detect identical rows and pricing conflicts.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CodexSourceRowKey {
    day_key: String,
    model: String,
    input: i64,
    cached: i64,
    output: i64,
    reasoning: Option<i64>,
}

impl From<&CodexSourceUsageRow> for CodexSourceRowKey {
    fn from(row: &CodexSourceUsageRow) -> Self {
        Self {
            day_key: row.day_key.clone(),
            model: row.model.clone(),
            input: row.input,
            cached: row.cached,
            output: row.output,
            reasoning: row.reasoning,
        }
    }
}

#[cfg(test)]
#[path = "source_rows_tests.rs"]
mod tests;
