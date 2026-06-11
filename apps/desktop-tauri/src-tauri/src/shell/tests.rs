use super::ShellTransitionRequest;
use super::geometry::{
    MonitorPlacement, inferred_tray_anchor_rect, inferred_tray_panel_position_for_monitor,
    surface_panel_size, tray_anchor_rect,
};
use super::position::visible_surface_position_for_mode_with_fallbacks;
use super::transition::{
    SurfaceSnapshot, TransitionResolution, hidden_surface_snapshot,
    monitor_for_preserved_visible_position, reclamp_preserved_visible_position,
    recovery_snapshot_for_failed_transition, resolve_transition_position,
    resolve_transition_request, restore_recovery_surface, restore_surface_snapshot,
    should_hide_tray_panel_on_toggle, should_synthesize_default_position,
};
use super::window::{hide_to_tray_state, prepare_hide_to_tray_if_current};

use crate::state::AppState;
use crate::surface::{SurfaceMode, SurfaceTransition};
use crate::surface_target::SurfaceTarget;
use crate::window_positioner::{self, Rect};

#[test]
fn hide_to_tray_resets_hidden_target_to_summary() {
    let mut state = AppState::new();
    state.current_target = SurfaceTarget::Settings {
        tab: "about".into(),
    };

    hide_to_tray_state(&mut state);

    assert_eq!(state.surface_machine.current(), SurfaceMode::Hidden);
    assert_eq!(state.current_target, SurfaceTarget::Summary);
}

#[test]
fn conditional_hide_to_tray_updates_matching_surface() {
    let mut state = AppState::new();
    state.transition_surface(SurfaceMode::TrayPanel, SurfaceTarget::Summary);

    let plan = prepare_hide_to_tray_if_current(&mut state, |mode| mode == SurfaceMode::TrayPanel)
        .expect("tray panel should be eligible");

    assert_eq!(plan.previous.mode, SurfaceMode::TrayPanel);
    assert_eq!(state.surface_machine.current(), SurfaceMode::Hidden);
    assert_eq!(state.current_target, SurfaceTarget::Summary);
    assert_eq!(plan.target, SurfaceTarget::Summary);
}

#[test]
fn conditional_hide_to_tray_leaves_non_matching_surface_alone() {
    let mut state = AppState::new();
    state.transition_surface(SurfaceMode::PopOut, SurfaceTarget::Dashboard);

    let plan = prepare_hide_to_tray_if_current(&mut state, |mode| mode == SurfaceMode::TrayPanel);

    assert!(plan.is_none());
    assert_eq!(state.surface_machine.current(), SurfaceMode::PopOut);
    assert_eq!(state.current_target, SurfaceTarget::Dashboard);
}

#[test]
fn tray_toggle_hides_only_when_panel_window_is_visible() {
    assert!(should_hide_tray_panel_on_toggle(
        SurfaceMode::TrayPanel,
        true
    ));
    assert!(!should_hide_tray_panel_on_toggle(
        SurfaceMode::TrayPanel,
        false
    ));
    assert!(!should_hide_tray_panel_on_toggle(SurfaceMode::Hidden, true));
}

#[test]
fn immediate_tray_click_consumes_blur_dismissal() {
    let mut state = AppState::new();
    let dismissed_at = std::time::Instant::now();

    state.mark_blur_dismissed(dismissed_at);

    assert!(state.take_recent_blur_dismissal(
        dismissed_at + std::time::Duration::from_millis(20),
        std::time::Duration::from_millis(250),
    ));
    assert!(!state.take_recent_blur_dismissal(
        dismissed_at + std::time::Duration::from_millis(20),
        std::time::Duration::from_millis(250),
    ));
}

#[test]
fn later_tray_click_does_not_consume_expired_blur_dismissal() {
    let mut state = AppState::new();
    let dismissed_at = std::time::Instant::now();

    state.mark_blur_dismissed(dismissed_at);

    assert!(!state.take_recent_blur_dismissal(
        dismissed_at + std::time::Duration::from_millis(251),
        std::time::Duration::from_millis(250),
    ));
}

#[test]
fn same_mode_about_request_resolves_as_retarget() {
    let mut state = AppState::new();
    state.transition_surface(
        SurfaceMode::Settings,
        SurfaceTarget::Settings {
            tab: "general".into(),
        },
    );

    let resolution = resolve_transition_request(
        &state,
        &ShellTransitionRequest {
            mode: SurfaceMode::Settings,
            target: SurfaceTarget::Settings {
                tab: "about".into(),
            },
            position: None,
        },
        false,
    );

    match resolution {
        TransitionResolution::SameModeRetarget { mode, target } => {
            assert_eq!(mode, SurfaceMode::Settings);
            assert_eq!(
                target,
                SurfaceTarget::Settings {
                    tab: "about".into()
                }
            );
        }
        _ => panic!("expected same-mode retarget"),
    }
}

#[test]
fn same_mode_provider_request_resolves_as_retarget() {
    let mut state = AppState::new();
    state.transition_surface(SurfaceMode::PopOut, SurfaceTarget::Dashboard);

    let resolution = resolve_transition_request(
        &state,
        &ShellTransitionRequest {
            mode: SurfaceMode::PopOut,
            target: SurfaceTarget::Provider {
                provider_id: "codex".into(),
            },
            position: None,
        },
        false,
    );

    match resolution {
        TransitionResolution::SameModeRetarget { mode, target } => {
            assert_eq!(mode, SurfaceMode::PopOut);
            assert_eq!(
                target,
                SurfaceTarget::Provider {
                    provider_id: "codex".into()
                }
            );
        }
        _ => panic!("expected same-mode retarget"),
    }
}

#[test]
fn same_mode_reopen_request_resolves_as_update() {
    let mut state = AppState::new();
    state.transition_surface(SurfaceMode::TrayPanel, SurfaceTarget::Summary);

    let resolution = resolve_transition_request(
        &state,
        &ShellTransitionRequest {
            mode: SurfaceMode::TrayPanel,
            target: SurfaceTarget::Summary,
            position: Some((10, 20)),
        },
        true,
    );

    match resolution {
        TransitionResolution::SameModeReopen { mode, target } => {
            assert_eq!(mode, SurfaceMode::TrayPanel);
            assert_eq!(target, SurfaceTarget::Summary);
        }
        _ => panic!("expected same-mode reopen update"),
    }
}

#[test]
fn same_mode_retarget_skips_default_position_synthesis() {
    let resolution = TransitionResolution::SameModeRetarget {
        mode: SurfaceMode::Settings,
        target: SurfaceTarget::Settings {
            tab: "about".into(),
        },
    };
    let mut fallback_called = false;

    let position = resolve_transition_position(None, &resolution, false, || {
        fallback_called = true;
        Some((10, 20))
    });

    assert_eq!(position, None);
    assert!(
        !fallback_called,
        "same-mode retarget should not request a default position"
    );
}

#[test]
fn same_mode_retarget_preserves_explicit_position() {
    let resolution = TransitionResolution::SameModeRetarget {
        mode: SurfaceMode::PopOut,
        target: SurfaceTarget::Provider {
            provider_id: "codex".into(),
        },
    };

    let position = resolve_transition_position(Some((10, 20)), &resolution, false, || {
        panic!("explicit same-mode retarget position should be used directly")
    });

    assert_eq!(position, Some((10, 20)));
}

#[test]
fn same_mode_reopen_still_uses_default_position() {
    let resolution = TransitionResolution::SameModeReopen {
        mode: SurfaceMode::TrayPanel,
        target: SurfaceTarget::Summary,
    };
    let mut fallback_called = false;

    let position = resolve_transition_position(None, &resolution, true, || {
        fallback_called = true;
        Some((42, 24))
    });

    assert_eq!(position, Some((42, 24)));
    assert!(
        fallback_called,
        "same-mode reopen should still synthesize a default position"
    );
}

#[test]
fn visible_mode_change_skips_default_position_synthesis() {
    let resolution = TransitionResolution::ModeChange {
        transition: SurfaceTransition {
            from: SurfaceMode::PopOut,
            to: SurfaceMode::Settings,
            properties: SurfaceMode::Settings.window_properties(),
        },
        target: SurfaceTarget::Settings {
            tab: "about".into(),
        },
    };
    let mut fallback_called = false;

    let position = resolve_transition_position(
        None,
        &resolution,
        should_synthesize_default_position(&resolution),
        || {
            fallback_called = true;
            Some((20, 30))
        },
    );

    assert_eq!(position, None);
    assert!(
        !fallback_called,
        "visible-to-visible mode changes should preserve the current window position"
    );
}

#[test]
fn larger_visible_destination_reclamps_preserved_top_left() {
    let current_top_left = (1492, 512);
    let monitor = Rect {
        x: 0,
        y: 0,
        width: 1920,
        height: 1080,
    };

    let reclamped =
        reclamp_preserved_visible_position(current_top_left, &monitor, SurfaceMode::Settings, 1.0);

    assert_eq!(reclamped, (1416, 492));
}

#[test]
fn preserved_visible_monitor_prefers_top_left_for_straddling_window() {
    let monitors = vec![
        (
            Rect {
                x: 0,
                y: 0,
                width: 1920,
                height: 1080,
            },
            1.0,
        ),
        (
            Rect {
                x: 1920,
                y: 0,
                width: 1920,
                height: 1080,
            },
            1.25,
        ),
    ];

    let selected = monitor_for_preserved_visible_position(&monitors, (1800, 120), Some((600, 700)))
        .expect("straddling window should resolve from its preserved top-left");

    assert_eq!(selected.0.x, 0);
    assert_eq!(selected.1, 1.0);
}

#[test]
fn visible_surface_position_falls_back_to_current_monitor_without_available_monitors() {
    let current_monitor = MonitorPlacement {
        bounds: Rect {
            x: 1920,
            y: 0,
            width: 1920,
            height: 1080,
        },
        work_area: Rect {
            x: 1920,
            y: 0,
            width: 1920,
            height: 1080,
        },
        scale_factor: 1.25,
    };
    let primary_monitor = MonitorPlacement {
        bounds: Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        },
        work_area: Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        },
        scale_factor: 1.0,
    };
    let anchor = crate::state::TrayAnchor {
        x: 10,
        y: 10,
        width: 16,
        height: 16,
    };

    let position = visible_surface_position_for_mode_with_fallbacks(
        SurfaceMode::PopOut,
        None,
        Some(anchor),
        Some(current_monitor),
        Some(((2000, 120), (600, 700))),
        Some(primary_monitor),
    );

    assert_eq!(
        position,
        Some(window_positioner::calculate_popout_position(
            None,
            &current_monitor.work_area,
            &surface_panel_size(SurfaceMode::PopOut),
            current_monitor.scale_factor,
        ))
    );
}

#[test]
fn visible_surface_position_anchor_lookup_uses_monitor_bounds() {
    let anchor_monitor = MonitorPlacement {
        bounds: Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        },
        work_area: Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
        },
        scale_factor: 1.0,
    };
    let current_monitor = MonitorPlacement {
        bounds: Rect {
            x: 1920,
            y: 0,
            width: 1920,
            height: 1080,
        },
        work_area: Rect {
            x: 1920,
            y: 0,
            width: 1920,
            height: 1080,
        },
        scale_factor: 1.25,
    };
    let anchor = crate::state::TrayAnchor {
        x: 1800,
        y: 1040,
        width: 24,
        height: 24,
    };

    let position = visible_surface_position_for_mode_with_fallbacks(
        SurfaceMode::PopOut,
        Some(&[anchor_monitor, current_monitor]),
        Some(anchor),
        Some(current_monitor),
        None,
        None,
    );

    assert_eq!(
        position,
        Some(window_positioner::calculate_popout_position(
            Some(&tray_anchor_rect(anchor)),
            &anchor_monitor.work_area,
            &surface_panel_size(SurfaceMode::PopOut),
            anchor_monitor.scale_factor,
        ))
    );
}

#[test]
fn visible_surface_position_settings_surface_uses_tray_anchor_position_when_available() {
    let anchor_monitor = MonitorPlacement {
        bounds: Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        },
        work_area: Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
        },
        scale_factor: 1.0,
    };
    let current_monitor = MonitorPlacement {
        bounds: Rect {
            x: 1920,
            y: 0,
            width: 1920,
            height: 1080,
        },
        work_area: Rect {
            x: 1920,
            y: 0,
            width: 1920,
            height: 1080,
        },
        scale_factor: 1.25,
    };
    let anchor = crate::state::TrayAnchor {
        x: 1800,
        y: 1040,
        width: 24,
        height: 24,
    };

    let position = visible_surface_position_for_mode_with_fallbacks(
        SurfaceMode::Settings,
        Some(&[anchor_monitor, current_monitor]),
        Some(anchor),
        Some(current_monitor),
        None,
        None,
    );

    assert_eq!(
        position,
        Some(window_positioner::calculate_popout_position(
            Some(&tray_anchor_rect(anchor)),
            &anchor_monitor.work_area,
            &surface_panel_size(SurfaceMode::Settings),
            anchor_monitor.scale_factor,
        ))
    );
}

#[test]
fn inferred_tray_anchor_defaults_to_bottom_right_of_work_area() {
    let monitor = MonitorPlacement {
        bounds: Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        },
        work_area: Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
        },
        scale_factor: 1.0,
    };

    let anchor = inferred_tray_anchor_rect(&monitor);

    assert_eq!(anchor.x, 1888);
    assert_eq!(anchor.y, 1048);
    assert_eq!(anchor.width, 24);
    assert_eq!(anchor.height, 24);
}

#[test]
fn inferred_tray_anchor_supports_top_taskbar_layouts() {
    let monitor = MonitorPlacement {
        bounds: Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        },
        work_area: Rect {
            x: 0,
            y: 40,
            width: 1920,
            height: 1040,
        },
        scale_factor: 1.0,
    };

    let anchor = inferred_tray_anchor_rect(&monitor);

    assert_eq!(anchor.x, 1888);
    assert_eq!(anchor.y, 8);
}

#[test]
fn inferred_tray_panel_position_uses_tray_style_corner_fallback() {
    let monitor = MonitorPlacement {
        bounds: Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1080,
        },
        work_area: Rect {
            x: 0,
            y: 0,
            width: 1920,
            height: 1040,
        },
        scale_factor: 1.0,
    };

    let position = inferred_tray_panel_position_for_monitor(&monitor);

    assert_eq!(
        position,
        window_positioner::calculate_panel_position(
            &Rect {
                x: 1888,
                y: 1048,
                width: 24,
                height: 24,
            },
            &monitor.work_area,
            &super::geometry::tray_panel_size(),
            monitor.scale_factor,
        )
    );
}

#[test]
fn hidden_mode_change_still_uses_default_position() {
    let resolution = TransitionResolution::ModeChange {
        transition: SurfaceTransition {
            from: SurfaceMode::Hidden,
            to: SurfaceMode::Settings,
            properties: SurfaceMode::Settings.window_properties(),
        },
        target: SurfaceTarget::Settings {
            tab: "general".into(),
        },
    };
    let mut fallback_called = false;

    let position = resolve_transition_position(
        None,
        &resolution,
        should_synthesize_default_position(&resolution),
        || {
            fallback_called = true;
            Some((64, 48))
        },
    );

    assert_eq!(position, Some((64, 48)));
    assert!(
        fallback_called,
        "hidden opens should still synthesize default placement"
    );
}

#[test]
fn failed_hide_transition_recovers_previous_visible_surface() {
    let previous = SurfaceSnapshot {
        mode: SurfaceMode::PopOut,
        target: SurfaceTarget::Provider {
            provider_id: "codex".into(),
        },
    };
    let transition = SurfaceTransition {
        from: SurfaceMode::PopOut,
        to: SurfaceMode::Hidden,
        properties: SurfaceMode::Hidden.window_properties(),
    };

    let recovery =
        recovery_snapshot_for_failed_transition(&transition, &previous, &SurfaceTarget::Summary);

    assert_eq!(recovery, previous);
}

#[test]
fn failed_show_transition_from_hidden_keeps_requested_visible_surface() {
    let previous = SurfaceSnapshot {
        mode: SurfaceMode::Hidden,
        target: SurfaceTarget::Summary,
    };
    let transition = SurfaceTransition {
        from: SurfaceMode::Hidden,
        to: SurfaceMode::TrayPanel,
        properties: SurfaceMode::TrayPanel.window_properties(),
    };

    let recovery =
        recovery_snapshot_for_failed_transition(&transition, &previous, &SurfaceTarget::Summary);

    assert_eq!(
        recovery,
        SurfaceSnapshot {
            mode: SurfaceMode::TrayPanel,
            target: SurfaceTarget::Summary,
        }
    );
}

#[test]
fn restore_surface_snapshot_reverts_mode_and_target() {
    let previous = SurfaceSnapshot {
        mode: SurfaceMode::Settings,
        target: SurfaceTarget::Settings {
            tab: "about".into(),
        },
    };
    let mut state = AppState::new();
    state.hide_surface();

    restore_surface_snapshot(&mut state, &previous);

    assert_eq!(state.surface_machine.current(), SurfaceMode::Settings);
    assert_eq!(state.current_target, previous.target);
}

#[test]
fn visible_recovery_propagates_visibility_errors() {
    let recovery = SurfaceSnapshot {
        mode: SurfaceMode::TrayPanel,
        target: SurfaceTarget::Summary,
    };

    let err = restore_recovery_surface(&recovery, |_| Err("show failed".into()))
        .expect_err("visible recovery should fail when properties are not restored");

    assert_eq!(err, "show failed");
}

#[test]
fn hidden_recovery_reapplies_hidden_properties() {
    let recovery = SurfaceSnapshot {
        mode: SurfaceMode::Hidden,
        target: SurfaceTarget::Summary,
    };

    let mut applied_hidden = false;
    let restored = restore_recovery_surface(&recovery, |properties| {
        applied_hidden = !properties.visible;
        Ok(())
    });

    assert!(restored.is_ok());
    assert!(applied_hidden);
}

#[test]
fn hidden_surface_snapshot_matches_non_visible_shell_state() {
    assert_eq!(
        hidden_surface_snapshot(),
        SurfaceSnapshot {
            mode: SurfaceMode::Hidden,
            target: SurfaceTarget::Summary,
        }
    );
}
