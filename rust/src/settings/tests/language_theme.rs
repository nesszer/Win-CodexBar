//! UI language and theme preference defaults, tags and round trips.

use super::*;

#[test]
fn test_language_defaults_to_english() {
    let settings = Settings::default();
    assert_eq!(settings.ui_language, Language::English);
}

#[test]
fn test_language_table_pins_display_names_serde_tags_and_aliases() {
    // (variant, display name, serde tag, extra inputs `resolve` must accept)
    let table: [(Language, &str, &str, &[&str]); 10] = [
        (Language::English, "English", "english", &[]),
        (Language::Chinese, "中文", "chinese", &[]),
        (
            Language::ChineseTraditional,
            "繁體中文",
            "chinesetraditional",
            &["zh-tw", "zh-hant-tw", "繁體中文"],
        ),
        (Language::Japanese, "日本語", "japanese", &[]),
        (Language::Korean, "한국어", "korean", &[]),
        (Language::Spanish, "Español", "spanish", &[]),
        (
            Language::PortugueseBrazil,
            "Português (Brasil)",
            "portuguesebrazil",
            &[
                "pt",
                "pt-BR",
                "Portuguese",
                "Português",
                "portugues",
                "Português (Brasil)",
            ],
        ),
        (
            Language::Russian,
            "Русский",
            "russian",
            &["ru-RU", "Русский"],
        ),
        (
            Language::Turkish,
            "Türkçe",
            "turkish",
            &["tr-TR", "Türkçe", "turkce"],
        ),
        (
            Language::Ukrainian,
            "Українська",
            "ukrainian",
            &["uk", "uk-UA", "Українська"],
        ),
    ];
    let listed: Vec<Language> = table.iter().map(|row| row.0).collect();
    assert_eq!(Language::all(), listed.as_slice());
    for (language, display_name, tag, aliases) in table {
        assert_eq!(language.display_name(), display_name);
        let quoted = format!("\"{tag}\"");
        assert_eq!(serde_json::to_string(&language).unwrap(), quoted);
        assert_eq!(serde_json::from_str::<Language>(&quoted).unwrap(), language);
        for input in std::iter::once(tag).chain(aliases.iter().copied()) {
            assert_eq!(
                Language::resolve(input),
                Some(language),
                "failed to resolve {input}"
            );
        }
    }
}

#[test]
fn test_settings_load_missing_language_field_defaults_to_english() {
    // Legacy settings JSON written before the ui_language field existed.
    let legacy_json = r#"{
            "enabled_providers": ["claude", "codex"],
            "refresh_interval_secs": 300,
            "start_minimized": false
        }"#;

    let settings: Settings = serde_json::from_str(legacy_json).expect("legacy settings parse");
    assert_eq!(settings.ui_language, Language::English);
}

#[test]
fn test_settings_roundtrip_with_language() {
    let settings = Settings {
        ui_language: Language::Chinese,
        ..Settings::default()
    };
    let (_, loaded) = round_trip(&settings);
    assert_eq!(loaded.ui_language, Language::Chinese);
}

#[test]
fn test_settings_with_utf8_bom_parses_perprovider_tray_mode() {
    let json = "\u{feff}{\n            \"enabled_providers\": [\"claude\", \"codex\"],\n            \"refresh_interval_secs\": 300,\n            \"tray_icon_mode\": \"perprovider\"\n        }";

    let settings: Settings = serde_json::from_str(json.trim_start_matches('\u{feff}')).unwrap();

    assert_eq!(settings.tray_icon_mode, TrayIconMode::PerProvider);
}

#[test]
fn stacked_tray_mode_preserves_provider_preferences() {
    let json = r#"{
        "tray_icon_mode": "stacked",
        "stacked_tray_top_provider": "claude",
        "stacked_tray_bottom_provider": "codex"
    }"#;

    let settings: Settings = serde_json::from_str(json).unwrap();

    assert_eq!(settings.tray_icon_mode, TrayIconMode::Stacked);
    assert_eq!(
        settings.stacked_tray_top_provider.as_deref(),
        Some("claude")
    );
    assert_eq!(
        settings.stacked_tray_bottom_provider.as_deref(),
        Some("codex")
    );

    let (_, reloaded) = round_trip(&settings);
    assert_eq!(
        reloaded.stacked_tray_top_provider.as_deref(),
        Some("claude")
    );
    assert_eq!(
        reloaded.stacked_tray_bottom_provider.as_deref(),
        Some("codex")
    );
}

#[test]
fn test_theme_defaults_to_auto() {
    let settings = Settings::default();
    assert_eq!(settings.theme, ThemePreference::Auto);
}

#[test]
fn test_theme_variants_serialize_to_stable_tags() {
    let table = [
        (ThemePreference::Auto, "\"auto\""),
        (ThemePreference::Light, "\"light\""),
        (ThemePreference::Dark, "\"dark\""),
    ];
    let listed: Vec<ThemePreference> = table.iter().map(|row| row.0).collect();
    assert_eq!(ThemePreference::all(), listed.as_slice());
    for (variant, quoted) in table {
        assert_eq!(serde_json::to_string(&variant).unwrap(), quoted);
        assert_eq!(
            serde_json::from_str::<ThemePreference>(quoted).unwrap(),
            variant
        );
    }
}

#[test]
fn test_settings_missing_theme_defaults_to_auto() {
    // Legacy settings JSON without the theme field should still parse.
    let legacy_json = r#"{
            "enabled_providers": ["claude", "codex"],
            "refresh_interval_secs": 300,
            "ui_language": "english"
        }"#;

    let settings: Settings = serde_json::from_str(legacy_json).unwrap();
    assert_eq!(settings.theme, ThemePreference::Auto);
}

#[test]
fn test_settings_roundtrip_with_theme() {
    let settings = Settings {
        theme: ThemePreference::Dark,
        ..Settings::default()
    };
    let (_, loaded) = round_trip(&settings);
    assert_eq!(loaded.theme, ThemePreference::Dark);
}
