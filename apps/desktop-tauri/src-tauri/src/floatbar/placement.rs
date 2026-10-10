//! Keep the FloatBar where the user put it (issue #625).
//!
//! Windows moves top-level windows on its own when the display layout
//! changes: sleep/wake, a monitor that drops off and comes back, a DPI or
//! resolution change, or an Explorer restart that briefly changes the work
//! area. A bar parked over the taskbar then lands above it. The old event
//! handler saved every `Moved` position, so the system's move overwrote the
//! user's placement for good.
//!
//! The tracker separates the two kinds of move. A move made while a mouse
//! button is held (or right after one) is the user dragging the bar and
//! becomes the user placement. Any other move is a system relocation: it is
//! never persisted, and once the monitor layout is back to the layout the user
//! placed the bar on, and has been quiet for [`SETTLE`], the bar is moved back.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::{PhysicalPosition, PhysicalSize};

/// How long the layout and the bar must stay still before a displaced bar is
/// moved back. Long enough to let Windows finish a display transition.
pub(super) const SETTLE: Duration = Duration::from_secs(3);

/// Moves this soon after a user drag move still belong to that drag (the
/// last `Moved` of a drag can be delivered after the button is released).
const DRAG_TAIL: Duration = Duration::from_millis(500);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct MonitorRect {
    pub position: PhysicalPosition<i32>,
    pub size: PhysicalSize<u32>,
    pub work_position: PhysicalPosition<i32>,
    pub work_size: PhysicalSize<u32>,
    /// Scale factor in thousandths so the layout compares exactly.
    pub scale_milli: u32,
}

/// Bounds, work area and scale of every connected display, sorted so the
/// enumeration order does not matter.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct MonitorLayout(Vec<MonitorRect>);

impl MonitorLayout {
    pub(super) fn new(mut monitors: Vec<MonitorRect>) -> Self {
        monitors.sort_by_key(|m| (m.position.x, m.position.y, m.size.width, m.size.height));
        Self(monitors)
    }

    pub(super) fn from_monitors(monitors: &[tauri::Monitor]) -> Self {
        Self::new(
            monitors
                .iter()
                .map(|monitor| MonitorRect {
                    position: *monitor.position(),
                    size: *monitor.size(),
                    work_position: monitor.work_area().position,
                    work_size: monitor.work_area().size,
                    scale_milli: scale_milli(monitor.scale_factor()),
                })
                .collect(),
        )
    }
}

fn scale_milli(scale_factor: f64) -> u32 {
    (crate::window_positioner::safe_scale(scale_factor) * 1000.0).round() as u32
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MoveKind {
    /// The user dragged the bar: remember and persist this position.
    User,
    /// Windows (or a recovery) moved the bar: keep the user placement.
    System,
}

#[derive(Debug, Default)]
pub(super) struct PlacementTracker {
    layout: Option<MonitorLayout>,
    layout_changed_at: Option<Instant>,
    last_move_at: Option<Instant>,
    last_user_move_at: Option<Instant>,
    user: Option<(MonitorLayout, PhysicalPosition<i32>)>,
}

impl PlacementTracker {
    fn observe_layout(&mut self, layout: &MonitorLayout, now: Instant) {
        match &self.layout {
            Some(previous) if previous == layout => {}
            Some(_) => {
                self.layout = Some(layout.clone());
                self.layout_changed_at = Some(now);
            }
            None => self.layout = Some(layout.clone()),
        }
    }

    fn within(since: Option<Instant>, now: Instant, window: Duration) -> bool {
        since.is_some_and(|at| now.saturating_duration_since(at) < window)
    }

    /// The bar was placed at `position` deliberately (shown at its stored or
    /// default spot): that is the user placement for `layout`.
    pub(super) fn record_user(
        &mut self,
        layout: MonitorLayout,
        position: PhysicalPosition<i32>,
        now: Instant,
    ) {
        self.observe_layout(&layout, now);
        self.user = Some((layout, position));
    }

    /// Classify a `Moved`/`Resized` event at `position`.
    pub(super) fn classify_move(
        &mut self,
        layout: MonitorLayout,
        position: PhysicalPosition<i32>,
        pointer_down: bool,
        now: Instant,
    ) -> MoveKind {
        self.observe_layout(&layout, now);
        self.last_move_at = Some(now);
        if self
            .user
            .as_ref()
            .is_some_and(|(_, user)| *user == position)
        {
            // A resize in place, or the bar moved back to the placement.
            return MoveKind::User;
        }
        if pointer_down || Self::within(self.last_user_move_at, now, DRAG_TAIL) {
            self.last_user_move_at = Some(now);
            self.user = Some((layout, position));
            MoveKind::User
        } else {
            MoveKind::System
        }
    }

    /// True when `position` is the user placement (or none is known), so
    /// persisting it cannot overwrite the user's choice with a system move.
    pub(super) fn is_user_position(&self, position: PhysicalPosition<i32>) -> bool {
        self.user.as_ref().is_none_or(|(_, user)| *user == position)
    }

    /// Where to move a displaced bar back to, if anywhere: only when the
    /// layout equals the one the user placed it on and both the layout and
    /// the bar have been still for [`SETTLE`].
    pub(super) fn restore_target(
        &mut self,
        layout: MonitorLayout,
        current: PhysicalPosition<i32>,
        now: Instant,
    ) -> Option<PhysicalPosition<i32>> {
        self.observe_layout(&layout, now);
        if Self::within(self.layout_changed_at, now, SETTLE)
            || Self::within(self.last_move_at, now, SETTLE)
        {
            return None;
        }
        let (user_layout, user_position) = self.user.as_ref()?;
        (*user_layout == layout && *user_position != current).then_some(*user_position)
    }

    /// The bar was closed: forget the session state but keep nothing that
    /// could outlive the window.
    pub(super) fn reset(&mut self) {
        *self = Self::default();
    }
}

static TRACKER: Mutex<Option<PlacementTracker>> = Mutex::new(None);

pub(super) fn with_tracker<T>(f: impl FnOnce(&mut PlacementTracker) -> T) -> T {
    let mut guard = TRACKER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    f(guard.get_or_insert_with(PlacementTracker::default))
}

/// True while a physical mouse button is held. A FloatBar drag always holds
/// one (left, or right with swapped buttons); a system relocation does not.
#[cfg(windows)]
pub(super) fn pointer_button_down() -> bool {
    const VK_LBUTTON: i32 = 0x01;
    const VK_RBUTTON: i32 = 0x02;
    // SAFETY: GetAsyncKeyState takes a virtual-key code and reads global
    // input state; it has no pointer arguments.
    unsafe {
        (GetAsyncKeyState(VK_LBUTTON) as u16 & 0x8000) != 0
            || (GetAsyncKeyState(VK_RBUTTON) as u16 & 0x8000) != 0
    }
}

/// Elsewhere every move counts as a user move (the previous behaviour).
#[cfg(not(windows))]
pub(super) fn pointer_button_down() -> bool {
    true
}

#[cfg(windows)]
#[link(name = "user32")]
unsafe extern "system" {
    fn GetAsyncKeyState(virtual_key: i32) -> i16;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn monitor(height: u32, work_height: u32) -> MonitorRect {
        MonitorRect {
            position: PhysicalPosition::new(0, 0),
            size: PhysicalSize::new(1_920, height),
            work_position: PhysicalPosition::new(0, 0),
            work_size: PhysicalSize::new(1_920, work_height),
            scale_milli: 1_000,
        }
    }

    fn desk() -> MonitorLayout {
        MonitorLayout::new(vec![monitor(1_080, 1_032)])
    }

    /// The display shrinks while the screen sleeps or reconnects.
    fn transient() -> MonitorLayout {
        MonitorLayout::new(vec![monitor(768, 720)])
    }

    const OVER_TASKBAR: PhysicalPosition<i32> = PhysicalPosition::new(1_200, 1_044);
    const ABOVE_TASKBAR: PhysicalPosition<i32> = PhysicalPosition::new(1_200, 676);

    #[test]
    fn system_move_during_a_display_change_keeps_and_restores_the_user_placement() {
        let start = Instant::now();
        let mut tracker = PlacementTracker::default();
        tracker.record_user(desk(), OVER_TASKBAR, start);

        // Windows pushes the bar into the shrunken work area: no button held.
        let at = start + Duration::from_secs(10);
        assert_eq!(
            tracker.classify_move(transient(), ABOVE_TASKBAR, false, at),
            MoveKind::System
        );
        assert!(!tracker.is_user_position(ABOVE_TASKBAR));

        // Still on the transient layout: nothing to restore.
        assert_eq!(
            tracker.restore_target(transient(), ABOVE_TASKBAR, at + SETTLE * 2),
            None
        );

        // The display comes back; wait for it to settle before moving.
        let back = at + SETTLE * 3;
        assert_eq!(tracker.restore_target(desk(), ABOVE_TASKBAR, back), None);
        assert_eq!(
            tracker.restore_target(desk(), ABOVE_TASKBAR, back + SETTLE),
            Some(OVER_TASKBAR)
        );
    }

    #[test]
    fn system_move_without_a_layout_change_is_also_undone() {
        let start = Instant::now();
        let mut tracker = PlacementTracker::default();
        tracker.record_user(desk(), OVER_TASKBAR, start);

        let at = start + Duration::from_secs(10);
        assert_eq!(
            tracker.classify_move(desk(), ABOVE_TASKBAR, false, at),
            MoveKind::System
        );
        assert_eq!(tracker.restore_target(desk(), ABOVE_TASKBAR, at), None);
        assert_eq!(
            tracker.restore_target(desk(), ABOVE_TASKBAR, at + SETTLE),
            Some(OVER_TASKBAR)
        );
    }

    #[test]
    fn user_drag_becomes_the_new_placement_including_its_tail() {
        let start = Instant::now();
        let mut tracker = PlacementTracker::default();
        tracker.record_user(desk(), OVER_TASKBAR, start);

        let at = start + Duration::from_secs(10);
        let mid = PhysicalPosition::new(900, 500);
        let end = PhysicalPosition::new(600, 300);
        assert_eq!(tracker.classify_move(desk(), mid, true, at), MoveKind::User);
        // The final Moved arrives just after the button is released.
        assert_eq!(
            tracker.classify_move(desk(), end, false, at + Duration::from_millis(100)),
            MoveKind::User
        );
        assert!(tracker.is_user_position(end));
        assert_eq!(tracker.restore_target(desk(), end, at + SETTLE * 2), None);
    }

    #[test]
    fn restore_waits_for_the_bar_to_stop_moving_and_needs_a_known_placement() {
        let start = Instant::now();
        let mut tracker = PlacementTracker::default();
        assert_eq!(tracker.restore_target(desk(), ABOVE_TASKBAR, start), None);

        tracker.record_user(desk(), OVER_TASKBAR, start);
        let at = start + Duration::from_secs(10);
        tracker.classify_move(desk(), ABOVE_TASKBAR, false, at);
        let later = at + Duration::from_secs(1);
        tracker.classify_move(desk(), ABOVE_TASKBAR, false, later);
        assert_eq!(
            tracker.restore_target(desk(), ABOVE_TASKBAR, at + SETTLE),
            None
        );
        assert_eq!(
            tracker.restore_target(desk(), ABOVE_TASKBAR, later + SETTLE),
            Some(OVER_TASKBAR)
        );
        // Resetting (bar closed) forgets the placement.
        tracker.reset();
        assert_eq!(
            tracker.restore_target(desk(), ABOVE_TASKBAR, later + SETTLE * 4),
            None
        );
    }

    #[test]
    fn layout_ignores_enumeration_order_and_sees_scale_changes() {
        let left = MonitorRect {
            position: PhysicalPosition::new(-1_920, 0),
            ..monitor(1_080, 1_032)
        };
        let main = monitor(1_080, 1_032);
        assert_eq!(
            MonitorLayout::new(vec![left, main]),
            MonitorLayout::new(vec![main, left])
        );
        let scaled = MonitorRect {
            scale_milli: scale_milli(1.25),
            ..main
        };
        assert_ne!(
            MonitorLayout::new(vec![main]),
            MonitorLayout::new(vec![scaled])
        );
        assert_eq!(scale_milli(f64::NAN), 1_000);
    }
}
