//! Cursor desktop app auth session (upstream 0.50.0 #2398).
//!
//! Reads Cursor's own read-only local session database
//! (`%APPDATA%\Cursor\User\globalStorage\state.vscdb`) and rebuilds the
//! `WorkosCursorSessionToken` cookie from the stored access token, so
//! Automatic mode prefers the signed-in app over browser cookies. The
//! database is only ever opened read-only; an idle WAL database whose
//! sidecars vanished is retried in SQLite immutable mode (never while a WAL
//! exists — that would ignore live uncheckpointed Cursor state).
//!
//! Cursor 3.27 stores the token as a `cursor-safe-storage:v1:` envelope around
//! Electron Safe Storage ciphertext (upstream #4790). On Windows that is
//! Chromium OSCrypt: `v10`, a 12-byte nonce and AES-256-GCM data under the
//! DPAPI-protected key in Cursor's `Local State`.

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use base64::Engine;
use rusqlite::OpenFlags;

const SAFE_STORAGE_PREFIX: &str = "cursor-safe-storage:";
const SAFE_STORAGE_V1: &str = "v1:";
const OSCRYPT_PREFIX: &[u8] = b"v10";
const GCM_NONCE_LEN: usize = 12;
const GCM_TAG_LEN: usize = 16;

/// Default `state.vscdb` location. Windows: `%APPDATA%\Cursor\…`; the
/// upstream macOS/Linux layouts differ per OS.
pub fn app_auth_db_path() -> Option<std::path::PathBuf> {
    let base = dirs::config_dir()?;
    Some(
        base.join("Cursor")
            .join("User")
            .join("globalStorage")
            .join("state.vscdb"),
    )
}

/// Read the stored `cursorAuth/accessToken` from Cursor's app database,
/// decrypting a Safe Storage envelope. An unreadable envelope yields no token,
/// so Automatic mode falls through to browser cookies.
pub fn load_app_auth_access_token() -> Option<String> {
    let stored = load_stored_access_token()?;
    resolve_stored_access_token(&stored, safe_storage_key)
}

/// Cursor's Local State AES key. Read only for an encrypted token, so a plain
/// JWT never touches DPAPI.
fn safe_storage_key() -> Option<Vec<u8>> {
    let local_state = dirs::config_dir()?.join("Cursor").join("Local State");
    crate::browser::cookies::CookieExtractor::get_chromium_encryption_key(&local_state)
        .inspect_err(|err| tracing::debug!(error = %err, "Cursor Safe Storage key unavailable"))
        .ok()
}

fn resolve_stored_access_token(
    stored: &str,
    key: impl FnOnce() -> Option<Vec<u8>>,
) -> Option<String> {
    let Some(envelope) = stored.trim_start().strip_prefix(SAFE_STORAGE_PREFIX) else {
        return Some(stored.to_string());
    };
    let token = envelope
        .strip_prefix(SAFE_STORAGE_V1)
        .and_then(|payload| decrypt_safe_storage(payload, key));
    if token.is_none() {
        tracing::debug!("Cursor app token is encrypted but unreadable; using browser cookies");
    }
    token
}

fn decrypt_safe_storage(payload: &str, key: impl FnOnce() -> Option<Vec<u8>>) -> Option<String> {
    let encrypted = base64::engine::general_purpose::STANDARD
        .decode(payload.trim())
        .ok()?;
    let body = encrypted.strip_prefix(OSCRYPT_PREFIX)?;
    if body.len() < GCM_NONCE_LEN + GCM_TAG_LEN {
        return None;
    }
    let cipher = Aes256Gcm::new_from_slice(&key()?).ok()?;
    let (nonce, ciphertext) = body.split_at(GCM_NONCE_LEN);
    let plaintext = cipher.decrypt(Nonce::from_slice(nonce), ciphertext).ok()?;
    let token = String::from_utf8(plaintext).ok()?;
    let token = token.trim();
    (!token.is_empty()).then(|| token.to_string())
}

fn load_stored_access_token() -> Option<String> {
    let db_path = app_auth_db_path()?;
    if !db_path.exists() {
        return None;
    }
    match read_item_table_value(&db_path, "cursorAuth/accessToken", false) {
        Ok(value) => value,
        Err(err) => {
            // Immutable retry only when both WAL sidecars are gone — an idle
            // WAL database can retain WAL mode in its header after the
            // sidecars disappear, and immutable mode reads the main file
            // without recreating them.
            let wal_missing = ["-wal", "-shm"]
                .into_iter()
                .all(|suffix| !sidecar(&db_path, suffix).exists());
            if !wal_missing {
                tracing::debug!("Cursor app auth read failed: {err}");
                return None;
            }
            read_item_table_value(&db_path, "cursorAuth/accessToken", true)
                .ok()
                .flatten()
                .or_else(|| {
                    tracing::debug!("Cursor app auth immutable read failed: {err}");
                    None
                })
        }
    }
}

fn sidecar(db_path: &std::path::Path, suffix: &str) -> std::path::PathBuf {
    let mut name = db_path.as_os_str().to_os_string();
    name.push(suffix);
    std::path::PathBuf::from(name)
}

/// Rebuild the `WorkosCursorSessionToken` cookie header from the app's
/// access token (`{userID}::{token}`, URL-encoded separator).
pub fn app_session_cookie_header(access_token: &str) -> Option<String> {
    let token = access_token.trim();
    if token.is_empty() {
        return None;
    }
    let user_id = crate::codex_accounts::api::jwt_payload(token)
        .and_then(|payload| {
            payload
                .get("sub")
                .and_then(|value| value.as_str())
                .map(str::to_string)
        })
        .unwrap_or_default();
    Some(format!("WorkosCursorSessionToken={user_id}%3A%3A{token}"))
}

/// Read one `ItemTable` value from the app database.
fn read_item_table_value(
    db_path: &std::path::Path,
    key: &str,
    immutable: bool,
) -> rusqlite::Result<Option<String>> {
    let mut flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let opened_path = if immutable {
        // URI + immutable=1: never recreates WAL sidecars on a cold database.
        flags |= OpenFlags::SQLITE_OPEN_URI;
        format!(
            "file:{}?immutable=1",
            db_path
                .to_str()
                .map(|p| p.replace('\\', "/"))
                .unwrap_or_default()
        )
    } else {
        db_path.to_string_lossy().to_string()
    };
    let conn = rusqlite::Connection::open_with_flags(&opened_path, flags)?;
    conn.busy_timeout(std::time::Duration::from_millis(250))?;
    let mut stmt = conn.prepare("SELECT value FROM ItemTable WHERE key = ? LIMIT 1;")?;
    let mut rows = stmt.query([key])?;
    match rows.next()? {
        Some(row) => Ok(row.get_ref(0).ok().and_then(decode_sqlite_string)),
        None => Ok(None),
    }
}

/// `ItemTable` values arrive as text or (sometimes) UTF-8/UTF-16LE blobs.
/// UTF-16LE bytes misread as UTF-8 decode "successfully" with interleaved
/// NULs, so a NUL-riddled result routes to the UTF-16LE decoder.
fn decode_sqlite_string(value: rusqlite::types::ValueRef<'_>) -> Option<String> {
    match value {
        rusqlite::types::ValueRef::Text(bytes) => String::from_utf8(bytes.to_vec()).ok(),
        rusqlite::types::ValueRef::Blob(bytes) => String::from_utf8(bytes.to_vec())
            .ok()
            .filter(|decoded| !decoded.contains('\0'))
            .or_else(|| decode_utf16_le(bytes)),
        _ => None,
    }
}

fn decode_utf16_le(bytes: &[u8]) -> Option<String> {
    if !bytes.len().is_multiple_of(2) {
        return None;
    }
    let units: Vec<u16> = bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
        .collect();
    String::from_utf16(&units).ok()
}

/// Local app session preference for Automatic mode: the validated header
/// persisted after a successful app-session fetch, else a fresh read of the
/// app database.
pub fn preferred_auto_cookie_header() -> Option<String> {
    if let Some(cached) =
        crate::browser::cookie_cache::CookieHeaderCache::load(crate::core::ProviderId::Cursor)
        && cached.source_label == "cursor-app"
        && !cached.is_stale(APP_SESSION_MAX_AGE_SECS)
    {
        return Some(cached.cookie_header);
    }
    let access_token = load_app_auth_access_token()?;
    app_session_cookie_header(&access_token)
}

/// Persist a validated app-session header for reuse across refreshes.
pub fn store_validated_app_session(cookie_header: &str) {
    if let Err(err) = crate::browser::cookie_cache::CookieHeaderCache::store(
        crate::core::ProviderId::Cursor,
        cookie_header,
        "cursor-app",
    ) {
        tracing::debug!("Could not persist Cursor app session: {err}");
    }
}

/// Validated app sessions go stale faster than browser imports: the app
/// rotates its token itself, so re-read the database every 15 minutes.
const APP_SESSION_MAX_AGE_SECS: i64 = 15 * 60;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_header_rebuilds_from_jwt_subject() {
        use base64::Engine;
        let payload =
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(br#"{"sub":"user_123"}"#);
        let token = format!("header.{payload}.signature");
        let header = app_session_cookie_header(&token).expect("cookie header");
        assert_eq!(
            header,
            format!("WorkosCursorSessionToken=user_123%3A%3A{token}")
        );
    }

    #[test]
    fn empty_or_malformed_token_yields_no_cookie() {
        assert!(app_session_cookie_header("   ").is_none());
        // A non-JWT token still builds a header with an empty subject — the
        // server rejects it and the caller falls back to browser cookies.
        assert_eq!(
            app_session_cookie_header("opaque-token").as_deref(),
            Some("WorkosCursorSessionToken=%3A%3Aopaque-token")
        );
    }

    #[test]
    fn utf16_le_blob_decodes() {
        let bytes: Vec<u8> = "WorkosCursorSessionToken"
            .encode_utf16()
            .flat_map(|unit| unit.to_le_bytes())
            .collect();
        assert_eq!(
            decode_sqlite_string(rusqlite::types::ValueRef::Blob(&bytes)).as_deref(),
            Some("WorkosCursorSessionToken")
        );
        // Unpaired surrogate (0xD800) is invalid UTF-16 → no value.
        assert!(
            decode_sqlite_string(rusqlite::types::ValueRef::Blob(&[0x00, 0xd8, 0x41, 0x00]))
                .is_none()
        );
    }

    const FIXTURE_KEY: [u8; 32] = [7; 32];

    fn envelope(token: &str, key: &[u8]) -> String {
        let cipher = Aes256Gcm::new_from_slice(key).unwrap();
        let nonce = [3u8; GCM_NONCE_LEN];
        let mut blob = OSCRYPT_PREFIX.to_vec();
        blob.extend_from_slice(&nonce);
        blob.extend(
            cipher
                .encrypt(Nonce::from_slice(&nonce), token.as_bytes())
                .unwrap(),
        );
        format!(
            "cursor-safe-storage:v1:{}",
            base64::engine::general_purpose::STANDARD.encode(blob)
        )
    }

    #[test]
    fn plain_token_is_returned_without_reading_the_key() {
        let token = resolve_stored_access_token("header.payload.signature", || {
            panic!("plain tokens must not read Safe Storage")
        });
        assert_eq!(token.as_deref(), Some("header.payload.signature"));
    }

    #[test]
    fn safe_storage_envelope_decrypts_to_the_jwt() {
        let stored = envelope("header.payload.signature", &FIXTURE_KEY);
        let token = resolve_stored_access_token(&stored, || Some(FIXTURE_KEY.to_vec()));
        assert_eq!(token.as_deref(), Some("header.payload.signature"));
    }

    #[test]
    fn unreadable_envelopes_yield_no_token() {
        let valid = envelope("header.payload.signature", &FIXTURE_KEY);
        let wrong_key = || Some(vec![9u8; 32]);
        assert!(resolve_stored_access_token(&valid, wrong_key).is_none());
        assert!(resolve_stored_access_token(&valid, || None).is_none());

        let truncated = format!(
            "cursor-safe-storage:v1:{}",
            base64::engine::general_purpose::STANDARD.encode(b"v10short")
        );
        let unknown_version = valid.replacen(":v1:", ":v2:", 1);
        for stored in [
            "cursor-safe-storage:v1:not base64!".to_string(),
            truncated,
            unknown_version,
            "cursor-safe-storage:".to_string(),
        ] {
            assert!(
                resolve_stored_access_token(&stored, || Some(FIXTURE_KEY.to_vec())).is_none(),
                "{stored}"
            );
        }
        // No token means no rebuilt app cookie; browser cookies take over.
        assert!(app_session_cookie_header("").is_none());
    }

    #[test]
    fn db_path_points_at_cursor_global_storage() {
        let path = app_auth_db_path().expect("path");
        assert!(path.ends_with("state.vscdb"));
        assert!(path.to_string_lossy().contains("Cursor"));
    }
}
