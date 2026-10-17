//! Bounded reader for quota records in the IDE's `idea.log`.
//!
//! The IDE logs every quota refresh but persists the quota XML rarely, so the
//! log can be fresher. Only the last [`TAIL_BYTE_COUNT`] bytes are read, and
//! only the parsed quota and refill fields leave this module.

use std::io::{Read, Seek, SeekFrom};
use std::path::Path;
use std::sync::LazyLock;

use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};
use regex_lite::Regex;
use serde::Deserialize;

use super::quota::{QuotaInfo, RefillInfo, TopUpQuota, parse_timestamp};
use super::shared_read::open_shared_read;

/// Largest window read from the end of `idea.log`.
pub(super) const TAIL_BYTE_COUNT: u64 = 4 * 1024 * 1024;

const QUOTA_MARKER: &str = "QuotaManager2Impl - New quota state is: ";
const REFILL_MARKER: &str = "QuotaManager2Impl - New quota refill state is: ";
/// Newer IDEs stop logging quota state after the AI Assistant to Air migration; Junie relays it as JSON.
const JUNIE_QUOTA_MARKER: &str =
    "AiaActivationTransmitter - Received message from junie: quota_update:";
/// Once AI Assistant is disabled its quota manager keeps logging this error between Junie's updates.
const DISABLED_ASSISTANT_QUOTA: &str = "QuotaManager2Impl - New quota state is: Error(exception=com.intellij.ml.llm.core.AIAssistantForbiddenException";
const TIMESTAMP_FORMAT: &str = "%Y-%m-%d %H:%M:%S,%3f";
const TIMESTAMP_LEN: usize = 23;

/// One unsigned decimal number in a logged quota record.
const NUMBER: &str = r"([0-9]+(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?)";

static QUOTA_RE: LazyLock<Regex> = LazyLock::new(|| {
    let details =
        format!(r"QuotaDetails\(current={NUMBER}, maximum={NUMBER}, available={NUMBER}\)");
    Regex::new(&format!(
        r"^Available\(current={NUMBER}, maximum={NUMBER}, until=([^,)\s]+), tariffQuota={details}(?:, topUpQuota={details})?\)$"
    ))
    .expect("valid JetBrains quota log regex")
});

/// AI Assistant 262.x wraps the same fields in `JbaiQuota(...)`. Identifiers are matched but never captured.
static JBAI_QUOTA_RE: LazyLock<Regex> = LazyLock::new(|| {
    let details = format!(r"Details\(current={NUMBER}, maximum={NUMBER}, available={NUMBER}\)");
    Regex::new(&format!(
        r"^Available\(quota=JbaiQuota\(license=[^,()]*, current={NUMBER}, maximum={NUMBER}, quotaID=JbaiQuotaId\([^()]*\), until=([^,)\s]+), tariffQuota={details}(?:, topUpQuota={details})?\)\)$"
    ))
    .expect("valid JetBrains JbaiQuota log regex")
});

static REFILL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"^Known\(next=([^,)\s]+), tariff=QuotaRefillInfoTariff\(amount={NUMBER}, duration=([^,)\s]+)\)\)$"
    ))
    .expect("valid JetBrains refill log regex")
});

static JBAI_REFILL_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(
        r"^Known\(refill=JbaiQuotaRefill\(tariff=JbaiQuotaTariff\(amount={NUMBER}, period=([^,)\s]+)\), next=([^,)\s]+), last=[^,)\s]+\)\)$"
    ))
    .expect("valid JetBrains JbaiQuotaRefill log regex")
});

/// The newest complete quota record in the log tail.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct LogEntry {
    pub timestamp: DateTime<Utc>,
    pub quota: QuotaInfo,
    pub refill: Option<RefillInfo>,
}

/// Read the newest quota record from the bounded tail of `path`.
///
/// Returns `None` when the file cannot be opened, the tail is unreadable, or
/// the newest quota record is missing, truncated, or unsupported.
pub(super) fn latest_entry(path: &Path) -> Option<LogEntry> {
    let mut file = open_shared_read(path).ok()?;
    let size = file.seek(SeekFrom::End(0)).ok()?;
    let content = read_tail(&mut file, size)?;
    latest_entry_in_content(&content)
}

/// Read up to [`TAIL_BYTE_COUNT`] bytes that end at `end`.
///
/// A window that starts mid-file drops its partial first line. A window whose
/// last byte is not a newline (an unfinished record), or whose remaining text
/// is not valid UTF-8, is rejected as a whole.
pub(super) fn read_tail<R: Read + Seek>(reader: &mut R, end: u64) -> Option<String> {
    let count = end.min(TAIL_BYTE_COUNT);
    let offset = end - count;
    reader.seek(SeekFrom::Start(offset)).ok()?;
    let mut buffer = Vec::with_capacity(usize::try_from(count).ok()?);
    reader.by_ref().take(count).read_to_end(&mut buffer).ok()?;
    if buffer.len() as u64 != count || buffer.last() != Some(&b'\n') {
        return None;
    }
    let complete = if offset > 0 {
        match buffer.iter().position(|&byte| byte == b'\n') {
            Some(newline) => &buffer[newline + 1..],
            None => &[],
        }
    } else {
        &buffer[..]
    };
    std::str::from_utf8(complete).ok().map(str::to_string)
}

/// Newest quota record in `content`, with the newest refill record found above it.
///
/// Scanning runs newest-first. An unsupported newest quota state invalidates
/// the log, so an older valid record is never used in its place.
pub(super) fn latest_entry_in_content(content: &str) -> Option<LogEntry> {
    let mut quota: Option<(DateTime<Utc>, QuotaInfo)> = None;
    let mut refill: Option<RefillInfo> = None;
    let mut found_refill = false;

    for line in content.lines().rev() {
        // A disabled AI Assistant says nothing about the account; other errors still invalidate the log below.
        if line.contains(DISABLED_ASSISTANT_QUOTA) {
            continue;
        }
        let is_legacy_quota = line.contains(QUOTA_MARKER);
        if is_legacy_quota || line.contains(JUNIE_QUOTA_MARKER) {
            let parsed = if is_legacy_quota {
                parse_quota_line(line)
            } else {
                parse_junie_quota_line(line)
            };
            match parsed {
                Some(parsed) => {
                    if quota.is_none() {
                        quota = Some(parsed);
                    }
                }
                None => {
                    quota?;
                    break;
                }
            }
        } else if !found_refill && line.contains(REFILL_MARKER) {
            found_refill = true;
            refill = parse_refill_line(line);
        }
        if quota.is_some() && found_refill {
            break;
        }
    }

    let (timestamp, quota) = quota?;
    Some(LogEntry {
        timestamp,
        quota,
        refill,
    })
}

/// Parse one `Available(...)` quota record. Values must be finite and
/// nonnegative, and the tariff maximum must be positive.
fn parse_quota_line(line: &str) -> Option<(DateTime<Utc>, QuotaInfo)> {
    let (timestamp, state) = record(line, QUOTA_MARKER)?;
    let captures = QUOTA_RE
        .captures(state)
        .or_else(|| JBAI_QUOTA_RE.captures(state))?;
    parse_timestamp(captures.get(3)?.as_str())?;

    // Groups in order: current, maximum, [until skipped], tariff current,
    // tariff maximum, tariff available, then the optional top-up triple.
    let numbers = (1..captures.len())
        .filter(|&index| index != 3)
        .filter_map(|index| captures.get(index))
        .map(|found| found.as_str().parse::<f64>().ok())
        .collect::<Option<Vec<f64>>>()?;
    if !numbers
        .iter()
        .all(|value| value.is_finite() && *value >= 0.0)
        || numbers[3] <= 0.0
    {
        return None;
    }

    let top_up = (numbers.len() == 8)
        .then(|| TopUpQuota::new(Some(numbers[6]), Some(numbers[7])))
        .flatten();
    Some((
        timestamp,
        QuotaInfo {
            used: numbers[2],
            maximum: numbers[3],
            top_up,
        },
    ))
}

/// Parse one `Known(...)` refill record. Only the next refill instant is kept.
fn parse_refill_line(line: &str) -> Option<RefillInfo> {
    let (_, state) = record(line, REFILL_MARKER)?;
    // Normalize both layouts to (next, amount).
    let (next, amount) = match REFILL_RE.captures(state) {
        Some(captures) => (captures.get(1)?.as_str(), captures.get(2)?.as_str()),
        None => {
            let captures = JBAI_REFILL_RE.captures(state)?;
            (captures.get(3)?.as_str(), captures.get(1)?.as_str())
        }
    };
    let next = parse_timestamp(next)?;
    let amount: f64 = amount.parse().ok()?;
    if !(amount.is_finite() && amount >= 0.0) {
        return None;
    }
    Some(RefillInfo { next: Some(next) })
}

/// The `{"amount": ...}` wrapper Junie uses for each quota number.
#[derive(Deserialize)]
struct JunieAmount {
    amount: JunieNumber,
}

/// Junie writes amounts as decimal strings; plain JSON numbers are accepted too.
#[derive(Deserialize)]
#[serde(untagged)]
enum JunieNumber {
    Text(String),
    Number(f64),
}

impl JunieAmount {
    fn value(&self) -> Option<f64> {
        match &self.amount {
            JunieNumber::Text(text) => text.parse().ok(),
            JunieNumber::Number(number) => Some(*number),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JunieDetails {
    current: JunieAmount,
    maximum: JunieAmount,
    available: JunieAmount,
}

/// Only the fields below are decoded. Other payload fields (license, quota IDs) are dropped.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct JunieQuotaUpdate {
    /// Expiry in epoch milliseconds. Validated like the other numbers, but not kept.
    until: f64,
    tariff_quota: JunieDetails,
    top_up_quota: Option<JunieDetails>,
}

/// Parse one Junie `quota_update` JSON record. Same rules as `Available(...)`:
/// finite nonnegative numbers and a positive tariff maximum.
fn parse_junie_quota_line(line: &str) -> Option<(DateTime<Utc>, QuotaInfo)> {
    let (timestamp, state) = record(line, JUNIE_QUOTA_MARKER)?;
    let update: JunieQuotaUpdate = serde_json::from_str(state).ok()?;
    let tariff = &update.tariff_quota;
    let used = tariff.current.value()?;
    let maximum = tariff.maximum.value()?;
    let available = tariff.available.value()?;
    let values = [used, maximum, available, update.until];
    if !values
        .iter()
        .all(|value| value.is_finite() && *value >= 0.0)
        || maximum <= 0.0
    {
        return None;
    }

    // An unparseable top-up amount rejects the record, as upstream's decoder does. An invalid
    // balance only drops the top-up row, through `TopUpQuota::new`.
    let top_up = match &update.top_up_quota {
        Some(details) => {
            details.current.value()?;
            let maximum = details.maximum.value()?;
            let available = details.available.value()?;
            TopUpQuota::new(Some(maximum), Some(available))
        }
        None => None,
    };
    Some((
        timestamp,
        QuotaInfo {
            used,
            maximum,
            top_up,
        },
    ))
}

/// Split a log line into its local-time timestamp and the text after `marker`.
///
/// The leading 23 characters must be exactly `yyyy-MM-dd HH:mm:ss,SSS`, so
/// non-padded or otherwise reformatted timestamps are rejected.
fn record<'a>(line: &'a str, marker: &str) -> Option<(DateTime<Utc>, &'a str)> {
    let start = line.find(marker)?;
    let prefix = line.get(..TIMESTAMP_LEN)?;
    let naive = NaiveDateTime::parse_from_str(prefix, TIMESTAMP_FORMAT).ok()?;
    if naive.format(TIMESTAMP_FORMAT).to_string() != prefix {
        return None;
    }
    let local = Local.from_local_datetime(&naive).earliest()?;
    Some((local.with_timezone(&Utc), &line[start + marker.len()..]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;
    use std::io::Cursor;

    /// Synthetic log lines in the IDE's format (no account data).
    const QUOTA_LINE: &str = "2026-10-05 15:30:55,732 [   8326] INFO - #c.i.m.l.c.q.QuotaManager2Impl - New quota state is: Available(current=346495.294, maximum=6489986.397, until=2028-09-22T21:00:00Z, tariffQuota=QuotaDetails(current=346495.294, maximum=1000000, available=653504.706), topUpQuota=QuotaDetails(current=0, maximum=5489986.397, available=5489986.397))";
    const QUOTA_LINE_NO_TOP_UP: &str = "2026-10-05 15:31:00,001 [   8326] INFO - #c.i.m.l.c.q.QuotaManager2Impl - New quota state is: Available(current=1, maximum=2, until=2028-09-22T21:00:00Z, tariffQuota=QuotaDetails(current=250, maximum=1000, available=750))";
    const REFILL_LINE: &str = "2026-10-05 15:30:56,100 [   8326] INFO - #c.i.m.l.c.q.QuotaManager2Impl - New quota refill state is: Known(next=2026-11-01T00:00:00Z, tariff=QuotaRefillInfoTariff(amount=1000000, duration=PT720H))";

    /// Newer IDE layout: Junie relays the quota as JSON (synthetic license).
    const JUNIE_QUOTA_LINE: &str = r#"2026-10-12 04:47:16,090 [  67837]   INFO - #c.i.m.l.a.m.AiaActivationTransmitter - Received message from junie: quota_update:{"license":"TESTLICENSE","current":{"amount":"16966.18"},"maximum":{"amount":"6489986.397"},"until":1853269200000,"quotaID":{"quotaId":"TESTLICENSE"},"tariffQuota":{"current":{"amount":"16966.18"},"maximum":{"amount":"1000000.0"},"available":{"amount":"983033.82"}},"topUpQuota":{"current":{"amount":"0.0"},"maximum":{"amount":"5489986.397"},"available":{"amount":"5489986.397"}}}"#;
    /// AI Assistant 262.x layout: the same fields wrapped in `JbaiQuota(...)`.
    const JBAI_QUOTA_LINE: &str = "2026-10-12 04:47:42,704 [  28196]   INFO - #c.i.m.l.c.q.QuotaManager2Impl - New quota state is: Available(quota=JbaiQuota(license=TESTLICENSE, current=16966.18, maximum=6489986.397, quotaID=JbaiQuotaId(quotaId=TESTLICENSE, userId=null), until=2028-09-22T21:00:00Z, tariffQuota=Details(current=16966.18, maximum=1000000, available=983033.82), topUpQuota=Details(current=0, maximum=5489986.397, available=5489986.397)))";
    const JBAI_REFILL_LINE: &str = "2026-10-12 04:47:51,747 [  37239]   INFO - #c.i.m.l.c.q.QuotaManager2Impl - New quota refill state is: Known(refill=JbaiQuotaRefill(tariff=JbaiQuotaTariff(amount=1000000, period=30d), next=2026-11-10T17:00:32.124Z, last=2026-10-11T17:00:32.124Z))";
    /// Logged after the AI Assistant to Air migration; the quota manager keeps reporting this.
    const AIR_MIGRATION_QUOTA_LINE: &str = "2026-10-12 04:51:43,486 [ 335233]   INFO - #c.i.m.l.c.q.QuotaManager2Impl - New quota state is: Error(exception=com.intellij.ml.llm.core.AIAssistantForbiddenException: Usage of AI Assistant is prohibited by machine-wide settings (AirMigration))";
    const AIR_MIGRATION_REFILL_LINE: &str = "2026-10-12 04:51:20,481 [ 312228]   INFO - #c.i.m.l.c.q.QuotaManager2Impl - New quota refill state is: Error(exception=com.intellij.ml.llm.core.AIAssistantForbiddenException: Usage of AI Assistant is prohibited by machine-wide settings (AirMigration))";

    /// The local wall-clock stamp `2026-10-05 15:30:55,732` as UTC.
    fn quota_line_timestamp() -> DateTime<Utc> {
        let naive = NaiveDate::from_ymd_opt(2026, 10, 5)
            .and_then(|date| date.and_hms_milli_opt(15, 30, 55, 732))
            .expect("valid fixture time");
        Local
            .from_local_datetime(&naive)
            .earliest()
            .expect("local time exists")
            .with_timezone(&Utc)
    }

    #[test]
    fn reads_quota_top_up_and_refill_from_the_newest_records() {
        let content = format!("{QUOTA_LINE}\n{REFILL_LINE}\n");
        let entry = latest_entry_in_content(&content).expect("entry");

        assert_eq!(entry.timestamp, quota_line_timestamp());
        assert!((entry.quota.used_percent() - 34.6495294).abs() < 1e-6);
        let top_up = entry.quota.top_up.expect("top-up");
        assert!((top_up.available_credits() - 54.89986397).abs() < 1e-6);
        assert_eq!(
            entry.refill.and_then(|refill| refill.next),
            Some(Utc.with_ymd_and_hms(2026, 11, 1, 0, 0, 0).unwrap())
        );
    }

    #[test]
    fn newest_quota_wins_over_older_records() {
        let content = format!("{QUOTA_LINE}\n{QUOTA_LINE_NO_TOP_UP}\n");
        let entry = latest_entry_in_content(&content).expect("entry");
        assert_eq!(entry.quota.used_percent(), 25.0);
        assert!(entry.quota.top_up.is_none());
    }

    #[test]
    fn unsupported_newest_quota_rejects_older_valid_record() {
        let malformed = "2026-10-05 15:32:00,000 [   8326] INFO - #c.i.m.l.c.q.QuotaManager2Impl - New quota state is: Unknown(state=1)";
        let content = format!("{QUOTA_LINE}\n{malformed}\n");
        assert!(latest_entry_in_content(&content).is_none());
    }

    #[test]
    fn invalid_tariff_maximum_or_nonfinite_values_are_rejected() {
        let zero_maximum = QUOTA_LINE_NO_TOP_UP.replace("maximum=1000,", "maximum=0,");
        assert!(parse_quota_line(&zero_maximum).is_none());
        let truncated = QUOTA_LINE.trim_end_matches(')');
        assert!(parse_quota_line(truncated).is_none());
        let bad_until = QUOTA_LINE.replace("until=2028-09-22T21:00:00Z", "until=soon");
        assert!(parse_quota_line(&bad_until).is_none());
    }

    #[test]
    fn missing_refill_leaves_refill_unset_without_dropping_quota() {
        let entry = latest_entry_in_content(QUOTA_LINE).expect("entry");
        assert!(entry.refill.is_none());
    }

    #[test]
    fn newest_refill_is_taken_from_any_position_and_newest_quota_still_wins() {
        let refill_above_quota = format!("{REFILL_LINE}\n{QUOTA_LINE}\n");
        let entry = latest_entry_in_content(&refill_above_quota).expect("entry");
        assert!(entry.refill.is_some());

        let quota_then_refill_then_newer_quota =
            format!("{QUOTA_LINE}\n{REFILL_LINE}\n{QUOTA_LINE_NO_TOP_UP}\n");
        let newest = latest_entry_in_content(&quota_then_refill_then_newer_quota).expect("entry");
        assert_eq!(newest.quota.used_percent(), 25.0);
        assert!(newest.refill.is_some());
    }

    #[test]
    fn timestamp_must_be_exactly_the_log_format() {
        let unpadded = QUOTA_LINE.replacen("2026-10-05", "2026-1-5", 1);
        assert!(parse_quota_line(&unpadded).is_none());
    }

    #[test]
    fn parses_junie_quota_update_after_the_air_migration() {
        let content = format!("{QUOTA_LINE}\n{JUNIE_QUOTA_LINE}\n{AIR_MIGRATION_REFILL_LINE}\n");
        let entry = latest_entry_in_content(&content).expect("entry");

        assert!((entry.quota.used - 16966.18).abs() < 1e-9);
        assert!((entry.quota.maximum - 1_000_000.0).abs() < 1e-9);
        assert!((entry.quota.used_percent() - 1.69661800).abs() < 1e-6);
        let top_up = entry.quota.top_up.expect("top-up");
        assert!((top_up.available_credits() - 54.89986397).abs() < 1e-6);
        assert!(entry.refill.is_none());
        assert!(!format!("{entry:?}").contains("TESTLICENSE"));
    }

    #[test]
    fn newest_quota_wins_across_legacy_and_junie_formats() {
        // Position decides, as for legacy records: the later legacy line is newest.
        let content = format!("{JUNIE_QUOTA_LINE}\n{QUOTA_LINE}\n");
        let entry = latest_entry_in_content(&content).expect("entry");
        assert!((entry.quota.used - 346_495.294).abs() < 1e-9);
    }

    #[test]
    fn invalid_latest_junie_quota_does_not_resurrect_older_quota() {
        let prefix = "2026-10-12 04:47:16,090 [  67837]   INFO - #c.i.m.l.a.m.AiaActivationTransmitter - Received message from junie: quota_update:";
        let payloads = [
            r#"{"tariffQuota":{}}"#,
            "not json",
            r#"{"until":1,"tariffQuota":{"current":{"amount":"1"},"maximum":{"amount":"0"},"available":{"amount":"0"}}}"#,
            r#"{"until":1,"tariffQuota":{"current":{"amount":"nope"},"maximum":{"amount":"1"},"available":{"amount":"0"}}}"#,
        ];
        for payload in payloads {
            let content = format!("{QUOTA_LINE}\n{prefix}{payload}\n");
            assert!(
                latest_entry_in_content(&content).is_none(),
                "payload must invalidate the log: {payload}"
            );
        }
    }

    #[test]
    fn parses_jbai_quota_state_and_refill_from_newer_ai_assistant_logs() {
        let content = format!("{QUOTA_LINE}\n{JBAI_QUOTA_LINE}\n{JBAI_REFILL_LINE}\n");
        let entry = latest_entry_in_content(&content).expect("entry");

        let naive = NaiveDate::from_ymd_opt(2026, 10, 12)
            .and_then(|date| date.and_hms_milli_opt(4, 47, 42, 704))
            .expect("valid fixture time");
        let local = Local
            .from_local_datetime(&naive)
            .earliest()
            .expect("local time exists");
        assert_eq!(entry.timestamp, local.with_timezone(&Utc));
        assert!((entry.quota.used - 16966.18).abs() < 1e-9);
        assert!((entry.quota.maximum - 1_000_000.0).abs() < 1e-9);
        let top_up = entry.quota.top_up.expect("top-up");
        assert!((top_up.available_credits() - 54.89986397).abs() < 1e-6);
        assert_eq!(
            entry.refill.and_then(|refill| refill.next),
            parse_timestamp("2026-11-10T17:00:32.124Z")
        );
        assert!(!format!("{entry:?}").contains("TESTLICENSE"));
    }

    #[test]
    fn malformed_latest_jbai_quota_does_not_resurrect_older_quota() {
        let malformed = JBAI_QUOTA_LINE.replacen("tariffQuota=Details(", "tariffQuota=Other(", 1);
        let content = format!("{QUOTA_LINE}\n{malformed}\n");
        assert!(latest_entry_in_content(&content).is_none());
    }

    #[test]
    fn disabled_ai_assistant_errors_do_not_hide_newer_junie_quota() {
        let content = format!(
            "{QUOTA_LINE}\n{JUNIE_QUOTA_LINE}\n{AIR_MIGRATION_QUOTA_LINE}\n{AIR_MIGRATION_REFILL_LINE}\n{AIR_MIGRATION_QUOTA_LINE}\n"
        );
        let entry = latest_entry_in_content(&content).expect("entry");
        assert!((entry.quota.used - 16966.18).abs() < 1e-9);
        assert!(entry.refill.is_none());
    }

    #[test]
    fn other_latest_quota_errors_still_invalidate_older_quota() {
        let network_error = AIR_MIGRATION_QUOTA_LINE.replacen(
            "com.intellij.ml.llm.core.AIAssistantForbiddenException",
            "java.net.SocketTimeoutException",
            1,
        );
        let content = format!("{JUNIE_QUOTA_LINE}\n{network_error}\n");
        assert!(latest_entry_in_content(&content).is_none());
    }

    #[test]
    fn read_tail_returns_small_files_whole() {
        let mut file = Cursor::new(b"one\ntwo\n".to_vec());
        assert_eq!(read_tail(&mut file, 8).as_deref(), Some("one\ntwo\n"));
    }

    #[test]
    fn read_tail_rejects_an_unfinished_final_line() {
        let mut file = Cursor::new(b"one\ntwo".to_vec());
        assert!(read_tail(&mut file, 7).is_none());
    }

    #[test]
    fn read_tail_is_bounded_and_starts_on_a_complete_line() {
        // Rows of 17 bytes, about twice the window size. 17 does not divide the
        // window start, so the window begins mid-row and the partial line is dropped.
        let window = usize::try_from(TAIL_BYTE_COUNT).expect("window fits in usize");
        let rows = (window / 17) * 2;
        let text: String = (0..rows)
            .map(|index| format!("row {index:012}\n"))
            .collect();
        let end = text.len() as u64;
        let mut file = Cursor::new(text.clone().into_bytes());

        let tail = read_tail(&mut file, end).expect("tail");
        assert!(tail.len() <= window);
        assert!(tail.len() > window - 17);
        assert!(tail.starts_with("row "));
        assert!(text.ends_with(&tail));
        let last_row = format!("row {:012}", rows - 1);
        assert_eq!(tail.lines().last(), Some(last_row.as_str()));
    }

    #[test]
    fn read_tail_rejects_a_window_with_invalid_utf8_after_the_partial_line() {
        // Window starts inside the 'a' run; the first complete line is "bad\xff".
        let mut bytes = vec![b'a'; usize::try_from(TAIL_BYTE_COUNT).expect("window fits in usize")];
        bytes.push(b'\n');
        bytes.extend_from_slice(b"bad\xff\n");
        let end = bytes.len() as u64;
        let mut file = Cursor::new(bytes);
        assert!(read_tail(&mut file, end).is_none());
    }

    #[test]
    fn log_entry_reads_from_a_shared_open_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("idea.log");
        std::fs::write(&path, format!("{QUOTA_LINE}\n{REFILL_LINE}\n")).expect("write");
        let entry = latest_entry(&path).expect("entry");
        assert!(entry.quota.top_up.is_some());
    }

    #[cfg(windows)]
    #[test]
    fn log_entry_reads_while_the_ide_keeps_it_open_for_writing() {
        use std::fs::OpenOptions;
        use std::io::Write;
        use std::os::windows::fs::OpenOptionsExt;

        const WRITER_SHARE_READ_WRITE: u32 = 0x0000_0001 | 0x0000_0002;
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("idea.log");
        std::fs::write(&path, format!("{QUOTA_LINE}\n{REFILL_LINE}\n")).expect("write");

        // Simulates the IDE: it holds the log open for writing and shares read/write.
        let mut writer = OpenOptions::new()
            .append(true)
            .share_mode(WRITER_SHARE_READ_WRITE)
            .open(&path)
            .expect("writer");
        writer.write_all(b"\n").expect("append while open");

        let entry = latest_entry(&path).expect("reader succeeds while writer is open");
        assert!(entry.quota.top_up.is_some());
    }
}
