use super::*;

fn snapshot_window_with(
    used_percent: f64,
    window_minutes: Option<u32>,
    resets_at: Option<chrono::DateTime<chrono::Utc>>,
    reset_description: Option<String>,
) -> RateWindowSnapshot {
    RateWindowSnapshot {
        used_percent,
        remaining_percent: 100.0 - used_percent,
        window_minutes,
        resets_at: resets_at.map(|dt| dt.to_rfc3339()),
        reset_description,
        ..Default::default()
    }
}

#[test]
fn tray_status_prefers_relative_reset_countdown() {
    let window = snapshot_window_with(
        13.0,
        Some(300),
        Some(chrono::Utc::now() + chrono::Duration::minutes(125)),
        Some("Jun 10 at 3:00PM".to_string()),
    );

    let label = compact_tray_status_label(&window, Language::English);

    assert!(label.starts_with("13% • Resets in 2h "));
    assert!(label.ends_with('m'));
    assert!(!label.contains("Jun 10"));
}

#[test]
fn tray_status_normalizes_fallback_reset_description() {
    let window = snapshot_window_with(8.0, Some(300), None, Some("2h 05m".to_string()));

    assert_eq!(
        compact_tray_status_label(&window, Language::English),
        "8% • Resets in 2h 05m"
    );
}

#[test]
fn credit_balance_detail_crosses_the_bridge_and_is_not_a_reset_phrase() {
    let rw = RateWindow::with_details(25.0, None, None, Some("750 / 1000 credits left".into()))
        .with_description_as_detail();
    let window = RateWindowSnapshot::from_rate_window(&rw);

    assert!(window.description_is_detail);
    assert_eq!(
        window.reset_description.as_deref(),
        Some("750 / 1000 credits left")
    );
    let json = serde_json::to_value(&window).unwrap();
    assert_eq!(json["descriptionIsDetail"], true);
    assert_eq!(compact_tray_status_label(&window, Language::English), "25%");

    let plain = RateWindowSnapshot::from_rate_window(&RateWindow::new(10.0));
    let json = serde_json::to_value(&plain).unwrap();
    assert_eq!(json["descriptionIsDetail"], false);
}

#[test]
fn japanese_tray_status_label_has_no_english_reset_text() {
    use codexbar::settings::Language;

    let window = snapshot_window_with(
        13.0,
        Some(300),
        Some(chrono::Utc::now() + chrono::Duration::minutes(125)),
        None,
    );

    let label = compact_tray_status_label(&window, Language::Japanese);

    assert!(label.contains("リセットまで"), "{label}");
    assert!(!label.to_ascii_lowercase().contains("resets in"), "{label}");
    assert!(label.contains("13%"), "{label}");
}

#[test]
fn japanese_tray_status_strips_english_fallback_reset_prefix() {
    use codexbar::settings::Language;

    let window = snapshot_window_with(8.0, Some(300), None, Some("Resets in 2h 05m".to_string()));

    let label = compact_tray_status_label(&window, Language::Japanese);

    assert!(label.contains("リセットまで"), "{label}");
    assert!(!label.to_ascii_lowercase().contains("resets in"), "{label}");
    assert!(label.contains("2時間 05分"), "{label}");
}

#[test]
fn tray_status_label_relocalizes_without_refetch() {
    let window = snapshot_window_with(
        13.0,
        Some(300),
        Some(chrono::Utc::now() + chrono::Duration::minutes(125)),
        None,
    );

    let english = compact_tray_status_label(&window, Language::English);
    let japanese = compact_tray_status_label(&window, Language::Japanese);

    assert!(english.contains("Resets in"), "{english}");
    assert!(japanese.contains("リセットまで"), "{japanese}");
    assert!(
        !japanese.to_ascii_lowercase().contains("resets in"),
        "{japanese}"
    );
}
