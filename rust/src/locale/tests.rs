use super::*;
use std::collections::HashSet;

#[test]
fn test_locale_key_english() {
    assert_eq!(
        get_text(Language::English, LocaleKey::TabGeneral),
        "General"
    );
    assert_eq!(
        get_text(Language::English, LocaleKey::InterfaceLanguage),
        "Interface Language"
    );
    assert_eq!(
        get_text(Language::English, LocaleKey::StartAtLogin),
        "Start at Login"
    );
}

#[test]
fn test_locale_key_chinese() {
    assert_eq!(get_text(Language::Chinese, LocaleKey::TabGeneral), "通用");
    assert_eq!(get_text(Language::Chinese, LocaleKey::TabCookies), "Cookie");
    assert_eq!(
        get_text(Language::Chinese, LocaleKey::InterfaceLanguage),
        "界面语言"
    );
    assert_eq!(
        get_text(Language::Chinese, LocaleKey::StartAtLogin),
        "开机启动"
    );
}

#[test]
fn test_locale_key_japanese() {
    assert_eq!(get_text(Language::Japanese, LocaleKey::TabGeneral), "一般");
    assert_eq!(
        get_text(Language::Japanese, LocaleKey::InterfaceLanguage),
        "表示言語"
    );
    assert_eq!(
        get_text(Language::Japanese, LocaleKey::StartAtLogin),
        "ログイン時に起動"
    );
}

#[test]
fn test_locale_key_spanish() {
    assert_eq!(
        get_text(Language::Spanish, LocaleKey::TabGeneral),
        "General"
    );
    assert_eq!(
        get_text(Language::Spanish, LocaleKey::InterfaceLanguage),
        "Idioma de la interfaz"
    );
    assert_eq!(
        get_text(Language::Spanish, LocaleKey::StartAtLogin),
        "Iniciar al arrancar"
    );
}

#[test]
fn test_locale_key_korean() {
    assert_eq!(get_text(Language::Korean, LocaleKey::TabGeneral), "일반");
    assert_eq!(
        get_text(Language::Korean, LocaleKey::InterfaceLanguage),
        "인터페이스 언어"
    );
    assert_eq!(
        get_text(Language::Korean, LocaleKey::StartAtLogin),
        "로그인 시 자동 실행"
    );
}

#[test]
fn test_locale_respects_language_setting() {
    // Test that English language returns English strings
    let lang = Language::English;
    assert_eq!(get_text(lang, LocaleKey::TabAbout), "About");

    // Test that Chinese language returns Chinese strings
    let lang = Language::Chinese;
    assert_eq!(get_text(lang, LocaleKey::TabAbout), "关于");

    // Test that Japanese language returns Japanese strings
    let lang = Language::Japanese;
    assert_eq!(get_text(lang, LocaleKey::TabAbout), "情報");

    // Test that Korean language returns Korean strings
    let lang = Language::Korean;
    assert_eq!(get_text(lang, LocaleKey::TabAbout), "정보");

    // Test that Spanish language returns Spanish strings
    let lang = Language::Spanish;
    assert_eq!(get_text(lang, LocaleKey::TabAbout), "Acerca de");
}

#[test]
fn test_all_locale_keys_have_all_languages() {
    let resources = [
        ("en-US", include_str!("en-US.ftl")),
        ("zh-CN", include_str!("zh-CN.ftl")),
        ("ja-JP", include_str!("ja-JP.ftl")),
        ("ko-KR", include_str!("ko-KR.ftl")),
        ("es-MX", include_str!("es-MX.ftl")),
    ];

    let resource_keys: Vec<(&str, HashSet<&str>)> = resources
        .into_iter()
        .map(|(locale, resource)| (locale, resource_key_names(resource)))
        .collect();

    for (key, name) in LocaleKey::ALL {
        assert_eq!(key.name(), *name);
        for (locale, keys) in &resource_keys {
            assert!(keys.contains(name), "missing Fluent key {name} in {locale}");
        }
        for lang in Language::all() {
            let text = LOCALES
                .try_lookup(language_id(*lang), name)
                .unwrap_or_default();
            assert!(
                !text.trim().is_empty(),
                "missing or empty Fluent text for {:?} in {:?}",
                key,
                lang
            );
        }
    }
}

#[test]
fn test_fluent_preserves_literal_placeholders_and_status_spacing() {
    assert_eq!(
        get_text(Language::English, LocaleKey::TrayStatusError),
        " (Error)"
    );
    assert_eq!(
        get_text(Language::English, LocaleKey::TrayCreditsRemaining),
        "Credits remaining {}%"
    );
    assert_eq!(
        get_text(Language::English, LocaleKey::UsedPercent),
        "{:.0}% used"
    );
    assert_eq!(
        get_text(Language::English, LocaleKey::RemainingAmount),
        "{:.2} remaining"
    );
}

fn resource_key_names(resource: &str) -> HashSet<&str> {
    resource
        .lines()
        .filter_map(|line| line.split_once('=').map(|(name, _)| name.trim()))
        .filter(|name| !name.is_empty())
        .collect()
}
