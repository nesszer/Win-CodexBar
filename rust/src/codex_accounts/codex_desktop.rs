//! Codex Desktop restart control (MSIX).
//!
//! Port of `windows/.../codex_desktop.py` (MIT): builds a hidden PowerShell
//! script that stops the Codex Desktop processes, syncs the MSIX session state
//! (backup/restore of the session entries) and relaunches the app.

use std::io;
use std::path::{Path, PathBuf};

use base64::Engine;

use super::file_locations::{
    DESKTOP_SESSION_STATE_ENTRIES, app_support_directory, codex_desktop_session_root,
    ensure_directories,
};

/// Friendly Codex Desktop control error.
#[derive(Debug, thiserror::Error)]
pub enum CodexDesktopControlError {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Io(#[from] io::Error),
}

pub fn restart_log_path() -> PathBuf {
    app_support_directory().join("codex-desktop-restart.log")
}

pub fn restart_script_path() -> PathBuf {
    app_support_directory().join("codex-desktop-restart.ps1")
}

/// Render the PowerShell restart script.
pub fn build_restart_script(
    delay_seconds: f64,
    session_root: Option<&Path>,
    backup_destination: Option<&Path>,
    restore_source: Option<&Path>,
) -> String {
    // Delay is bounded, non-negative seconds rendered to whole milliseconds;
    // rounding already makes the value whole, so f64->u64 cannot overflow.
    #[expect(
        clippy::cast_possible_truncation,
        reason = "delay is non-negative seconds; whole milliseconds by design"
    )]
    let delay_ms = (delay_seconds.max(0.0) * 1000.0).round() as u64;
    let log_path = powershell_literal_path(&restart_log_path());
    let effective_session_root = session_root
        .map(Path::to_path_buf)
        .or_else(codex_desktop_session_root);
    let session_root_literal = powershell_path_or_null(effective_session_root.as_deref());
    let backup_destination_literal = powershell_path_or_null(backup_destination);
    let restore_source_literal = powershell_path_or_null(restore_source);
    let session_entries_literal = powershell_string_array(DESKTOP_SESSION_STATE_ENTRIES);

    format!(
        r#"$ErrorActionPreference = 'Stop'
$logPath = {log_path}
$sessionRoot = {session_root_literal}
$backupDestination = {backup_destination_literal}
$restoreSource = {restore_source_literal}
$sessionEntries = {session_entries_literal}
New-Item -ItemType Directory -Path ([System.IO.Path]::GetDirectoryName($logPath)) -Force | Out-Null
trap {{
    Add-Content -LiteralPath $logPath -Value ("Restart failed: " + $_.Exception.Message)
    exit 1
}}
function Write-Log([string]$message) {{
    Add-Content -LiteralPath $logPath -Value ("[{{0}}] {{1}}" -f (Get-Date -Format 'yyyy-MM-dd HH:mm:ss.fff'), $message)
}}
function Clear-SessionEntry([string]$root, [string]$relativePath) {{
    if (-not $root) {{
        return
    }}
    $resolvedRoot = [System.IO.Path]::GetFullPath($root).TrimEnd('\') + '\'
    $targetPath = [System.IO.Path]::GetFullPath((Join-Path $resolvedRoot $relativePath))
    if (-not $targetPath.StartsWith($resolvedRoot, [StringComparison]::OrdinalIgnoreCase)) {{
        throw 'Session entry escaped its root.'
    }}
    if (Test-Path -LiteralPath $targetPath) {{
        Remove-Item -LiteralPath $targetPath -Recurse -Force -ErrorAction Stop
    }}
}}
function Copy-SessionEntry([string]$sourceRoot, [string]$destinationRoot, [string]$relativePath) {{
    if (-not $sourceRoot -or -not $destinationRoot) {{
        return
    }}
    $sourcePath = Join-Path $sourceRoot $relativePath
    if (-not (Test-Path -LiteralPath $sourcePath)) {{
        return
    }}
    $destinationPath = Join-Path $destinationRoot $relativePath
    $parentPath = [System.IO.Path]::GetDirectoryName($destinationPath)
    if ($parentPath) {{
        New-Item -ItemType Directory -Path $parentPath -Force | Out-Null
    }}
    $item = Get-Item -LiteralPath $sourcePath -Force
    if ($item.PSIsContainer) {{
        Copy-Item -LiteralPath $sourcePath -Destination $destinationPath -Recurse -Force
        return
    }}
    Copy-Item -LiteralPath $sourcePath -Destination $destinationPath -Force
}}
function Sync-DesktopSessionState() {{
    if (-not $sessionRoot) {{
        Write-Log 'Desktop session root was not detected.'
        return
    }}
    Write-Log ("Desktop session root: " + $sessionRoot)
    Write-Log ("Backup destination: " + $(if ($backupDestination) {{ $backupDestination }} else {{ '<none>' }}))
    Write-Log ("Restore source: " + $(if ($restoreSource) {{ $restoreSource }} else {{ '<none>' }}))
    if (-not (Test-Path -LiteralPath $sessionRoot)) {{
        Write-Log ("Desktop session root is missing: " + $sessionRoot)
        return
    }}
    if ($backupDestination) {{
        New-Item -ItemType Directory -Path $backupDestination -Force | Out-Null
        foreach ($relativePath in $sessionEntries) {{
            try {{
                Clear-SessionEntry $backupDestination $relativePath
                Copy-SessionEntry $sessionRoot $backupDestination $relativePath
                Write-Log ("Backed up session entry: " + $relativePath)
            }} catch {{
                Write-Log ("Failed to back up session entry " + $relativePath + ": " + $_.Exception.Message)
                throw
            }}
        }}
        Write-Log ("Backed up desktop session state to " + $backupDestination)
    }}
    if ($restoreSource) {{
        if (-not (Test-Path -LiteralPath $restoreSource)) {{
            Write-Log ("No saved session for target; clearing the previous desktop session: " + $restoreSource)
        }}
        foreach ($relativePath in $sessionEntries) {{
            try {{
                Clear-SessionEntry $sessionRoot $relativePath
                Copy-SessionEntry $restoreSource $sessionRoot $relativePath
                Write-Log ("Restored session entry: " + $relativePath)
            }} catch {{
                Write-Log ("Failed to restore session entry " + $relativePath + ": " + $_.Exception.Message)
                throw
            }}
        }}
        Write-Log ("Restored desktop session state from " + $restoreSource)
    }}
}}
Write-Log 'Restart requested.'
$package = Get-AppxPackage -Name OpenAI.Codex | Sort-Object Version -Descending | Select-Object -First 1
if (-not $package -or -not $package.InstallLocation) {{
    throw 'Unable to locate the Codex Desktop package.'
}}
$packageRoot = [System.IO.Path]::GetFullPath($package.InstallLocation).TrimEnd('\') + '\'
[xml]$manifest = Get-Content -LiteralPath (Join-Path $packageRoot 'AppxManifest.xml')
$application = $manifest.Package.Applications.Application | Select-Object -First 1
$launcherPath = [System.IO.Path]::GetFullPath((Join-Path $packageRoot $application.Executable))
if (-not $launcherPath.StartsWith($packageRoot, [StringComparison]::OrdinalIgnoreCase) -or
    -not (Test-Path -LiteralPath $launcherPath -PathType Leaf)) {{
    throw 'Unable to locate the Codex Desktop executable.'
}}
Write-Log ("Using package launcher path: " + $launcherPath)
Start-Sleep -Milliseconds {delay_ms}
# Do not kill process trees: this helper can descend from a Codex task.
# Stop only this package's GUI executable, including its renderer processes.
# Standalone codex.exe processes and the restart helper must stay running.
$codexProcesses = Get-CimInstance Win32_Process | Where-Object {{
    $_.ExecutablePath -and $_.ExecutablePath -ieq $launcherPath
}}
foreach ($process in $codexProcesses) {{
    if (-not (Get-Process -Id $process.ProcessId -ErrorAction SilentlyContinue)) {{ continue }}
    Write-Log ("Stopping Desktop GUI process " + $process.ProcessId)
    # Killing the first GUI PID often reaps sibling renderer PIDs. taskkill
    # then prints "process not found"; with $ErrorActionPreference=Stop that
    # used to abort before session restore, so Switch never reached Desktop.
    $previousEap = $ErrorActionPreference
    $ErrorActionPreference = 'SilentlyContinue'
    & taskkill.exe /PID $process.ProcessId /F 2>&1 | Out-Null
    $killExit = $LASTEXITCODE
    $ErrorActionPreference = $previousEap
    if ($killExit -ne 0 -and (Get-Process -Id $process.ProcessId -ErrorAction SilentlyContinue)) {{
        throw 'Unable to stop Codex Desktop. Session files were left unchanged.'
    }}
}}
$deadline = (Get-Date).AddSeconds(8)
do {{
    $remaining = Get-CimInstance Win32_Process | Where-Object {{
        $_.ExecutablePath -and $_.ExecutablePath -ieq $launcherPath
    }}
    if (-not $remaining) {{ break }}
    Start-Sleep -Milliseconds 250
}} while ((Get-Date) -lt $deadline)
if ($remaining) {{
    throw 'Codex Desktop is still running. Session files were left unchanged.'
}}
Sync-DesktopSessionState
Start-Sleep -Milliseconds 700
Start-Process -FilePath $launcherPath
Write-Log 'Codex Desktop relaunched.'
"#
    )
    .trim_end()
    .to_string()
}

/// Encode a script as base64 UTF-16LE (PowerShell `-EncodedCommand`).
pub fn encode_powershell_script(script: &str) -> String {
    let utf16: Vec<u16> = script.encode_utf16().collect();
    let bytes: Vec<u8> = utf16
        .into_iter()
        .flat_map(|unit| unit.to_le_bytes())
        .collect();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// Build the hidden PowerShell command line for a script file.
pub fn build_restart_command(script_path: &Path) -> Vec<String> {
    let powershell_exe = std::env::var("WINDIR")
        .map(|windir| {
            PathBuf::from(windir).join("System32\\WindowsPowerShell\\v1.0\\powershell.exe")
        })
        .unwrap_or_else(|_| {
            PathBuf::from(r"C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe")
        });
    vec![
        powershell_exe.to_string_lossy().to_string(),
        "-NoProfile".to_string(),
        "-NonInteractive".to_string(),
        "-WindowStyle".to_string(),
        "Hidden".to_string(),
        "-ExecutionPolicy".to_string(),
        "Bypass".to_string(),
        "-File".to_string(),
        script_path.to_string_lossy().to_string(),
    ]
}

/// Run the hidden restart script and report failures to the invoking surface.
pub fn restart_codex_desktop(
    delay_seconds: f64,
    session_root: Option<&Path>,
    backup_destination: Option<&Path>,
    restore_source: Option<&Path>,
) -> Result<(), CodexDesktopControlError> {
    ensure_directories()?;
    let script = build_restart_script(
        delay_seconds,
        session_root,
        backup_destination,
        restore_source,
    );
    fs_write(restart_script_path(), script)?;
    launch_hidden_powershell(&restart_script_path())
}

#[cfg(windows)]
fn fs_write(path: PathBuf, content: String) -> io::Result<()> {
    std::fs::write(path, content)
}

#[cfg(windows)]
fn launch_hidden_powershell(script_path: &Path) -> Result<(), CodexDesktopControlError> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    let mut command = Command::new(&build_restart_command(script_path)[0]);
    command.args(&build_restart_command(script_path)[1..]);
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP (never DETACHED_PROCESS).
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    command.creation_flags(CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP);
    let status = command.status().map_err(|error| {
        CodexDesktopControlError::Message(format!("Failed to restart Codex Desktop: {error}"))
    })?;
    if !status.success() {
        return Err(CodexDesktopControlError::Message(format!(
            "Codex Desktop could not restart. Its session backup or restore may have failed. See {} for details.",
            restart_log_path().display(),
        )));
    }
    Ok(())
}

#[cfg(not(windows))]
fn fs_write(path: PathBuf, content: String) -> io::Result<()> {
    std::fs::write(path, content)
}

#[cfg(not(windows))]
fn launch_hidden_powershell(_script_path: &Path) -> Result<(), CodexDesktopControlError> {
    Err(CodexDesktopControlError::Message(
        "Codex Desktop restart is only available on Windows.".to_string(),
    ))
}

fn powershell_literal_path(path: &Path) -> String {
    let normalized = path.to_string_lossy().replace('/', "\\");
    format!("'{}'", normalized.replace('\'', "''"))
}

fn powershell_path_or_null(path: Option<&Path>) -> String {
    match path {
        Some(path) => powershell_literal_path(path),
        None => "$null".to_string(),
    }
}

fn powershell_string_array(values: &[&str]) -> String {
    let quoted: Vec<String> = values
        .iter()
        .map(|value| format!("'{}'", value.replace('\'', "''")))
        .collect();
    format!("@({})", quoted.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn hidden_script_failure_is_returned_to_the_caller() {
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("failure.ps1");
        std::fs::write(&script, "throw 'Simulated session copy failure'").unwrap();
        assert!(
            launch_hidden_powershell(&script)
                .unwrap_err()
                .to_string()
                .contains("could not restart")
        );
    }

    #[cfg(windows)]
    #[test]
    fn restart_uses_manifest_gui_and_leaves_standalone_cli_alone() {
        let dir = tempfile::tempdir().unwrap();
        super::super::file_locations::with_app_support_directory(dir.path().join("support"));
        let package = dir.path().join("package");
        std::fs::create_dir_all(package.join("app")).unwrap();
        std::fs::write(package.join("app/ChatGPT.exe"), b"fixture").unwrap();
        std::fs::write(package.join("AppxManifest.xml"),
            r#"<Package><Applications><Application Id="App" Executable="app/ChatGPT.exe" /></Applications></Package>"#).unwrap();
        let session = dir.path().join("session");
        let backup = dir.path().join("backup");
        std::fs::create_dir_all(session.join("Local Storage")).unwrap();
        std::fs::write(session.join("Local Storage/old-account"), b"old session").unwrap();
        std::fs::write(session.join("Preferences"), b"old preferences").unwrap();
        std::fs::write(session.join("unrelated-file"), b"preserve").unwrap();
        let script = format!(
            r#"
$fixturePackage = {package}
$script:stopped = $false
function Get-AppxPackage {{ [pscustomobject]@{{ InstallLocation = $fixturePackage; Version = '1.0' }} }}
function Get-CimInstance {{
    [pscustomobject]@{{ ExecutablePath = 'C:\standalone\codex.exe'; ProcessId = 987654320; Name = 'codex.exe'; CommandLine = 'codex app-server' }}
    if (-not $script:stopped) {{
        [pscustomobject]@{{ ExecutablePath = (Join-Path $fixturePackage 'app\ChatGPT.exe'); ProcessId = 987654321; Name = 'ChatGPT.exe'; CommandLine = 'ChatGPT.exe' }}
    }}
}}
function taskkill.exe {{
    if ($args -contains '/T') {{ throw 'Process-tree shutdown would kill the restart helper' }}
    if ($args[1] -ne 987654321) {{ throw 'Attempted to stop standalone CLI' }}
    $script:stopped = $true
    $global:LASTEXITCODE = 0
}}
function Get-Process {{ param($Id) [pscustomobject]@{{ Id = $Id }} }}
function Start-Sleep {{}}
function Start-Process {{ param($FilePath)
    if ($FilePath -ne (Join-Path $fixturePackage 'app\ChatGPT.exe')) {{ throw 'Wrong launcher' }}
    $script:launched = $true
}}
{restart}
if (-not $script:stopped -or -not $script:launched) {{ throw 'Restart did not complete' }}
"#,
            package = powershell_literal_path(&package),
            restart = build_restart_script(
                0.0,
                Some(&session),
                Some(&backup),
                Some(&dir.path().join("missing-target"))
            )
        );
        super::super::file_locations::clear_app_support_directory_override();
        let path = dir.path().join("test-restart.ps1");
        std::fs::write(&path, script).unwrap();
        let argv = build_restart_command(&path);
        let output = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!session.join("Local Storage").exists());
        assert!(!session.join("Preferences").exists());
        assert_eq!(
            std::fs::read(backup.join("Local Storage/old-account")).unwrap(),
            b"old session"
        );
        assert_eq!(
            std::fs::read(backup.join("Preferences")).unwrap(),
            b"old preferences"
        );
        assert_eq!(
            std::fs::read(session.join("unrelated-file")).unwrap(),
            b"preserve"
        );
    }

    #[cfg(windows)]
    #[test]
    fn restart_continues_when_a_sibling_gui_pid_is_already_gone() {
        let dir = tempfile::tempdir().unwrap();
        super::super::file_locations::with_app_support_directory(dir.path().join("support"));
        let package = dir.path().join("package");
        std::fs::create_dir_all(package.join("app")).unwrap();
        std::fs::write(package.join("app/ChatGPT.exe"), b"fixture").unwrap();
        std::fs::write(
            package.join("AppxManifest.xml"),
            r#"<Package><Applications><Application Id="App" Executable="app/ChatGPT.exe" /></Applications></Package>"#,
        )
        .unwrap();
        let session = dir.path().join("session");
        let backup = dir.path().join("backup");
        std::fs::create_dir_all(&session).unwrap();
        std::fs::write(session.join("Preferences"), b"old preferences").unwrap();
        let script = format!(
            r#"
$ErrorActionPreference = 'Stop'
$fixturePackage = {package}
$launcher = Join-Path $fixturePackage 'app\ChatGPT.exe'
$script:killed = @()
function Get-AppxPackage {{ [pscustomobject]@{{ InstallLocation = $fixturePackage; Version = '1.0' }} }}
function Get-CimInstance {{
    if ($script:killed -contains 222) {{ return }}
    [pscustomobject]@{{ ExecutablePath = $launcher; ProcessId = 111; Name = 'ChatGPT.exe' }}
    [pscustomobject]@{{ ExecutablePath = $launcher; ProcessId = 222; Name = 'ChatGPT.exe' }}
}}
function taskkill.exe {{
    if ($args -contains '/T') {{ throw 'Process-tree shutdown would kill the restart helper' }}
    $target = $args[1]
    $script:killed += $target
    if ($target -eq 111) {{
        $global:LASTEXITCODE = 0
        return
    }}
    if ($target -eq 222) {{
        $global:LASTEXITCODE = 128
        Write-Error 'ERROR: The process "222" not found.'
        return
    }}
    throw 'Unexpected pid'
}}
function Get-Process {{
    param($Id)
    if ($script:killed -contains 222) {{ return }}
    [pscustomobject]@{{ Id = $Id }}
}}
function Start-Sleep {{}}
function Start-Process {{ param($FilePath)
    if ($FilePath -ne $launcher) {{ throw 'Wrong launcher' }}
    $script:launched = $true
}}
{restart}
if ($script:killed.Count -lt 1) {{ throw 'Did not attempt to stop GUI processes' }}
if (-not $script:launched) {{ throw 'Restart aborted before relaunch' }}
"#,
            package = powershell_literal_path(&package),
            restart = build_restart_script(
                0.0,
                Some(&session),
                Some(&backup),
                Some(&dir.path().join("missing-target"))
            )
        );
        super::super::file_locations::clear_app_support_directory_override();
        let path = dir.path().join("test-restart-gone-pid.ps1");
        std::fs::write(&path, script).unwrap();
        let argv = build_restart_command(&path);
        let output = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .output()
            .unwrap();
        let log = std::fs::read_to_string(dir.path().join("support/codex-desktop-restart.log"))
            .unwrap_or_default();
        assert!(
            output.status.success(),
            "status={:?} stdout={} stderr={} log={}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr),
            log
        );
        assert!(!session.join("Preferences").exists());
        assert_eq!(
            std::fs::read(backup.join("Preferences")).unwrap(),
            b"old preferences"
        );
    }

    #[test]
    fn build_restart_script_includes_restart_flow() {
        let script = build_restart_script(1.25, None, None, None);
        assert!(script.contains("Write-Log"));
        assert!(script.contains("Get-CimInstance Win32_Process"));
        assert!(script.contains("Get-AppxPackage"));
        assert!(script.contains("taskkill.exe /PID"));
        assert!(script.contains("$ErrorActionPreference = 'SilentlyContinue'"));
        assert!(script.contains("$manifest.Package.Applications.Application"));
        assert!(script.contains("$_.ExecutablePath -ieq $launcherPath"));
        assert!(!script.contains("$_.Name -ieq 'Codex.exe'"));
        assert!(script.contains("Start-Process -FilePath $launcherPath"));
        assert!(script.contains("Start-Sleep -Milliseconds 1250"));
    }

    #[test]
    fn build_restart_script_includes_desktop_session_sync() {
        let script = build_restart_script(
            0.5,
            Some(Path::new(
                r"C:\Users\test\AppData\Local\Packages\OpenAI.Codex_test\LocalCache\Roaming\Codex",
            )),
            Some(Path::new(
                r"C:\Users\test\AppData\Roaming\CodexControl\managed-homes\current\desktop-session",
            )),
            Some(Path::new(
                r"C:\Users\test\AppData\Roaming\CodexControl\managed-homes\target\desktop-session",
            )),
        );

        assert!(script.contains(
            "$sessionRoot = 'C:\\Users\\test\\AppData\\Local\\Packages\\OpenAI.Codex_test\\LocalCache\\Roaming\\Codex'"
        ));
        assert!(script.contains(
            "$backupDestination = 'C:\\Users\\test\\AppData\\Roaming\\CodexControl\\managed-homes\\current\\desktop-session'"
        ));
        assert!(script.contains(
            "$restoreSource = 'C:\\Users\\test\\AppData\\Roaming\\CodexControl\\managed-homes\\target\\desktop-session'"
        ));
        assert!(script.contains("function Sync-DesktopSessionState()"));
        assert!(script.contains("Copy-SessionEntry $sessionRoot $backupDestination $relativePath"));
        assert!(script.contains("Copy-SessionEntry $restoreSource $sessionRoot $relativePath"));
        assert!(script.contains("Clear-SessionEntry $sessionRoot $relativePath"));
        assert!(
            script.contains("No saved session for target; clearing the previous desktop session")
        );
        assert!(script.contains("Failed to back up session entry"));
        assert!(script.contains("Failed to restore session entry"));
    }

    #[test]
    fn encode_powershell_script_round_trips_utf16le() {
        use base64::Engine;
        let script = "Start-Process -FilePath 'Codex.exe'";
        let encoded = encode_powershell_script(script);
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        let units: Vec<u16> = bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]))
            .collect();
        let decoded = String::from_utf16(&units).unwrap();
        assert_eq!(decoded, script);
    }

    #[test]
    fn build_restart_command_invokes_hidden_powershell_file() {
        let command = build_restart_command(Path::new(r"C:\temp\restart.ps1"));
        assert!(command[0].to_lowercase().ends_with("powershell.exe"));
        assert!(command.contains(&"-WindowStyle".to_string()));
        assert!(command.contains(&"Hidden".to_string()));
        assert_eq!(command[command.len() - 2], "-File");
        assert!(
            command
                .last()
                .unwrap()
                .to_lowercase()
                .ends_with("temp\\restart.ps1")
        );
    }
}
