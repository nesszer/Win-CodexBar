//! Pixel-level tray icon renderer, decoupled from any platform icon API.
//!
//! Returns raw RGBA bytes so callers (egui tray manager, Tauri shell, tests)
//! can adapt the result to their own icon type without pulling in extra deps.

use image::{ImageBuffer, Rgba, RgbaImage};

use super::icon::UsageLevel;

/// Side length of the generated tray icon in pixels.
pub const TRAY_ICON_SIZE: u32 = 32;

/// Optional pace tint for a tray icon's usage indicator.
///
/// The upstream pace treatment uses green when usage is behind the expected
/// pace and red when it is ahead. On-track and unavailable stages keep the
/// normal usage-level colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayPaceColor {
    Behind,
    Ahead,
}

impl TrayPaceColor {
    /// Map the canonical bridge pace stage to its tray tint.
    pub fn from_stage(stage: &str) -> Option<Self> {
        match stage {
            "slightly_behind" | "behind" | "far_behind" => Some(Self::Behind),
            "slightly_ahead" | "ahead" | "far_ahead" => Some(Self::Ahead),
            _ => None,
        }
    }

    fn rgb(self) -> (u8, u8, u8) {
        match self {
            Self::Behind => (76, 175, 80),
            Self::Ahead => (244, 67, 54),
        }
    }
}

const ICON_INSET: u32 = 2;
const BAR_LEFT: u32 = 4;
const BAR_RIGHT: u32 = TRAY_ICON_SIZE - 4;
const ICON_BACKGROUND_RGB: [u8; 3] = [60, 60, 70];
const BAR_BACKGROUND: Rgba<u8> = Rgba([80, 80, 90, 255]);

fn new_icon_canvas(has_error: bool) -> RgbaImage {
    let mut image: RgbaImage = ImageBuffer::new(TRAY_ICON_SIZE, TRAY_ICON_SIZE);
    let background = Rgba([
        ICON_BACKGROUND_RGB[0],
        ICON_BACKGROUND_RGB[1],
        ICON_BACKGROUND_RGB[2],
        if has_error { 180 } else { 255 },
    ]);
    for y in ICON_INSET..TRAY_ICON_SIZE - ICON_INSET {
        for x in ICON_INSET..TRAY_ICON_SIZE - ICON_INSET {
            image.put_pixel(x, y, background);
        }
    }
    image
}

/// Minimum WCAG contrast ratio a provider accent needs against the tray canvas
/// (upstream "Color by provider" uses the same 2:1 floor).
const PROVIDER_TINT_MIN_CONTRAST: f64 = 2.0;

fn relative_luminance((r, g, b): (u8, u8, u8)) -> f64 {
    fn channel(value: u8) -> f64 {
        let c = f64::from(value) / 255.0;
        if c <= 0.03928 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    }
    0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
}

/// Whether a provider accent stays readable on the tray canvas. Accents below
/// the floor fall back to the usage-level colours, as upstream does.
fn provider_tint_is_legible(rgb: (u8, u8, u8)) -> bool {
    let tint = relative_luminance(rgb);
    let canvas = relative_luminance((
        ICON_BACKGROUND_RGB[0],
        ICON_BACKGROUND_RGB[1],
        ICON_BACKGROUND_RGB[2],
    ));
    let (lighter, darker) = if tint >= canvas {
        (tint, canvas)
    } else {
        (canvas, tint)
    };
    (lighter + 0.05) / (darker + 0.05) >= PROVIDER_TINT_MIN_CONTRAST
}

fn usage_color(
    percent: f64,
    has_error: bool,
    pace_color: Option<TrayPaceColor>,
    provider_tint: Option<(u8, u8, u8)>,
) -> Rgba<u8> {
    // Pace colours keep their own green/red meaning, so they win over the
    // accent. Error (stale) icons keep the desaturated look, so no accent.
    let provider_tint = provider_tint.filter(|rgb| !has_error && provider_tint_is_legible(*rgb));
    let (r, g, b) = pace_color
        .map(TrayPaceColor::rgb)
        .or(provider_tint)
        .unwrap_or_else(|| UsageLevel::from_percent(percent).color());
    if has_error {
        #[allow(
            clippy::cast_possible_truncation,
            reason = "mean of three u8 channels is bounded to 0..=255"
        )]
        let gray = ((r as u16 + g as u16 + b as u16) / 3) as u8;
        Rgba([gray, gray, gray, 255])
    } else {
        Rgba([r, g, b, 255])
    }
}

fn draw_bar_row(
    image: &mut RgbaImage,
    y_start: u32,
    y_end: u32,
    percent: f64,
    has_error: bool,
    pace_color: Option<TrayPaceColor>,
    provider_tint: Option<(u8, u8, u8)>,
) {
    let bar_width = BAR_RIGHT - BAR_LEFT;
    #[allow(
        clippy::cast_possible_truncation,
        reason = "percent is clamped to 0..=100 and scaled to a 24-pixel meter"
    )]
    let fill = ((percent.clamp(0.0, 100.0) / 100.0) * bar_width as f64) as u32;
    let fill_end = (BAR_LEFT + fill).min(BAR_RIGHT);
    let color = usage_color(percent, has_error, pace_color, provider_tint);

    for y in y_start..y_end {
        for x in BAR_LEFT..BAR_RIGHT {
            image.put_pixel(x, y, BAR_BACKGROUND);
        }
        for x in BAR_LEFT..fill_end {
            image.put_pixel(x, y, color);
        }
    }
}

/// Render a usage-bar tray icon as raw RGBA bytes.
///
/// - `session_percent`: primary bar fill (0–100), colour-coded by [`UsageLevel`]
/// - `weekly_percent`: optional secondary bar fill (0–100). When `Some`, two thin
///   bars are drawn (session top, weekly bottom). When `None`, a single thick bar
///   is drawn instead.
/// - `has_error`: desaturate all bar colours to grey to signal an error/unknown state.
///
/// Returns `(rgba_bytes, width, height)` for a [`TRAY_ICON_SIZE`]×[`TRAY_ICON_SIZE`] icon.
pub fn render_bar_icon_rgba(
    session_percent: f64,
    weekly_percent: Option<f64>,
    has_error: bool,
) -> (Vec<u8>, u32, u32) {
    render_bar_icon_rgba_with_pace(session_percent, weekly_percent, has_error, None)
}

/// Render a usage-bar tray icon with an optional pace tint.
pub fn render_bar_icon_rgba_with_pace(
    session_percent: f64,
    weekly_percent: Option<f64>,
    has_error: bool,
    pace_color: Option<TrayPaceColor>,
) -> (Vec<u8>, u32, u32) {
    render_bar_icon_rgba_tinted(session_percent, weekly_percent, has_error, pace_color, None)
}

/// Render a usage-bar tray icon with an optional pace tint and an optional
/// provider accent (upstream "Color by provider"). Pace wins over the accent;
/// error icons ignore the accent; a low-contrast accent falls back to the
/// usage-level colours.
pub fn render_bar_icon_rgba_tinted(
    session_percent: f64,
    weekly_percent: Option<f64>,
    has_error: bool,
    pace_color: Option<TrayPaceColor>,
    provider_tint: Option<(u8, u8, u8)>,
) -> (Vec<u8>, u32, u32) {
    let mut image = new_icon_canvas(has_error);

    match weekly_percent {
        Some(weekly) => {
            draw_bar_row(
                &mut image,
                8,
                15,
                session_percent,
                has_error,
                pace_color,
                provider_tint,
            );
            draw_bar_row(
                &mut image,
                18,
                23,
                weekly,
                has_error,
                pace_color,
                provider_tint,
            );
        }
        None => {
            draw_bar_row(
                &mut image,
                10,
                22,
                session_percent,
                has_error,
                pace_color,
                provider_tint,
            );
        }
    }

    (image.into_raw(), TRAY_ICON_SIZE, TRAY_ICON_SIZE)
}

/// Render two providers as equally prominent stacked usage meters.
///
/// Unlike [`render_bar_icon_rgba`], both rows represent the selected metric
/// for separate providers. The upper and lower rows therefore use equal
/// height so neither provider is presented as a secondary quota window.
pub fn render_stacked_bar_icon_rgba(
    top_percent: f64,
    bottom_percent: f64,
    has_error: bool,
) -> (Vec<u8>, u32, u32) {
    render_stacked_bar_icon_rgba_tinted(top_percent, bottom_percent, has_error, None, None)
}

/// Render two stacked provider meters, each with its own optional accent.
pub fn render_stacked_bar_icon_rgba_tinted(
    top_percent: f64,
    bottom_percent: f64,
    has_error: bool,
    top_tint: Option<(u8, u8, u8)>,
    bottom_tint: Option<(u8, u8, u8)>,
) -> (Vec<u8>, u32, u32) {
    let mut image = new_icon_canvas(has_error);
    draw_bar_row(&mut image, 6, 14, top_percent, has_error, None, top_tint);
    draw_bar_row(
        &mut image,
        18,
        26,
        bottom_percent,
        has_error,
        None,
        bottom_tint,
    );
    (image.into_raw(), TRAY_ICON_SIZE, TRAY_ICON_SIZE)
}

/// Render the session and weekly meters of one provider as two equal-height
/// rows (upstream "Progress bars stacked", #4436), with the same optional pace
/// tint as [`render_bar_icon_rgba_with_pace`].
///
/// With no weekly window the output is exactly the single thick bar from
/// [`render_bar_icon_rgba_with_pace`], so a provider with one window looks the
/// same under either bar style.
pub fn render_stacked_bars_icon_rgba_with_pace(
    session_percent: f64,
    weekly_percent: Option<f64>,
    has_error: bool,
    pace_color: Option<TrayPaceColor>,
) -> (Vec<u8>, u32, u32) {
    render_stacked_bars_icon_rgba_tinted(
        session_percent,
        weekly_percent,
        has_error,
        pace_color,
        None,
    )
}

/// [`render_stacked_bars_icon_rgba_with_pace`] with an optional provider
/// accent on both rows, following the same rules as
/// [`render_bar_icon_rgba_tinted`].
pub fn render_stacked_bars_icon_rgba_tinted(
    session_percent: f64,
    weekly_percent: Option<f64>,
    has_error: bool,
    pace_color: Option<TrayPaceColor>,
    provider_tint: Option<(u8, u8, u8)>,
) -> (Vec<u8>, u32, u32) {
    let Some(weekly) = weekly_percent else {
        return render_bar_icon_rgba_tinted(
            session_percent,
            None,
            has_error,
            pace_color,
            provider_tint,
        );
    };
    let mut image = new_icon_canvas(has_error);
    draw_bar_row(
        &mut image,
        6,
        14,
        session_percent,
        has_error,
        pace_color,
        provider_tint,
    );
    draw_bar_row(
        &mut image,
        18,
        26,
        weekly,
        has_error,
        pace_color,
        provider_tint,
    );
    (image.into_raw(), TRAY_ICON_SIZE, TRAY_ICON_SIZE)
}

/// Render a compact numeric percent tray icon as raw RGBA bytes.
pub fn render_percent_icon_rgba(percent: f64, has_error: bool) -> (Vec<u8>, u32, u32) {
    render_percent_icon_rgba_with_pace(percent, has_error, None)
}

/// Render a compact numeric percent tray icon with an optional pace tint.
pub fn render_percent_icon_rgba_with_pace(
    percent: f64,
    has_error: bool,
    pace_color: Option<TrayPaceColor>,
) -> (Vec<u8>, u32, u32) {
    render_percent_icon_rgba_tinted(percent, has_error, pace_color, None)
}

/// Render a compact numeric percent tray icon with an optional pace tint and
/// an optional provider accent (see [`render_bar_icon_rgba_tinted`]).
pub fn render_percent_icon_rgba_tinted(
    percent: f64,
    has_error: bool,
    pace_color: Option<TrayPaceColor>,
    provider_tint: Option<(u8, u8, u8)>,
) -> (Vec<u8>, u32, u32) {
    const SZ: u32 = TRAY_ICON_SIZE;
    let mut img = new_icon_canvas(has_error);

    // percent clamped to 0–100 before rounding, so the cast to u32 cannot truncate.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "percent is clamped to 0–100 and rounded; the integer fits u32"
    )]
    let pct = percent.clamp(0.0, 100.0).round() as u32;
    let text = if pct >= 100 {
        "100".to_string()
    } else {
        format!("{pct}%")
    };
    let glyph_width = 3u32;
    let glyph_gap = 1u32;
    let scale = if text.len() >= 3 { 2u32 } else { 3u32 };
    // text is "100" or at most "NN%", so its length is ≤ 4 and fits u32.
    #[allow(
        clippy::cast_possible_truncation,
        reason = "text is \"100\" or \"NN%\", so len ≤ 4 and fits u32"
    )]
    let text_len = text.len() as u32;
    let text_width = text_len * glyph_width * scale + (text_len - 1) * glyph_gap;
    let text_height = 5 * scale;
    let start_x = (SZ.saturating_sub(text_width)) / 2;
    let start_y = (SZ.saturating_sub(text_height)) / 2;

    let color = usage_color(percent, has_error, pace_color, provider_tint);

    let mut x = start_x;
    for ch in text.chars() {
        draw_glyph(&mut img, ch, x, start_y, scale, color);
        x += glyph_width * scale + glyph_gap;
    }

    (img.into_raw(), SZ, SZ)
}

fn draw_glyph(img: &mut RgbaImage, ch: char, x: u32, y: u32, scale: u32, color: Rgba<u8>) {
    let Some(rows) = glyph_rows(ch) else {
        return;
    };
    for (row_idx, row) in rows.iter().enumerate() {
        for col in 0..3 {
            let bit = 1 << (2 - col);
            if row & bit == 0 {
                continue;
            }
            for yy in 0..scale {
                for xx in 0..scale {
                    let px = x + col * scale + xx;
                    // row_idx is bounded by the 5-row glyph array, so it fits u32.
                    #[allow(
                        clippy::cast_possible_truncation,
                        reason = "row_idx iterates over a fixed [u8; 5] glyph, so it is 0..5 and fits u32"
                    )]
                    let py = y + row_idx as u32 * scale + yy;
                    if px < TRAY_ICON_SIZE && py < TRAY_ICON_SIZE {
                        img.put_pixel(px, py, color);
                    }
                }
            }
        }
    }
}

fn glyph_rows(ch: char) -> Option<[u8; 5]> {
    Some(match ch {
        '0' => [0b111, 0b101, 0b101, 0b101, 0b111],
        '1' => [0b010, 0b110, 0b010, 0b010, 0b111],
        '2' => [0b111, 0b001, 0b111, 0b100, 0b111],
        '3' => [0b111, 0b001, 0b111, 0b001, 0b111],
        '4' => [0b101, 0b101, 0b111, 0b001, 0b001],
        '5' => [0b111, 0b100, 0b111, 0b001, 0b111],
        '6' => [0b111, 0b100, 0b111, 0b101, 0b111],
        '7' => [0b111, 0b001, 0b010, 0b010, 0b010],
        '8' => [0b111, 0b101, 0b111, 0b101, 0b111],
        '9' => [0b111, 0b101, 0b111, 0b001, 0b111],
        '%' => [0b101, 0b001, 0b010, 0b100, 0b101],
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_produces_correct_dimensions() {
        let (rgba, w, h) = render_bar_icon_rgba(50.0, None, false);
        assert_eq!(w, TRAY_ICON_SIZE);
        assert_eq!(h, TRAY_ICON_SIZE);
        assert_eq!(u32::try_from(rgba.len()).unwrap(), w * h * 4);
    }

    #[test]
    fn render_two_bar_has_correct_size() {
        let (rgba, w, h) = render_bar_icon_rgba(30.0, Some(60.0), false);
        assert_eq!(u32::try_from(rgba.len()).unwrap(), w * h * 4);
    }

    #[test]
    fn single_quota_uses_one_centered_prominent_meter() {
        let (single, _, _) = render_bar_icon_rgba(50.0, None, false);
        let (multiple, _, _) = render_bar_icon_rgba(50.0, Some(25.0), false);

        let pixel = |rgba: &[u8], x: u32, y: u32| {
            let index = ((y * TRAY_ICON_SIZE + x) * 4) as usize;
            [
                rgba[index],
                rgba[index + 1],
                rgba[index + 2],
                rgba[index + 3],
            ]
        };

        // The single-quota layout occupies one centered, thick lane.
        assert_eq!(pixel(&single, 20, 9), [60, 60, 70, 255]);
        assert_eq!(pixel(&single, 20, 10), [80, 80, 90, 255]);
        assert_eq!(pixel(&single, 20, 21), [80, 80, 90, 255]);
        assert_eq!(pixel(&single, 20, 22), [60, 60, 70, 255]);

        // Multiple quotas retain distinct upper and lower lanes.
        assert_eq!(pixel(&multiple, 20, 8), [80, 80, 90, 255]);
        assert_eq!(pixel(&multiple, 20, 15), [60, 60, 70, 255]);
        assert_eq!(pixel(&multiple, 20, 18), [80, 80, 90, 255]);
        assert_eq!(pixel(&multiple, 20, 23), [60, 60, 70, 255]);
    }

    #[test]
    fn zero_fill_gives_gray_only_bar() {
        let (rgba, w, _h) = render_bar_icon_rgba(0.0, None, false);
        // Sample a pixel near the centre of the bar track area (y=16, x=8)
        let idx = ((16 * w + 8) * 4) as usize;
        // Should be the gray track colour, not a usage colour
        assert_eq!(rgba[idx], 80); // R
        assert_eq!(rgba[idx + 1], 80); // G
        assert_eq!(rgba[idx + 2], 90); // B
    }

    #[test]
    fn full_fill_gives_colored_bar() {
        let (rgba, w, _h) = render_bar_icon_rgba(100.0, None, false);
        // At 100% used the bar is at Critical level
        let idx = ((16 * w + 8) * 4) as usize;
        let (er, eg, eb) = UsageLevel::Critical.color();
        assert_eq!(rgba[idx], er);
        assert_eq!(rgba[idx + 1], eg);
        assert_eq!(rgba[idx + 2], eb);
    }

    #[test]
    fn error_state_desaturates_colors() {
        let (normal, _, _) = render_bar_icon_rgba(100.0, None, false);
        let (error, _, _) = render_bar_icon_rgba(100.0, None, true);
        // In error mode all three channels at the filled bar pixel should be equal (grey)
        let idx = ((16 * 32 + 8) * 4) as usize;
        assert_ne!(normal[idx], normal[idx + 1]); // colour has distinct channels
        assert_eq!(error[idx], error[idx + 1]); // grey: R == G
        assert_eq!(error[idx + 1], error[idx + 2]); // grey: G == B
    }

    #[test]
    fn percent_icon_produces_correct_dimensions() {
        let (rgba, w, h) = render_percent_icon_rgba(72.0, false);
        assert_eq!(w, TRAY_ICON_SIZE);
        assert_eq!(h, TRAY_ICON_SIZE);
        assert_eq!(u32::try_from(rgba.len()).unwrap(), w * h * 4);
    }

    #[test]
    fn percent_icon_draws_visible_text() {
        let (rgba, _, _) = render_percent_icon_rgba(72.0, false);
        assert!(
            rgba.as_chunks::<4>()
                .0
                .iter()
                .any(|px| px[3] == 255 && px[0] != 60)
        );
    }

    #[test]
    fn percent_icon_clamps_to_hundred() {
        let (rgba, w, h) = render_percent_icon_rgba(125.0, false);
        assert_eq!(u32::try_from(rgba.len()).unwrap(), w * h * 4);
    }

    #[test]
    fn pace_color_maps_canonical_stage_and_tints_bars() {
        assert_eq!(
            TrayPaceColor::from_stage("slightly_behind"),
            Some(TrayPaceColor::Behind)
        );
        assert_eq!(
            TrayPaceColor::from_stage("far_ahead"),
            Some(TrayPaceColor::Ahead)
        );
        assert_eq!(TrayPaceColor::from_stage("on_track"), None);
        assert_eq!(TrayPaceColor::from_stage("unknown"), None);

        let (behind, _, _) =
            render_bar_icon_rgba_with_pace(100.0, None, false, Some(TrayPaceColor::Behind));
        let (ahead, _, _) =
            render_bar_icon_rgba_with_pace(100.0, None, false, Some(TrayPaceColor::Ahead));
        let idx = ((16 * TRAY_ICON_SIZE + 8) * 4) as usize;
        assert_eq!(&behind[idx..idx + 3], &[76, 175, 80]);
        assert_eq!(&ahead[idx..idx + 3], &[244, 67, 54]);
    }

    #[test]
    fn stacked_provider_icon_uses_equal_separate_rows() {
        let (rgba, width, height) = render_stacked_bar_icon_rgba(100.0, 0.0, false);
        assert_eq!((width, height), (TRAY_ICON_SIZE, TRAY_ICON_SIZE));

        let pixel = |x: u32, y: u32| {
            let index = ((y * width + x) * 4) as usize;
            [
                rgba[index],
                rgba[index + 1],
                rgba[index + 2],
                rgba[index + 3],
            ]
        };
        let (r, g, b) = UsageLevel::Critical.color();
        assert_eq!(pixel(8, 8), [r, g, b, 255]);
        assert_eq!(pixel(8, 20), [80, 80, 90, 255]);
        assert_eq!(pixel(8, 15), [60, 60, 70, 255]);
    }

    #[test]
    fn normal_and_stacked_bars_share_error_color_policy() {
        let (normal, width, _) = render_bar_icon_rgba(100.0, None, true);
        let (stacked, _, _) = render_stacked_bar_icon_rgba(100.0, 0.0, true);
        let pixel = |rgba: &[u8], x: u32, y: u32| {
            let index = ((y * width + x) * 4) as usize;
            [
                rgba[index],
                rgba[index + 1],
                rgba[index + 2],
                rgba[index + 3],
            ]
        };

        assert_eq!(pixel(&normal, 8, 12), pixel(&stacked, 8, 8));
        assert_eq!(pixel(&normal, 8, 12)[0], pixel(&normal, 8, 12)[1]);
    }

    const TEAL: (u8, u8, u8) = (73, 163, 176);

    fn pixel_at(rgba: &[u8], x: u32, y: u32) -> [u8; 4] {
        let index = ((y * TRAY_ICON_SIZE + x) * 4) as usize;
        [
            rgba[index],
            rgba[index + 1],
            rgba[index + 2],
            rgba[index + 3],
        ]
    }

    /// Rows in `rows` whose pixel at column `x` is part of a bar (track or fill),
    /// i.e. not the icon background gap.
    fn painted_rows(rgba: &[u8], x: u32, rows: std::ops::Range<u32>) -> Vec<u32> {
        rows.filter(|&y| pixel_at(rgba, x, y) != [60, 60, 70, 255])
            .collect()
    }

    /// Horizontal extent `(first_x, end_x)` of usage-coloured pixels (neither the
    /// track nor the background) across `rows`, or `None` when nothing is filled.
    fn filled_span(rgba: &[u8], rows: std::ops::Range<u32>) -> Option<(u32, u32)> {
        let mut span: Option<(u32, u32)> = None;
        for y in rows {
            for x in BAR_LEFT..BAR_RIGHT {
                let px = pixel_at(rgba, x, y);
                if px != [80, 80, 90, 255] && px != [60, 60, 70, 255] {
                    span = Some(match span {
                        None => (x, x + 1),
                        Some((first, end)) => (first.min(x), end.max(x + 1)),
                    });
                }
            }
        }
        span
    }

    #[test]
    fn stacked_bars_draws_two_equal_height_rows() {
        let (rgba, width, height) =
            render_stacked_bars_icon_rgba_with_pace(100.0, Some(0.0), false, None);
        assert_eq!((width, height), (TRAY_ICON_SIZE, TRAY_ICON_SIZE));

        let (r, g, b) = UsageLevel::Critical.color();
        // Session row is filled to the end; weekly row is empty track.
        assert_eq!(pixel_at(&rgba, 8, 8), [r, g, b, 255]);
        assert_eq!(pixel_at(&rgba, 8, 20), [80, 80, 90, 255]);
        // Gap between the rows keeps the icon background.
        assert_eq!(pixel_at(&rgba, 8, 15), [60, 60, 70, 255]);

        // Both rows are painted over the same 8-pixel height, separated by a 4-pixel gap.
        let top: Vec<u32> = (6..14).collect();
        let bottom: Vec<u32> = (18..26).collect();
        // Rows 2..30 are the inset canvas; the outer border is transparent.
        assert_eq!(
            painted_rows(&rgba, 20, 2..30),
            [top.clone(), bottom.clone()].concat()
        );
        assert_eq!(top.len(), bottom.len());
    }

    #[test]
    fn stacked_bars_fill_widths_follow_each_window() {
        let (rgba, _, _) = render_stacked_bars_icon_rgba_with_pace(50.0, Some(100.0), false, None);
        // Session at 50% fills half of the 24-px track (x 4..16); weekly fills all of it.
        assert_eq!(filled_span(&rgba, 6..14), Some((BAR_LEFT, BAR_LEFT + 12)));
        assert_eq!(filled_span(&rgba, 18..26), Some((BAR_LEFT, BAR_RIGHT)));
        // Nothing fills the gap between the rows.
        assert_eq!(filled_span(&rgba, 14..18), None);
    }

    #[test]
    fn stacked_bars_without_weekly_matches_single_bars_fallback() {
        let (stacked, _, _) = render_stacked_bars_icon_rgba_with_pace(42.0, None, false, None);
        let (bars, _, _) = render_bar_icon_rgba_with_pace(42.0, None, false, None);
        assert_eq!(stacked, bars);

        let (stacked_err, _, _) = render_stacked_bars_icon_rgba_with_pace(42.0, None, true, None);
        let (bars_err, _, _) = render_bar_icon_rgba_with_pace(42.0, None, true, None);
        assert_eq!(stacked_err, bars_err);
    }

    #[test]
    fn stacked_bars_applies_pace_tint_to_both_rows() {
        let (rgba, _, _) = render_stacked_bars_icon_rgba_with_pace(
            100.0,
            Some(100.0),
            false,
            Some(TrayPaceColor::Behind),
        );
        assert_eq!(pixel_at(&rgba, 8, 8), [76, 175, 80, 255]);
        assert_eq!(pixel_at(&rgba, 8, 20), [76, 175, 80, 255]);
    }

    #[test]
    fn stacked_bars_error_state_desaturates_both_rows() {
        let (rgba, _, _) = render_stacked_bars_icon_rgba_with_pace(100.0, Some(100.0), true, None);
        for (x, y) in [(8, 8), (8, 20)] {
            let px = pixel_at(&rgba, x, y);
            assert_eq!(px[0], px[1]);
            assert_eq!(px[1], px[2]);
        }
    }

    #[test]
    fn stacked_bars_take_the_provider_tint_on_both_rows() {
        let (rgba, _, _) =
            render_stacked_bars_icon_rgba_tinted(100.0, Some(100.0), false, None, Some(TEAL));
        assert_eq!(pixel_at(&rgba, 8, 8), [TEAL.0, TEAL.1, TEAL.2, 255]);
        assert_eq!(pixel_at(&rgba, 8, 20), [TEAL.0, TEAL.1, TEAL.2, 255]);

        let (untinted, _, _) =
            render_stacked_bars_icon_rgba_tinted(100.0, Some(100.0), false, None, None);
        let (plain, _, _) =
            render_stacked_bars_icon_rgba_with_pace(100.0, Some(100.0), false, None);
        assert_eq!(untinted, plain);
    }

    #[test]
    fn provider_tint_colors_bar_fill_and_keeps_track() {
        let (tinted, _, _) = render_bar_icon_rgba_tinted(100.0, None, false, None, Some(TEAL));
        // Filled part of the single meter takes the accent.
        assert_eq!(pixel_at(&tinted, 8, 16), [TEAL.0, TEAL.1, TEAL.2, 255]);
        // Unfilled track and canvas stay the standard tray colours.
        let (partial, _, _) = render_bar_icon_rgba_tinted(25.0, None, false, None, Some(TEAL));
        assert_eq!(pixel_at(&partial, 20, 16), [80, 80, 90, 255]);
        assert_eq!(pixel_at(&partial, 2, 16), [60, 60, 70, 255]);
    }

    #[test]
    fn provider_tint_colors_both_bar_rows() {
        let (tinted, _, _) =
            render_bar_icon_rgba_tinted(100.0, Some(100.0), false, None, Some(TEAL));
        assert_eq!(pixel_at(&tinted, 8, 10), [TEAL.0, TEAL.1, TEAL.2, 255]);
        assert_eq!(pixel_at(&tinted, 8, 20), [TEAL.0, TEAL.1, TEAL.2, 255]);
    }

    #[test]
    fn provider_tint_colors_percent_glyphs() {
        let usage = UsageLevel::from_percent(72.0).color();
        let (tinted, _, _) = render_percent_icon_rgba_tinted(72.0, false, None, Some(TEAL));
        let pixels = tinted.as_chunks::<4>().0;
        assert!(pixels.iter().any(|px| px[..3] == [TEAL.0, TEAL.1, TEAL.2]));
        assert!(
            !pixels
                .iter()
                .any(|px| px[..3] == [usage.0, usage.1, usage.2])
        );
    }

    #[test]
    fn provider_tint_is_off_by_default_and_matches_plain_render() {
        let (plain, _, _) = render_bar_icon_rgba(100.0, Some(40.0), false);
        let (tinted_none, _, _) = render_bar_icon_rgba_tinted(100.0, Some(40.0), false, None, None);
        assert_eq!(plain, tinted_none);
        let (percent_plain, _, _) = render_percent_icon_rgba(72.0, false);
        let (percent_none, _, _) = render_percent_icon_rgba_tinted(72.0, false, None, None);
        assert_eq!(percent_plain, percent_none);
    }

    #[test]
    fn pace_color_wins_over_provider_tint() {
        let (rgba, _, _) = render_bar_icon_rgba_tinted(
            100.0,
            None,
            false,
            Some(TrayPaceColor::Behind),
            Some(TEAL),
        );
        assert_eq!(pixel_at(&rgba, 8, 16), [76, 175, 80, 255]);
    }

    #[test]
    fn error_icons_ignore_provider_tint() {
        let (plain, _, _) = render_bar_icon_rgba_tinted(100.0, None, true, None, None);
        let (tinted, _, _) = render_bar_icon_rgba_tinted(100.0, None, true, None, Some(TEAL));
        assert_eq!(plain, tinted);
        let idx = ((16 * TRAY_ICON_SIZE + 8) * 4) as usize;
        assert_eq!(tinted[idx], tinted[idx + 1]);
        assert_eq!(tinted[idx + 1], tinted[idx + 2]);
    }

    #[test]
    fn low_contrast_provider_tint_falls_back_to_usage_colors() {
        // Near-black accents would vanish on the tray canvas.
        let dark = (20, 20, 20);
        assert!(!provider_tint_is_legible(dark));
        assert!(!provider_tint_is_legible((60, 60, 70)));
        // Cursor's brand orange is legible on the canvas.
        assert!(provider_tint_is_legible((245, 78, 0)));
        assert!(provider_tint_is_legible(TEAL));

        let (fallback, _, _) = render_bar_icon_rgba_tinted(100.0, None, false, None, Some(dark));
        let (plain, _, _) = render_bar_icon_rgba(100.0, None, false);
        assert_eq!(fallback, plain);
        let (fallback_percent, _, _) =
            render_percent_icon_rgba_tinted(100.0, false, None, Some(dark));
        let (plain_percent, _, _) = render_percent_icon_rgba(100.0, false);
        assert_eq!(fallback_percent, plain_percent);
    }

    #[test]
    fn stacked_rows_take_their_own_provider_tint() {
        let orange = (245, 78, 0);
        let (rgba, width, _) =
            render_stacked_bar_icon_rgba_tinted(100.0, 100.0, false, Some(TEAL), Some(orange));
        let pixel = |x: u32, y: u32| {
            let index = ((y * width + x) * 4) as usize;
            [
                rgba[index],
                rgba[index + 1],
                rgba[index + 2],
                rgba[index + 3],
            ]
        };
        assert_eq!(pixel(8, 8), [TEAL.0, TEAL.1, TEAL.2, 255]);
        assert_eq!(pixel(8, 20), [orange.0, orange.1, orange.2, 255]);
        // Untinted stacked render keeps the usage-level colour on both rows.
        let (plain, _, _) = render_stacked_bar_icon_rgba(100.0, 100.0, false);
        let (none, _, _) = render_stacked_bar_icon_rgba_tinted(100.0, 100.0, false, None, None);
        assert_eq!(plain, none);
    }
}
