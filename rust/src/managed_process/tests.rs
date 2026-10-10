
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
        env: ProcessEnvironment::default(),
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
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, WaitForSingleObject,
    };

    // SAFETY: OpenProcess returns a handle owned by this function and closed below.
    match unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SYNCHRONIZE,
            false,
            pid,
        )
    } {
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
fn failed_spawn_closes_pseudoconsole_promptly() {
    let mut config = test_config();
    config.args.push(OsString::from("\0"));
    let (sender, receiver) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let failed = ManagedProcess::spawn(&config).is_err();
        sender
            .send(failed)
            .expect("failure result receiver remains available");
    });

    assert!(
        receiver
            .recv_timeout(Duration::from_secs(3))
            .expect("post-ConPTY setup failure should return promptly")
    );
}

#[test]
fn listener_table_finds_current_process_port() {
    let listener = std::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .expect("bind a local IPv4 listener");
    let port = listener.local_addr().expect("listener address").port();

    let ports =
        listening_ports_for_pid(std::process::id()).expect("read the Windows TCP listener table");

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

#[test]
fn managed_process_reports_exit_code_259_as_exited() {
    let mut config = test_config();
    config.program = PathBuf::from("cmd.exe");
    config.args = vec![OsString::from("/C"), OsString::from("exit /B 259")];
    let mut process = ManagedProcess::spawn(&config).expect("start an exit-code test child");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = process.try_wait().expect("poll the exit-code test child") {
            assert_eq!(status.exit_code(), 259);
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the child with exit code 259 was reported as running"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    drop(process);
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
        env: ProcessEnvironment::default(),
        cwd: None,
        pty_rows: 30,
        pty_cols: 120,
        label: "test-descendant".to_string(),
    };
    let process = ManagedProcess::spawn(&config).expect("start a managed test process");

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
