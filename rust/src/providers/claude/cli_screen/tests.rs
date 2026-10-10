use super::*;
use crate::providers::claude::ClaudeProvider;

/// Byte-exact upstream fixtures (`Tests/CodexBarTests/Fixtures/Providers/Claude`
/// at v0.65.0); `.gitattributes` keeps their CR bytes.
const USAGE_FIXTURE: &str =
    include_str!("../../fixtures/claude/usage-pty-differential-redraw.ansi");
const STATUS_FIXTURE: &str =
    include_str!("../../fixtures/claude/status-pty-differential-redraw.ansi");

fn replay(text: &str) -> String {
    render(text, false)
}

#[test]
fn cursor_and_erase_operations_preserve_visible_cells() {
    let vectors = [
        ("abc\rZ", "Zbc"),
        ("one\r\ntwo\nthree", "one\ntwo\nthree"),
        ("abc\u{8}Z", "abZ"),
        ("abc\r\u{1b}[2CZ", "abZ"),
        ("abc\u{1b}[2DZ", "aZc"),
        ("a\u{1b}[2Cb", "a  b"),
        ("abc\r\u{1b}[CZ", "aZc"),
        ("abc\u{1b}[DZ", "abZ"),
        ("one\r\ntwo\u{1b}[1A\u{1b}[1GX", "Xne\ntwo"),
        ("one\r\u{1b}[2Btwo", "one\n\ntwo"),
        ("abc\u{1b}[2GZ", "aZc"),
        ("abc\u{1b}[2;2HZ", "abc\n Z"),
        ("abc\u{1b}[2;2fZ", "abc\n Z"),
        ("abc\u{1b}[0;0HZ", "Zbc"),
        ("one\r\ntwo\u{1b}[HZ", "Zne\ntwo"),
        ("abc\r\u{1b}[2G\u{1b}[K", "a"),
        ("abc\u{1b}[2G\u{1b}[1K", "  c"),
        ("abc\u{1b}[2K", ""),
        ("abc\nxyz\u{1b}[1;2H\u{1b}[J", "a"),
        ("abc\nxyz\u{1b}[2;2H\u{1b}[1J", "\n  z"),
        ("abc\nxyz\u{1b}[2JQ", "\n   Q"),
        ("abcdef\r\u{1b}[2C\u{1b}[2XZ", "abZ ef"),
        ("abc\r\u{1b}[XZ", "Zbc"),
        ("abc\u{1b}[2G\u{1b}[99X", "a"),
        ("中文x\r\u{1b}[1C\u{1b}[1X", "  文x"),
        // A ConPTY redraw over an earlier frame's row: ECH blanks the gaps.
        (
            "ResetsiOctr6, 4am\rResets\u{1b}[1X\u{1b}[1COct\u{1b}[1X\u{1b}[1C6, 4am",
            "Resets Oct 6, 4am",
        ),
        ("a\u{1b}[38;5;42mb\u{1b}[0m\u{1b}[?25lc", "abc"),
        ("a\u{1b}]0;hidden\u{7}b\u{1b}]8;;hidden\u{1b}\\c", "abc"),
        ("a\u{1b}]0;hidden", "a"),
        ("a\u{1b}[123;", "a"),
        ("a\u{1b}[?2J\u{1b}[99K\u{1b}[99Jb", "ab"),
        ("Org: 中文\u{1b}[10G Labs", "Org: 中文 Labs"),
        ("🎉 ok\u{1b}[4GX", "🎉 Xk"),
        ("⚠️ ok\u{1b}[4GX", "⚠️ Xk"),
        ("██▌\u{1b}[5G51%", "██▌ 51%"),
        ("中文\u{1b}[2Gab", " ab"),
        ("中文x\u{1b}[4G\u{1b}[K", "中"),
        ("e\u{1b}[0m\u{301}x", "e\u{301}x"),
    ];
    for (stream, expected) in vectors {
        assert_eq!(replay(stream), expected, "stream {stream:?}");
    }
}

#[test]
fn joiner_sequences_and_flags_occupy_one_wide_cell() {
    assert_eq!(replay("👨\u{200D}👩 ok\u{1b}[4GX"), "👨\u{200D}👩 Xk");
    assert_eq!(replay("👍\u{1F3FD} ok\u{1b}[4GX"), "👍\u{1F3FD} Xk");
    assert_eq!(replay("🇸🇪 ok\u{1b}[4GX"), "🇸🇪 Xk");
}

#[test]
fn unsupported_zero_width_characters_are_dropped() {
    assert_eq!(replay("a\u{200B}b\u{7}c\u{7f}d"), "abcd");
}

#[test]
fn unterminated_osc_leaves_a_final_line_break_visible() {
    assert_eq!(replay("a\u{1b}]0;title\nb"), "a");
    assert_eq!(replay("a\u{1b}]0;title\r\n"), "a");
    assert_eq!(replay("a\u{1b}\nb"), "a\nb");
    assert_eq!(replay("a\u{1b}"), "a");
}

#[test]
fn synthetic_differential_frame_matches_the_visible_panel_golden() {
    let frame = "\u{1b}[HCurrent session\n3% used\nCurrent week (all models)\n27% used\n\
                 Current week (Fable)\nabc xye jkl\u{1b}[6;1H51%\u{1b}[5Gus\u{1b}[8Gd\u{1b}[9G\u{1b}[K";
    let golden = "Current session\n3% used\nCurrent week (all models)\n27% used\n\
                  Current week (Fable)\n51% used";
    assert_eq!(replay(frame), golden);
    assert_eq!(render(frame, true), golden);

    let provider = ClaudeProvider::new();
    let actual = provider.parse_cli_output(frame).unwrap().usage;
    let expected = provider.parse_cli_output(golden).unwrap().usage;
    assert_eq!(actual.primary.used_percent, expected.primary.used_percent);
    assert_eq!(
        actual.secondary.map(|w| w.used_percent),
        expected.secondary.map(|w| w.used_percent)
    );
    assert_eq!(
        actual.extra_rate_windows.len(),
        expected.extra_rate_windows.len()
    );
}

#[test]
fn backspace_edits_are_replayed_without_a_csi_sequence() {
    let usage = ClaudeProvider::new()
        .parse_cli_output("Current session\n39\u{8}5% used")
        .unwrap()
        .usage;
    assert_eq!(usage.primary.used_percent, 35.0);
}

#[test]
fn clearing_the_screen_cannot_resurrect_an_earlier_quota() {
    let frame = "\u{1b}[HCurrent session\n3% used\u{1b}[2J";
    assert!(ClaudeProvider::new().parse_cli_output(frame).is_err());
}

#[test]
fn positions_and_overflowing_parameters_stay_within_the_pty_geometry() {
    let huge = "9".repeat(100);
    let frame = format!("\u{1b}[{huge};{huge}HQ\u{1b}[{huge}AZ\u{1b}[{huge}DX");
    let output = replay(&frame);
    let lines: Vec<&str> = output.split('\n').collect();
    assert_eq!(lines.len(), ROWS);
    assert!(lines.iter().all(|line| line.chars().count() <= COLUMNS));
    assert_eq!(lines[0], format!("X{}Z", " ".repeat(COLUMNS - 2)));
    assert_eq!(lines[ROWS - 1], format!("{}Q", " ".repeat(COLUMNS - 1)));
}

#[test]
fn wrap_and_line_feed_retain_only_the_visible_screen() {
    let full = "x".repeat(COLUMNS);
    assert_eq!(replay(&format!("{full}Z")), format!("{full}\nZ"));
    assert_eq!(replay(&format!("{full}\u{1b}[31mZ")), format!("{full}\nZ"));
    assert_eq!(replay(&format!("{full}\u{1b}[99KZ")), format!("{full}\nZ"));
    let narrow = "x".repeat(COLUMNS - 1);
    assert_eq!(replay(&format!("{narrow}中Z")), format!("{narrow}\n中Z"));
    let rows: Vec<String> = (0..=ROWS).map(|n| n.to_string()).collect();
    assert_eq!(replay(&rows.join("\r\n")), rows[1..].join("\n"));
}

#[test]
fn every_range_table_is_sorted_and_disjoint() {
    for table in [EXTEND, FORMAT, WIDE, EMOJI_PRESENTATION, EMOJI] {
        for pair in table.windows(2) {
            assert!(pair[0].0 <= pair[0].1 && pair[0].1 < pair[1].0, "{pair:x?}");
        }
    }
}

#[test]
fn plain_reports_keep_cr_delimiters_and_ignore_osc_contents() {
    let title = "\u{1b}]0;\u{1b}[HCurrent session 99% used\u{7}";
    let text = format!("{title}\u{1b}[35mCurrent session\u{1b}[0m\r3% used\r");
    assert_eq!(render(&text, true), "Current session\r3% used\r");
    let usage = ClaudeProvider::new().parse_cli_output(&text).unwrap().usage;
    assert_eq!(usage.primary.used_percent, 3.0);
}

#[test]
fn plain_reports_are_not_clipped_or_wrapped_to_the_pty_geometry() {
    let padding = "report detail\n".repeat(55);
    let organization = "Example".repeat(30);
    let text = format!(
        "\u{1b}[32mCurrent session\n3% used\n{padding}Org: {organization}\nEmail: fixture@example.com\u{1b}[0m"
    );
    let rendered = render(&text, true);
    assert!(rendered.contains(&format!("Org: {organization}\n")));
    let usage = ClaudeProvider::new().parse_cli_output(&text).unwrap().usage;
    assert_eq!(usage.primary.used_percent, 3.0);
    assert_eq!(usage.account_email.as_deref(), Some("fixture@example.com"));
}

#[test]
fn a_cursor_sequence_after_plain_text_switches_to_replay() {
    assert_eq!(render("abc\u{1b}[1D\u{1b}[K", true), "ab");
    assert_eq!(render("color \u{1b}[31monly\u{1b}[0m", true), "color only");
}

#[test]
fn differential_redraw_fixture_keeps_the_scoped_weekly_quota() {
    // Plain escape stripping fuses the redraw fragments, which is the bug.
    let stripped = strip_ansi(USAGE_FIXTURE);
    assert!(stripped.contains("51%usd"), "{stripped}");

    let provider = ClaudeProvider::new();
    let result = provider.parse_cli_output(USAGE_FIXTURE).unwrap();
    let usage = result.usage;
    assert_eq!(usage.primary.used_percent, 3.0, "97% left");
    let weekly = usage.secondary.expect("weekly window");
    assert_eq!(weekly.used_percent, 27.0, "73% left");
    assert_eq!(
        weekly.reset_description.as_deref(),
        Some("Resets Sep 23 at 3pm (Europe/Stockholm)")
    );
    let fable = usage
        .extra_rate_windows
        .iter()
        .find(|window| window.id == "claude-weekly-scoped-fable")
        .expect("scoped Fable window");
    assert_eq!(fable.title, "Fable"); // label is taken verbatim from "Current week (Fable)"
    assert_eq!(fable.window.used_percent, 51.0);
}

#[test]
fn rendering_a_rendered_capture_changes_nothing() {
    for fixture in [USAGE_FIXTURE, STATUS_FIXTURE] {
        let once = render(fixture, true);
        assert_eq!(render(&once, true), once);
    }
}

#[test]
fn redrawn_identity_uses_the_final_frame_with_cursor_positioned_spaces() {
    assert_eq!(
        render(STATUS_FIXTURE, true),
        "Email: fixture@example.com\nOrg: Example Org\nLogin method: Claude Max Account"
    );
    let text = format!("{STATUS_FIXTURE}\nCurrent session\n3% used");
    let usage = ClaudeProvider::new().parse_cli_output(&text).unwrap().usage;
    assert_eq!(usage.account_email.as_deref(), Some("fixture@example.com"));
    assert_eq!(usage.login_method.as_deref(), Some("Claude Max Account"));
}

#[test]
fn rendered_text_never_carries_an_escape() {
    let inputs = [
        USAGE_FIXTURE,
        STATUS_FIXTURE,
        "plain \u{1b}[1mLogin method: Claude Max\u{1b}[22m",
        "osc \u{1b}]0;title\u{7}tail \u{1b}]8;;x\u{1b}\\link",
        "lone \u{1b}\n\u{1b}Xtrail \u{1b}",
        "abc\u{1b}[2DZ \u{1b}(B done",
    ];
    for input in inputs {
        for preserve in [true, false] {
            let rendered = render(input, preserve);
            assert!(!rendered.contains('\u{1b}'), "{rendered:?}");
            assert_eq!(strip_ansi(&rendered), rendered);
        }
    }
}

/// Plain escape stripping, the pre-replay behavior these tests compare against.
fn strip_ansi(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '\x1B' {
            // Skip CSI sequences: ESC[...letter
            if chars.peek() == Some(&'[') {
                chars.next();
                let mut final_char = None;
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if next.is_ascii_alphabetic() {
                        final_char = Some(next);
                        break;
                    }
                }
                if final_char == Some('C') {
                    result.push(' ');
                }
            // Skip OSC sequences: ESC]...BEL
            } else if chars.peek() == Some(&']') {
                for next in chars.by_ref() {
                    if next == '\x07' || next == '\\' {
                        break;
                    }
                }
            }
        } else {
            result.push(c);
        }
    }

    result
}
