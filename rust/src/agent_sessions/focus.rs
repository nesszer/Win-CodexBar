use super::*;

pub fn focus_session(session: &AgentSession) -> SessionFocusResult {
    match session.focus_target {
        AgentSessionFocusTarget::Transcript { .. } => SessionFocusResult::unsupported(
            "This file-only session has no focusable Windows window.",
        ),
        AgentSessionFocusTarget::None => {
            SessionFocusResult::unsupported("This session has no focus target on Windows.")
        }
        AgentSessionFocusTarget::Process { pid } => {
            if !is_local_host(&session.host) {
                return SessionFocusResult::unsupported(
                    "Remote session focus is not supported from this Windows desktop.",
                );
            }

            focus_process(pid)
        }
    }
}

fn is_local_host(host: &str) -> bool {
    if host.eq_ignore_ascii_case("localhost")
        || host == "127.0.0.1"
        || host == "::1"
        || host.is_empty()
    {
        return true;
    }

    std::env::var("COMPUTERNAME")
        .map(|name| name.eq_ignore_ascii_case(host))
        .unwrap_or(false)
}

#[cfg(windows)]
fn focus_process(pid: u32) -> SessionFocusResult {
    use windows::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetWindowThreadProcessId, IsWindowVisible, SW_RESTORE, SetForegroundWindow,
        ShowWindow,
    };

    struct Search {
        pid: u32,
        window: Option<HWND>,
    }

    unsafe extern "system" fn find_window(hwnd: HWND, data: LPARAM) -> BOOL {
        let search = unsafe { &mut *(data.0 as *mut Search) };
        let mut window_pid = 0;
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut window_pid)) };
        if window_pid == search.pid && unsafe { IsWindowVisible(hwnd).as_bool() } {
            search.window = Some(hwnd);
            return BOOL(0);
        }
        BOOL(1)
    }

    let mut search = Search { pid, window: None };
    let result = unsafe { EnumWindows(Some(find_window), LPARAM(&mut search as *mut _ as isize)) };
    if result.is_err() {
        return SessionFocusResult::failed("Windows could not enumerate application windows.");
    }

    let Some(window) = search.window else {
        return SessionFocusResult::failed("No focusable window was found for this session.");
    };
    unsafe {
        let _ = ShowWindow(window, SW_RESTORE);
        if SetForegroundWindow(window).as_bool() {
            SessionFocusResult::focused()
        } else {
            SessionFocusResult::failed("Windows denied the request to focus this session.")
        }
    }
}

#[cfg(not(windows))]
fn focus_process(_pid: u32) -> SessionFocusResult {
    SessionFocusResult::unsupported("Process focus requires the Windows desktop shell.")
}
