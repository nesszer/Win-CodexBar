//! Hostile-text sanitization for external display strings.
//!
//! cswap output is attacker-influenced only in the sense that a compromised or
//! unusual executable controls it, so nothing copied into UI/logs may carry
//! terminal escapes, control characters, or invisible Unicode formatting. Raw
//! subprocess stdout is never surfaced.

/// Bound on display-only label fields copied from cswap (upstream uses 256 scalars).
pub const MAX_LABEL_CHARS: usize = 256;
/// Bound on diagnostic strings copied from a cswap error envelope (upstream: 512).
pub const MAX_DIAGNOSTIC_CHARS: usize = 512;

/// True for Unicode format / default-ignorable code points that must never
/// reach the UI or logs.
///
/// Bidi controls can reorder or visually spoof an otherwise-bound label, and
/// default-ignorable code points (soft hyphen, zero-width and word-joiner
/// characters, variation selectors, tag characters, ...) can hide content
/// while looking like legitimate text. Ordinary combining marks are not in
/// this set and survive sanitization.
fn is_unsafe_format_character(ch: char) -> bool {
    matches!(
        ch,
        '\u{00AD}'                    // SOFT HYPHEN
        | '\u{034F}'                  // COMBINING GRAPHEME JOINER
        | '\u{061C}'                  // ARABIC LETTER MARK
        | '\u{115F}'..='\u{1160}'     // HANGUL CHOSEONG / JUNGSEONG FILLERS
        | '\u{17B4}'..='\u{17B5}'     // KHMER VOWEL INHERENT AQ / AA
        | '\u{180B}'..='\u{180F}'     // MONGOLIAN FREE VARIATION SELECTORS
        | '\u{200B}'..='\u{200F}'     // ZERO WIDTH SPACE / JOINERS / LRM / RLM
        | '\u{202A}'..='\u{202E}'     // BIDI EMBEDDINGS AND OVERRIDES
        | '\u{2060}'..='\u{2064}'     // WORD JOINER AND INVISIBLE OPERATORS
        | '\u{2065}'                  // reserved format character
        | '\u{2066}'..='\u{206F}'     // BIDI ISOLATES AND DEPRECATED FORMATS
        | '\u{3164}'                  // HANGUL FILLER
        | '\u{FE00}'..='\u{FE0F}'     // VARIATION SELECTORS
        | '\u{FEFF}'                  // ZERO WIDTH NO-BREAK SPACE / BOM
        | '\u{FFA0}'                  // HALFWIDTH HANGUL FILLER
        | '\u{FFF0}'..='\u{FFF8}'     // interlinear annotation / object replacement
        | '\u{1BCA0}'..='\u{1BCA3}'   // SHORTHAND FORMAT CONTROLS
        | '\u{1D173}'..='\u{1D17A}'   // MUSICAL SYMBOL BEGIN / END
        | '\u{E0000}'..='\u{E0FFF}'   // TAGS AND VARIATION SELECTOR SUPPLEMENT
    )
}

/// Sanitize an external display string: strip ANSI/VT escape sequences,
/// control characters, bidi controls, and default-ignorable code points;
/// collapse line breaks to spaces; and bound length.
pub fn sanitize_display(text: &str, max_chars: usize) -> String {
    let mut out = String::with_capacity(text.len().min(max_chars));
    let mut count = 0usize;
    let mut chars = text.chars();
    while let Some(ch) = chars.next() {
        match ch {
            '\u{1b}' => match chars.next() {
                Some('[') => {
                    // CSI ... final byte in 0x40..=0x7e.
                    for c in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&c) {
                            break;
                        }
                    }
                }
                Some(']') => {
                    // OSC ... terminated by BEL or ST.
                    let mut previous = '\0';
                    for c in chars.by_ref() {
                        if c == '\u{07}' || (previous == '\u{1b}' && c == '\\') {
                            break;
                        }
                        previous = c;
                    }
                }
                Some(_) | None => {}
            },
            '\r' | '\n' | '\u{2028}' | '\u{2029}' => {
                // Collapse runs of line breaks (and adjacent literal spaces)
                // into a single separating space.
                if !out.ends_with(' ') {
                    out.push(' ');
                    count += 1;
                }
            }
            ' ' => {
                if !out.ends_with(' ') {
                    out.push(' ');
                    count += 1;
                }
            }
            _ if ch.is_control() => {}
            _ if is_unsafe_format_character(ch) => {}
            _ => {
                out.push(ch);
                count += 1;
            }
        }
        if count >= max_chars {
            break;
        }
    }
    out.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_terminal_escapes_and_bounds_length() {
        let csi = "\u{1b}[31mred\u{1b}[0m";
        assert_eq!(sanitize_display(csi, MAX_LABEL_CHARS), "red");

        let osc = "a\u{1b}]0;ignored\u{07}b";
        assert_eq!(sanitize_display(osc, MAX_LABEL_CHARS), "ab");

        let long = "x".repeat(MAX_LABEL_CHARS + 50);
        assert_eq!(
            sanitize_display(&long, MAX_LABEL_CHARS).chars().count(),
            MAX_LABEL_CHARS
        );
    }

    #[test]
    fn collapses_line_breaks_and_repeated_spaces() {
        let multiline = "one\r\ntwo\u{2028}three";
        assert_eq!(
            sanitize_display(multiline, MAX_LABEL_CHARS),
            "one two three"
        );
        assert_eq!(sanitize_display("  a   b  ", MAX_LABEL_CHARS), "a b");
    }

    #[test]
    fn strips_bidi_controls_that_could_reorder_text() {
        // RLO ... PDF around reversed text would otherwise render as "txet".
        assert_eq!(
            sanitize_display("safe\u{202E}txet\u{202C}", MAX_LABEL_CHARS),
            "safetxet"
        );
        // LRM / RLM / ALM / isolates.
        assert_eq!(
            sanitize_display("left\u{200F}right\u{200E}\u{061C}", MAX_LABEL_CHARS),
            "leftright"
        );
        assert_eq!(
            sanitize_display("a\u{2066}b\u{2069}c", MAX_LABEL_CHARS),
            "abc"
        );
        assert_eq!(
            sanitize_display("x\u{202A}y\u{202B}z\u{202D}", MAX_LABEL_CHARS),
            "xyz"
        );
    }

    #[test]
    fn strips_default_ignorable_code_points() {
        assert_eq!(sanitize_display("co\u{00AD}de", MAX_LABEL_CHARS), "code");
        assert_eq!(
            sanitize_display("a\u{200B}\u{200C}\u{200D}b", MAX_LABEL_CHARS),
            "ab"
        );
        assert_eq!(
            sanitize_display("word\u{2060}joiner", MAX_LABEL_CHARS),
            "wordjoiner"
        );
        assert_eq!(sanitize_display("\u{FEFF}bom", MAX_LABEL_CHARS), "bom");
        assert_eq!(
            sanitize_display("e\u{FE0F}motion", MAX_LABEL_CHARS),
            "emotion"
        );
        assert_eq!(
            sanitize_display("tag\u{E0061}\u{E007F}end", MAX_LABEL_CHARS),
            "tagend"
        );
        assert_eq!(
            sanitize_display("filler\u{3164}text", MAX_LABEL_CHARS),
            "fillertext"
        );
    }

    #[test]
    fn preserves_ordinary_combining_marks() {
        // U+0301 is a combining acute accent (Mn), not default-ignorable.
        assert_eq!(sanitize_display("e\u{0301}", MAX_LABEL_CHARS), "e\u{0301}");
    }
}
