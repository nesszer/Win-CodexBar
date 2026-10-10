use super::*;
use crate::codex_costs::codex_period_start;
use crate::core::test_fixtures::test_file_usage;
use crate::core::{CodexSessionLineage, CostUsagePricing};
use chrono::{Duration, FixedOffset, Local, NaiveTime, TimeZone};
use claude_pricing::FALLBACK_CLAUDE_MODEL;
use daily_history::mark_claude_daily_token_coverage;
use std::io::Write;
use support::{
    app_scanner, cached_file, cached_input_total, cached_usage_with_packed,
    claude_usage_record_from_event, codex_scan_dirs, for_each_claude_usage_record, partition_dir,
    recent_codex_fixture_time, recent_fixture_time_at, write_codex_fork_session_fixture,
    write_codex_session_fixture, write_codex_session_fixture_with_inputs,
};

#[path = "tests/claude_swap.rs"]
mod claude_swap;
#[cfg(test)]
#[path = "tests/claude_today.rs"]
mod claude_today;
#[cfg(test)]
#[path = "tests/claude_walk.rs"]
mod claude_walk;
#[cfg(test)]
#[path = "tests/copied_prefix.rs"]
mod copied_prefix;
#[cfg(test)]
#[path = "tests/direct_fork.rs"]
mod direct_fork;
#[cfg(test)]
#[path = "tests/fork_resume.rs"]
mod fork_resume;
#[cfg(test)]
#[path = "tests/lineage_cache.rs"]
mod lineage_cache;
#[cfg(test)]
#[path = "tests/paginated.rs"]
mod paginated;
#[cfg(test)]
#[path = "tests/support.rs"]
mod support;

#[path = "tests/period.rs"]
mod period;

#[cfg(test)]
#[path = "tests/archived.rs"]
mod archived;

#[path = "tests/claude_rates.rs"]
mod claude_rates;

#[path = "tests/codex_usage.rs"]
mod codex_usage;

#[path = "tests/claude_records.rs"]
mod claude_records;

#[path = "tests/claude_daily.rs"]
mod claude_daily;

#[path = "tests/codex_fork.rs"]
mod codex_fork;

#[path = "tests/codex_cache.rs"]
mod codex_cache;

#[path = "tests/codex_bounded.rs"]
mod codex_bounded;

#[path = "tests/codex_pending.rs"]
mod codex_pending;

#[path = "tests/totals.rs"]
mod totals;
