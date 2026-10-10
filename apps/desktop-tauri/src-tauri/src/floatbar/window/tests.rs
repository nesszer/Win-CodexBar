use super::*;

#[test]
fn opacity_to_alpha_clamps_low_values() {
    assert_eq!(opacity_to_alpha(0), opacity_to_alpha(30));
    assert_eq!(opacity_to_alpha(10), opacity_to_alpha(30));
}

#[test]
fn opacity_to_alpha_full_is_255() {
    assert_eq!(opacity_to_alpha(100), 255);
}

#[test]
fn opacity_to_alpha_is_monotonic() {
    let a = opacity_to_alpha(30);
    let b = opacity_to_alpha(60);
    let c = opacity_to_alpha(100);
    assert!(a < b);
    assert!(b < c);
}

#[test]
fn opacity_to_alpha_midpoint() {
    // 50% should be roughly half of 255.
    let alpha = opacity_to_alpha(50);
    assert!((125..=130).contains(&alpha), "got {alpha}");
}

#[test]
fn initial_size_picks_orientation() {
    assert_eq!(
        initial_size("horizontal"),
        (FLOATBAR_DEFAULT_WIDTH_H, FLOATBAR_DEFAULT_HEIGHT_H)
    );
    assert_eq!(
        initial_size("vertical"),
        (FLOATBAR_DEFAULT_WIDTH_V, FLOATBAR_DEFAULT_HEIGHT_V)
    );
    // Unknown values fall through to horizontal so a corrupted setting
    // can't yield an unreadable strip.
    assert_eq!(
        initial_size("diagonal"),
        (FLOATBAR_DEFAULT_WIDTH_H, FLOATBAR_DEFAULT_HEIGHT_H)
    );
}

#[test]
fn windows_minimized_position_is_not_restored() {
    assert!(is_windows_minimized_position(-32_000, -32_000));
}

#[test]
fn legitimate_negative_monitor_positions_are_preserved() {
    assert!(!is_windows_minimized_position(-3_840, 0));
    assert!(!is_windows_minimized_position(-8_000, -8_000));
    assert!(!is_windows_minimized_position(-16_000, -16_000));
}

#[test]
fn minimized_or_parked_physical_positions_are_not_remembered() {
    assert!(!should_remember_physical_position(true, 100, 100));
    assert!(!should_remember_physical_position(false, -32_000, -32_000));
    assert!(should_remember_physical_position(false, -3_840, 100));
}

#[test]
fn physical_multi_monitor_origin_is_remembered() {
    assert!(should_remember_physical_position(false, -8_000, -8_000));
}

#[test]
fn window_must_intersect_an_active_monitor_to_be_visible() {
    let window_size = PhysicalSize::new(211, 40);
    let monitor_size = PhysicalSize::new(1_920, 1_032);

    assert!(physical_rects_intersect(
        PhysicalPosition::new(780, 8),
        window_size,
        PhysicalPosition::new(0, 0),
        monitor_size,
    ));
    assert!(!physical_rects_intersect(
        PhysicalPosition::new(-32_000, -32_000),
        window_size,
        PhysicalPosition::new(0, 0),
        monitor_size,
    ));
    assert!(physical_rects_intersect(
        PhysicalPosition::new(-3_840, 8),
        window_size,
        PhysicalPosition::new(-3_840, 0),
        monitor_size,
    ));
}

#[test]
fn taskbar_strip_outside_work_area_still_counts_as_on_monitor() {
    // Full monitor 1920x1080; work area is 1920x1032 (48px taskbar strip).
    // A taskbar-style bar sitting fully in the strip is on-screen for
    // visibility even though it has zero work-area intersection.
    let window_pos = PhysicalPosition::new(800, 1_040);
    let window_size = PhysicalSize::new(211, 40);
    let monitor_pos = PhysicalPosition::new(0, 0);
    let monitor_size = PhysicalSize::new(1_920, 1_080);
    let work_area_pos = PhysicalPosition::new(0, 0);
    let work_area_size = PhysicalSize::new(1_920, 1_032);

    assert!(physical_rects_intersect(
        window_pos,
        window_size,
        monitor_pos,
        monitor_size,
    ));
    assert!(!physical_rects_intersect(
        window_pos,
        window_size,
        work_area_pos,
        work_area_size,
    ));
}

#[test]
fn hidpi_parked_logical_geometry_is_rejected() {
    assert!(is_unusable_stored_logical_position(-32_000, -32_000));
    // -32000 physical at 1.25x / 1.5x / 2.0x scale factors.
    assert!(is_unusable_stored_logical_position(-25_600, -25_600));
    assert!(is_unusable_stored_logical_position(-21_333, -21_333));
    assert!(is_unusable_stored_logical_position(-16_000, -16_000));
    // Legitimate multi-monitor logical origins must still restore.
    assert!(!is_unusable_stored_logical_position(-3_840, 0));
    assert!(!is_unusable_stored_logical_position(-8_000, -8_000));
}

#[test]
fn disconnected_monitor_position_falls_back_to_primary_work_area() {
    let work_area_position = PhysicalPosition::new(0, 0);
    let work_area_size = PhysicalSize::new(1_920, 1_032);
    let window_size = PhysicalSize::new(211, 40);

    assert_eq!(
        fallback_physical_position(
            work_area_position,
            work_area_size,
            window_size,
            1.0,
            "floating",
        ),
        PhysicalPosition::new(855, 8),
    );
    assert_eq!(
        fallback_physical_position(
            work_area_position,
            work_area_size,
            window_size,
            1.0,
            "taskbar",
        ),
        PhysicalPosition::new(855, 984),
    );
}

#[test]
fn default_logical_origin_matches_first_open_policy() {
    assert_eq!(
        default_logical_origin(0.0, 0.0, 1_920.0, 1_080.0, 211.0, 40.0, "floating"),
        (854.5, 8.0),
    );
    assert_eq!(
        default_logical_origin(0.0, 0.0, 1_920.0, 1_080.0, 211.0, 40.0, "taskbar"),
        (854.5, 1_032.0),
    );
}
