//! Best-effort coding-agent process detection for Adaptive refresh (Windows).
//!
//! Looks for well-known agent process names. Does not inspect command lines or
//! env (no secret leakage). Consent is the Settings Adaptive toggle itself.

/// Process base names (lowercase, without `.exe`) that count as coding activity.
const AGENT_PROCESS_NAMES: &[&str] = &[
    "claude",
    "codex",
    "cursor",
    "cursor-agent",
    "gemini",
    "opencode",
    "aider",
    "continue",
    "windsurf",
    "codeium",
    "copilot",
];

/// Returns true when at least one known coding-agent process is running.
pub fn coding_agent_process_active() -> bool {
    #[cfg(windows)]
    {
        windows_agent_running()
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(windows)]
fn windows_agent_running() -> bool {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStringExt;

    #[repr(C)]
    struct ProcessEntry32W {
        dw_size: u32,
        cnt_usage: u32,
        th32_process_id: u32,
        th32_default_heap_id: usize,
        th32_module_id: u32,
        cnt_threads: u32,
        th32_parent_process_id: u32,
        pc_pri_class_base: i32,
        dw_flags: u32,
        sz_exe_file: [u16; 260],
    }

    const TH32CS_SNAPPROCESS: u32 = 0x0000_0002;
    const INVALID_HANDLE_VALUE: *mut std::ffi::c_void = -1isize as *mut std::ffi::c_void;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn CreateToolhelp32Snapshot(
            dw_flags: u32,
            th32_process_id: u32,
        ) -> *mut std::ffi::c_void;
        fn Process32FirstW(snapshot: *mut std::ffi::c_void, entry: *mut ProcessEntry32W) -> i32;
        fn Process32NextW(snapshot: *mut std::ffi::c_void, entry: *mut ProcessEntry32W) -> i32;
        fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
    }

    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap.is_null() || snap == INVALID_HANDLE_VALUE {
            return false;
        }
        let mut entry = ProcessEntry32W {
            dw_size: std::mem::size_of::<ProcessEntry32W>() as u32,
            cnt_usage: 0,
            th32_process_id: 0,
            th32_default_heap_id: 0,
            th32_module_id: 0,
            cnt_threads: 0,
            th32_parent_process_id: 0,
            pc_pri_class_base: 0,
            dw_flags: 0,
            sz_exe_file: [0; 260],
        };
        let mut found = false;
        if Process32FirstW(snap, &mut entry) != 0 {
            loop {
                let len = entry
                    .sz_exe_file
                    .iter()
                    .position(|&c| c == 0)
                    .unwrap_or(entry.sz_exe_file.len());
                let name = OsString::from_wide(&entry.sz_exe_file[..len])
                    .to_string_lossy()
                    .to_ascii_lowercase();
                let stem = name.strip_suffix(".exe").unwrap_or(&name);
                if AGENT_PROCESS_NAMES.iter().any(|n| *n == stem) {
                    found = true;
                    break;
                }
                if Process32NextW(snap, &mut entry) == 0 {
                    break;
                }
            }
        }
        let _ = CloseHandle(snap);
        found
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_name_list_is_nonempty() {
        assert!(!AGENT_PROCESS_NAMES.is_empty());
        assert!(AGENT_PROCESS_NAMES.contains(&"claude"));
        assert!(AGENT_PROCESS_NAMES.contains(&"codex"));
    }

    #[test]
    fn detection_does_not_panic() {
        let _ = coding_agent_process_active();
    }
}
