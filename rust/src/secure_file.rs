//! Small helper for storing local secret-bearing JSON files.

use std::io;
use std::path::{Path, PathBuf};

use base64::Engine;
use serde::{Deserialize, Serialize};

const FORMAT: &str = "codexbar.secure-file";
const VERSION: u32 = 1;
const WINDOWS_DPAPI_USER: &str = "windows-dpapi-user";
const WINDOWS_DPAPI_MACHINE: &str = "windows-dpapi-machine";

#[derive(Debug, Serialize, Deserialize)]
struct ProtectedFile {
    format: String,
    version: u32,
    protection: String,
    payload: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecureFileStatus {
    Missing,
    Plaintext,
    Protected(String),
    Unreadable(String),
}

/// Return a non-secret storage status for diagnostics/UI surfaces.
pub fn status(path: &Path) -> SecureFileStatus {
    if !path.exists() {
        return SecureFileStatus::Missing;
    }

    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(e) => return SecureFileStatus::Unreadable(e.to_string()),
    };

    let Ok(file) = serde_json::from_str::<ProtectedFile>(&raw) else {
        return SecureFileStatus::Plaintext;
    };

    if file.format != FORMAT {
        return SecureFileStatus::Plaintext;
    }
    if file.version != VERSION {
        return SecureFileStatus::Unreadable(format!(
            "unsupported secure file version {}",
            file.version
        ));
    }

    match file.protection.as_str() {
        WINDOWS_DPAPI_USER | WINDOWS_DPAPI_MACHINE => SecureFileStatus::Protected(file.protection),
        other => {
            SecureFileStatus::Unreadable(format!("unsupported secure file protection {other}"))
        }
    }
}

/// Read a UTF-8 file that may be protected by this module.
pub fn read_string(path: &Path) -> io::Result<String> {
    let raw = std::fs::read_to_string(path)?;
    let Ok(file) = serde_json::from_str::<ProtectedFile>(&raw) else {
        return Ok(raw);
    };

    if file.format != FORMAT {
        return Ok(raw);
    }
    if file.version != VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported secure file version {}", file.version),
        ));
    }

    match file.protection.as_str() {
        WINDOWS_DPAPI_USER | WINDOWS_DPAPI_MACHINE => {
            let encrypted = base64::engine::general_purpose::STANDARD
                .decode(file.payload.as_bytes())
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            let plain = unprotect(&encrypted)?;
            String::from_utf8(plain).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
        }
        other => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("unsupported secure file protection {other}"),
        )),
    }
}

/// Write a UTF-8 file, protecting it with Windows DPAPI when available.
///
/// Writes to a unique sibling temp file and atomically renames it over the
/// target. A crash mid-write, or two writers racing, can therefore never leave
/// a truncated/interleaved secret file — which would deserialize to empty and
/// silently discard all stored credentials on the next load.
pub fn write_string(path: &Path, contents: &str) -> io::Result<()> {
    let bytes = protected_file_bytes(contents)?;
    let tmp = unique_temp_path(path);

    if let Err(e) = std::fs::write(&tmp, &bytes) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    // Tighten the temp file's permissions before it becomes the target; the
    // owner-only DACL / 0o600 mode moves with the file across the rename.
    if let Err(e) = restrict_file_permissions(&tmp) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

/// A unique sibling path (same directory as `path`) for staging an atomic write.
fn unique_temp_path(path: &Path) -> PathBuf {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let mut name = path
        .file_name()
        .map(|f| f.to_os_string())
        .unwrap_or_default();
    name.push(format!(".tmp.{}.{}", std::process::id(), n));
    path.with_file_name(name)
}

#[cfg(windows)]
fn protected_file_bytes(contents: &str) -> io::Result<Vec<u8>> {
    let (protection, encrypted) = protect(contents.as_bytes())?;
    let file = ProtectedFile {
        format: FORMAT.to_string(),
        version: VERSION,
        protection: protection.to_string(),
        payload: base64::engine::general_purpose::STANDARD.encode(encrypted),
    };
    serde_json::to_vec_pretty(&file).map_err(io::Error::other)
}

#[cfg(not(windows))]
fn protected_file_bytes(contents: &str) -> io::Result<Vec<u8>> {
    Ok(contents.as_bytes().to_vec())
}

#[cfg(windows)]
fn protect(plain: &[u8]) -> io::Result<(&'static str, Vec<u8>)> {
    use windows::Win32::Security::Cryptography::{
        CRYPTPROTECT_LOCAL_MACHINE, CRYPTPROTECT_UI_FORBIDDEN,
    };

    match protect_with_flags(plain, CRYPTPROTECT_UI_FORBIDDEN) {
        Ok(encrypted) => Ok((WINDOWS_DPAPI_USER, encrypted)),
        Err(user_error) => protect_with_flags(
            plain,
            CRYPTPROTECT_UI_FORBIDDEN | CRYPTPROTECT_LOCAL_MACHINE,
        )
        .map(|encrypted| {
            // Machine-scoped DPAPI blobs are decryptable by *any* account on
            // this host, not just the current user. Fall back to it so the
            // write still succeeds, but make the weaker protection visible.
            // `write_string` additionally tightens the file's DACL to the
            // current user, which is the primary mitigation for these blobs.
            tracing::warn!(
                "User-scope DPAPI failed ({user_error}); stored credentials with machine-scope DPAPI, which any local account can decrypt. Re-save credentials once user-scope DPAPI is available."
            );
            (WINDOWS_DPAPI_MACHINE, encrypted)
        })
        .map_err(|machine_error| {
            io::Error::other(format!(
                "CryptProtectData failed with user scope ({user_error}) and machine scope ({machine_error})"
            ))
        }),
    }
}

#[cfg(windows)]
fn protect_with_flags(plain: &[u8], flags: u32) -> io::Result<Vec<u8>> {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{CRYPT_INTEGER_BLOB, CryptProtectData};

    unsafe {
        let input_blob = CRYPT_INTEGER_BLOB {
            cbData: plain.len() as u32,
            pbData: plain.as_ptr() as *mut u8,
        };
        let mut output_blob = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };

        CryptProtectData(&input_blob, None, None, None, None, flags, &mut output_blob)
            .map_err(|e| io::Error::other(format!("CryptProtectData failed: {e:?}")))?;

        if output_blob.pbData.is_null() {
            return Err(io::Error::other("CryptProtectData returned null output"));
        }

        let encrypted =
            std::slice::from_raw_parts(output_blob.pbData, output_blob.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(output_blob.pbData as *mut _));
        Ok(encrypted)
    }
}

#[cfg(windows)]
fn unprotect(encrypted: &[u8]) -> io::Result<Vec<u8>> {
    use windows::Win32::Foundation::{HLOCAL, LocalFree};
    use windows::Win32::Security::Cryptography::{
        CRYPT_INTEGER_BLOB, CRYPTPROTECT_UI_FORBIDDEN, CryptUnprotectData,
    };

    unsafe {
        let input_blob = CRYPT_INTEGER_BLOB {
            cbData: encrypted.len() as u32,
            pbData: encrypted.as_ptr() as *mut u8,
        };
        let mut output_blob = CRYPT_INTEGER_BLOB {
            cbData: 0,
            pbData: std::ptr::null_mut(),
        };

        CryptUnprotectData(
            &input_blob,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output_blob,
        )
        .map_err(|e| io::Error::other(format!("CryptUnprotectData failed: {e:?}")))?;

        if output_blob.pbData.is_null() {
            return Err(io::Error::other("CryptUnprotectData returned null output"));
        }

        let plain =
            std::slice::from_raw_parts(output_blob.pbData, output_blob.cbData as usize).to_vec();
        let _ = LocalFree(HLOCAL(output_blob.pbData as *mut _));
        Ok(plain)
    }
}

#[cfg(not(windows))]
fn unprotect(_encrypted: &[u8]) -> io::Result<Vec<u8>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "Windows DPAPI-protected files can only be read on Windows by the same user",
    ))
}

#[cfg(unix)]
fn restrict_file_permissions(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let mut perms = std::fs::metadata(path)?.permissions();
    perms.set_mode(0o600);
    std::fs::set_permissions(path, perms)
}

/// Tighten the DACL to the current user (parity with the unix `0o600` path).
///
/// Removes inherited ACEs and grants Full control only to the current user, so
/// a secret file is not readable by other local accounts. This matters most
/// for machine-scoped DPAPI blobs (decryptable by any local account) but is
/// applied to every secret file as defense in depth.
///
/// Best-effort: failures are logged, not fatal. The file is still
/// DPAPI-encrypted, and returning an error here would break credential writes.
#[cfg(windows)]
fn restrict_file_permissions(path: &Path) -> io::Result<()> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;

    const CREATE_NO_WINDOW: u32 = 0x08000000;

    let Some(user) = current_user_account() else {
        tracing::warn!("Could not determine current user; left default ACL on {path:?}");
        return Ok(());
    };

    // Resolve icacls to an absolute System32 path so ACL hardening cannot
    // itself be hijacked by a planted `icacls.exe` on the search path.
    let grant = format!("{user}:(F)");
    let result = Command::new(system32_binary("icacls.exe"))
        .arg(path)
        .arg("/inheritance:r")
        .arg("/grant:r")
        .arg(&grant)
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    match result {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => {
            tracing::warn!(
                "Failed to restrict ACL on {path:?}: icacls exited with {} ({})",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            );
            Ok(())
        }
        Err(e) => {
            tracing::warn!("Could not run icacls to restrict ACL on {path:?}: {e}");
            Ok(())
        }
    }
}

/// `DOMAIN\User` (or bare `User`) for the current account, from the
/// environment. Returns `None` when neither is set.
#[cfg(windows)]
fn current_user_account() -> Option<String> {
    let name = std::env::var("USERNAME").ok().filter(|s| !s.is_empty())?;
    match std::env::var("USERDOMAIN").ok().filter(|s| !s.is_empty()) {
        Some(domain) => Some(format!("{domain}\\{name}")),
        None => Some(name),
    }
}

/// Absolute path to a Windows System32 binary, falling back to the bare name
/// only if `%SystemRoot%` is unset or the file is missing.
#[cfg(windows)]
fn system32_binary(name: &str) -> std::path::PathBuf {
    std::env::var_os("SystemRoot")
        .map(std::path::PathBuf::from)
        .map(|root| root.join("System32").join(name))
        .filter(|p| p.exists())
        .unwrap_or_else(|| std::path::PathBuf::from(name))
}

#[cfg(not(any(unix, windows)))]
fn restrict_file_permissions(_path: &Path) -> io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_plaintext_json_without_wrapper() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plain.json");
        std::fs::write(&path, r#"{"hello":"world"}"#).unwrap();

        assert_eq!(read_string(&path).unwrap(), r#"{"hello":"world"}"#);
    }

    #[test]
    fn write_roundtrips_on_this_platform() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secure.json");
        write_string(&path, r#"{"secret":"value"}"#).unwrap();

        assert_eq!(read_string(&path).unwrap(), r#"{"secret":"value"}"#);
    }

    #[test]
    fn concurrent_writes_leave_a_valid_file_and_no_temp() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secure.json");
        write_string(&path, r#"{"seed":true}"#).unwrap();

        let mut handles = Vec::new();
        for writer in 0..4 {
            let path = path.clone();
            handles.push(std::thread::spawn(move || {
                for n in 0..20 {
                    let content = format!(r#"{{"writer":{writer},"n":{n}}}"#);
                    // Rename-replace can transiently conflict under heavy
                    // concurrency; retry so the test exercises interleaving.
                    for _ in 0..50 {
                        if write_string(&path, &content).is_ok() {
                            break;
                        }
                        std::thread::yield_now();
                    }
                }
            }));
        }
        for handle in handles {
            handle.join().unwrap();
        }

        // Atomic replace guarantees the file is always one complete value, never
        // a truncated/interleaved write that would deserialize to empty.
        let read = read_string(&path).unwrap();
        let _: serde_json::Value =
            serde_json::from_str(&read).expect("final secret file must be valid JSON");

        let leftover_temps = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
            .count();
        assert_eq!(leftover_temps, 0, "atomic write must not leak temp files");
    }

    #[test]
    fn status_reports_missing_plaintext_and_protected_files() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing.json");
        assert_eq!(status(&missing), SecureFileStatus::Missing);

        let plain = dir.path().join("plain.json");
        std::fs::write(&plain, r#"{"secret":"value"}"#).unwrap();
        assert_eq!(status(&plain), SecureFileStatus::Plaintext);

        let protected = dir.path().join("protected.json");
        std::fs::write(
            &protected,
            serde_json::to_string(&ProtectedFile {
                format: FORMAT.to_string(),
                version: VERSION,
                protection: WINDOWS_DPAPI_USER.to_string(),
                payload: "AA==".to_string(),
            })
            .unwrap(),
        )
        .unwrap();
        assert_eq!(
            status(&protected),
            SecureFileStatus::Protected(WINDOWS_DPAPI_USER.to_string())
        );
    }

    #[test]
    fn status_reports_unsupported_wrappers_as_unreadable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("protected.json");
        std::fs::write(
            &path,
            serde_json::to_string(&ProtectedFile {
                format: FORMAT.to_string(),
                version: VERSION + 1,
                protection: WINDOWS_DPAPI_USER.to_string(),
                payload: "AA==".to_string(),
            })
            .unwrap(),
        )
        .unwrap();

        assert!(matches!(status(&path), SecureFileStatus::Unreadable(_)));
    }

    #[cfg(windows)]
    #[test]
    fn windows_restrict_permissions_keeps_owner_access() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secure.json");
        std::fs::write(&path, b"data").unwrap();

        // Owner keeps Full control, so the file is still readable after the
        // DACL is tightened, and the call is non-fatal regardless.
        restrict_file_permissions(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"data");
    }

    #[cfg(windows)]
    #[test]
    fn windows_current_user_account_is_available() {
        assert!(current_user_account().is_some());
    }

    #[cfg(windows)]
    #[test]
    fn windows_write_uses_protected_wrapper() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secure.json");
        write_string(&path, r#"{"secret":"value"}"#).unwrap();

        let raw = std::fs::read_to_string(&path).unwrap();
        let file: ProtectedFile = serde_json::from_str(&raw).unwrap();

        assert_eq!(file.format, FORMAT);
        assert_eq!(file.version, VERSION);
        assert!(matches!(
            file.protection.as_str(),
            WINDOWS_DPAPI_USER | WINDOWS_DPAPI_MACHINE
        ));
        assert!(
            !raw.contains("secret") && !raw.contains("value"),
            "protected Windows file must not contain plaintext JSON"
        );
    }
}
