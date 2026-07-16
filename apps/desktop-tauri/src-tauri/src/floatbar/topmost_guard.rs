//! Event-driven Windows z-order repair for a floatbar placed over the taskbar.
//!
//! The Windows taskbar is itself a topmost shell window. Activating or
//! reordering it can therefore put it in front of another topmost window
//! without removing that other window's `WS_EX_TOPMOST` style. We listen for
//! the two shell events that can produce that ordering change and re-assert the
//! floatbar only when it actually overlaps a taskbar.

#[cfg(any(windows, test))]
const EVENT_SYSTEM_FOREGROUND: u32 = 0x0003;
#[cfg(any(windows, test))]
const EVENT_OBJECT_REORDER: u32 = 0x8004;

#[cfg(any(windows, test))]
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct Rect {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

#[cfg(any(windows, test))]
fn rects_overlap(a: Rect, b: Rect) -> bool {
    a.left < b.right && a.right > b.left && a.top < b.bottom && a.bottom > b.top
}

#[cfg(any(windows, test))]
fn is_relevant_event(event: u32) -> bool {
    event == EVENT_SYSTEM_FOREGROUND || event == EVENT_OBJECT_REORDER
}

#[cfg(windows)]
mod platform {
    use super::{
        EVENT_OBJECT_REORDER, EVENT_SYSTEM_FOREGROUND, Rect, is_relevant_event, rects_overlap,
    };
    use raw_window_handle::HasWindowHandle;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Mutex, OnceLock};
    use std::time::Duration;
    use tauri::Manager;

    use super::super::window::{self, FLOATBAR_LABEL};

    const WINEVENT_OUTOFCONTEXT: u32 = 0x0000;
    const WINEVENT_SKIPOWNPROCESS: u32 = 0x0002;
    const REASSERT_DELAY: Duration = Duration::from_millis(40);

    static APP_HANDLE: OnceLock<tauri::AppHandle> = OnceLock::new();
    static HOOKS: OnceLock<Mutex<Vec<isize>>> = OnceLock::new();
    static FLOATBAR_ACTIVE: AtomicBool = AtomicBool::new(false);
    static REASSERT_PENDING: AtomicBool = AtomicBool::new(false);

    pub fn install(app: &tauri::AppHandle) {
        let _ = APP_HANDLE.set(app.clone());
        let hooks = HOOKS.get_or_init(|| Mutex::new(Vec::new()));
        let mut hooks = hooks.lock().unwrap();
        if !hooks.is_empty() {
            return;
        }

        for event in [EVENT_SYSTEM_FOREGROUND, EVENT_OBJECT_REORDER] {
            let hook = unsafe {
                SetWinEventHook(
                    event,
                    event,
                    0,
                    Some(win_event_proc),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                )
            };
            if hook == 0 {
                tracing::warn!(
                    event,
                    error = %std::io::Error::last_os_error(),
                    "failed to install floatbar z-order event hook"
                );
            } else {
                hooks.push(hook);
            }
        }
    }

    unsafe extern "system" fn win_event_proc(
        _hook: isize,
        event: u32,
        _hwnd: isize,
        _object_id: i32,
        _child_id: i32,
        _event_thread: u32,
        _event_time: u32,
    ) {
        if FLOATBAR_ACTIVE.load(Ordering::Acquire) && is_relevant_event(event) {
            schedule_reassert();
        }
    }

    pub fn set_active(active: bool) {
        FLOATBAR_ACTIVE.store(active, Ordering::Release);
    }

    fn schedule_reassert() {
        if REASSERT_PENDING.swap(true, Ordering::AcqRel) {
            return;
        }
        let Some(app) = APP_HANDLE.get().cloned() else {
            REASSERT_PENDING.store(false, Ordering::Release);
            return;
        };

        tauri::async_runtime::spawn(async move {
            tokio::time::sleep(REASSERT_DELAY).await;
            let dispatcher = app.clone();
            let result = dispatcher.run_on_main_thread(move || {
                if let Some(floatbar) = app.get_webview_window(FLOATBAR_LABEL)
                    && floatbar.is_visible().unwrap_or(false)
                    && overlaps_taskbar(&floatbar)
                {
                    window::apply_always_on_top(&floatbar);
                }
                REASSERT_PENDING.store(false, Ordering::Release);
            });
            if result.is_err() {
                REASSERT_PENDING.store(false, Ordering::Release);
            }
        });
    }

    fn overlaps_taskbar(window: &tauri::WebviewWindow) -> bool {
        let Ok(handle) = window.window_handle() else {
            return false;
        };
        let raw_window_handle::RawWindowHandle::Win32(handle) = handle.as_raw() else {
            return false;
        };

        let mut floatbar_rect = Rect::default();
        if unsafe { GetWindowRect(handle.hwnd.get(), &mut floatbar_rect) } == 0 {
            return false;
        }

        taskbar_rects()
            .into_iter()
            .any(|taskbar_rect| rects_overlap(floatbar_rect, taskbar_rect))
    }

    fn taskbar_rects() -> Vec<Rect> {
        let primary_class = wide("Shell_TrayWnd");
        let secondary_class = wide("Shell_SecondaryTrayWnd");
        let mut rects = Vec::new();

        let primary = unsafe { FindWindowW(primary_class.as_ptr(), std::ptr::null()) };
        push_window_rect(primary, &mut rects);

        let mut previous = 0;
        loop {
            let taskbar =
                unsafe { FindWindowExW(0, previous, secondary_class.as_ptr(), std::ptr::null()) };
            if taskbar == 0 {
                break;
            }
            push_window_rect(taskbar, &mut rects);
            previous = taskbar;
        }

        rects
    }

    fn push_window_rect(hwnd: isize, rects: &mut Vec<Rect>) {
        if hwnd == 0 || unsafe { IsWindowVisible(hwnd) } == 0 {
            return;
        }
        let mut rect = Rect::default();
        if unsafe { GetWindowRect(hwnd, &mut rect) } != 0 {
            rects.push(rect);
        }
    }

    fn wide(value: &str) -> Vec<u16> {
        value.encode_utf16().chain(std::iter::once(0)).collect()
    }

    type WinEventProc = unsafe extern "system" fn(isize, u32, isize, i32, i32, u32, u32);

    #[link(name = "user32")]
    unsafe extern "system" {
        fn SetWinEventHook(
            event_min: u32,
            event_max: u32,
            module: isize,
            callback: Option<WinEventProc>,
            process_id: u32,
            thread_id: u32,
            flags: u32,
        ) -> isize;
        fn FindWindowW(class_name: *const u16, window_name: *const u16) -> isize;
        fn FindWindowExW(
            parent: isize,
            child_after: isize,
            class_name: *const u16,
            window_name: *const u16,
        ) -> isize;
        fn GetWindowRect(hwnd: isize, rect: *mut Rect) -> i32;
        fn IsWindowVisible(hwnd: isize) -> i32;
    }
}

#[cfg(windows)]
pub fn install(app: &tauri::AppHandle) {
    platform::install(app);
}

#[cfg(windows)]
pub fn set_active(active: bool) {
    platform::set_active(active);
}

#[cfg(not(windows))]
pub fn install(_app: &tauri::AppHandle) {}

#[cfg(not(windows))]
pub fn set_active(_active: bool) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn overlap_requires_positive_area() {
        let taskbar = Rect {
            left: 0,
            top: 1040,
            right: 1920,
            bottom: 1080,
        };
        assert!(rects_overlap(
            Rect {
                left: 800,
                top: 1044,
                right: 1120,
                bottom: 1076,
            },
            taskbar
        ));
        assert!(!rects_overlap(
            Rect {
                left: 800,
                top: 1000,
                right: 1120,
                bottom: 1040,
            },
            taskbar
        ));
    }

    #[test]
    fn foreground_and_window_reorder_events_are_relevant() {
        assert!(is_relevant_event(EVENT_SYSTEM_FOREGROUND));
        assert!(is_relevant_event(EVENT_OBJECT_REORDER));
        assert!(!is_relevant_event(0x800B));
    }
}
