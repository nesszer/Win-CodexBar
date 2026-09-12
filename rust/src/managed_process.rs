//! Provider-neutral ownership of a short-lived, task-owned interactive child
//! process.
//!
//! A provider decides *whether* to launch a helper and supplies its
//! configuration; this module owns the *lifecycle* of the child it created:
//! PTY start, Windows Job Object containment, terminal drain, readiness
//! introspection, restart, shutdown and Drop cleanup. It never adopts or
//! terminates a process it did not create, so user-owned processes are always
//! isolated from cleanup.
//!
//! The child joins its kill-on-close job through the process-creation
//! `PROC_THREAD_ATTRIBUTE_JOB_LIST` attribute, so containment is atomic: no
//! thread of the child (or a descendant it starts) can run before the kernel
//! has bound the process tree to the job. The PTY is attached with the
//! `PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE` attribute in the same creation call,
//! preserving the previous ConPTY behavior.

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle};
use std::path::{Path, PathBuf};
use std::time::Duration;

use windows::Win32::Foundation::{HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::NetworkManagement::IpHelper::{
    GetExtendedTcpTable, MIB_TCPROW_OWNER_PID, MIB_TCPTABLE_OWNER_PID, TCP_TABLE_OWNER_PID_LISTENER,
};
use windows::Win32::System::Console::{
    ClosePseudoConsole, CreatePseudoConsole, HPCON, PSEUDOCONSOLE_INHERIT_CURSOR,
};
#[cfg(test)]
use windows::Win32::System::JobObjects::AssignProcessToJobObject;
use windows::Win32::System::JobObjects::{
    CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JOBOBJECT_BASIC_LIMIT_INFORMATION,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject, TerminateJobObject,
};
use windows::Win32::System::Pipes::CreatePipe;
use windows::Win32::System::Threading::{
    CREATE_UNICODE_ENVIRONMENT, CreateProcessW, DeleteProcThreadAttributeList,
    EXTENDED_STARTUPINFO_PRESENT, GetExitCodeProcess, GetProcessId, INFINITE,
    InitializeProcThreadAttributeList, LPPROC_THREAD_ATTRIBUTE_LIST,
    PROC_THREAD_ATTRIBUTE_JOB_LIST, PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE, PROCESS_INFORMATION,
    STARTF_USESTDHANDLES, STARTUPINFOEXW, STARTUPINFOW, TerminateProcess,
    UpdateProcThreadAttribute, WaitForSingleObject,
};
use windows::core::{PCWSTR, PWSTR};

/// Maximum number of terminal cursor-position replies sent to one child.
const MAX_CURSOR_REPLIES: usize = 32;

/// `GetExitCodeProcess` value reported while the process is still running.
const STILL_ACTIVE: u32 = 259;

/// ConPTY flags portable-pty applies to preserve interactive startup behavior.
/// The `windows` crate only names `PSEUDOCONSOLE_INHERIT_CURSOR`.
const PSEUDOCONSOLE_RESIZE_QUIRK: u32 = 0x2;
const PSEUDOCONSOLE_WIN32_INPUT_MODE: u32 = 0x4;

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
    child: Option<ManagedChild>,
    pid: u32,
    job: Option<OwnedHandle>,
    master: Option<PseudoConsole>,
    drain_thread: Option<std::thread::JoinHandle<()>>,
}

impl ManagedProcess {
    /// Start `config.program` in a PTY inside its own kill-on-close job.
    ///
    /// The job and the pseudoconsole are both supplied as process-creation
    /// attributes, so the child is contained before any of its threads execute
    /// and cannot spawn an uncontained descendant during startup.
    pub fn spawn(config: &ManagedProcessConfig) -> ManagedProcessResult<Self> {
        let job = create_managed_job(&config.label)?;
        let (mut pty, child) = spawn_pty_child(config, &job)?;
        let pid = child.process_id();
        if pid == 0 {
            // The job owns the child now; dropping it closes the job handle and
            // its KILL_ON_JOB_CLOSE limit terminates the untracked process.
            return Err(ManagedProcessError::new(format!(
                "Failed to determine the managed {} process id",
                config.label
            )));
        }

        let reader = pty.try_clone_reader().map_err(|error| {
            ManagedProcessError::new(format!(
                "Failed to read the {} terminal: {error}",
                config.label
            ))
        })?;
        let writer = pty.take_writer().ok_or_else(|| {
            ManagedProcessError::new(format!("Failed to open the {} terminal", config.label))
        })?;
        let drain_thread = spawn_drain_thread(reader, writer);
        Ok(Self {
            config: config.clone(),
            child: Some(child),
            pid,
            job: Some(job),
            master: Some(pty),
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
            .map(|status| status.map(portable_pty::ExitStatus::with_exit_code))
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
    child: ManagedChild,
    job: Option<OwnedHandle>,
    master: Option<PseudoConsole>,
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
            self.child.kill();
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

/// The exact process created for this owner. It holds the only process handle
/// that cleanup waits on; containment comes from the job bound at creation.
struct ManagedChild {
    process: OwnedHandle,
}

impl ManagedChild {
    fn process_id(&self) -> u32 {
        // SAFETY: the process handle remains owned and valid for `self`.
        unsafe { GetProcessId(win_handle(&self.process)) }
    }

    fn try_wait(&mut self) -> std::io::Result<Option<u32>> {
        let mut code = 0_u32;
        // SAFETY: the process handle and output pointer are valid.
        unsafe { GetExitCodeProcess(win_handle(&self.process), &mut code) }
            .map_err(std::io::Error::other)?;
        Ok((code != STILL_ACTIVE).then_some(code))
    }

    fn wait(&mut self) -> std::io::Result<u32> {
        // SAFETY: the process handle remains owned and valid for `self`.
        unsafe {
            WaitForSingleObject(win_handle(&self.process), INFINITE);
        }
        let mut code = 0_u32;
        // SAFETY: the process handle and output pointer are valid.
        unsafe { GetExitCodeProcess(win_handle(&self.process), &mut code) }
            .map_err(std::io::Error::other)?;
        Ok(code)
    }

    fn kill(&mut self) {
        // SAFETY: the process handle remains owned and valid for `self`.
        drop(unsafe { TerminateProcess(win_handle(&self.process), 1) });
    }
}

/// The host side of a ConPTY. Dropping it closes the pseudoconsole, which makes
/// the drain reader observe EOF and lets the drain thread finish.
struct PseudoConsole {
    con: HPCON,
    input: Option<OwnedHandle>,
    output: OwnedHandle,
}

impl PseudoConsole {
    fn try_clone_reader(&self) -> std::io::Result<Box<dyn Read + Send>> {
        let handle = self.output.try_clone()?;
        Ok(Box::new(File::from(handle)))
    }

    fn take_writer(&mut self) -> Option<Box<dyn Write + Send>> {
        self.input
            .take()
            .map(|handle| Box::new(File::from(handle)) as Box<dyn Write + Send>)
    }
}

impl Drop for PseudoConsole {
    fn drop(&mut self) {
        if !self.con.is_invalid() {
            // SAFETY: this pseudoconsole was created here and is closed once.
            unsafe { ClosePseudoConsole(self.con) };
        }
    }
}

/// Create the PTY and the owned child in one process-creation call, binding the
/// child to `job` and attaching the pseudoconsole atomically.
fn spawn_pty_child(
    config: &ManagedProcessConfig,
    job: &OwnedHandle,
) -> ManagedProcessResult<(PseudoConsole, ManagedChild)> {
    let cols = i16::try_from(config.pty_cols).map_err(|error| {
        ManagedProcessError::new(format!("Invalid {} terminal width: {error}", config.label))
    })?;
    let rows = i16::try_from(config.pty_rows).map_err(|error| {
        ManagedProcessError::new(format!("Invalid {} terminal height: {error}", config.label))
    })?;

    let (input_read, input_write) = create_pipe(&config.label)?;
    let (output_read, output_write) = create_pipe(&config.label)?;
    // SAFETY: both handles are valid and the pseudoconsole duplicates them.
    let con = unsafe {
        CreatePseudoConsole(
            windows::Win32::System::Console::COORD { X: cols, Y: rows },
            win_handle(&input_read),
            win_handle(&output_write),
            PSEUDOCONSOLE_INHERIT_CURSOR
                | PSEUDOCONSOLE_RESIZE_QUIRK
                | PSEUDOCONSOLE_WIN32_INPUT_MODE,
        )
    }
    .map_err(|error| {
        ManagedProcessError::new(format!(
            "Failed to create a terminal for {}: {error}",
            config.label
        ))
    })?;
    // The pseudoconsole owns duplicates of the child-side pipe ends.
    drop(input_read);
    drop(output_write);
    let pty = PseudoConsole {
        con,
        input: Some(input_write),
        output: output_read,
    };

    let mut cmdline = build_command_line(&config.program, &config.args)?;
    let env_block = build_environment_block(&config.env)?;
    let cwd = config
        .cwd
        .as_ref()
        .map(|dir| encode_wide_nul(dir.as_os_str()))
        .transpose()?;

    let mut attributes = Attributes::new(2)?;
    let jobs = [win_handle(job)];
    // SAFETY: `jobs` and `con` stay alive through CreateProcessW. The job-list
    // attribute binds the child before its first thread can run; the
    // pseudoconsole attribute wires its stdio to the PTY above.
    unsafe {
        UpdateProcThreadAttribute(
            attributes.ptr(),
            0,
            PROC_THREAD_ATTRIBUTE_JOB_LIST as usize,
            Some(jobs.as_ptr().cast()),
            std::mem::size_of_val(&jobs),
            None,
            None,
        )
        .map_err(|error| {
            ManagedProcessError::new(format!(
                "Failed to contain {} process: {error}",
                config.label
            ))
        })?;
        UpdateProcThreadAttribute(
            attributes.ptr(),
            0,
            PROC_THREAD_ATTRIBUTE_PSEUDOCONSOLE as usize,
            Some(con.0 as *const core::ffi::c_void),
            std::mem::size_of::<HPCON>(),
            None,
            None,
        )
        .map_err(|error| {
            ManagedProcessError::new(format!(
                "Failed to attach the {} terminal: {error}",
                config.label
            ))
        })?;
    }

    let startup = STARTUPINFOEXW {
        StartupInfo: STARTUPINFOW {
            cb: u32::try_from(std::mem::size_of::<STARTUPINFOEXW>()).map_err(|error| {
                ManagedProcessError::new(format!("Invalid {} startup size: {error}", config.label))
            })?,
            dwFlags: STARTF_USESTDHANDLES,
            // The pseudoconsole owns the child's stdio; invalid handles stop the
            // child from inheriting this process's redirected handles.
            hStdInput: INVALID_HANDLE_VALUE,
            hStdOutput: INVALID_HANDLE_VALUE,
            hStdError: INVALID_HANDLE_VALUE,
            ..Default::default()
        },
        lpAttributeList: attributes.ptr(),
    };
    let mut info = PROCESS_INFORMATION::default();
    // SAFETY: all buffers (command line, environment, cwd, attribute list,
    // startup info) outlive this call; the kernel fills `info` on success.
    unsafe {
        CreateProcessW(
            PCWSTR::null(),
            PWSTR(cmdline.as_mut_ptr()),
            None,
            None,
            false,
            EXTENDED_STARTUPINFO_PRESENT | CREATE_UNICODE_ENVIRONMENT,
            Some(env_block.as_ptr().cast()),
            cwd.as_ref()
                .map_or(PCWSTR::null(), |value| PCWSTR(value.as_ptr())),
            &startup.StartupInfo,
            &mut info,
        )
    }
    .map_err(|error| {
        ManagedProcessError::new(format!(
            "Failed to launch the {} CLI: {error}",
            config.label
        ))
    })?;
    // SAFETY: CreateProcessW returned a valid thread handle closed exactly once.
    drop(unsafe { OwnedHandle::from_raw_handle(info.hThread.0) });
    // SAFETY: CreateProcessW returned a valid process handle owned by the child.
    let process = unsafe { OwnedHandle::from_raw_handle(info.hProcess.0) };
    Ok((pty, ManagedChild { process }))
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

/// Post-creation assignment retained only for the job-termination unit test.
#[cfg(test)]
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

fn create_pipe(label: &str) -> ManagedProcessResult<(OwnedHandle, OwnedHandle)> {
    let mut read = HANDLE::default();
    let mut write = HANDLE::default();
    // SAFETY: CreatePipe writes both read/write handles on success.
    unsafe { CreatePipe(&mut read, &mut write, None, 0) }.map_err(|error| {
        ManagedProcessError::new(format!("Failed to create a {label} terminal pipe: {error}"))
    })?;
    // SAFETY: CreatePipe returned two unique valid, non-inheritable handles.
    let read = unsafe { OwnedHandle::from_raw_handle(read.0) };
    // SAFETY: CreatePipe returned two unique valid, non-inheritable handles.
    let write = unsafe { OwnedHandle::from_raw_handle(write.0) };
    Ok((read, write))
}

/// Owned `PROC_THREAD_ATTRIBUTE_LIST` storage for one process-creation call.
struct Attributes(Vec<usize>);

impl Attributes {
    fn new(count: u32) -> ManagedProcessResult<Self> {
        let mut bytes = 0_usize;
        // SAFETY: the sizing call writes only to `bytes` and is expected to
        // report insufficient buffer; the result is intentionally ignored.
        drop(unsafe {
            InitializeProcThreadAttributeList(
                LPPROC_THREAD_ATTRIBUTE_LIST::default(),
                count,
                0,
                &mut bytes,
            )
        });
        if bytes == 0 {
            return Err(ManagedProcessError::new(
                "Failed to size a process attribute list".to_string(),
            ));
        }
        let mut value = Self(vec![0; bytes.div_ceil(std::mem::size_of::<usize>())]);
        // SAFETY: the owned allocation is aligned for and at least `bytes` long.
        unsafe { InitializeProcThreadAttributeList(value.ptr(), count, 0, &mut bytes) }.map_err(
            |error| {
                ManagedProcessError::new(format!(
                    "Failed to allocate a process attribute list: {error}"
                ))
            },
        )?;
        Ok(value)
    }

    fn ptr(&mut self) -> LPPROC_THREAD_ATTRIBUTE_LIST {
        LPPROC_THREAD_ATTRIBUTE_LIST(self.0.as_mut_ptr().cast())
    }
}

impl Drop for Attributes {
    fn drop(&mut self) {
        if !self.0.is_empty() {
            // SAFETY: this list was initialized and has not yet been deleted.
            unsafe { DeleteProcThreadAttributeList(self.ptr()) };
        }
    }
}

fn build_command_line(program: &Path, args: &[OsString]) -> ManagedProcessResult<Vec<u16>> {
    let mut cmdline = Vec::new();
    append_quoted(program.as_os_str(), &mut cmdline)?;
    for arg in args {
        cmdline.push(b' ' as u16);
        append_quoted(arg, &mut cmdline)?;
    }
    cmdline.push(0);
    Ok(cmdline)
}

fn build_environment_block(overrides: &[(OsString, OsString)]) -> ManagedProcessResult<Vec<u16>> {
    let mut values: Vec<(OsString, OsString)> = std::env::vars_os().collect();
    for (key, value) in overrides {
        values.retain(|(existing, _)| {
            !existing
                .to_string_lossy()
                .eq_ignore_ascii_case(&key.to_string_lossy())
        });
        values.push((key.clone(), value.clone()));
    }
    // CreateProcessW expects Unicode environment blocks sorted case-insensitively.
    values.sort_by_cached_key(|(key, _)| key.to_string_lossy().to_uppercase());
    let mut block = Vec::new();
    for (key, value) in values {
        let mut entry = key;
        entry.push("=");
        entry.push(value);
        block.extend(encode_wide(&entry)?);
        block.push(0);
    }
    block.push(0);
    Ok(block)
}

fn encode_wide(value: &OsStr) -> ManagedProcessResult<Vec<u16>> {
    let wide: Vec<u16> = value.encode_wide().collect();
    if wide.contains(&0) {
        return Err(ManagedProcessError::new(
            "Process argument contains an embedded NUL".to_string(),
        ));
    }
    Ok(wide)
}

fn encode_wide_nul(value: &OsStr) -> ManagedProcessResult<Vec<u16>> {
    let mut wide = encode_wide(value)?;
    wide.push(0);
    Ok(wide)
}

/// Quote one argument using the MSDN command-line rules.
fn append_quoted(value: &OsStr, output: &mut Vec<u16>) -> ManagedProcessResult<()> {
    let value = encode_wide(value)?;
    let needs_quotes = value.is_empty()
        || value
            .iter()
            .any(|code| matches!(*code, 0x20 | 0x09 | 0x0a | 0x0b | 0x22));
    if !needs_quotes {
        output.extend(value);
        return Ok(());
    }

    output.push(b'"' as u16);
    let mut backslashes = 0_usize;
    for code in value {
        if code == b'\\' as u16 {
            backslashes += 1;
            continue;
        }
        let trailing = if code == b'"' as u16 {
            backslashes * 2 + 1
        } else {
            backslashes
        };
        output.extend(std::iter::repeat_n(b'\\' as u16, trailing));
        output.push(code);
        backslashes = 0;
    }
    output.extend(std::iter::repeat_n(b'\\' as u16, backslashes * 2));
    output.push(b'"' as u16);
    Ok(())
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
    if query != windows::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER.0 && query != NO_ERROR {
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
        if result != windows::Win32::Foundation::ERROR_INSUFFICIENT_BUFFER.0 {
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

    #[tokio::test]
    async fn managed_process_contains_descendants_of_the_pty_child() {
        let marker = std::env::temp_dir().join(format!(
            "codexbar-managed-descendant-{}.txt",
            std::process::id()
        ));
        drop(std::fs::remove_file(&marker));
        let script = format!(
            "$psi = New-Object System.Diagnostics.ProcessStartInfo; \
             $psi.FileName = 'powershell.exe'; \
             $psi.Arguments = '-NoLogo -NoProfile -Command \"Start-Sleep -Seconds 120\"'; \
             $psi.UseShellExecute = $false; \
             $p = [System.Diagnostics.Process]::Start($psi); \
             Set-Content -LiteralPath '{}' -Value $p.Id; \
             Start-Sleep -Seconds 120",
            marker.display()
        );
        let config = ManagedProcessConfig {
            program: PathBuf::from("powershell.exe"),
            args: vec![
                OsString::from("-NoLogo"),
                OsString::from("-NoProfile"),
                OsString::from("-NonInteractive"),
                OsString::from("-Command"),
                OsString::from(script),
            ],
            env: Vec::new(),
            cwd: None,
            pty_rows: 30,
            pty_cols: 120,
            label: "test-descendant".to_string(),
        };
        let mut process = ManagedProcess::spawn(&config).expect("start a managed test process");

        let descendant = wait_for_descendant_pid(&marker);
        assert!(
            process_is_alive(descendant),
            "the descendant of the PTY child should be running before shutdown"
        );

        process.shutdown(Duration::from_secs(10)).await;

        wait_for_exit_within(descendant, Duration::from_secs(10));
        drop(std::fs::remove_file(&marker));
    }

    fn wait_for_descendant_pid(marker: &Path) -> u32 {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Ok(contents) = std::fs::read_to_string(marker)
                && let Ok(pid) = contents.trim().parse::<u32>()
            {
                return pid;
            }
            assert!(
                Instant::now() < deadline,
                "the PTY child did not report a descendant pid"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn wait_for_exit_within(pid: u32, budget: Duration) {
        let deadline = Instant::now() + budget;
        while process_is_alive(pid) {
            assert!(
                Instant::now() < deadline,
                "descendant {pid} survived the managed job cleanup"
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}
