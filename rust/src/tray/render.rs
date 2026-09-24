//! Pixel-level tray icon renderer, decoupled from any platform icon API.
//!
//! Returns raw RGBA bytes so callers (egui tray manager, Tauri shell, tests)
//! can adapt the result to their own icon type without pulling in extra deps.

use image::{ImageBuffer, Rgba, RgbaImage};

use super::icon::UsageLevel;

/// Side length of the generated tray icon in pixels.
pub const TRAY_ICON_SIZE: u32 = 32;

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

fn usage_color(percent: f64, has_error: bool) -> Rgba<u8> {
    let (r, g, b) = UsageLevel::from_percent(percent).color();
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

fn draw_bar_row(image: &mut RgbaImage, y_start: u32, y_end: u32, percent: f64, has_error: bool) {
    let bar_width = BAR_RIGHT - BAR_LEFT;
    #[allow(
        clippy::cast_possible_truncation,
        reason = "percent is clamped to 0..=100 and scaled to a 24-pixel meter"
    )]
    let fill = ((percent.clamp(0.0, 100.0) / 100.0) * bar_width as f64) as u32;
    let fill_end = (BAR_LEFT + fill).min(BAR_RIGHT);
    let color = usage_color(percent, has_error);

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
    let mut image = new_icon_canvas(has_error);

    match weekly_percent {
        Some(weekly) => {
            draw_bar_row(&mut image, 8, 15, session_percent, has_error);
            draw_bar_row(&mut image, 18, 23, weekly, has_error);
        }
        None => {
            draw_bar_row(&mut image, 10, 22, session_percent, has_error);
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
    let mut image = new_icon_canvas(has_error);
    draw_bar_row(&mut image, 6, 14, top_percent, has_error);
    draw_bar_row(&mut image, 18, 26, bottom_percent, has_error);
    (image.into_raw(), TRAY_ICON_SIZE, TRAY_ICON_SIZE)
}

/// Render a compact numeric percent tray icon as raw RGBA bytes.
pub fn render_percent_icon_rgba(percent: f64, has_error: bool) -> (Vec<u8>, u32, u32) {
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

    let color = usage_color(percent, has_error);

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
}
