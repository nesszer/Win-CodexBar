/// A rectangle in physical pixels (monitor work area or icon bounds).
#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    // Physical pixel extents are bounded well below i32::MAX.
    #[expect(clippy::cast_possible_wrap, reason = "pixel dimensions fit in i32")]
    pub const fn signed_width(&self) -> i32 {
        self.width as i32
    }

    #[expect(clippy::cast_possible_wrap, reason = "pixel dimensions fit in i32")]
    pub const fn signed_height(&self) -> i32 {
        self.height as i32
    }

    pub const fn right(&self) -> i32 {
        self.x + self.signed_width()
    }

    pub const fn bottom(&self) -> i32 {
        self.y + self.signed_height()
    }
}

/// Substitute 1.0 for a non-finite or non-positive monitor scale factor.
pub fn safe_scale(scale_factor: f64) -> f64 {
    if scale_factor.is_finite() && scale_factor > 0.0 {
        scale_factor
    } else {
        1.0
    }
}

/// Panel dimensions in logical pixels.
#[derive(Debug, Clone, Copy)]
pub struct PanelSize {
    pub width: u32,
    pub height: u32,
}

/// Margin kept between the panel edge and the monitor work-area edge.
const MARGIN: i32 = 8;
const GAP: i32 = 8;

fn physical_panel_size(panel_size: &PanelSize, scale_factor: f64) -> (i32, i32) {
    let scale_factor = safe_scale(scale_factor);

    // Physical size is rounded to whole pixels before truncation.
    #[expect(clippy::cast_possible_truncation, reason = "whole units by design")]
    let width = ((panel_size.width as f64) * scale_factor).round().max(1.0) as i32;
    #[expect(clippy::cast_possible_truncation, reason = "whole units by design")]
    let height = ((panel_size.height as f64) * scale_factor).round().max(1.0) as i32;
    (width, height)
}

pub fn clamp_position_to_work_area(
    target_x: i32,
    target_y: i32,
    monitor_rect: &Rect,
    panel_size: &PanelSize,
    scale_factor: f64,
) -> (i32, i32) {
    let (pw, ph) = physical_panel_size(panel_size, scale_factor);
    let min_x = monitor_rect.x + MARGIN;
    let min_y = monitor_rect.y + MARGIN;
    let max_x = (monitor_rect.right() - pw - MARGIN).max(min_x);
    let max_y = (monitor_rect.bottom() - ph - MARGIN).max(min_y);

    (target_x.clamp(min_x, max_x), target_y.clamp(min_y, max_y))
}

fn calculate_anchored_position(
    icon_rect: &Rect,
    monitor_rect: &Rect,
    panel_size: &PanelSize,
    scale_factor: f64,
    anchor_y: i32,
    open_above: bool,
) -> (i32, i32) {
    let (pw, ph) = physical_panel_size(panel_size, scale_factor);
    let anchor_x = icon_rect.x + icon_rect.signed_width() / 2;
    let target_x = anchor_x - pw / 2;
    let target_y = if open_above {
        anchor_y - ph - GAP
    } else {
        anchor_y + GAP
    };

    clamp_position_to_work_area(target_x, target_y, monitor_rect, panel_size, scale_factor)
}

/// Calculate panel position anchored to a tray icon rectangle.
///
/// Placement rules:
/// - Horizontally centered on the icon, clamped to the monitor work area.
/// - Left/right taskbars bottom-align the panel to the work area.
/// - If the icon is in the bottom half of the monitor (bottom taskbar), the
///   panel opens *above* the icon. Otherwise it opens *below*.
pub fn calculate_panel_position(
    icon_rect: &Rect,
    monitor_bounds: &Rect,
    work_area: &Rect,
    panel_size: &PanelSize,
    scale_factor: f64,
) -> (i32, i32) {
    let my = work_area.y;
    let mh = work_area.signed_height();

    let icon_cy = icon_rect.y + icon_rect.signed_height() / 2;
    let monitor_cy = my + mh / 2;

    let open_above = icon_cy > monitor_cy;
    let anchor_y = if open_above {
        icon_rect.y
    } else {
        icon_rect.bottom()
    };

    let position = calculate_anchored_position(
        icon_rect,
        work_area,
        panel_size,
        scale_factor,
        anchor_y,
        open_above,
    );
    if work_area.x > monitor_bounds.x || work_area.right() < monitor_bounds.right() {
        let (_, ph) = physical_panel_size(panel_size, scale_factor);
        (position.0, work_area.bottom() - ph - MARGIN)
    } else {
        position
    }
}

/// Position beside a desktop launch's physical cursor point, kept in the work area.
pub fn calculate_cursor_position(
    cursor: (f64, f64),
    work_area: &Rect,
    panel_size: &PanelSize,
    scale_factor: f64,
) -> (i32, i32) {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "physical cursor coordinates fit i32"
    )]
    let x = (cursor.0.round() as i32).saturating_add(GAP);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "physical cursor coordinates fit i32"
    )]
    let y = (cursor.1.round() as i32).saturating_add(GAP);
    clamp_position_to_work_area(x, y, work_area, panel_size, scale_factor)
}

/// Legacy shortcut placement: 22 % from left, vertically centred.
pub fn calculate_shortcut_position(
    monitor_rect: &Rect,
    panel_size: &PanelSize,
    scale_factor: f64,
) -> (i32, i32) {
    let (pw, ph) = physical_panel_size(panel_size, scale_factor);
    let mx = monitor_rect.x;
    let my = monitor_rect.y;
    let mw = monitor_rect.signed_width();
    let mh = monitor_rect.signed_height();

    // Shortcut offset is truncated to a whole pixel by design.
    #[expect(clippy::cast_possible_truncation, reason = "whole units by design")]
    let x = mx + ((mw as f64) * 0.22) as i32;
    let y = my + (mh - ph) / 2;

    let x = x.max(mx + MARGIN).min(mx + mw - pw - MARGIN);
    let y = y.max(my + MARGIN).min(my + mh - ph - MARGIN);

    (x, y)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hd_monitor() -> Rect {
        Rect::new(0, 0, 1920, 1080)
    }

    fn panel() -> PanelSize {
        PanelSize {
            width: 420,
            height: 560,
        }
    }

    // --- tray-anchor tests ---

    #[test]
    fn bottom_taskbar_panel_opens_above() {
        let icon = Rect::new(1800, 1040, 24, 24);
        let monitor = hd_monitor();
        let (_, y) = calculate_panel_position(&icon, &monitor, &monitor, &panel(), 1.0);
        assert!(y < icon.y, "panel should sit above the icon");
    }

    #[test]
    fn top_taskbar_panel_opens_below() {
        let icon = Rect::new(900, 4, 24, 24);
        let monitor = hd_monitor();
        let (_, y) = calculate_panel_position(&icon, &monitor, &monitor, &panel(), 1.0);
        assert!(
            y >= icon.y + i32::try_from(icon.height).unwrap(),
            "panel should sit below the icon"
        );
    }

    #[test]
    fn horizontal_centre_on_icon() {
        let icon = Rect::new(960, 1040, 24, 24);
        let monitor = hd_monitor();
        let (x, _) = calculate_panel_position(&icon, &monitor, &monitor, &panel(), 1.0);
        let icon_cx = icon.x + 12;
        let panel_cx = x + 210;
        assert!(
            (icon_cx - panel_cx).abs() <= 1,
            "panel should be centred on icon (off by {})",
            (icon_cx - panel_cx).abs()
        );
    }

    #[test]
    fn clamped_left_edge() {
        let icon = Rect::new(0, 1040, 24, 24);
        let monitor = hd_monitor();
        let (x, _) = calculate_panel_position(&icon, &monitor, &monitor, &panel(), 1.0);
        assert!(x >= MARGIN, "panel must not exceed left margin");
    }

    #[test]
    fn clamped_right_edge() {
        let icon = Rect::new(1900, 1040, 24, 24);
        let monitor = hd_monitor();
        let (x, _) = calculate_panel_position(&icon, &monitor, &monitor, &panel(), 1.0);
        assert!(
            x + i32::try_from(panel().width).unwrap() + MARGIN <= 1920,
            "panel must not exceed right margin"
        );
    }

    #[test]
    fn clamped_top_edge() {
        let icon = Rect::new(960, 4, 24, 24);
        let monitor = hd_monitor();
        let (_, y) = calculate_panel_position(&icon, &monitor, &monitor, &panel(), 1.0);
        assert!(y >= MARGIN, "panel must not exceed top margin");
    }

    #[test]
    fn top_taskbar_work_area_clamps_open_below_to_min_y() {
        let work_area = Rect::new(0, 40, 1920, 1040);
        let icon = Rect::new(960, 4, 24, 24);
        let monitor = hd_monitor();
        let (_, y) = calculate_panel_position(&icon, &monitor, &work_area, &panel(), 1.0);
        assert_eq!(y, work_area.y + MARGIN);
    }

    #[test]
    fn left_taskbar_bottom_aligns_panel() {
        let monitor = hd_monitor();
        let work_area = Rect::new(40, 0, 1880, 1080);
        let icon = Rect::new(8, 1048, 24, 24);

        let (_, y) = calculate_panel_position(&icon, &monitor, &work_area, &panel(), 1.0);

        assert_eq!(y, 1080 - 560 - MARGIN);
    }

    #[test]
    fn high_dpi_right_taskbar_bottom_aligns_panel() {
        let monitor = Rect::new(0, 0, 3840, 2160);
        let work_area = Rect::new(0, 0, 3760, 2160);
        let icon = Rect::new(3808, 2128, 24, 24);

        let (_, y) = calculate_panel_position(&icon, &monitor, &work_area, &panel(), 2.0);

        assert_eq!(y, 2160 - (560 * 2) - MARGIN);
    }

    #[test]
    fn multi_monitor_offset() {
        let monitor = Rect::new(1920, 0, 1920, 1080);
        let icon = Rect::new(3700, 1040, 24, 24);
        let (x, _) = calculate_panel_position(&icon, &monitor, &monitor, &panel(), 1.0);
        assert!(x >= monitor.x + MARGIN);
        assert!(
            x + i32::try_from(panel().width).unwrap() + MARGIN
                <= monitor.x + i32::try_from(monitor.width).unwrap()
        );
    }

    #[test]
    fn high_dpi_positioning() {
        let icon = Rect::new(960, 1040, 24, 24);
        let monitor = hd_monitor();
        let (x1, y1) = calculate_panel_position(&icon, &monitor, &monitor, &panel(), 1.0);
        let (x2, y2) = calculate_panel_position(&icon, &monitor, &monitor, &panel(), 2.0);
        assert!(
            x2 < x1,
            "higher scale should shift the panel left to fit its physical width"
        );
        assert!(
            y2 < y1,
            "higher scale should shift the panel upward to fit its physical height"
        );
    }

    // --- shortcut-anchor tests ---

    #[test]
    fn desktop_open_stays_beside_cursor_after_content_grows() {
        let monitor = hd_monitor();
        let cursor = (50.0, 100.0);
        assert_eq!(
            calculate_cursor_position(cursor, &monitor, &panel(), 1.0),
            (58, 108)
        );
        let taller = PanelSize {
            width: 360,
            height: 800,
        };
        assert_eq!(
            calculate_cursor_position(cursor, &monitor, &taller, 1.0),
            (58, 108)
        );
    }

    #[test]
    fn cursor_open_clamps_on_high_dpi_monitor_with_negative_origin() {
        let work_area = Rect::new(-2560, 0, 2560, 1400);
        let panel = PanelSize {
            width: 400,
            height: 600,
        };
        assert_eq!(
            calculate_cursor_position((-10.0, 1390.0), &work_area, &panel, 2.0),
            (-808, 192)
        );
    }

    #[test]
    fn oversized_cursor_panel_keeps_its_top_left_visible() {
        let work_area = Rect::new(1920, -100, 500, 400);
        assert_eq!(
            calculate_cursor_position((2200.0, 250.0), &work_area, &panel(), 2.0),
            (1928, -92)
        );
    }

    #[test]
    fn shortcut_position_22_pct_from_left() {
        let monitor = hd_monitor();
        let (x, _) = calculate_shortcut_position(&monitor, &panel(), 1.0);
        // 1920 * 0.22 = 422.4 — a constant that fits i32.
        #[expect(clippy::cast_possible_truncation, reason = "constant offset fits i32")]
        let expected_x = (1920.0 * 0.22) as i32;
        assert_eq!(x, expected_x);
    }

    #[test]
    fn shortcut_position_vertically_centred() {
        let (_, y) = calculate_shortcut_position(&hd_monitor(), &panel(), 1.0);
        let expected_y = (1080 - 560) / 2;
        assert_eq!(y, expected_y);
    }

    #[test]
    fn shortcut_clamped_small_monitor() {
        let monitor = Rect::new(0, 0, 500, 600);
        let (x, y) = calculate_shortcut_position(&monitor, &panel(), 1.0);
        assert!(x >= MARGIN);
        assert!(
            x + i32::try_from(panel().width).unwrap() + MARGIN
                <= i32::try_from(monitor.width).unwrap()
        );
        assert!(y >= MARGIN);
        assert!(
            y + i32::try_from(panel().height).unwrap() + MARGIN
                <= i32::try_from(monitor.height).unwrap()
        );
    }
}
