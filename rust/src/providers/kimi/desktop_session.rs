//! Current Kimi Desktop session discovery (upstream `KimiDesktopSessionDiscovery`, #4416 /
//! #3536).
//!
//! For China, the Desktop session is the exact `https://www.kimi.com` / `access_token` record in
//! the app's primary Local Storage, `%APPDATA%\kimi-desktop\Local Storage\leveldb` on Windows
//! (upstream reads `~/Library/Application Support/kimi-desktop/...`). It is read with the strict
//! LevelDB reader, so a deleted (signed-out) or replaced session is never recovered from an older
//! record, and only a bare, unexpired access JWT for the `kimi.com` audience is accepted. China
//! never falls back to the historical Desktop `kimi-auth` cookie. International keeps its
//! existing Desktop cookie discovery; its Local Storage is not read.
//!
//! The returned credential re-reads the session before each request (see
//! [`KimiWebCredential::check_dispatch`]). Tokens are secrets and are never logged.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};

use super::KimiRegion;
use super::credential::KimiWebCredential;
use super::desktop_token::KimiDesktopAuthToken;
use crate::browser::leveldb::read_current_value_strict;

const DESKTOP_APP_DIR: &str = "kimi-desktop";
const DESKTOP_ORIGIN: &str = "https://www.kimi.com";
const ACCESS_TOKEN_KEY: &str = "access_token";
const REQUIRED_AUDIENCE: &str = "kimi.com";
const LATIN1_FORMAT: u8 = 1;
/// Upstream bound: a Latin-1 format byte plus at most 16 KiB of token text.
const MAX_RAW_VALUE_BYTES: usize = 16 * 1024 + 1;

const JWT_SEGMENT: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::URL_SAFE,
    GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
);

/// The automatic Kimi Desktop web credential for `region`, or `None` when there is no usable
/// session. Callers gate this on the automatic cookie source.
pub(super) fn desktop_credential(region: KimiRegion) -> Option<KimiWebCredential> {
    match region {
        KimiRegion::China => DesktopSession::new(dirs::data_dir()?, unix_now).credential(),
        KimiRegion::International => {
            KimiDesktopAuthToken::load_for_region(region).map(KimiWebCredential::plain)
        }
    }
}

#[derive(Clone)]
struct DesktopSession {
    /// Roaming AppData (Electron `userData` parent). Only this host-owned root may be a link;
    /// the fixed profile path below it must not be.
    data_root: PathBuf,
    now: fn() -> f64,
}

impl DesktopSession {
    fn new(data_root: PathBuf, now: fn() -> f64) -> Self {
        Self { data_root, now }
    }

    fn credential(self) -> Option<KimiWebCredential> {
        let token = self.access_token()?;
        let expected = token.clone();
        Some(KimiWebCredential::desktop(token, move || {
            self.access_token().as_deref() == Some(expected.as_str())
        }))
    }

    fn access_token(&self) -> Option<String> {
        let dir = self.local_storage_dir()?;
        let raw = read_current_value_strict(&dir, &raw_key())
            .inspect_err(|error| {
                tracing::debug!(%error, "Kimi Desktop session is not readable");
            })
            .ok()??;
        access_token_from_raw(&raw, (self.now)())
    }

    fn local_storage_dir(&self) -> Option<PathBuf> {
        let mut dir = self.data_root.clone();
        for component in [DESKTOP_APP_DIR, "Local Storage", "leveldb"] {
            dir.push(component);
            if !is_plain_directory(&dir) {
                return None;
            }
        }
        Some(dir)
    }
}

/// `symlink_metadata` does not traverse links; on Windows junctions report as symlinks too.
fn is_plain_directory(path: &Path) -> bool {
    std::fs::symlink_metadata(path)
        .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
}

/// Chromium Local Storage key: `_<origin>\0` then a Latin-1 format byte and the item key.
fn raw_key() -> Vec<u8> {
    let mut key = format!("_{DESKTOP_ORIGIN}\0").into_bytes();
    key.push(LATIN1_FORMAT);
    key.extend_from_slice(ACCESS_TOKEN_KEY.as_bytes());
    key
}

/// Upstream `accessToken(rawValue:now:)`: a Latin-1 bare JWT (no JSON wrapper) with a non-`none`
/// algorithm, `typ` `access`, a finite numeric expiry after `now`, the `kimi.com` audience, and an
/// optional not-before at or before `now`. These are structural checks only; the Kimi service
/// still authenticates the signature.
fn access_token_from_raw(raw: &[u8], now: f64) -> Option<String> {
    let (&format, body) = raw.split_first()?;
    if format != LATIN1_FORMAT
        || raw.len() > MAX_RAW_VALUE_BYTES
        || body.is_empty()
        || !now.is_finite()
    {
        return None;
    }
    if !body
        .iter()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return None;
    }
    let token = std::str::from_utf8(body).ok()?;
    let segments: Vec<&str> = token.split('.').collect();
    let [header, claims, signature] = segments.as_slice() else {
        return None;
    };
    if header.is_empty() || claims.is_empty() || signature.is_empty() {
        return None;
    }

    let header = decode_object(header)?;
    let algorithm = header.get("alg")?.as_str()?;
    if algorithm.is_empty() || algorithm.eq_ignore_ascii_case("none") {
        return None;
    }

    let claims = decode_object(claims)?;
    if claims.get("typ")?.as_str()? != "access" {
        return None;
    }
    if numeric_date(claims.get("exp")?)? <= now {
        return None;
    }
    if !audiences(claims.get("aud")).contains(&REQUIRED_AUDIENCE) {
        return None;
    }
    if let Some(not_before) = claims.get("nbf")
        && numeric_date(not_before)? > now
    {
        return None;
    }
    Some(token.to_string())
}

fn decode_object(segment: &str) -> Option<serde_json::Map<String, serde_json::Value>> {
    let bytes = JWT_SEGMENT.decode(segment).ok()?;
    match serde_json::from_slice(&bytes).ok()? {
        serde_json::Value::Object(object) => Some(object),
        _ => None,
    }
}

/// JSON numbers only: booleans and numeric strings are not dates.
fn numeric_date(value: &serde_json::Value) -> Option<f64> {
    value.as_f64().filter(|date| date.is_finite())
}

/// `aud` as a string or an array made only of strings; anything else has no audience.
fn audiences(value: Option<&serde_json::Value>) -> Vec<&str> {
    match value {
        Some(serde_json::Value::String(audience)) => vec![audience.as_str()],
        Some(serde_json::Value::Array(items)) => items
            .iter()
            .map(serde_json::Value::as_str)
            .collect::<Option<Vec<_>>>()
            .unwrap_or_default(),
        _ => Vec::new(),
    }
}

fn unix_now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(f64::NAN, |elapsed| elapsed.as_secs_f64())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    const NOW: f64 = 1_800_000_000.0;

    fn fixed_now() -> f64 {
        NOW
    }

    fn segment(value: &serde_json::Value) -> String {
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(value.to_string())
    }

    /// Synthetic JWT built from readable claims; no real credential.
    fn jwt(claims: serde_json::Value) -> String {
        format!(
            "{}.{}.signature",
            segment(&serde_json::json!({ "alg": "HS256", "typ": "JWT" })),
            segment(&claims)
        )
    }

    fn access_claims(sub: &str) -> serde_json::Value {
        serde_json::json!({
            "typ": "access",
            "aud": ["kimi.com"],
            "exp": NOW + 3600.0,
            "nbf": NOW - 60.0,
            "sub": sub,
        })
    }

    fn latin1(token: &str) -> Vec<u8> {
        let mut raw = vec![LATIN1_FORMAT];
        raw.extend_from_slice(token.as_bytes());
        raw
    }

    fn accepted(raw: &[u8]) -> bool {
        access_token_from_raw(raw, NOW).is_some()
    }

    #[test]
    fn current_access_jwt_is_accepted_bare() {
        let token = jwt(access_claims("a"));
        assert_eq!(access_token_from_raw(&latin1(&token), NOW), Some(token));
        let single_audience = jwt(serde_json::json!({
            "typ": "access", "aud": "kimi.com", "exp": NOW + 1.0
        }));
        assert!(accepted(&latin1(&single_audience)));
    }

    #[test]
    fn wrapped_encoded_or_oversized_values_are_rejected() {
        let token = jwt(access_claims("a"));
        let mut utf16 = vec![0u8];
        utf16.extend(token.bytes().flat_map(|byte| [byte, 0]));
        let oversized = format!("{}.{}.{}", "a".repeat(8 * 1024), "b".repeat(8 * 1024), "c");
        for raw in [
            latin1(&format!("\"{token}\"")),
            latin1(&format!(" {token}")),
            latin1(&format!("{token}; kimi-auth=x")),
            utf16,
            latin1(""),
            latin1("a.b"),
            latin1("a..c"),
            latin1(&oversized),
            Vec::new(),
        ] {
            assert!(!accepted(&raw));
        }
    }

    #[test]
    fn claims_policy_rejects_refresh_expired_and_lookalike_tokens() {
        let base = access_claims("a");
        let with = |key: &str, value: serde_json::Value| {
            let mut claims = base.clone();
            claims[key] = value;
            latin1(&jwt(claims))
        };
        let without = |key: &str| {
            let mut claims = base.clone();
            claims.as_object_mut().unwrap().remove(key);
            latin1(&jwt(claims))
        };
        for raw in [
            with("typ", serde_json::json!("refresh")),
            without("typ"),
            with("exp", serde_json::json!(NOW)),
            with("exp", serde_json::json!(NOW - 1.0)),
            with("exp", serde_json::json!("1900000000")),
            with("exp", serde_json::json!(true)),
            without("exp"),
            with("aud", serde_json::json!(["kimi.com.evil"])),
            with("aud", serde_json::json!("www.kimi.com")),
            with("aud", serde_json::json!(["kimi.com", 7])),
            without("aud"),
            with("nbf", serde_json::json!(NOW + 60.0)),
            with("nbf", serde_json::json!("soon")),
        ] {
            assert!(!accepted(&raw));
        }
        assert!(accepted(&without("nbf")));
        assert!(access_token_from_raw(&latin1(&jwt(base)), f64::NAN).is_none());
    }

    #[test]
    fn unsigned_or_headerless_tokens_are_rejected() {
        let claims = segment(&access_claims("a"));
        for header in [
            serde_json::json!({ "alg": "none" }),
            serde_json::json!({ "alg": "NONE" }),
            serde_json::json!({ "alg": "" }),
            serde_json::json!({ "typ": "JWT" }),
            serde_json::json!(["HS256"]),
        ] {
            let token = format!("{}.{claims}.signature", segment(&header));
            assert!(!accepted(&latin1(&token)));
        }
    }

    /// One LevelDB log record holding the Desktop `access_token` put (or delete when `None`).
    fn write_log(sequence: u64, token: Option<&str>) -> Vec<u8> {
        write_log_for(&raw_key(), sequence, token)
    }

    fn write_log_for(key: &[u8], sequence: u64, token: Option<&str>) -> Vec<u8> {
        let mut batch = sequence.to_le_bytes().to_vec();
        batch.extend_from_slice(&1u32.to_le_bytes());
        batch.push(u8::from(token.is_some()));
        batch.push(u8::try_from(key.len()).unwrap());
        batch.extend_from_slice(key);
        if let Some(token) = token {
            let value = latin1(token);
            let mut length = Vec::new();
            let mut remaining = value.len();
            while remaining >= 0x80 {
                length.push(u8::try_from(remaining & 0x7f).unwrap() | 0x80);
                remaining >>= 7;
            }
            length.push(u8::try_from(remaining).unwrap());
            batch.extend_from_slice(&length);
            batch.extend_from_slice(&value);
        }
        let mut record = vec![0, 0, 0, 0];
        record.extend_from_slice(&u16::try_from(batch.len()).unwrap().to_le_bytes());
        record.push(1);
        record.extend_from_slice(&batch);
        record
    }

    fn profile_root() -> (tempfile::TempDir, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let dir = root
            .path()
            .join(DESKTOP_APP_DIR)
            .join("Local Storage")
            .join("leveldb");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("CURRENT"), b"MANIFEST-000001\n").unwrap();
        (root, dir)
    }

    fn write_records(dir: &Path, records: &[(u64, Option<&str>)]) {
        let log: Vec<u8> = records
            .iter()
            .flat_map(|(sequence, token)| write_log(*sequence, *token))
            .collect();
        std::fs::write(dir.join("000003.log"), log).unwrap();
    }

    fn session(root: &Path) -> DesktopSession {
        DesktopSession::new(root.to_path_buf(), fixed_now)
    }

    #[test]
    fn newest_desktop_session_is_used_and_stale_tokens_are_never_recovered() {
        let (root, dir) = profile_root();
        let old = jwt(access_claims("old"));
        let current = jwt(access_claims("current"));

        write_records(&dir, &[(1, Some(&old)), (2, Some(&current))]);
        assert_eq!(session(root.path()).access_token(), Some(current.clone()));

        // Signed out: the deletion hides every older record.
        write_records(&dir, &[(1, Some(&old)), (2, Some(&current)), (3, None)]);
        assert_eq!(session(root.path()).access_token(), None);

        // The current record is expired: no older live token is recovered either.
        let expired = jwt(serde_json::json!({
            "typ": "access", "aud": "kimi.com", "exp": NOW - 1.0
        }));
        write_records(&dir, &[(1, Some(&old)), (2, Some(&expired))]);
        assert_eq!(session(root.path()).access_token(), None);
    }

    #[test]
    fn other_origins_and_incomplete_profiles_yield_no_session() {
        let (root, dir) = profile_root();
        let token = jwt(access_claims("a"));
        let key_for = |origin: &str, item: &str| {
            let mut key = format!("_{origin}\0").into_bytes();
            key.push(LATIN1_FORMAT);
            key.extend_from_slice(item.as_bytes());
            key
        };
        let mut log = write_log_for(
            &key_for("https://www.kimi.ai", ACCESS_TOKEN_KEY),
            1,
            Some(&token),
        );
        log.extend(write_log_for(
            &key_for(DESKTOP_ORIGIN, "refresh_token"),
            2,
            Some(&token),
        ));
        log.extend(write_log_for(
            &key_for(DESKTOP_ORIGIN, "access_token_v2"),
            3,
            Some(&token),
        ));
        std::fs::write(dir.join("000003.log"), log).unwrap();
        assert_eq!(session(root.path()).access_token(), None);

        // An unreadable table could hide a newer deletion, so the session is not trusted.
        write_records(&dir, &[(1, Some(&token))]);
        assert_eq!(session(root.path()).access_token(), Some(token));
        std::fs::write(dir.join("000004.ldb"), b"truncated table").unwrap();
        assert_eq!(session(root.path()).access_token(), None);

        let missing = tempfile::tempdir().unwrap();
        assert_eq!(session(missing.path()).access_token(), None);
    }

    #[cfg(windows)]
    #[test]
    fn junctioned_profile_directory_is_rejected() {
        let (target_root, target_dir) = profile_root();
        write_records(&target_dir, &[(1, Some(&jwt(access_claims("a"))))]);
        assert!(session(target_root.path()).access_token().is_some());

        let root = tempfile::tempdir().unwrap();
        let link = root.path().join(DESKTOP_APP_DIR);
        let status = std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(&link)
            .arg(target_root.path().join(DESKTOP_APP_DIR))
            .output()
            .unwrap();
        assert!(status.status.success(), "mklink /J failed");
        assert!(session(root.path()).access_token().is_none());
    }

    static CHANGING_NOW: AtomicU64 = AtomicU64::new(0);

    fn changing_now() -> f64 {
        f64::from_bits(CHANGING_NOW.load(Ordering::SeqCst))
    }

    #[test]
    fn acquired_credential_revalidates_the_session_before_dispatch() {
        CHANGING_NOW.store(NOW.to_bits(), Ordering::SeqCst);
        let (root, dir) = profile_root();
        let current = jwt(access_claims("current"));
        let replacement = jwt(access_claims("replacement"));
        write_records(&dir, &[(1, Some(&current))]);

        let client = reqwest::Client::new();
        let url = KimiRegion::China.web_api_url("svc");
        let check = |credential: &KimiWebCredential| {
            let request = super::super::kimi_web_request(
                &client,
                &url,
                KimiRegion::China,
                credential.token(),
                serde_json::json!({}),
            )
            .unwrap();
            credential.check_dispatch(&request, KimiRegion::China)
        };

        let credential = DesktopSession::new(root.path().to_path_buf(), changing_now)
            .credential()
            .expect("current session");
        assert_eq!(credential.token(), current);
        assert!(check(&credential).is_ok());

        // Expired by the time of dispatch.
        CHANGING_NOW.store((NOW + 7200.0).to_bits(), Ordering::SeqCst);
        assert!(check(&credential).is_err());
        CHANGING_NOW.store(NOW.to_bits(), Ordering::SeqCst);
        assert!(check(&credential).is_err(), "invalidation is sticky");

        // Replaced by another account: the old credential is rejected; a fresh read uses the new one.
        let credential = DesktopSession::new(root.path().to_path_buf(), changing_now)
            .credential()
            .unwrap();
        write_records(&dir, &[(1, Some(&current)), (2, Some(&replacement))]);
        assert!(check(&credential).is_err());
        let fresh = DesktopSession::new(root.path().to_path_buf(), changing_now)
            .credential()
            .unwrap();
        assert_eq!(fresh.token(), replacement);
        assert!(check(&fresh).is_ok());

        // Signed out after acquisition.
        write_records(&dir, &[(1, Some(&replacement)), (2, None)]);
        assert!(check(&fresh).is_err());
    }
}
