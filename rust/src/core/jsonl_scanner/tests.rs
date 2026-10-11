use super::*;
use crate::core::test_fixtures::test_file_usage;
use chrono::TimeZone;
use std::io::Write;

impl CodexParserState {
    fn new(initial_model: Option<String>, initial_totals: Option<CodexTotals>) -> Self {
        Self::from_mode(CodexParseMode::Standard {
            start_offset: 0,
            initial_model,
            initial_totals,
            previous_token_timestamp: None,
            token_timestamps_monotonic: None,
        })
    }

    fn process_line(&mut self, line: &str, range: &CostUsageDayRange) {
        self.process_line_with_source_offset(line, range, 0);
    }
}

#[cfg(test)]
#[path = "tests/codex_metadata.rs"]
mod codex_metadata;

#[cfg(test)]
#[path = "tests/save_skip.rs"]
mod save_skip;

#[path = "tests/cache_zone.rs"]
mod cache_zone;

#[path = "tests/timestamps.rs"]
mod timestamps;

#[path = "tests/token_accounting.rs"]
mod token_accounting;

#[path = "tests/line_parsing.rs"]
mod line_parsing;

#[path = "tests/usage_rows.rs"]
mod usage_rows;

#[path = "tests/cache_files.rs"]
mod cache_files;
