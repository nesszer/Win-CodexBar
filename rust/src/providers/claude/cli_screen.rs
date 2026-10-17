//! Virtual screen replay for Claude CLI `/usage` captures.
//!
//! Claude's terminal UI redraws panels differentially: it moves the cursor and
//! rewrites only the cells that changed. Stripping escape sequences from such a
//! capture glues unrelated fragments together (`51%us` + jump + `d` becomes
//! `51%usd`). Replaying the cursor and erase operations onto a bounded screen
//! that matches the PTY geometry keeps the unchanged cells of earlier frames.
//!
//! Ported from upstream `ClaudeCLIScreen.swift` (v0.65.0). The screen has no
//! scrollback: erased or scrolled-off content is never reused.

/// Screen width, equal to the PTY columns in `cli::tty_runner`.
pub(super) const COLUMNS: usize = 160;
/// Screen height, equal to the rows of the Claude usage PTY (`claude/mod.rs`).
/// Claude Code 2.1.294's `/usage` panel adds session stats above the quota rows
/// and an insights list below them, about 100 rows for a heavy user. The inline
/// renderer scrolls rows past the top of a shorter PTY, and those rows are
/// missing from the replayed screen, "Current session" first. Keep the PTY well
/// taller than the panel. Upstream `ClaudeCLIScreen.rows`.
pub(super) const ROWS: usize = 200;
/// Largest CSI parameter kept before arithmetic; larger values clamp here.
const MAX_PARAMETER: usize = if COLUMNS > ROWS { COLUMNS } else { ROWS };

/// Replay `text` onto a 160x200 screen and return the visible rows.
///
/// With `preserve_plain_reports`, a capture that never moves the cursor or
/// erases cells (and has no backspace) is returned as its original text with
/// escape sequences removed: color-only or CR-delimited reports are neither
/// clipped nor wrapped to the PTY geometry. Replay takes over at the first
/// cursor or erase sequence, or backspace.
pub(super) fn render(text: &str, preserve_plain_reports: bool) -> String {
    let mut screen = Screen::new();
    let mut replay = !preserve_plain_reports;
    let mut plain = String::new();
    for token in tokens(text) {
        match token {
            Token::Csi(sequence) => {
                if screen.apply_csi(sequence) {
                    replay = true;
                }
            }
            Token::Text(run) => {
                if !replay {
                    plain.push_str(run);
                    replay = run.contains('\u{8}');
                }
                for cluster in Clusters::new(run) {
                    screen.write(&cluster);
                }
            }
        }
    }
    if replay { screen.into_text() } else { plain }
}

enum Token<'a> {
    /// Parameter, intermediate and final bytes after `ESC [`.
    Csi(&'a str),
    /// A run of non-escape characters.
    Text(&'a str),
}

/// Split a capture into CSI sequences and text runs. OSC strings and other
/// escape sequences are consumed whole and produce no token.
///
/// Mirrors upstream's pattern
/// `ESC(?:\[[0-?]*[ -/]*[@-~]?|\][\s\S]*?(?:BEL|ESC\\|$)|.)|[^ESC]+`, where
/// `.` does not match a line terminator and `$` also matches before a final one.
fn tokens(text: &str) -> impl Iterator<Item = Token<'_>> {
    let mut rest = text;
    std::iter::from_fn(move || {
        loop {
            if rest.is_empty() {
                return None;
            }
            let Some(after_escape) = rest.strip_prefix('\u{1b}') else {
                let end = rest.find('\u{1b}').unwrap_or(rest.len());
                let (run, tail) = rest.split_at(end);
                rest = tail;
                return Some(Token::Text(run));
            };
            if let Some(body) = after_escape.strip_prefix('[') {
                let end = csi_length(body);
                let (sequence, tail) = body.split_at(end);
                rest = tail;
                return Some(Token::Csi(sequence));
            }
            if let Some(body) = after_escape.strip_prefix(']') {
                rest = &body[osc_length(body)..];
                continue;
            }
            match after_escape.chars().next() {
                Some(next) if !is_line_terminator(next) => {
                    rest = &after_escape[next.len_utf8()..];
                }
                // A lone escape before a line terminator or at the end is dropped.
                _ => rest = after_escape,
            }
        }
    })
}

/// Byte length of `[0-?]*[ -/]*[@-~]?` at the start of `body`.
fn csi_length(body: &str) -> usize {
    let bytes = body.as_bytes();
    let mut end = 0;
    while end < bytes.len() && (0x30..=0x3F).contains(&bytes[end]) {
        end += 1;
    }
    while end < bytes.len() && (0x20..=0x2F).contains(&bytes[end]) {
        end += 1;
    }
    if end < bytes.len() && (0x40..=0x7E).contains(&bytes[end]) {
        end += 1;
    }
    end
}

/// Byte length of an OSC string body including its terminator (BEL or `ESC \`).
/// An unterminated string runs to the end of the input, or to a final line
/// terminator, which stays visible text.
fn osc_length(body: &str) -> usize {
    for (index, c) in body.char_indices() {
        match c {
            '\u{7}' => return index + 1,
            '\u{1b}' if body[index + 1..].starts_with('\\') => return index + 2,
            _ => {}
        }
    }
    let trimmed = body
        .strip_suffix("\r\n")
        .or_else(|| body.strip_suffix(is_line_terminator))
        .unwrap_or(body);
    trimmed.len()
}

fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{85}' | '\u{2028}' | '\u{2029}')
}

#[derive(Clone, PartialEq, Eq)]
enum Cell {
    Blank,
    Glyph(String),
    /// Right half of a double-width glyph.
    Trail,
}

struct Screen {
    lines: Vec<Vec<Cell>>,
    row: usize,
    column: usize,
}

impl Screen {
    fn new() -> Self {
        Self {
            lines: vec![vec![Cell::Blank; COLUMNS]; ROWS],
            row: 0,
            column: 0,
        }
    }

    fn into_text(self) -> String {
        let mut rows: Vec<String> = self
            .lines
            .into_iter()
            .map(|mut line| {
                while line.last() == Some(&Cell::Blank) {
                    line.pop();
                }
                line.iter()
                    .map(|cell| match cell {
                        Cell::Blank => " ",
                        Cell::Glyph(glyph) => glyph,
                        Cell::Trail => "",
                    })
                    .collect()
            })
            .collect();
        while rows.last().is_some_and(String::is_empty) {
            rows.pop();
        }
        rows.join("\n")
    }

    fn newline(&mut self) {
        self.column = 0;
        self.row += 1;
        if self.row == ROWS {
            self.lines.remove(0);
            self.lines.push(vec![Cell::Blank; COLUMNS]);
            self.row -= 1;
        }
    }

    fn write(&mut self, cluster: &str) {
        match cluster {
            "\r" => self.column = 0,
            "\n" | "\r\n" => self.newline(),
            "\u{8}" => self.column = self.column.min(COLUMNS - 1).saturating_sub(1),
            "\t" => self.column = (COLUMNS - 1).min((self.column / 8 + 1) * 8),
            _ => self.write_glyph(cluster),
        }
    }

    fn write_glyph(&mut self, cluster: &str) {
        if !cluster.chars().all(|c| c >= ' ' && c != '\u{7f}') {
            return;
        }
        let width = cell_width(cluster);
        if width == 0 {
            self.attach_combining(cluster);
            return;
        }
        if self.column + width > COLUMNS {
            self.newline();
        }
        for cell in self.column..self.column + width {
            self.clear_cell(self.row, cell);
        }
        let glyph = if cluster == " " {
            Cell::Blank
        } else {
            Cell::Glyph(cluster.to_string())
        };
        self.lines[self.row][self.column] = glyph;
        if width == 2 {
            self.lines[self.row][self.column + 1] = Cell::Trail;
        }
        self.column += width;
    }

    /// A zero-width cluster joins the previous cell when it is a combining
    /// mark or joiner; other zero-width characters are dropped.
    fn attach_combining(&mut self, cluster: &str) {
        let Some(first) = cluster.chars().next() else {
            return;
        };
        if !is_extend(first) {
            return;
        }
        let Some(mut previous) = self.column.min(COLUMNS).checked_sub(1) else {
            return;
        };
        if self.lines[self.row][previous] == Cell::Trail {
            let Some(lead) = previous.checked_sub(1) else {
                return;
            };
            previous = lead;
        }
        let joined = match &self.lines[self.row][previous] {
            Cell::Blank => format!(" {cluster}"),
            Cell::Glyph(glyph) => format!("{glyph}{cluster}"),
            Cell::Trail => return,
        };
        self.lines[self.row][previous] = Cell::Glyph(joined);
    }

    /// Apply a cursor or erase sequence. Returns false when the sequence is
    /// not one the replay handles (colors, modes, private sequences, ...).
    fn apply_csi(&mut self, sequence: &str) -> bool {
        let Some(command) = sequence.chars().next_back() else {
            return false;
        };
        if !"ABCDGHfKJX".contains(command) {
            return false;
        }
        let parameters = &sequence[..sequence.len() - command.len_utf8()];
        if !parameters.bytes().all(|b| b.is_ascii_digit() || b == b';') {
            return false;
        }
        // At most two parameters, each clamped to the larger screen dimension
        // before any arithmetic, even for overflowing decimal input.
        let values: Vec<usize> = parameters
            .splitn(3, ';')
            .take(2)
            .map(|value| match value {
                "" => 0,
                digits => digits
                    .parse()
                    .map_or(MAX_PARAMETER, |n: usize| n.min(MAX_PARAMETER)),
            })
            .collect();
        let mode = values.first().copied().unwrap_or(0);
        let amount = mode.max(1);
        if matches!(command, 'K' | 'J') && mode > 2 {
            return false;
        }
        self.column = self.column.min(COLUMNS - 1);
        match command {
            'A' => self.row = self.row.saturating_sub(amount),
            'B' => self.row = (ROWS - 1).min(self.row + amount),
            'C' => self.column = (COLUMNS - 1).min(self.column + amount),
            'D' => self.column = self.column.saturating_sub(amount),
            'G' => self.column = (COLUMNS - 1).min(amount - 1),
            'H' | 'f' => {
                self.row = (ROWS - 1).min(amount - 1);
                let column = values.get(1).copied().unwrap_or(0).max(1);
                self.column = (COLUMNS - 1).min(column - 1);
            }
            // `ECH`: blank `amount` cells from the cursor without moving it.
            // ConPTY redraws rows with it, so ignoring it leaves the previous
            // frame's characters between words.
            'X' => {
                for column in self.column..(self.column + amount).min(COLUMNS) {
                    self.clear_cell(self.row, column);
                }
            }
            _ => self.erase(command == 'K', mode),
        }
        true
    }

    /// `EL` (`K`, line) or `ED` (`J`, screen) with mode 0 (cursor to end),
    /// 1 (start to cursor) or 2 (all).
    fn erase(&mut self, line_only: bool, mode: usize) {
        let cursor = self.row * COLUMNS + self.column;
        let (start, end) = if line_only {
            (self.row * COLUMNS, (self.row + 1) * COLUMNS)
        } else {
            (0, ROWS * COLUMNS)
        };
        let first = if mode == 0 { cursor } else { start };
        let last = if mode == 1 { cursor + 1 } else { end };
        for cell in first..last {
            self.clear_cell(cell / COLUMNS, cell % COLUMNS);
        }
    }

    /// Blank a cell, also blanking the other half of a wide glyph it overlaps.
    fn clear_cell(&mut self, row: usize, column: usize) {
        let line = &mut self.lines[row];
        if column > 0 && line[column] == Cell::Trail {
            line[column - 1] = Cell::Blank;
        }
        if column + 1 < COLUMNS && line[column + 1] == Cell::Trail {
            line[column + 1] = Cell::Blank;
        }
        line[column] = Cell::Blank;
    }
}

/// Grapheme-like clusters of a text run: a base character with its combining
/// marks, joiner sequences, emoji modifiers and regional-indicator pairs, plus
/// `CR LF` as one unit. The crate has no grapheme segmentation dependency, so
/// this covers the sequences terminal panels emit rather than full UAX #29.
struct Clusters<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
}

impl<'a> Clusters<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            chars: text.chars().peekable(),
        }
    }
}

impl Iterator for Clusters<'_> {
    type Item = String;

    fn next(&mut self) -> Option<String> {
        let first = self.chars.next()?;
        let mut cluster = String::from(first);
        if first == '\r' {
            if self.chars.next_if_eq(&'\n').is_some() {
                cluster.push('\n');
            }
            return Some(cluster);
        }
        if first.is_control() {
            return Some(cluster);
        }
        let mut last = first;
        if is_regional_indicator(first)
            && let Some(second) = self.chars.next_if(|c| is_regional_indicator(*c))
        {
            cluster.push(second);
            last = second;
        }
        while let Some(&next) = self.chars.peek() {
            let joins = is_extend(next)
                || is_emoji_modifier(next)
                || (last == '\u{200D}' && is_pictographic(next));
            if !joins {
                break;
            }
            cluster.push(next);
            last = next;
            self.chars.next();
        }
        Some(cluster)
    }
}

/// Number of cells a cluster occupies: 0 for bare combining marks, 2 for wide
/// and emoji-presentation glyphs, otherwise 1 (usage-bar blocks stay 1).
fn cell_width(cluster: &str) -> usize {
    let mut chars = cluster.chars();
    if let (Some(only), None) = (chars.next(), chars.next())
        && only.is_ascii()
    {
        return 1;
    }
    let Some(base) = cluster.chars().find(|c| !is_zero_width(*c)) else {
        return 0;
    };
    let emoji_presentation = cluster.chars().any(|c| in_ranges(c, EMOJI_PRESENTATION));
    let emoji_with_selector = in_ranges(base, EMOJI) && cluster.contains('\u{FE0F}');
    if emoji_presentation || emoji_with_selector || in_ranges(base, WIDE) {
        2
    } else {
        1
    }
}

type Ranges = &'static [(u32, u32)];

fn in_ranges(c: char, ranges: Ranges) -> bool {
    let value = c as u32;
    ranges
        .binary_search_by(|&(low, high)| {
            if value < low {
                std::cmp::Ordering::Greater
            } else if value > high {
                std::cmp::Ordering::Less
            } else {
                std::cmp::Ordering::Equal
            }
        })
        .is_ok()
}

/// Nonspacing and enclosing marks plus the joiners and selectors that extend
/// the preceding character (Unicode `Grapheme_Extend`, approximated by block).
fn is_extend(c: char) -> bool {
    in_ranges(c, EXTEND)
}

/// Marks, joiners and other format characters (general category Mn, Me, Cf).
fn is_zero_width(c: char) -> bool {
    is_extend(c) || in_ranges(c, FORMAT)
}

fn is_emoji_modifier(c: char) -> bool {
    ('\u{1F3FB}'..='\u{1F3FF}').contains(&c)
}

fn is_regional_indicator(c: char) -> bool {
    ('\u{1F1E6}'..='\u{1F1FF}').contains(&c)
}

/// Characters that may follow a zero-width joiner inside an emoji sequence.
fn is_pictographic(c: char) -> bool {
    matches!(c as u32, 0x2190..=0x2BFF | 0x1F000..=0x1FAFF)
}

// Sorted, non-overlapping code point ranges for `in_ranges`.

const EXTEND: Ranges = &[
    (0x0300, 0x036F),
    (0x0483, 0x0489),
    (0x0591, 0x05BD),
    (0x05BF, 0x05BF),
    (0x05C1, 0x05C2),
    (0x05C4, 0x05C5),
    (0x05C7, 0x05C7),
    (0x0610, 0x061A),
    (0x064B, 0x065F),
    (0x0670, 0x0670),
    (0x06D6, 0x06DC),
    (0x06DF, 0x06E4),
    (0x06E7, 0x06E8),
    (0x06EA, 0x06ED),
    (0x0E31, 0x0E31),
    (0x0E34, 0x0E3A),
    (0x0E47, 0x0E4E),
    (0x1AB0, 0x1AFF),
    (0x1DC0, 0x1DFF),
    (0x200C, 0x200D),
    (0x20D0, 0x20F0),
    (0x302A, 0x302F),
    (0x3099, 0x309A),
    (0xFE00, 0xFE0F),
    (0xFE20, 0xFE2F),
    (0xE0020, 0xE007F),
    (0xE0100, 0xE01EF),
];

const FORMAT: Ranges = &[
    (0x00AD, 0x00AD),
    (0x0600, 0x0605),
    (0x061C, 0x061C),
    (0x06DD, 0x06DD),
    (0x070F, 0x070F),
    (0x200B, 0x200B),
    (0x200E, 0x200F),
    (0x202A, 0x202E),
    (0x2060, 0x2064),
    (0x2066, 0x206F),
    (0xFEFF, 0xFEFF),
    (0xE0001, 0xE0001),
];

/// Wide and fullwidth blocks, as listed upstream (block characters used by
/// usage bars are deliberately absent and stay one cell).
const WIDE: Ranges = &[
    (0x1100, 0x115F),
    (0x2E80, 0x303E),
    (0x3041, 0x33FF),
    (0x3400, 0x4DBF),
    (0x4E00, 0x9FFF),
    (0xA000, 0xA4CF),
    (0xA960, 0xA97F),
    (0xAC00, 0xD7A3),
    (0xF900, 0xFAFF),
    (0xFE10, 0xFE19),
    (0xFE30, 0xFE6F),
    (0xFF00, 0xFF60),
    (0xFFE0, 0xFFE6),
    (0x16FE0, 0x16FE4),
    (0x17000, 0x18AFF),
    (0x1B000, 0x1B2FF),
    (0x1F200, 0x1F251),
    (0x20000, 0x2FFFD),
    (0x30000, 0x3FFFD),
];

/// Unicode `Emoji_Presentation`: rendered as a two-cell emoji by default.
const EMOJI_PRESENTATION: Ranges = &[
    (0x231A, 0x231B),
    (0x23E9, 0x23EC),
    (0x23F0, 0x23F0),
    (0x23F3, 0x23F3),
    (0x25FD, 0x25FE),
    (0x2614, 0x2615),
    (0x2648, 0x2653),
    (0x267F, 0x267F),
    (0x2693, 0x2693),
    (0x26A1, 0x26A1),
    (0x26AA, 0x26AB),
    (0x26BD, 0x26BE),
    (0x26C4, 0x26C5),
    (0x26CE, 0x26CE),
    (0x26D4, 0x26D4),
    (0x26EA, 0x26EA),
    (0x26F2, 0x26F3),
    (0x26F5, 0x26F5),
    (0x26FA, 0x26FA),
    (0x26FD, 0x26FD),
    (0x2705, 0x2705),
    (0x270A, 0x270B),
    (0x2728, 0x2728),
    (0x274C, 0x274C),
    (0x274E, 0x274E),
    (0x2753, 0x2755),
    (0x2757, 0x2757),
    (0x2795, 0x2797),
    (0x27B0, 0x27B0),
    (0x27BF, 0x27BF),
    (0x2B1B, 0x2B1C),
    (0x2B50, 0x2B50),
    (0x2B55, 0x2B55),
    (0x1F004, 0x1F004),
    (0x1F0CF, 0x1F0CF),
    (0x1F18E, 0x1F18E),
    (0x1F191, 0x1F19A),
    (0x1F1E6, 0x1F1FF),
    (0x1F201, 0x1F201),
    (0x1F21A, 0x1F21A),
    (0x1F22F, 0x1F22F),
    (0x1F232, 0x1F236),
    (0x1F238, 0x1F23A),
    (0x1F250, 0x1F251),
    (0x1F300, 0x1F320),
    (0x1F32D, 0x1F335),
    (0x1F337, 0x1F37C),
    (0x1F37E, 0x1F393),
    (0x1F3A0, 0x1F3CA),
    (0x1F3CF, 0x1F3D3),
    (0x1F3E0, 0x1F3F0),
    (0x1F3F4, 0x1F3F4),
    (0x1F3F8, 0x1F43E),
    (0x1F440, 0x1F440),
    (0x1F442, 0x1F4FC),
    (0x1F4FF, 0x1F53D),
    (0x1F54B, 0x1F54E),
    (0x1F550, 0x1F567),
    (0x1F57A, 0x1F57A),
    (0x1F595, 0x1F596),
    (0x1F5A4, 0x1F5A4),
    (0x1F5FB, 0x1F64F),
    (0x1F680, 0x1F6C5),
    (0x1F6CC, 0x1F6CC),
    (0x1F6D0, 0x1F6D2),
    (0x1F6D5, 0x1F6D7),
    (0x1F6DC, 0x1F6DF),
    (0x1F6EB, 0x1F6EC),
    (0x1F6F4, 0x1F6FC),
    (0x1F7E0, 0x1F7EB),
    (0x1F7F0, 0x1F7F0),
    (0x1F90C, 0x1F93A),
    (0x1F93C, 0x1F945),
    (0x1F947, 0x1F9FF),
    (0x1FA70, 0x1FA7C),
    (0x1FA80, 0x1FA89),
    (0x1FA8F, 0x1FAC6),
    (0x1FACE, 0x1FADC),
    (0x1FADF, 0x1FAE9),
    (0x1FAF0, 0x1FAF8),
];

/// Unicode `Emoji`: characters that become two-cell emoji when followed by
/// U+FE0F (text-presentation symbols such as the warning sign).
const EMOJI: Ranges = &[
    (0x0023, 0x0023),
    (0x002A, 0x002A),
    (0x0030, 0x0039),
    (0x00A9, 0x00A9),
    (0x00AE, 0x00AE),
    (0x203C, 0x203C),
    (0x2049, 0x2049),
    (0x2122, 0x2122),
    (0x2139, 0x2139),
    (0x2194, 0x2199),
    (0x21A9, 0x21AA),
    (0x231A, 0x231B),
    (0x2328, 0x2328),
    (0x23CF, 0x23CF),
    (0x23E9, 0x23F3),
    (0x23F8, 0x23FA),
    (0x24C2, 0x24C2),
    (0x25AA, 0x25AB),
    (0x25B6, 0x25B6),
    (0x25C0, 0x25C0),
    (0x25FB, 0x25FE),
    (0x2600, 0x2604),
    (0x260E, 0x260E),
    (0x2611, 0x2611),
    (0x2614, 0x2615),
    (0x2618, 0x2618),
    (0x261D, 0x261D),
    (0x2620, 0x2620),
    (0x2622, 0x2623),
    (0x2626, 0x2626),
    (0x262A, 0x262A),
    (0x262E, 0x262F),
    (0x2638, 0x263A),
    (0x2640, 0x2640),
    (0x2642, 0x2642),
    (0x2648, 0x2653),
    (0x265F, 0x2660),
    (0x2663, 0x2663),
    (0x2665, 0x2666),
    (0x2668, 0x2668),
    (0x267B, 0x267B),
    (0x267E, 0x267F),
    (0x2692, 0x2697),
    (0x2699, 0x2699),
    (0x269B, 0x269C),
    (0x26A0, 0x26A1),
    (0x26A7, 0x26A7),
    (0x26AA, 0x26AB),
    (0x26B0, 0x26B1),
    (0x26BD, 0x26BE),
    (0x26C4, 0x26C5),
    (0x26C8, 0x26C8),
    (0x26CE, 0x26CF),
    (0x26D1, 0x26D1),
    (0x26D3, 0x26D4),
    (0x26E9, 0x26EA),
    (0x26F0, 0x26F5),
    (0x26F7, 0x26FA),
    (0x26FD, 0x26FD),
    (0x2702, 0x2702),
    (0x2705, 0x2705),
    (0x2708, 0x270D),
    (0x270F, 0x270F),
    (0x2712, 0x2712),
    (0x2714, 0x2714),
    (0x2716, 0x2716),
    (0x271D, 0x271D),
    (0x2721, 0x2721),
    (0x2728, 0x2728),
    (0x2733, 0x2734),
    (0x2744, 0x2744),
    (0x2747, 0x2747),
    (0x274C, 0x274C),
    (0x274E, 0x274E),
    (0x2753, 0x2755),
    (0x2757, 0x2757),
    (0x2763, 0x2764),
    (0x2795, 0x2797),
    (0x27A1, 0x27A1),
    (0x27B0, 0x27B0),
    (0x27BF, 0x27BF),
    (0x2934, 0x2935),
    (0x2B05, 0x2B07),
    (0x2B1B, 0x2B1C),
    (0x2B50, 0x2B50),
    (0x2B55, 0x2B55),
    (0x3030, 0x3030),
    (0x303D, 0x303D),
    (0x3297, 0x3297),
    (0x3299, 0x3299),
    (0x1F004, 0x1F004),
    (0x1F0CF, 0x1F0CF),
    (0x1F170, 0x1F251),
    (0x1F300, 0x1FAFF),
];

#[cfg(test)]
mod tests;
