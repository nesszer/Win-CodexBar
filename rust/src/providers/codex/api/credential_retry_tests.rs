//! Upstream 0.69.0 #4088: bounded reread of `auth.json` while its owner
//! publishes it (`CodexOAuthExpiryPipelineTests`).
//!
//! The loop tests drive [`CodexApi::reread_during_owner_publication`] with the
//! real file read, parser and gate, and count reads the way upstream's
//! injected reader does. The clock is paused, so the 50 ms retry delays are
//! exact and the tests never race a writer thread.

use super::credentials::CREDENTIAL_READ_RETRY_DELAY;
use super::*;
use base64::Engine;
use std::cell::Cell;
use std::path::Path;
use std::time::Duration;

/// Upstream's fresh fixture expiry (2100-01-01T00:00:00Z).
const FRESH_EXPIRY: i64 = 4_102_444_800;

fn api_for(home: &Path) -> CodexApi {
    CodexApi::new().with_codex_home(home)
}

/// A native OAuth `auth.json` whose JWT access token expires at `exp`
/// (seconds since the epoch). It carries `last_refresh`, so the gate decides
/// on expiry alone and never reads the host settings.
fn oauth_auth_json(exp: i64) -> String {
    let payload =
        base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(format!(r#"{{"exp":{exp}}}"#));
    format!(
        r#"{{"last_refresh":"2000-01-01T00:00:00Z","tokens":{{"access_token":"h.{payload}.s","refresh_token":"r","account_id":"acct_pub"}}}}"#
    )
}

fn access_token_of(auth_json: &str) -> String {
    let json: serde_json::Value = serde_json::from_str(auth_json).expect("fixture json");
    json["tokens"]["access_token"]
        .as_str()
        .expect("fixture access token")
        .to_string()
}

/// Replace `auth.json` atomically, the way the Codex CLI publishes it.
fn publish(home: &Path, contents: &str) {
    let staging = home.join("auth.json.tmp");
    std::fs::write(&staging, contents).expect("stage auth.json");
    std::fs::rename(&staging, home.join("auth.json")).expect("publish auth.json");
}

/// Leave `auth.json` in the named state of an unusable or in-progress file.
fn write_state(home: &Path, state: &str) {
    let auth = home.join("auth.json");
    match state {
        "missing" => {}
        // A directory named auth.json exists but cannot be read as a file.
        "unreadable" => std::fs::create_dir(&auth).expect("auth.json directory"),
        "partial" => std::fs::write(&auth, r#"{"tokens":"#).expect("partial auth.json"),
        "incomplete" => std::fs::write(&auth, r#"{"tokens":{}}"#).expect("incomplete auth.json"),
        "expired" => std::fs::write(&auth, oauth_auth_json(1)).expect("expired auth.json"),
        "near-expiry" => std::fs::write(&auth, oauth_auth_json(Utc::now().timestamp() + 120))
            .expect("near-expiry auth.json"),
        other => panic!("unknown auth.json state {other}"),
    }
}

fn auth_bytes(home: &Path) -> Option<Vec<u8>> {
    std::fs::read(home.join("auth.json")).ok()
}

/// Upstream `OAuth fetch retries an owner publication in progress`: the first
/// read sees the publication in progress (missing, torn, incomplete, expired
/// or inside the renewal window), the second read sees the owner's
/// replacement, keeps its workspace, and nothing is written back.
#[tokio::test(start_paused = true)]
async fn usage_read_retries_an_owner_publication_in_progress() {
    for publication in ["missing", "partial", "incomplete", "expired", "near-expiry"] {
        let dir = tempfile::tempdir().expect("codex home");
        let api = api_for(dir.path());
        let fresh = oauth_auth_json(FRESH_EXPIRY);
        write_state(dir.path(), publication);
        let reads = Cell::new(0);

        let credentials = CodexApi::reread_during_owner_publication(|| {
            reads.set(reads.get() + 1);
            let read = api.load_credentials_once();
            if reads.get() == 1 {
                publish(dir.path(), &fresh);
            }
            read
        })
        .await
        .unwrap_or_else(|error| panic!("{publication}: {error:?}"));

        assert_eq!(
            credentials.access_token,
            access_token_of(&fresh),
            "{publication}"
        );
        assert_eq!(
            credentials.account_id.as_deref(),
            Some("acct_pub"),
            "{publication}"
        );
        assert_eq!(reads.get(), 2, "{publication}");
        assert_eq!(
            auth_bytes(dir.path()),
            Some(fresh.into_bytes()),
            "{publication}"
        );
    }
}

/// Upstream `OAuth read retries are bounded and preserve the final error`:
/// three reads, then the last failure keeps its category (unchanged stale
/// credentials still need their owner's renewal) and nothing is written.
#[tokio::test(start_paused = true)]
async fn usage_read_retries_are_bounded_and_keep_the_final_error() {
    for failure in ["missing", "partial", "incomplete", "expired", "unreadable"] {
        let dir = tempfile::tempdir().expect("codex home");
        let api = api_for(dir.path());
        write_state(dir.path(), failure);
        let before = auth_bytes(dir.path());
        let reads = Cell::new(0);

        let error = CodexApi::reread_during_owner_publication(|| {
            reads.set(reads.get() + 1);
            api.load_credentials_once()
        })
        .await
        .err()
        .unwrap_or_else(|| panic!("{failure}: expected a credential error"));

        let category_kept = match failure {
            "missing" => matches!(error, ProviderError::NotInstalled(_)),
            "partial" | "incomplete" => matches!(error, ProviderError::Parse(_)),
            "expired" => matches!(error, ProviderError::AuthRequired),
            "unreadable" => matches!(error, ProviderError::Other(_)),
            _ => false,
        };
        assert!(category_kept, "{failure}: {error:?}");
        assert_eq!(reads.get(), 3, "{failure}");
        assert_eq!(auth_bytes(dir.path()), before, "{failure}");
    }
}

/// `load_credentials` waits exactly two retry delays before it reports a
/// missing file or unchanged stale credentials.
#[tokio::test(start_paused = true)]
async fn load_credentials_rereads_before_reporting_the_final_error() {
    for failure in ["missing", "expired"] {
        let dir = tempfile::tempdir().expect("codex home");
        write_state(dir.path(), failure);
        let started = tokio::time::Instant::now();

        let error = api_for(dir.path())
            .load_credentials()
            .await
            .err()
            .expect("credential error");

        let waited = started.elapsed();
        assert!(
            waited >= 2 * CREDENTIAL_READ_RETRY_DELAY && waited < 3 * CREDENTIAL_READ_RETRY_DELAY,
            "{failure}: waited {waited:?}"
        );
        if failure == "missing" {
            assert!(matches!(error, ProviderError::NotInstalled(_)), "{error:?}");
        } else {
            assert!(matches!(error, ProviderError::AuthRequired), "{error:?}");
        }
    }
}

#[tokio::test(start_paused = true)]
async fn load_credentials_returns_usable_credentials_without_waiting() {
    let dir = tempfile::tempdir().expect("codex home");
    let fresh = oauth_auth_json(FRESH_EXPIRY);
    publish(dir.path(), &fresh);
    let started = tokio::time::Instant::now();

    let credentials = api_for(dir.path())
        .load_credentials()
        .await
        .expect("fresh credentials");

    assert_eq!(started.elapsed(), Duration::ZERO);
    assert_eq!(credentials.access_token, access_token_of(&fresh));
}

/// Upstream `cancelled OAuth fetch does not read credentials`: a load that is
/// never polled does not read, and dropping a load during its retry delay
/// stops further reads.
#[tokio::test(start_paused = true)]
async fn dropping_the_load_cancels_the_retry_delay() {
    let dir = tempfile::tempdir().expect("codex home");
    let api = api_for(dir.path());
    let reads = Cell::new(0);
    let read = || {
        reads.set(reads.get() + 1);
        api.load_credentials_once()
    };

    drop(CodexApi::reread_during_owner_publication(read));
    assert_eq!(reads.get(), 0, "an unpolled load must not read");

    let outcome = tokio::time::timeout(
        Duration::from_millis(20),
        CodexApi::reread_during_owner_publication(read),
    )
    .await;
    assert!(outcome.is_err(), "the retry delay must be cancellable");
    assert_eq!(reads.get(), 1, "no read after the load was dropped");
}
