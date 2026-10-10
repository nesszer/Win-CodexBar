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
    fn sanitizes_display_text() {
        let rows = [
            // Terminal escapes (CSI, OSC).
            ("\u{1b}[31mred\u{1b}[0m", "red"),
            ("a\u{1b}]0;ignored\u{07}b", "ab"),
            // Line breaks and repeated spaces collapse.
            ("one\r\ntwo\u{2028}three", "one two three"),
            ("  a   b  ", "a b"),
            // Bidi controls: RLO ... PDF around reversed text would otherwise render
            // as "txet"; then LRM / RLM / ALM / isolates.
            ("safe\u{202E}txet\u{202C}", "safetxet"),
            ("left\u{200F}right\u{200E}\u{061C}", "leftright"),
            ("a\u{2066}b\u{2069}c", "abc"),
            ("x\u{202A}y\u{202B}z\u{202D}", "xyz"),
            // Default-ignorable code points.
            ("co\u{00AD}de", "code"),
            ("a\u{200B}\u{200C}\u{200D}b", "ab"),
            ("word\u{2060}joiner", "wordjoiner"),
            ("\u{FEFF}bom", "bom"),
            ("e\u{FE0F}motion", "emotion"),
            ("tag\u{E0061}\u{E007F}end", "tagend"),
            ("filler\u{3164}text", "fillertext"),
            // U+0301 is a combining acute accent (Mn), not default-ignorable.
            ("e\u{0301}", "e\u{0301}"),
        ];
        for (input, expected) in rows {
            assert_eq!(
                sanitize_display(input, MAX_LABEL_CHARS),
                expected,
                "{input:?}"
            );
        }
    }

    #[test]
    fn bounds_display_length() {
        let long = "x".repeat(MAX_LABEL_CHARS + 50);
        assert_eq!(
            sanitize_display(&long, MAX_LABEL_CHARS).chars().count(),
            MAX_LABEL_CHARS
        );
    }
}
