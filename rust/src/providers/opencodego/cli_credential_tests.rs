use super::*;
use rusqlite::Connection;

fn now() -> DateTime<Utc> {
    DateTime::parse_from_rfc3339("2026-10-26T12:00:00Z")
        .unwrap()
        .with_timezone(&Utc)
}

fn future_ms() -> i64 {
    now().timestamp_millis() + 3_600_000
}

fn store(dir: &Path) -> CredentialStore {
    CredentialStore {
        auth_path: dir.join("auth.json"),
        db_path: dir.join("opencode.db"),
    }
}

fn credential_db(store: &CredentialStore, rows: &[(&str, i64, &str)]) {
    let conn = Connection::open(&store.db_path).unwrap();
    conn.execute_batch(
        "CREATE TABLE credential (id INTEGER PRIMARY KEY, integration_id TEXT NOT NULL, active INTEGER NOT NULL, value TEXT NOT NULL);",
    )
    .unwrap();
    for (integration, active, value) in rows {
        conn.execute(
            "INSERT INTO credential (integration_id, active, value) VALUES (?1, ?2, ?3)",
            rusqlite::params![integration, active, value],
        )
        .unwrap();
    }
}

fn legacy_auth(store: &CredentialStore, key: &str) {
    std::fs::write(
        &store.auth_path,
        format!(r#"{{"opencode-go":{{"type":"api","key":"{key}"}}}}"#),
    )
    .unwrap();
}

fn oauth(access: &str, expires: i64) -> String {
    format!(r#"{{"type":"oauth","access":"{access}","refresh":"r","expires":{expires}}}"#)
}

#[test]
fn active_go_key_wins_over_the_general_oauth_login() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    credential_db(
        &store,
        &[
            ("opencode", 1, &oauth("general-access", future_ms())),
            ("opencode-go", 1, r#"{"type":"key","key":"go-key"}"#),
            ("opencode-go", 0, r#"{"type":"key","key":"inactive"}"#),
        ],
    );
    assert_eq!(
        load_from_store(&store, now()).unwrap().as_deref(),
        Some("go-key")
    );
}

#[test]
fn general_login_counts_only_as_unexpired_oauth() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    credential_db(
        &store,
        &[("opencode", 1, &oauth("general-access", future_ms()))],
    );
    assert_eq!(
        load_from_store(&store, now()).unwrap().as_deref(),
        Some("general-access")
    );

    let dir = tempfile::tempdir().unwrap();
    let key_only = self::store(dir.path());
    credential_db(
        &key_only,
        &[("opencode", 1, r#"{"type":"key","key":"general-key"}"#)],
    );
    assert_eq!(load_from_store(&key_only, now()).unwrap(), None);

    let dir = tempfile::tempdir().unwrap();
    let expired = self::store(dir.path());
    credential_db(
        &expired,
        &[("opencode", 1, &oauth("stale", now().timestamp_millis() - 1))],
    );
    assert!(matches!(
        load_from_store(&expired, now()),
        Err(ProviderError::AuthRequired)
    ));
}

#[test]
fn ambiguous_or_malformed_credentials_fail_closed() {
    let cases: [&[(&str, i64, &str)]; 4] = [
        &[
            ("opencode-go", 1, r#"{"type":"key","key":"one"}"#),
            ("opencode-go", 1, r#"{"type":"key","key":"two"}"#),
        ],
        &[("opencode-go", 1, "not json")],
        &[("opencode-go", 1, r#"{"type":"key","key":"has space"}"#)],
        &[("opencode-go", 1, r#"{"type":"wellknown","key":"x"}"#)],
    ];
    for rows in cases {
        let dir = tempfile::tempdir().unwrap();
        let store = store(dir.path());
        credential_db(&store, rows);
        assert!(matches!(
            load_from_store(&store, now()),
            Err(ProviderError::AuthRequired)
        ));
    }
}

#[test]
fn a_credential_table_never_revives_stale_legacy_auth() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    credential_db(&store, &[]);
    legacy_auth(&store, "stale-legacy");
    assert_eq!(load_from_store(&store, now()).unwrap(), None);
}

#[test]
fn legacy_auth_is_used_without_a_credential_table() {
    let dir = tempfile::tempdir().unwrap();
    let store = store(dir.path());
    legacy_auth(&store, "legacy-key");
    assert_eq!(
        load_from_store(&store, now()).unwrap().as_deref(),
        Some("legacy-key")
    );

    // A history-only database without a credential table keeps the legacy file.
    Connection::open(&store.db_path)
        .unwrap()
        .execute_batch("CREATE TABLE message (id TEXT PRIMARY KEY, data TEXT);")
        .unwrap();
    assert_eq!(
        load_from_store(&store, now()).unwrap().as_deref(),
        Some("legacy-key")
    );

    std::fs::write(
        &store.auth_path,
        r#"{"opencode-go":{"type":"oauth","access":"x"}}"#,
    )
    .unwrap();
    assert_eq!(load_from_store(&store, now()).unwrap(), None);
}

#[test]
fn no_store_files_means_no_login() {
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(load_from_store(&store(dir.path()), now()).unwrap(), None);
}

#[test]
fn store_lookup_honors_xdg_data_home_and_opencode_db() {
    let defaults = vec![PathBuf::from(r"C:\Users\me\.local\share\opencode")];
    let data_home = if cfg!(windows) { r"D:\data" } else { "/data" };
    let stores = credential_stores(Some(data_home), None, defaults.clone());
    assert_eq!(
        stores,
        vec![CredentialStore {
            auth_path: Path::new(data_home).join("opencode").join("auth.json"),
            db_path: Path::new(data_home).join("opencode").join("opencode.db"),
        }]
    );

    // A relative XDG_DATA_HOME is ignored, like the CLI does.
    let stores = credential_stores(Some("relative"), Some("custom.db"), defaults.clone());
    assert_eq!(stores[0].db_path, defaults[0].join("custom.db"));

    let absolute_db = Path::new(data_home).join("elsewhere.db");
    let stores = credential_stores(None, absolute_db.to_str(), defaults.clone());
    assert_eq!(stores[0].db_path, absolute_db);
    assert_eq!(stores[0].auth_path, defaults[0].join("auth.json"));

    assert!(credential_stores(None, Some(":memory:"), defaults).is_empty());
}
