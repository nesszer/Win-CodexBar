//! JetBrains AI provider
//!
//! Reads the monthly quota and purchased top-up credits from the AI Assistant
//! quota XML in the IDE configuration. A quota record in the IDE's `idea.log`
//! replaces the XML only when it is newer. Everything is local; there is no
//! web source.

mod ide;
mod quota;
mod quota_log;
mod quota_xml;
mod shared_read;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::core::{
    FetchContext, Provider, ProviderDisplayDetail, ProviderError, ProviderFetchResult, ProviderId,
    ProviderMetadata, RateWindow, SourceMode, UsageSnapshot,
};

use ide::{IdeInstall, VendorRoot};
use quota::{QuotaInfo, RefillInfo};
use quota_log::LogEntry;
use shared_read::open_shared_read;

/// Largest quota XML read into memory. Real files are far smaller.
const MAX_QUOTA_XML_BYTES: u64 = 16 * 1024 * 1024;
const NOT_INSTALLED_MESSAGE: &str =
    "JetBrains AI Assistant not found. Install from JetBrains IDE Marketplace.";
const TOP_UP_DETAIL_ID: &str = "jetbrains-top-up-remaining";
const TOP_UP_SECTION: &str = "Top-up credits";
const TOP_UP_ROW_TITLE: &str = "Remaining";

/// Quota state plus the IDE installation it belongs to, when known.
#[derive(Debug, Clone, PartialEq)]
struct Snapshot {
    quota: QuotaInfo,
    refill: Option<RefillInfo>,
    ide: Option<IdeInstall>,
}

/// JetBrains AI provider
pub struct JetBrainsProvider {
    metadata: ProviderMetadata,
}

impl JetBrainsProvider {
    pub fn new() -> Self {
        Self {
            metadata: ProviderMetadata {
                id: ProviderId::JetBrains,
                display_name: "JetBrains AI",
                session_label: "Credits",
                weekly_label: "Monthly",
                supports_opus: false,
                supports_credits: true,
                default_enabled: false,
                is_primary: false,
                dashboard_url: Some("https://www.jetbrains.com/ai/"),
                status_page_url: None,
                tertiary_label_key: None,
            },
        }
    }
}

impl Default for JetBrainsProvider {
    fn default() -> Self {
        Self::new()
    }
}

/// Custom IDE configuration path from settings, trimmed. Empty means auto-detect.
fn custom_base_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

/// Read the quota for the configured source. Performs blocking file IO.
///
/// With `custom_base` set, only that configuration directory is used. Otherwise
/// the IDE whose quota XML was written most recently is used. With no quota XML
/// anywhere, the latest valid log record is used.
fn read_snapshot(
    custom_base: Option<&str>,
    roots: &[VendorRoot],
) -> Result<Snapshot, ProviderError> {
    let (base_path, quota_file, detected) = match custom_base {
        Some(path) => {
            let base = PathBuf::from(path);
            let quota_file = ide::quota_file_for(&base);
            (base, quota_file, None)
        }
        None => match ide::latest_install(&ide::detect_installs(roots, false)) {
            Some(install) => {
                let quota_file = install.quota_file();
                (install.base_path.clone(), quota_file, Some(install))
            }
            None => return log_only_snapshot(roots),
        },
    };

    let log_entry =
        ide::log_path_for(&base_path, roots).and_then(|path| quota_log::latest_entry(&path));
    match read_quota_xml(&quota_file) {
        Ok((quota, refill)) => {
            let xml_modified = ide::modified_at(&quota_file);
            let xml = Snapshot {
                quota,
                refill,
                ide: detected,
            };
            Ok(apply_log_entry(xml, log_entry, xml_modified))
        }
        Err(error) => match log_entry {
            Some(entry) => Ok(Snapshot {
                quota: entry.quota,
                refill: entry.refill,
                ide: detected,
            }),
            None => Err(error),
        },
    }
}

/// Use the log's quota and refill together when the record is strictly newer
/// than the XML. An unknown XML time keeps the XML, and a log refill that is
/// absent leaves the reset date unset rather than borrowing it from the XML.
fn apply_log_entry(
    xml: Snapshot,
    entry: Option<LogEntry>,
    xml_modified: Option<DateTime<Utc>>,
) -> Snapshot {
    match (entry, xml_modified) {
        (Some(entry), Some(modified)) if entry.timestamp > modified => Snapshot {
            quota: entry.quota,
            refill: entry.refill,
            ide: xml.ide,
        },
        _ => xml,
    }
}

/// No IDE has quota XML: use the newest valid log record and its own IDE identity.
fn log_only_snapshot(roots: &[VendorRoot]) -> Result<Snapshot, ProviderError> {
    ide::detect_installs(roots, true)
        .into_iter()
        .filter_map(|install| {
            let entry = ide::log_path_for(&install.base_path, roots)
                .and_then(|path| quota_log::latest_entry(&path))?;
            Some((install, entry))
        })
        .max_by_key(|(_, entry)| entry.timestamp)
        .map(|(install, entry)| Snapshot {
            quota: entry.quota,
            refill: entry.refill,
            ide: Some(install),
        })
        .ok_or_else(not_installed)
}

fn not_installed() -> ProviderError {
    ProviderError::NotInstalled(NOT_INSTALLED_MESSAGE.to_string())
}

fn read_quota_xml(path: &Path) -> Result<(QuotaInfo, Option<RefillInfo>), ProviderError> {
    if !path.is_file() {
        return Err(ProviderError::NotInstalled(format!(
            "JetBrains AI quota file not found at {}. Enable AI Assistant in your IDE.",
            path.display()
        )));
    }
    let file = open_shared_read(path)
        .map_err(|error| ProviderError::Parse(format!("Failed to read file: {error}")))?;
    let mut bytes = Vec::new();
    file.take(MAX_QUOTA_XML_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| ProviderError::Parse(format!("Failed to read file: {error}")))?;
    if bytes.len() as u64 > MAX_QUOTA_XML_BYTES {
        return Err(ProviderError::Parse(
            "JetBrains AI quota file is too large to read".to_string(),
        ));
    }
    quota_xml::parse_quota_xml(&bytes)
}

/// Monthly percentage with the refill date as its reset, and the IDE name as
/// the account-style organization line.
fn usage_snapshot(snapshot: &Snapshot) -> UsageSnapshot {
    let resets_at = snapshot.refill.as_ref().and_then(|refill| refill.next);
    let primary = RateWindow::with_details(snapshot.quota.used_percent(), None, resets_at, None);
    let usage = UsageSnapshot::new(primary).with_login_method("JetBrains AI");
    match &snapshot.ide {
        Some(install) => usage.with_organization(install.display_name()),
        None => usage,
    }
}

/// Purchased top-up credits as a balance row (no usage bar). Shown only when
/// the balance is valid, and an exhausted balance still shows `0.00 credits`.
fn top_up_details(quota: &QuotaInfo) -> Vec<ProviderDisplayDetail> {
    quota
        .top_up
        .iter()
        .filter_map(|top_up| {
            ProviderDisplayDetail::new(
                TOP_UP_DETAIL_ID,
                TOP_UP_ROW_TITLE,
                format!("{:.2} credits", top_up.available_credits()),
            )?
            .with_section_title(TOP_UP_SECTION)
        })
        .collect()
}

fn fetch_result(snapshot: &Snapshot) -> ProviderFetchResult {
    ProviderFetchResult::new(usage_snapshot(snapshot), "local")
        .with_display_details(top_up_details(&snapshot.quota))
}

#[async_trait]
impl Provider for JetBrainsProvider {
    fn id(&self) -> ProviderId {
        ProviderId::JetBrains
    }

    fn metadata(&self) -> &ProviderMetadata {
        &self.metadata
    }

    async fn fetch_usage(&self, ctx: &FetchContext) -> Result<ProviderFetchResult, ProviderError> {
        tracing::debug!("Fetching JetBrains AI usage");

        match ctx.source_mode {
            SourceMode::Auto | SourceMode::Cli => {
                let snapshot = tokio::task::spawn_blocking(|| {
                    let custom = crate::settings::Settings::load()
                        .jetbrains_ide_base_path()
                        .to_string();
                    read_snapshot(
                        custom_base_path(&custom).as_deref(),
                        &ide::default_vendor_roots(),
                    )
                })
                .await
                .map_err(|error| {
                    ProviderError::Other(format!("JetBrains AI read task failed: {error}"))
                })??;
                Ok(fetch_result(&snapshot))
            }
            SourceMode::Web | SourceMode::OAuth => {
                // JetBrains AI doesn't have web API access
                Err(ProviderError::UnsupportedSource(ctx.source_mode))
            }
        }
    }

    fn available_sources(&self) -> Vec<SourceMode> {
        vec![SourceMode::Auto, SourceMode::Cli]
    }

    fn supports_web(&self) -> bool {
        false
    }

    fn supports_cli(&self) -> bool {
        true
    }
    /// JetBrains' local IDE probe raises `NotInstalled` when the AI
    /// Assistant plugin is not found in any IDE configuration — an
    /// installation gap, not a credential problem — so it surfaces as an
    /// offline local runtime (matching the pre-backend classifier's
    /// treatment of plugin-presence failures). This is the provider's only
    /// `NotInstalled` producer, so the variant maps wholesale.
    fn error_state_kind(&self, error: &ProviderError) -> crate::core::ProviderStateKind {
        match error {
            ProviderError::NotInstalled(_) => crate::core::ProviderStateKind::LocalRuntimeOffline,
            _ => error.state_kind(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, SystemTime};

    /// Synthetic quota and refill JSON (no account data).
    const QUOTA_JSON: &str = r#"{"type":"Available","current":"68433.145","maximum":"6489986.397","tariffQuota":{"current":"68433.145","maximum":"1000000","available":"931566.855"},"topUpQuota":{"current":"0","maximum":"5489986.397","available":"5489986.397"}}"#;
    const REFILL_JSON: &str = r#"{"type":"Known","next":"2026-11-01T00:00:00Z","tariff":{"amount":"1000000","duration":"PT720H"}}"#;
    /// Synthetic idea.log records. Timestamps are local 2026-10-05 15:30-15:31.
    const LOG_QUOTA: &str = "2026-10-05 15:30:55,732 [   8326] INFO - #c.i.m.l.c.q.QuotaManager2Impl - New quota state is: Available(current=346495.294, maximum=6489986.397, until=2028-09-22T21:00:00Z, tariffQuota=QuotaDetails(current=346495.294, maximum=1000000, available=653504.706), topUpQuota=QuotaDetails(current=0, maximum=5489986.397, available=5489986.397))";
    const LOG_QUOTA_LATER_NO_TOP_UP: &str = "2026-10-05 15:31:00,001 [   8326] INFO - #c.i.m.l.c.q.QuotaManager2Impl - New quota state is: Available(current=1, maximum=2, until=2028-09-22T21:00:00Z, tariffQuota=QuotaDetails(current=250, maximum=1000, available=750))";
    const LOG_REFILL: &str = "2026-10-05 15:30:56,100 [   8326] INFO - #c.i.m.l.c.q.QuotaManager2Impl - New quota refill state is: Known(next=2026-11-01T00:00:00Z, tariff=QuotaRefillInfoTariff(amount=1000000, duration=PT720H))";
    /// Newer Junie relay record (synthetic license); the IDE stops logging `New quota state` after the Air migration.
    const LOG_JUNIE_QUOTA: &str = r#"2026-10-12 04:47:16,090 [  67837]   INFO - #c.i.m.l.a.m.AiaActivationTransmitter - Received message from junie: quota_update:{"license":"TESTLICENSE","current":{"amount":"16966.18"},"maximum":{"amount":"6489986.397"},"until":1853269200000,"quotaID":{"quotaId":"TESTLICENSE"},"tariffQuota":{"current":{"amount":"16966.18"},"maximum":{"amount":"1000000.0"},"available":{"amount":"983033.82"}},"topUpQuota":{"current":{"amount":"0.0"},"maximum":{"amount":"5489986.397"},"available":{"amount":"5489986.397"}}}"#;

    const XML_MODIFIED_2020: u64 = 1_577_836_800;
    const XML_MODIFIED_2030: u64 = 1_893_456_000;

    fn encode(raw: &str) -> String {
        raw.replace('&', "&amp;")
            .replace('"', "&quot;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    }

    fn quota_xml(quota_json: &str, refill_json: Option<&str>) -> String {
        let refill = refill_json
            .map(|raw| {
                format!(
                    "\n  <option name=\"nextRefill\" value=\"{}\" />",
                    encode(raw)
                )
            })
            .unwrap_or_default();
        format!(
            "<application>\n<component name=\"AIAssistantQuotaManager2\">\n  \
             <option name=\"quotaInfo\" value=\"{}\" />{refill}\n</component>\n</application>\n",
            encode(quota_json)
        )
    }

    fn at(seconds: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(seconds)
    }

    /// Write an IDE quota XML under `config/<ide>/options` and set its mtime.
    fn write_ide_xml(config: &Path, ide: &str, content: &str, modified: u64) -> PathBuf {
        let base = config.join(ide);
        let file = ide::quota_file_for(&base);
        fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
        fs::write(&file, content).expect("write xml");
        fs::OpenOptions::new()
            .write(true)
            .open(&file)
            .expect("open xml")
            .set_modified(at(modified))
            .expect("set mtime");
        base
    }

    /// Write an IDE `idea.log` under `logs/<ide>/log`.
    fn write_ide_log(logs: &Path, ide: &str, lines: &[&str]) {
        let file = logs.join(ide).join("log").join("idea.log");
        fs::create_dir_all(file.parent().expect("parent")).expect("mkdir");
        fs::write(file, lines.join("\n") + "\n").expect("write log");
    }

    struct Roots {
        _dir: tempfile::TempDir,
        config: PathBuf,
        logs: PathBuf,
        roots: Vec<VendorRoot>,
    }

    fn roots() -> Roots {
        let dir = tempfile::tempdir().expect("tempdir");
        let config = dir.path().join("Roaming").join("JetBrains");
        let logs = dir.path().join("Local").join("JetBrains");
        fs::create_dir_all(&config).expect("config root");
        fs::create_dir_all(&logs).expect("logs root");
        let roots = vec![VendorRoot {
            config: config.clone(),
            logs: logs.clone(),
        }];
        Roots {
            _dir: dir,
            config,
            logs,
            roots,
        }
    }

    fn top_up_value(snapshot: &Snapshot) -> Option<String> {
        top_up_details(&snapshot.quota)
            .first()
            .map(|row| row.value().to_string())
    }

    #[test]
    fn plugin_presence_maps_to_local_runtime_offline() {
        assert_eq!(
            JetBrainsProvider::new().error_state_kind(&ProviderError::NotInstalled(
                "JetBrains AI Assistant not found. Install from JetBrains IDE Marketplace."
                    .to_string(),
            )),
            crate::core::ProviderStateKind::LocalRuntimeOffline
        );
    }

    #[test]
    fn custom_path_is_trimmed_and_blank_means_auto_detect() {
        assert_eq!(
            custom_base_path("  C:\\JB\\IntelliJ2025.3  "),
            Some("C:\\JB\\IntelliJ2025.3".to_string())
        );
        assert_eq!(custom_base_path("   "), None);
        assert_eq!(custom_base_path(""), None);
    }

    #[test]
    fn custom_path_reads_monthly_and_top_up_from_xml_alone() {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = write_ide_xml(
            dir.path(),
            "IntelliJIdea2025.3",
            &quota_xml(QUOTA_JSON, Some(REFILL_JSON)),
            XML_MODIFIED_2020,
        );

        let snapshot = read_snapshot(base.to_str(), &[]).expect("snapshot");

        assert!((snapshot.quota.used_percent() - 6.8433145).abs() < 1e-6);
        assert_eq!(top_up_value(&snapshot).as_deref(), Some("54.90 credits"));
        assert!(snapshot.ide.is_none());
    }

    #[test]
    fn detected_ide_newer_log_replaces_quota_and_refill_as_one_snapshot() {
        let fixture = roots();
        write_ide_xml(
            &fixture.config,
            "IntelliJIdea2025.3",
            &quota_xml(QUOTA_JSON, Some(REFILL_JSON)),
            XML_MODIFIED_2020,
        );
        // Newer log has no top-up group and no refill record.
        write_ide_log(
            &fixture.logs,
            "IntelliJIdea2025.3",
            &[LOG_QUOTA_LATER_NO_TOP_UP],
        );

        let snapshot = read_snapshot(None, &fixture.roots).expect("snapshot");

        assert!((snapshot.quota.used_percent() - 25.0).abs() < 1e-9);
        assert!(
            top_up_value(&snapshot).is_none(),
            "top-up must come from the log, not XML"
        );
        assert!(
            snapshot.refill.is_none(),
            "a log without refill must not borrow the XML reset date"
        );
        assert_eq!(
            snapshot
                .ide
                .as_ref()
                .map(IdeInstall::display_name)
                .as_deref(),
            Some("IntelliJ IDEA 2025.3")
        );
    }

    #[test]
    fn detected_ide_newer_junie_log_replaces_xml_quota_and_drops_xml_refill() {
        let fixture = roots();
        write_ide_xml(
            &fixture.config,
            "IntelliJIdea2025.3",
            &quota_xml(QUOTA_JSON, Some(REFILL_JSON)),
            XML_MODIFIED_2020,
        );
        // Junie's record is the newest quota state and carries no refill record.
        write_ide_log(&fixture.logs, "IntelliJIdea2025.3", &[LOG_JUNIE_QUOTA]);

        let snapshot = read_snapshot(None, &fixture.roots).expect("snapshot");

        assert!((snapshot.quota.used_percent() - 1.696618).abs() < 1e-6);
        assert_eq!(top_up_value(&snapshot).as_deref(), Some("54.90 credits"));
        assert!(
            snapshot.refill.is_none(),
            "a log without refill must not borrow the XML reset date"
        );
    }

    #[test]
    fn older_log_keeps_the_xml_quota_and_refill() {
        let fixture = roots();
        write_ide_xml(
            &fixture.config,
            "IntelliJIdea2025.3",
            &quota_xml(QUOTA_JSON, Some(REFILL_JSON)),
            XML_MODIFIED_2030,
        );
        write_ide_log(
            &fixture.logs,
            "IntelliJIdea2025.3",
            &[LOG_QUOTA, LOG_REFILL],
        );

        let snapshot = read_snapshot(None, &fixture.roots).expect("snapshot");

        assert!((snapshot.quota.used_percent() - 6.8433145).abs() < 1e-6);
        assert_eq!(top_up_value(&snapshot).as_deref(), Some("54.90 credits"));
        assert!(snapshot.refill.is_some());
    }

    #[test]
    fn log_of_another_ide_never_replaces_the_selected_ides_xml() {
        let fixture = roots();
        // IntelliJ has the newest XML and no log of its own. PyCharm has an
        // older XML and a newer log, which must not replace IntelliJ's quota.
        write_ide_xml(
            &fixture.config,
            "IntelliJIdea2025.3",
            &quota_xml(QUOTA_JSON, Some(REFILL_JSON)),
            XML_MODIFIED_2030,
        );
        write_ide_xml(
            &fixture.config,
            "PyCharm2025.2",
            &quota_xml(QUOTA_JSON, None),
            XML_MODIFIED_2020,
        );
        write_ide_log(&fixture.logs, "PyCharm2025.2", &[LOG_QUOTA_LATER_NO_TOP_UP]);

        let snapshot = read_snapshot(None, &fixture.roots).expect("snapshot");
        assert!((snapshot.quota.used_percent() - 6.8433145).abs() < 1e-6);
        assert_eq!(
            snapshot
                .ide
                .as_ref()
                .map(IdeInstall::display_name)
                .as_deref(),
            Some("IntelliJ IDEA 2025.3")
        );
    }

    #[test]
    fn auto_mode_selects_the_ide_with_the_newest_quota_xml() {
        let fixture = roots();
        write_ide_xml(
            &fixture.config,
            "IntelliJIdea2025.3",
            &quota_xml(QUOTA_JSON, None),
            XML_MODIFIED_2020,
        );
        write_ide_xml(
            &fixture.config,
            "PyCharm2025.2",
            &quota_xml(
                r#"{"current":"25","maximum":"100","tariffQuota":{"current":"25","maximum":"100"}}"#,
                None,
            ),
            XML_MODIFIED_2030,
        );

        let snapshot = read_snapshot(None, &fixture.roots).expect("snapshot");
        assert_eq!(snapshot.quota.used_percent(), 25.0);
        assert_eq!(
            snapshot
                .ide
                .as_ref()
                .map(IdeInstall::display_name)
                .as_deref(),
            Some("PyCharm 2025.2")
        );
    }

    #[test]
    fn unparsable_xml_falls_back_to_the_selected_ides_log() {
        let fixture = roots();
        write_ide_xml(
            &fixture.config,
            "IntelliJIdea2025.3",
            "<application><component name=\"AIAssistantQuotaManager2\"></component></application>",
            XML_MODIFIED_2020,
        );
        write_ide_log(
            &fixture.logs,
            "IntelliJIdea2025.3",
            &[LOG_QUOTA, LOG_REFILL],
        );

        let snapshot = read_snapshot(None, &fixture.roots).expect("snapshot from log");
        assert!((snapshot.quota.used_percent() - 34.6495294).abs() < 1e-6);
        assert!(snapshot.refill.is_some());
        assert!(snapshot.ide.is_some());
    }

    #[test]
    fn unparsable_xml_without_a_log_is_a_parse_error() {
        let fixture = roots();
        write_ide_xml(
            &fixture.config,
            "IntelliJIdea2025.3",
            "<application/>",
            XML_MODIFIED_2020,
        );

        let error = read_snapshot(None, &fixture.roots).expect_err("no quota");
        assert!(matches!(error, ProviderError::Parse(_)));
    }

    #[test]
    fn custom_path_without_xml_or_log_is_not_installed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let base = dir.path().join("IntelliJIdea2025.3");
        fs::create_dir_all(&base).expect("base");

        let error = read_snapshot(base.to_str(), &[]).expect_err("missing");
        assert!(matches!(error, ProviderError::NotInstalled(_)));
    }

    #[test]
    fn no_ide_and_no_logs_is_not_installed() {
        let fixture = roots();
        let error = read_snapshot(None, &fixture.roots).expect_err("nothing installed");
        assert!(
            matches!(error, ProviderError::NotInstalled(message) if message == NOT_INSTALLED_MESSAGE)
        );
    }

    #[test]
    fn log_only_detection_uses_the_ide_with_the_newest_log_record() {
        let fixture = roots();
        // Two IDEs with no quota XML. IntelliJ's record is newer.
        fs::create_dir_all(fixture.config.join("PyCharm2025.2")).expect("pycharm");
        fs::create_dir_all(fixture.config.join("IntelliJIdea2025.3")).expect("intellij");
        write_ide_log(&fixture.logs, "PyCharm2025.2", &[LOG_QUOTA]);
        write_ide_log(
            &fixture.logs,
            "IntelliJIdea2025.3",
            &[LOG_QUOTA_LATER_NO_TOP_UP],
        );

        let snapshot = read_snapshot(None, &fixture.roots).expect("log-only snapshot");
        assert!((snapshot.quota.used_percent() - 25.0).abs() < 1e-9);
        assert_eq!(
            snapshot
                .ide
                .as_ref()
                .map(IdeInstall::display_name)
                .as_deref(),
            Some("IntelliJ IDEA 2025.3")
        );
    }

    #[test]
    fn apply_log_entry_keeps_xml_when_its_time_is_unknown() {
        let xml = Snapshot {
            quota: QuotaInfo {
                used: 1.0,
                maximum: 4.0,
                top_up: None,
            },
            refill: None,
            ide: None,
        };
        let entry = LogEntry {
            timestamp: DateTime::<Utc>::from(at(XML_MODIFIED_2030)),
            quota: QuotaInfo {
                used: 3.0,
                maximum: 4.0,
                top_up: None,
            },
            refill: None,
        };
        assert_eq!(apply_log_entry(xml.clone(), Some(entry), None), xml);
    }

    #[test]
    fn usage_maps_refill_date_to_reset_and_top_up_to_a_balance_row() {
        let snapshot = Snapshot {
            quota: QuotaInfo {
                used: 68_433.145,
                maximum: 1_000_000.0,
                top_up: quota::TopUpQuota::new(Some(5_489_986.397), Some(5_489_986.397)),
            },
            refill: Some(RefillInfo {
                next: Some(DateTime::<Utc>::from(at(XML_MODIFIED_2030))),
            }),
            ide: None,
        };
        let usage = usage_snapshot(&snapshot);
        let primary = usage.primary;
        assert!((primary.used_percent - 6.8433145).abs() < 1e-6);
        assert_eq!(
            primary.resets_at,
            Some(DateTime::<Utc>::from(at(XML_MODIFIED_2030)))
        );

        let fetch = fetch_result(&snapshot);
        let row = fetch.display_details().first().expect("top-up row");
        assert_eq!(row.section_title(), Some("Top-up credits"));
        assert_eq!(row.title(), "Remaining");
        assert_eq!(row.value(), "54.90 credits");
        assert!(
            row.progress().is_none(),
            "top-up is a balance, not a usage bar"
        );
    }

    #[test]
    fn exhausted_top_up_shows_zero_and_missing_top_up_shows_no_row() {
        let exhausted = QuotaInfo {
            used: 0.0,
            maximum: 100.0,
            top_up: quota::TopUpQuota::new(Some(5_489_986.397), Some(0.0)),
        };
        assert_eq!(
            top_up_details(&exhausted)
                .first()
                .map(|row| row.value().to_string()),
            Some("0.00 credits".to_string())
        );

        let none = QuotaInfo {
            used: 0.0,
            maximum: 100.0,
            top_up: None,
        };
        assert!(top_up_details(&none).is_empty());
    }

    #[test]
    fn fetch_result_keeps_the_local_source_label() {
        let snapshot = Snapshot {
            quota: QuotaInfo {
                used: 0.0,
                maximum: 100.0,
                top_up: None,
            },
            refill: None,
            ide: None,
        };
        assert_eq!(fetch_result(&snapshot).source_label, "local");
    }
}
