//! Provider-neutral ownership of a short-lived, task-owned interactive child
//! process.
//!
//! A provider decides *whether* to launch a helper and supplies its
//! configuration; this module owns the *lifecycle* of the child it created:
//! PTY start, Windows Job Object containment, terminal drain, readiness
//! introspection, restart, shutdown and Drop cleanup. It never adopts or
//! terminates a process it did not create, so user-owned processes are always
//! isolated from cleanup.

use std::ffi::OsString;
use std::io::{Read, Write};
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::path::PathBuf;
use std::time::Duration;

use windows::Win32::Foundation::{ERROR_INSUFFICIENT_BUFFER, HANDLE};
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCPROW_OWNER_PID, MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_BASIC_LIMIT_INFORMATION, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
};
use windows::core::PCWSTR;

/// Maximum number of terminal cursor-position replies sent to one child.
const MAX_CURSOR_REPLIES: usize = 32;

/// Provider-supplied configuration for one managed child process.
///
/// The provider owns this policy: which executable, arguments and environment
/// to use, and what readiness means for its protocol. The owner only enforces
/// lifecycle and containment.
#[derive(Debug, Clone)]
pub struct ManagedProcessConfig {
    /// Executable to launch.
    pub program: PathBuf,
    /// Arguments passed to the executable.
    pub args: Vec<OsString>,
    /// Additional environment variables for the child.
    pub env: Vec<(OsString, OsString)>,
    /// Working directory, when the provider wants to pin one.
    pub cwd: Option<PathBuf>,
    /// PTY geometry.
    pub pty_rows: u16,
    /// PTY geometry.
    pub pty_cols: u16,
    /// Short label used only in diagnostics (e.g. `"agy"`).
    pub label: String,
}

/// Error raised by the managed-process owner. It stays provider-neutral; the
/// caller maps it into its own error surface.
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ManagedProcessError(String);

impl ManagedProcessError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

/// Result type for managed-process operations.
pub type ManagedProcessResult<T> = Result<T, ManagedProcessError>;

/// RAII owner for the exact child process it started. Dropping it cannot affect
/// any process that was already running.
pub struct ManagedProcess {
    config: ManagedProcessConfig,
    child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
    pid: u32,
    job: Option<OwnedHandle>,
    master: Option<Box<dyn portable_pty::MasterPty + Send>>,
    drain_thread: Option<std::thread::JoinHandle<()>>,
}

impl ManagedProcess {
    /// Start `config.program` in a PTY inside its own kill-on-close job.
    pub fn spawn(config: &ManagedProcessConfig) -> ManagedProcessResult<Self> {
        let job = create_managed_job(&config.label)?;
        let pty_system = portable_pty::native_pty_system();
        let pair = pty_system
            .openpty(portable_pty::PtySize {
                rows: config.pty_rows,
                cols: config.pty_cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|error| {
                ManagedProcessError::new(format!(
                    "Failed to create a terminal for {}: {error}",
                    config.label
                ))
            })?;
        let mut command = portable_pty::CommandBuilder::new(config.program.as_os_str());
        for arg in &config.args {
            command.arg(arg);
        }
        if let Some(cwd) = &config.cwd {
            command.cwd(cwd.as_os_str());
        }
        for (key, value) in &config.env {
            command.env(key, value);
        }

        let reader = pair.master.try_clone_reader().map_err(|error| {
            ManagedProcessError::new(format!(
                "Failed to read the {} terminal: {error}",
                config.label
            ))
        })?;
        let writer = pair.master.take_writer().map_err(|error| {
            ManagedProcessError::new(format!(
                "Failed to open the {} terminal: {error}",
                config.label
            ))
        })?;
        let mut child = pair.slave.spawn_command(command).map_err(|error| {
            ManagedProcessError::new(format!(
                "Failed to launch the {} CLI: {error}",
                config.label
            ))
        })?;
        drop(pair.slave);

        let Some(pid) = child.process_id() else {
            drop(child.kill());
            drop(child.wait());
            return Err(ManagedProcessError::new(format!(
                "Failed to determine the managed {} process id",
                config.label
            )));
        };
        let Some(process_handle) = child.as_raw_handle() else {
            drop(child.kill());
            drop(child.wait());
            return Err(ManagedProcessError::new(format!(
                "Failed to access the managed {} process handle",
                config.label
            )));
        };
        if let Err(error) = assign_process_to_job(&job, process_handle, &config.label) {
            drop(child.kill());
            drop(child.wait());
            return Err(error);
        }
        let drain_thread = spawn_drain_thread(reader, writer);
        Ok(Self {
            config: config.clone(),
            child: Some(child),
            pid,
            job: Some(job),
            master: Some(pair.master),
            drain_thread: Some(drain_thread),
        })
    }

    /// Process id of the owned child.
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Poll the owned child without blocking.
    pub fn try_wait(&mut self) -> ManagedProcessResult<Option<portable_pty::ExitStatus>> {
        self.child
            .as_mut()
            .expect("managed child is present until cleanup")
            .try_wait()
            .map_err(|error| {
                ManagedProcessError::new(format!(
                    "Failed to inspect the {} CLI: {error}",
                    self.config.label
                ))
            })
    }

    /// Candidate IPv4 loopback ports the owned child is currently listening on.
    /// Providers use this to decide when the child's local service is ready.
    pub fn listening_ports(&self) -> ManagedProcessResult<Vec<u16>> {
        listening_ports_for_pid(self.pid)
    }

    /// Terminate and reap the owned child, bounded by `cleanup_reserve`.
    pub async fn shutdown(mut self, cleanup_reserve: Duration) {
        let Some(resources) = self.take_resources() else {
            return;
        };
        let cleanup = tokio::task::spawn_blocking(move || resources.terminate_and_reap());
        // A stuck platform wait must not hold the async provider worker. The
        // blocking cleanup task remains detached and still owns every handle.
        drop(tokio::time::timeout(cleanup_reserve, cleanup).await);
    }

    /// Replace the owned child with a fresh one from the same configuration.
    /// The previous child is terminated and reaped before the new child starts,
    /// so a restart cannot leak a process, job, or drain thread.
    pub fn restart(&mut self) -> ManagedProcessResult<()> {
        if let Some(resources) = self.take_resources() {
            resources.terminate_and_reap();
        }
        let replacement = Self::spawn(&self.config)?;
        *self = replacement;
        Ok(())
    }

    fn take_resources(&mut self) -> Option<ManagedProcessResources> {
        Some(ManagedProcessResources {
            child: self.child.take()?,
            job: Some(
                self.job
                    .take()
                    .expect("managed job is present until cleanup"),
            ),
            master: self.master.take(),
            drain_thread: self.drain_thread.take(),
        })
    }
}

impl Drop for ManagedProcess {
    fn drop(&mut self) {
        let Some(mut resources) = self.take_resources() else {
            return;
        };
        resources.terminate();
        // Drop can run when an outer timeout cancels the fetch. Reaping and
        // joining the terminal drain must therefore never block that worker.
        drop(
            std::thread::Builder::new()
                .name("codexbar-proc-cleanup".to_string())
                .spawn(move || resources.reap()),
        );
    }
}

struct ManagedProcessResources {
    child: Box<dyn portable_pty::Child + Send + Sync>,
    job: Option<OwnedHandle>,
    master: Option<Box<dyn portable_pty::MasterPty + Send>>,
    drain_thread: Option<std::thread::JoinHandle<()>>,
}

impl ManagedProcessResources {
    fn terminate(&mut self) {
        // SAFETY: this job is private to the single process launched above;
        // user-owned processes were never assigned to it.
        let terminated = self
            .job
            .as_ref()
            .is_some_and(|job| unsafe { TerminateJobObject(win_handle(job), 1) }.is_ok());
        // KILL_ON_JOB_CLOSE is the second termination path if the explicit API
        // fails. Close it before wait so a failure cannot strand the reaper.
        drop(self.job.take());
        if !terminated {
            drop(self.child.kill());
        }
    }

    fn reap(mut self) {
        drop(self.child.wait());
        drop(self.master.take());
        if let Some(thread) = self.drain_thread.take() {
            drop(thread.join());
        }
    }

    fn terminate_and_reap(mut self) {
        self.terminate();
        self.reap();
    }
}

/// Drain PTY output without logging it: terminal output can contain account
/// data. Windows ConPTY programs may request the cursor position and wait for a
/// terminal response before continuing initialization, so answer a bounded
/// number of those requests.
fn spawn_drain_thread(
    mut reader: Box<dyn Read + Send>,
    mut writer: Box<dyn Write + Send>,
) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut buffer = [0_u8; 4096];
        let mut tail = Vec::with_capacity(3);
        let mut cursor_replies = 0_usize;
        while let Ok(read) = reader.read(&mut buffer) {
            if read == 0 {
                break;
            }
            let requested = terminal_cursor_position_request_count(&mut tail, &buffer[..read]);
            let allowed = terminal_cursor_reply_allowance(cursor_replies, requested);
            for _ in 0..allowed {
                drop(writer.write_all(b"\x1b[1;1R"));
            }
            if allowed > 0 {
                drop(writer.flush());
                cursor_replies += allowed;
            }
        }
    })
}

fn terminal_cursor_position_request_count(tail: &mut Vec<u8>, chunk: &[u8]) -> usize {
    tail.extend_from_slice(chunk);
    let requested = tail.windows(4).filter(|bytes| *bytes == b"\x1b[6n").count();
    if tail.len() > 3 {
        tail.drain(..tail.len() - 3);
    }
    requested
}

fn terminal_cursor_reply_allowance(sent: usize, requested: usize) -> usize {
    requested.min(MAX_CURSOR_REPLIES.saturating_sub(sent))
}

fn create_managed_job(label: &str) -> ManagedProcessResult<OwnedHandle> {
    // SAFETY: a successful call transfers a unique job handle to this owner.
    let raw = unsafe { CreateJobObjectW(None, PCWSTR::null()) }.map_err(|error| {
        ManagedProcessError::new(format!("Failed to create {label} job: {error}"))
    })?;
    // SAFETY: `raw` is a unique valid handle returned by CreateJobObjectW.
    let job = unsafe { OwnedHandle::from_raw_handle(raw.0) };
    let limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION {
        BasicLimitInformation: JOBOBJECT_BASIC_LIMIT_INFORMATION {
            LimitFlags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            ..Default::default()
        },
        ..Default::default()
    };
    let size = u32::try_from(std::mem::size_of_val(&limits)).map_err(|error| {
        ManagedProcessError::new(format!("Invalid {label} job limit size: {error}"))
    })?;
    // SAFETY: `job` is valid and `limits` is initialized for the requested class.
    unsafe {
        SetInformationJobObject(
            win_handle(&job),
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            size,
        )
    }
    .map_err(|error| {
        ManagedProcessError::new(format!("Failed to configure {label} job: {error}"))
    })?;
    Ok(job)
}

fn assign_process_to_job(
    job: &OwnedHandle,
    process: RawHandle,
    label: &str,
) -> ManagedProcessResult<()> {
    // SAFETY: both handles are valid and remain owned by their respective wrappers.
    unsafe { AssignProcessToJobObject(win_handle(job), HANDLE(process)) }.map_err(|error| {
        ManagedProcessError::new(format!("Failed to contain {label} process: {error}"))
    })
}

fn win_handle(value: &OwnedHandle) -> HANDLE {
    HANDLE(value.as_raw_handle())
}

/// Enumerate IPv4 TCP listener ports for a PID through the Windows IP Helper API.
/// This avoids starting PowerShell inside a readiness poll.
///
/// Known follow-up: only the AF_INET table is enumerated and candidates are
/// probed at `127.0.0.1`, so an IPv6-only loopback listener would be missed.
/// Accepted for now because the managed `agy` service is observed to bind IPv4
/// on Windows; add AF_INET6 enumeration with `[::1]` probes later.
pub fn listening_ports_for_pid(pid: u32) -> ManagedProcessResult<Vec<u16>> {
    const AF_INET_FAMILY: u32 = 2;
    const NO_ERROR: u32 = 0;

    let mut bytes = 0_u32;
    // SAFETY: the first call supplies no destination buffer and only asks Windows
    // for the required byte count.
    let query = unsafe {
        GetExtendedTcpTable(
            None,
            &mut bytes,
            false,
            AF_INET_FAMILY,
            TCP_TABLE_OWNER_PID_LISTENER,
            0,
        )
    };
    if query != ERROR_INSUFFICIENT_BUFFER.0 && query != NO_ERROR {
        return Err(ManagedProcessError::new(format!(
            "Failed to size the Windows TCP listener table (error {query})"
        )));
    }
    if bytes < u32::try_from(std::mem::size_of::<u32>()).unwrap_or(u32::MAX) {
        return Ok(Vec::new());
    }

    let mut buffer = Vec::new();
    let mut loaded = false;
    // The table can grow between the sizing call and the read. Retry with
    // the updated size instead of failing on that benign race.
    for _ in 0..3 {
        // A u32 allocation supplies the alignment required by the all-DWORD MIB rows.
        let words = (bytes as usize).div_ceil(std::mem::size_of::<u32>());
        buffer.resize(words, 0_u32);
        // SAFETY: `buffer` is writable for at least `bytes` bytes and remains alive
        // while the returned table is inspected.
        let result = unsafe {
            GetExtendedTcpTable(
                Some(buffer.as_mut_ptr().cast()),
                &mut bytes,
                false,
                AF_INET_FAMILY,
                TCP_TABLE_OWNER_PID_LISTENER,
                0,
            )
        };
        if result == NO_ERROR {
            loaded = true;
            break;
        }
        if result != ERROR_INSUFFICIENT_BUFFER.0 {
            return Err(ManagedProcessError::new(format!(
                "Failed to read the Windows TCP listener table (error {result})"
            )));
        }
    }
    if !loaded {
        return Err(ManagedProcessError::new(
            "Windows TCP listener table kept changing during the query".to_string(),
        ));
    }

    let table = buffer.as_ptr().cast::<MIB_TCPTABLE_OWNER_PID>();
    // SAFETY: Windows initialized the header on the successful call above.
    let count = unsafe { (*table).dwNumEntries as usize };
    let rows_offset = std::mem::offset_of!(MIB_TCPTABLE_OWNER_PID, table);
    let available = (bytes as usize).saturating_sub(rows_offset);
    let max_rows = available / std::mem::size_of::<MIB_TCPROW_OWNER_PID>();
    if count > max_rows {
        return Err(ManagedProcessError::new(
            "Windows returned an invalid TCP listener table".to_string(),
        ));
    }
    // SAFETY: `count` was bounded by the returned buffer size. Windows lays out
    // the fixed-size owner-PID rows consecutively after the table header.
    let rows = unsafe {
        std::slice::from_raw_parts(
            std::ptr::addr_of!((*table).table).cast::<MIB_TCPROW_OWNER_PID>(),
            count,
        )
    };
    let mut ports: Vec<u16> = rows
        .iter()
        .filter(|row| row.dwOwningPid == pid)
        .filter_map(|row| u16::try_from(row.dwLocalPort).ok())
        .map(u16::from_be)
        .filter(|port| *port != 0)
        .collect();
    ports.sort_unstable();
    ports.dedup();
    Ok(ports)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn test_config() -> ManagedProcessConfig {
        ManagedProcessConfig {
            program: PathBuf::from("powershell.exe"),
            args: vec![
                OsString::from("-NoLogo"),
                OsString::from("-NoProfile"),
                OsString::from("-NonInteractive"),
                OsString::from("-Command"),
                OsString::from("Start-Sleep -Seconds 30"),
            ],
            env: Vec::new(),
            cwd: None,
            pty_rows: 30,
            pty_cols: 120,
            label: "test".to_string(),
        }
    }

    fn start_test_process() -> ManagedProcess {
        ManagedProcess::spawn(&test_config()).expect("start a managed test process")
    }

    fn wait_for_exit(pid: u32) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while process_is_alive(pid) {
            assert!(
                Instant::now() < deadline,
                "managed child {pid} was not cleaned up"
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    fn process_is_alive(pid: u32) -> bool {
        use windows::Win32::Foundation::{CloseHandle, WAIT_OBJECT_0};
        use windows::Win32::System::Threading::{
            OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, WaitForSingleObject,
        };

        // SAFETY: OpenProcess returns a handle owned by this function and closed below.
        match unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) } {
            Ok(handle) => {
                // SAFETY: `handle` is a valid process handle.
                let exited = unsafe { WaitForSingleObject(handle, 0) } == WAIT_OBJECT_0;
                // SAFETY: closing the handle returned by OpenProcess exactly once.
                drop(unsafe { CloseHandle(handle) });
                !exited
            }
            Err(_) => false,
        }
    }

    #[test]
    fn terminal_detects_cursor_request_across_reads() {
        let mut tail = Vec::new();

        assert_eq!(
            terminal_cursor_position_request_count(&mut tail, b"ready\x1b["),
            0
        );
        assert_eq!(terminal_cursor_position_request_count(&mut tail, b"6n"), 1);
        assert_eq!(
            terminal_cursor_position_request_count(&mut tail, b"plain output"),
            0
        );
        assert_eq!(
            terminal_cursor_position_request_count(&mut tail, b"\x1b[6nmore\x1b[6n"),
            2
        );
    }

    #[test]
    fn terminal_caps_cursor_replies() {
        assert_eq!(terminal_cursor_reply_allowance(0, 2), 2);
        assert_eq!(terminal_cursor_reply_allowance(31, 4), 1);
        assert_eq!(terminal_cursor_reply_allowance(MAX_CURSOR_REPLIES, 1), 0);
    }

    #[test]
    fn listener_table_finds_current_process_port() {
        let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .expect("bind a local IPv4 listener");
        let port = listener.local_addr().expect("listener address").port();

        let ports = listening_ports_for_pid(std::process::id())
            .expect("read the Windows TCP listener table");

        assert!(
            ports.contains(&port),
            "listener table should contain {port}"
        );
    }

    #[test]
    fn managed_job_terminates_its_owned_process() {
        use std::os::windows::io::AsRawHandle as _;
        use std::os::windows::process::CommandExt as _;

        const CREATE_NO_WINDOW: u32 = 0x08000000;
        let mut command = std::process::Command::new("powershell.exe");
        command
            .args([
                "-NoLogo",
                "-NoProfile",
                "-Command",
                "Start-Sleep -Seconds 30",
            ])
            .creation_flags(CREATE_NO_WINDOW);
        let mut child = command.spawn().expect("spawn an isolated test child");
        let job = create_managed_job("test").expect("create a kill-on-close job");
        if let Err(error) = assign_process_to_job(&job, child.as_raw_handle(), "test") {
            drop(child.kill());
            drop(child.wait());
            panic!("assign the test child to its job: {error}");
        }

        // SAFETY: only the isolated test child was assigned to this private job.
        unsafe { TerminateJobObject(win_handle(&job), 1) }.expect("terminate the private job");
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if child.try_wait().expect("inspect the test child").is_some() {
                break;
            }
            if Instant::now() >= deadline {
                drop(child.kill());
                drop(child.wait());
                panic!("job termination did not stop the test child");
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    #[tokio::test]
    async fn managed_process_shutdown_stops_owned_child() {
        let mut process = start_test_process();
        let pid = process.pid();
        assert!(pid > 0, "managed process exposes its pid");
        assert!(
            process.try_wait().expect("poll the child").is_none(),
            "managed child is running before shutdown"
        );

        process.shutdown(Duration::from_secs(5)).await;

        assert!(!process_is_alive(pid), "shutdown stops the owned child");
    }

    #[test]
    fn managed_process_drop_terminates_owned_child() {
        let process = start_test_process();
        let pid = process.pid();
        assert!(pid > 0, "managed process exposes its pid");

        drop(process);

        // Drop terminates synchronously and detaches reaping; wait for the OS
        // to report the process gone so the test does not race the cleanup thread.
        wait_for_exit(pid);
    }

    #[test]
    fn managed_process_restart_replaces_without_leaking() {
        let mut process = start_test_process();
        let first = process.pid();

        process.restart().expect("restart the managed child");
        let second = process.pid();

        assert_ne!(first, second, "restart launches a fresh child");
        assert!(!process_is_alive(first), "restart reaps the previous child");
        assert!(
            process.try_wait().expect("poll the replacement").is_none(),
            "replacement is running after restart"
        );

        drop(process);
        wait_for_exit(second);
    }
}
