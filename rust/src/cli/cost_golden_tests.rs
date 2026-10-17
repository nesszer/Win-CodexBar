//! Golden outputs for the `cost` command's provider-gated modes: project and
//! session grouping, the partial-history notice, identified-request subtotals
//! and host summaries. Every case uses temporary fixture homes and default
//! settings, so nothing reads the user's real Codex history or settings.

use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use chrono::Local;
use tempfile::TempDir;

use super::*;
use crate::spend_contract::{LocalHistoryCoverage, LocalTokenHistorySummary};

const PERIOD: CostReportingPeriod = CostReportingPeriod::Rolling(30);

fn result(provider: ProviderId, summary: CostSummary, supported: bool) -> CostResult {
    CostResult {
        provider: provider.cli_name().to_string(),
        display_name: provider.display_name().to_string(),
        summary,
        supported,
        token_history: None,
    }
}

fn codex_summary(history_coverage_established: bool) -> CostSummary {
    let mut summary = CostSummary {
        total_cost_usd: 1.25,
        input_tokens: 12_000,
        output_tokens: 3_400,
        cached_tokens: 500,
        sessions_count: 2,
        history_coverage_established,
        ..CostSummary::default()
    };
    summary.by_model.insert("gpt-5".to_string(), 1.25);
    summary
}

fn claude_summary() -> CostSummary {
    let mut summary = CostSummary {
        total_cost_usd: 0.5,
        input_tokens: 1_000,
        output_tokens: 200,
        sessions_count: 1,
        history_coverage_established: false,
        ..CostSummary::default()
    };
    summary.by_model.insert("claude-sonnet-4".to_string(), 0.5);
    summary
}

fn antigravity_result() -> CostResult {
    CostResult {
        token_history: Some(LocalTokenHistorySummary {
            total_tokens: 4_321,
            session_count: 3,
            coverage: LocalHistoryCoverage::Complete,
            ..Default::default()
        }),
        ..result(ProviderId::Antigravity, CostSummary::default(), true)
    }
}

/// Fixture Codex home with one project holding two sessions.
struct CodexFixture {
    tmp: TempDir,
}

impl CodexFixture {
    fn empty() -> Self {
        let tmp = TempDir::new().unwrap();
        fs::create_dir_all(tmp.path().join("codex")).unwrap();
        Self { tmp }
    }

    fn with_project() -> Self {
        let fixture = Self::empty();
        let day = Local::now().date_naive().format("%Y-%m-%d").to_string();
        let home = fixture.home();
        let project = fixture.root().join("project");
        fs::create_dir_all(&project).unwrap();
        // Pin the SQLite home so a developer's CODEX_SQLITE_HOME cannot interfere.
        fs::write(
            home.join("config.toml"),
            format!(
                "sqlite_home = '{}'\n",
                home.canonicalize().unwrap().display()
            ),
        )
        .unwrap();
        rusqlite::Connection::open(home.join("state_5.sqlite"))
            .unwrap()
            .execute_batch("CREATE TABLE threads (id TEXT PRIMARY KEY, title TEXT);")
            .unwrap();
        write_session(&home, &day, "alpha-session-0001", &project, 20_000, 1_000);
        write_session(&home, &day, "beta", &project, 1_000, 100);
        fixture
    }

    fn root(&self) -> &Path {
        self.tmp.path()
    }

    fn home(&self) -> PathBuf {
        self.root().join("codex")
    }

    /// Replaces the temporary root and the machine-local activity times.
    fn redact(&self, text: String) -> String {
        let canonical = self.root().canonicalize().unwrap();
        let mut text = text;
        for root in [canonical.clone(), self.root().to_path_buf()] {
            let root = root.to_string_lossy().into_owned();
            text = text.replace(&root.replace('\\', "/"), "<ROOT>");
            text = text.replace(&root, "<ROOT>");
        }
        let mut lines: Vec<String> = Vec::new();
        for line in text.split('\n') {
            let after_session = lines
                .last()
                .is_some_and(|prev| prev.starts_with("  Session "));
            if after_session && line.starts_with("    ") {
                lines.push("    <ACTIVITY>".to_string());
            } else {
                lines.push(line.to_string());
            }
        }
        lines.join("\n")
    }
}

fn write_session(home: &Path, day: &str, id: &str, cwd: &Path, input: i32, output: i32) {
    let parts: Vec<_> = day.split('-').collect();
    let folder = home
        .join("sessions")
        .join(parts[0])
        .join(parts[1])
        .join(parts[2]);
    fs::create_dir_all(&folder).unwrap();
    let mut file = File::create(folder.join(format!("{id}.jsonl"))).unwrap();
    writeln!(
        file,
        r#"{{"timestamp":"{day}T10:00:00.000Z","type":"session_meta","payload":{{"session_id":"{id}","cwd":"{cwd}","originator":"codex_exec","source":"cli"}}}}"#,
        cwd = cwd.to_string_lossy().replace('\\', "\\\\"),
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"timestamp":"{day}T10:00:01.000Z","type":"turn_context","payload":{{"model":"gpt-5"}}}}"#
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"timestamp":"{day}T10:00:02.000Z","type":"event_msg","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":{input},"cached_input_tokens":0,"output_tokens":{output}}}}}}}}}"#
    )
    .unwrap();
}

/// Text report exactly as `print_text_output` would write it, with Codex
/// local history read from `fixture` instead of the real home.
fn render(
    results: &[CostResult],
    use_color: bool,
    group_by: CostGroupBy,
    hide_personal_info: bool,
    fixture: &CodexFixture,
) -> String {
    let codex = crate::codex_costs::CodexCostCli::with_paths(
        fixture.home(),
        fixture.root().join("sidecar.sqlite"),
    );
    let extension_for = |provider: &str| -> Option<&dyn CostCliExtension> {
        if provider == "codex" {
            Some(&codex)
        } else {
            registered_extension(provider)
        }
    };
    let mut text = String::new();
    for (i, result) in results.iter().enumerate() {
        let mut out = String::new();
        render_result_text(
            &mut out,
            result,
            use_color,
            PERIOD,
            group_by,
            hide_personal_info,
            &extension_for,
        );
        if i < results.len() - 1 {
            out.push('\n');
        }
        text.push_str(&out);
    }
    fixture.redact(text)
}

fn plain(results: &[CostResult], group_by: CostGroupBy) -> String {
    render(results, false, group_by, false, &CodexFixture::empty())
}

const CODEX_DEFAULT: &str = "\
Codex Cost (Last 30 days)
  Total:    $1.25
  Tokens:   12,000 input, 3,400 output, 500 cached
  Sessions: 2
  By model:
    gpt-5: $1.25
";

#[test]
fn golden_codex_default_report() {
    let codex = [result(ProviderId::Codex, codex_summary(true), true)];
    assert_eq!(plain(&codex, CostGroupBy::None), CODEX_DEFAULT);
}

#[test]
fn golden_codex_default_report_marks_partial_history() {
    let codex = [result(ProviderId::Codex, codex_summary(false), true)];
    assert_eq!(
        plain(&codex, CostGroupBy::None),
        format!("{CODEX_DEFAULT}  Coverage: partial (history catch-up in progress)\n")
    );
}

#[test]
fn golden_codex_colored_report_marks_partial_history() {
    let codex = [result(ProviderId::Codex, codex_summary(false), true)];
    assert_eq!(
        render(
            &codex,
            true,
            CostGroupBy::None,
            false,
            &CodexFixture::empty()
        ),
        "\x1b[1mCodex Cost (Last 30 days)\x1b[0m
  Total:    \x1b[32m$1.25\x1b[0m
  Tokens:   12,000 input, 3,400 output, 500 cached
  Sessions: 2
  By model:
    gpt-5: $1.25
  Coverage: partial (history catch-up in progress)
"
    );
}

#[test]
fn golden_codex_empty_history_has_no_coverage_notice() {
    let summary = CostSummary {
        known_zero: true,
        ..CostSummary::default()
    };
    let codex = [result(ProviderId::Codex, summary, true)];
    assert_eq!(
        plain(&codex, CostGroupBy::None),
        "Codex Cost (Last 30 days)\n  No usage in the selected period (scan complete)\n"
    );
}

#[test]
fn golden_claude_default_report_has_no_coverage_notice() {
    let claude = [result(ProviderId::Claude, claude_summary(), true)];
    assert_eq!(
        plain(&claude, CostGroupBy::None),
        "\
Claude Cost (Last 30 days)
  Total:    $0.50
  Tokens:   1,000 input, 200 output, 0 cached
  Sessions: 1
  By model:
    claude-sonnet-4: $0.50
"
    );
}

#[test]
fn golden_codex_and_claude_reports_are_separated_by_a_blank_line() {
    let both = [
        result(ProviderId::Codex, codex_summary(false), true),
        result(ProviderId::Claude, claude_summary(), true),
    ];
    let text = plain(&both, CostGroupBy::None);
    assert!(text.starts_with(&format!(
        "{CODEX_DEFAULT}  Coverage: partial (history catch-up in progress)\n\nClaude Cost"
    )));
    assert_eq!(text.matches("Coverage: partial").count(), 1);
}

const PROJECT_UNSUPPORTED: &str = "  Project grouping is only available for Codex local projects\n";
const SESSION_UNSUPPORTED: &str =
    "  Session grouping is only available for Codex local conversations\n";

#[test]
fn golden_project_grouping_is_unavailable_for_other_providers() {
    let claude = [result(ProviderId::Claude, claude_summary(), true)];
    assert_eq!(
        plain(&claude, CostGroupBy::Project),
        format!("Claude Cost (Last 30 days)\n{PROJECT_UNSUPPORTED}")
    );
    let cursor = [result(ProviderId::Cursor, CostSummary::default(), false)];
    assert_eq!(
        plain(&cursor, CostGroupBy::Project),
        format!("Cursor Cost (Last 30 days)\n{PROJECT_UNSUPPORTED}")
    );
    // Token-history providers get the unavailable line, not their history.
    assert_eq!(
        plain(&[antigravity_result()], CostGroupBy::Project),
        format!("Antigravity Token History (Last 30 days)\n{PROJECT_UNSUPPORTED}")
    );
}

#[test]
fn golden_session_grouping_is_unavailable_for_other_providers() {
    let claude = [result(ProviderId::Claude, claude_summary(), true)];
    assert_eq!(
        plain(&claude, CostGroupBy::Session),
        format!("Claude Cost (Last 30 days)\n{SESSION_UNSUPPORTED}")
    );
    let cursor = [result(ProviderId::Cursor, CostSummary::default(), false)];
    assert_eq!(
        plain(&cursor, CostGroupBy::Session),
        format!("Cursor Cost (Last 30 days)\n{SESSION_UNSUPPORTED}")
    );
    // Token-history providers keep printing their history for session grouping.
    assert_eq!(
        plain(&[antigravity_result()], CostGroupBy::Session),
        "\
Antigravity Token History (Last 30 days)
  Tokens:   4,321 total
  Sessions: 3
  Local token history; dollar costs unavailable
"
    );
}

#[test]
fn golden_unsupported_provider_default_report() {
    let cursor = [result(ProviderId::Cursor, CostSummary::default(), false)];
    assert_eq!(
        plain(&cursor, CostGroupBy::None),
        "\
Cursor Cost (Last 30 days)
  Local cost scanning not available for this provider
  (Only Codex and Claude have local logs)
"
    );
}

#[test]
fn golden_codex_project_report() {
    let fixture = CodexFixture::with_project();
    let codex = [result(ProviderId::Codex, codex_summary(true), true)];
    assert_eq!(
        render(&codex, false, CostGroupBy::Project, false, &fixture),
        "\
Codex Cost (Last 30 days)
  Projects (last 30 days):
  project: $0.04 · 22,100 tokens
    <ROOT>/project
  Not a subscription bill or plan value · local usage × public API prices
"
    );
}

#[test]
fn golden_codex_project_report_marks_partial_history_and_hides_paths() {
    let fixture = CodexFixture::with_project();
    let codex = [result(ProviderId::Codex, codex_summary(false), true)];
    assert_eq!(
        render(&codex, false, CostGroupBy::Project, true, &fixture),
        "\
Codex Cost (Last 30 days)
  Coverage: partial (history catch-up in progress)
  Projects (last 30 days):
  Workspace #1: $0.04 · 22,100 tokens
  Not a subscription bill or plan value · local usage × public API prices
"
    );
}

#[test]
fn golden_codex_project_report_for_empty_history() {
    let codex = [result(ProviderId::Codex, codex_summary(false), true)];
    assert_eq!(
        plain(&codex, CostGroupBy::Project),
        "\
Codex Cost (Last 30 days)
  Coverage: partial (history catch-up in progress)
  Projects (last 30 days):
  —
  Not a subscription bill or plan value · local usage × public API prices
"
    );
}

#[test]
fn golden_codex_session_report() {
    let fixture = CodexFixture::with_project();
    let codex = [result(ProviderId::Codex, codex_summary(false), true)];
    assert_eq!(
        render(&codex, false, CostGroupBy::Session, false, &fixture),
        "\
Codex Cost (Last 30 days)
  Conversations (last 30 days):
  Session alph...ion-0001: $0.04 · 21,000 tokens · gpt-5
    <ACTIVITY>
  Session beta: $0.00 · 1,100 tokens · gpt-5
    <ACTIVITY>
  Coverage: partial (cost history catch-up in progress)
  Not a subscription bill or plan value · local usage × public API prices
"
    );
}

#[test]
fn golden_codex_session_report_for_empty_history() {
    let codex = [result(ProviderId::Codex, codex_summary(true), true)];
    assert_eq!(
        plain(&codex, CostGroupBy::Session),
        "\
Codex Cost (Last 30 days)
  Conversations (last 30 days):
  Conversation history is incomplete while local indexing catches up.
  —
  Not a subscription bill or plan value · local usage × public API prices
"
    );
}

#[test]
fn golden_mixed_providers_under_project_grouping() {
    let fixture = CodexFixture::with_project();
    let results = [
        result(ProviderId::Codex, codex_summary(true), true),
        result(ProviderId::Claude, claude_summary(), true),
    ];
    let text = render(&results, false, CostGroupBy::Project, false, &fixture);
    assert!(text.contains("  project: $0.04 · 22,100 tokens\n"));
    assert!(text.ends_with(&format!(
        "public API prices\n\nClaude Cost (Last 30 days)\n{PROJECT_UNSUPPORTED}"
    )));
}

/// Runs `codexbar cost <argv>` with default settings and returns the error
/// text. Every case fails validation before any history or network read.
async fn run_error(argv: &[&str]) -> String {
    let args = parse_args(argv);
    match run_with_settings(args, Settings::default).await {
        Ok(()) => panic!("expected `cost {}` to fail", argv.join(" ")),
        Err(err) => format!("{err:#}"),
    }
}

fn parse_args(argv: &[&str]) -> CostArgs {
    #[derive(clap::Parser)]
    struct Wrapper {
        #[command(flatten)]
        args: CostArgs,
    }
    use clap::Parser;
    Wrapper::try_parse_from(std::iter::once("cost").chain(argv.iter().copied()))
        .expect("cost args parse")
        .args
}

const IDENTIFIED_ERROR: &str = "Use --identified-only --provider codex with JSON, --days 1...365 and one optional --remote host. --identified-end must be Unix milliseconds; period, grouping, breakdown and separate host summaries are unsupported.";

#[tokio::test]
async fn golden_identified_usage_rejects_other_providers_and_modes() {
    for argv in [
        &["--provider", "claude", "--identified-only", "--json"][..],
        &["--provider", "all", "--identified-only", "--json"],
        &["--identified-only", "--json"],
        // Aliases resolve to Codex elsewhere, but the identified mode
        // requires the literal provider name.
        &["--provider", "openai", "--identified-only", "--json"],
        &["--provider", "codex", "--identified-only"],
        &[
            "--provider",
            "codex",
            "--json",
            "--identified-end",
            "1700000000000",
        ],
        &[
            "--provider",
            "claude",
            "--json",
            "--identified-end",
            "1700000000000",
        ],
        &[
            "--provider",
            "codex",
            "--identified-only",
            "--json",
            "--group-by",
            "project",
        ],
        &[
            "--provider",
            "codex",
            "--identified-only",
            "--json",
            "--period",
            "all",
        ],
        &[
            "--provider",
            "codex",
            "--identified-only",
            "--json",
            "--summary-only",
        ],
        &[
            "--provider",
            "codex",
            "--identified-only",
            "--json",
            "--days",
            "0",
        ],
        &[
            "--provider",
            "codex",
            "--identified-only",
            "--json",
            "--days",
            "366",
        ],
    ] {
        assert_eq!(run_error(argv).await, IDENTIFIED_ERROR, "{argv:?}");
    }
}

#[tokio::test]
async fn golden_identified_usage_validates_the_remote_host() {
    let error = run_error(&[
        "--provider",
        "codex",
        "--identified-only",
        "--json",
        "--remote=-oProxyCommand=x",
    ])
    .await;
    assert_eq!(error, GOLDEN_INVALID_REMOTE);
}

#[tokio::test]
async fn golden_identified_usage_checks_the_provider_name_first() {
    assert_eq!(
        run_error(&["--provider", "nope", "--identified-only", "--json"]).await,
        "Unknown provider: 'nope'. Use --help to see available providers."
    );
}

const HOST_PROVIDER_ERROR: &str =
    "--remote and --summary-only require exactly --provider codex; they do not support all or both";

#[tokio::test]
async fn golden_host_summaries_reject_other_providers() {
    for argv in [
        &["--provider", "claude", "--remote", "build-host"][..],
        &["--provider", "all", "--summary-only", "--json"],
        &["--provider", "both", "--remote", "build-host"],
        &["--remote", "build-host"],
        &[
            "--provider",
            "claude",
            "--remote",
            "build-host",
            "--group-by",
            "project",
        ],
    ] {
        assert_eq!(run_error(argv).await, HOST_PROVIDER_ERROR, "{argv:?}");
    }
}

#[tokio::test]
async fn golden_host_summaries_validate_codex_arguments() {
    let group_by = "--remote and --summary-only cannot be combined with --group-by";
    let cases: [(&[&str], &str); 7] = [
        (
            &[
                "--provider",
                "codex",
                "--remote",
                "build-host",
                "--group-by",
                "project",
            ],
            group_by,
        ),
        // Host summaries resolve the provider id, so the alias is accepted.
        (
            &[
                "--provider",
                "openai",
                "--remote",
                "build-host",
                "--group-by",
                "session",
            ],
            group_by,
        ),
        (
            &[
                "--provider",
                "codex",
                "--remote",
                "build-host",
                "--summary-only",
                "--json",
            ],
            "--remote and --summary-only cannot be combined",
        ),
        (
            &["--provider", "codex", "--summary-only"],
            "--summary-only requires --format json or --json",
        ),
        (
            &[
                "--provider",
                "codex",
                "--summary-only",
                "--json",
                "--period",
                "bogus",
            ],
            "--period must be month-to-date or all",
        ),
        (
            &[
                "--provider",
                "codex",
                "--remote",
                "build-host",
                "--period",
                "all",
            ],
            "host summaries support 1...365 days; override All with --days N",
        ),
        (
            &["--provider", "codex", "--remote=-oProxyCommand=x"],
            GOLDEN_INVALID_REMOTE,
        ),
    ];
    for (argv, expected) in cases {
        assert_eq!(run_error(argv).await, expected, "{argv:?}");
    }
}

const GOLDEN_INVALID_REMOTE: &str = "host must not start with '-'";
