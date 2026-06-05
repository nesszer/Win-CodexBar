use super::*;

// ── Locale / i18n commands ───────────────────────────────────────────

/// Snapshot of every localized UI string in a given language.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocaleStrings {
    /// Serialized language code (`"english"` or `"chinese"`).
    pub language: &'static str,
    /// Map of serialized `LocaleKey` variant name → localized text.
    pub entries: HashMap<&'static str, &'static str>,
}

fn locale_strings_for(lang: Language) -> LocaleStrings {
    let mut entries = HashMap::with_capacity(locale::LocaleKey::ALL.len());
    for (key, name) in locale::LocaleKey::ALL {
        entries.insert(*name, locale::get_text(lang, *key));
    }
    LocaleStrings {
        language: language_label(lang),
        entries,
    }
}

/// Return every UI string for the requested language.
///
/// When `language` is `None`, the user's current persisted language is used.
/// The `language` argument accepts either the short code (`"en"`, `"zh"`),
/// the persisted label (`"english"`, `"chinese"`), or the full name
/// (`"English"`, `"Chinese"`, `"中文"`).
#[tauri::command]
pub fn get_locale_strings(language: Option<String>) -> Result<LocaleStrings, String> {
    let lang = match language.as_deref() {
        None => locale::current_language(),
        Some(raw) => {
            parse_locale_language(raw).ok_or_else(|| format!("unknown language code: {raw}"))?
        }
    };
    Ok(locale_strings_for(lang))
}

fn parse_locale_language(raw: &str) -> Option<Language> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "en" | "en-us" | "english" => Some(Language::English),
        "zh" | "zh-cn" | "zh-hans" | "chinese" | "中文" => Some(Language::Chinese),
        _ => None,
    }
}

/// Persist the UI language and emit a `locale-changed` event so the
/// frontend can refetch its locale table without a restart.
#[tauri::command]
pub fn set_ui_language(app: tauri::AppHandle, language: String) -> Result<(), String> {
    let lang =
        parse_locale_language(&language).ok_or_else(|| format!("unknown language: {language}"))?;
    let mut settings = Settings::load();
    if settings.ui_language == lang {
        return Ok(());
    }
    settings.ui_language = lang;
    settings.save().map_err(|e| e.to_string())?;
    let _ = app.emit(events::LOCALE_CHANGED, language_label(lang));
    Ok(())
}

#[cfg(test)]
mod locale_tests {
    use super::*;

    #[test]
    fn locale_strings_roundtrip_english() {
        let bundle = locale_strings_for(Language::English);
        assert_eq!(bundle.language, "english");
        assert_eq!(
            bundle.entries.get("TabGeneral").copied(),
            Some("General"),
            "TabGeneral should resolve to English"
        );
        assert_eq!(
            bundle.entries.get("ProviderSidebarSearch").copied(),
            Some("Search"),
            "ProviderSidebarSearch should resolve instead of leaking the key"
        );
        assert_eq!(bundle.entries.len(), locale::LocaleKey::ALL.len());
    }

    #[test]
    fn locale_strings_roundtrip_chinese() {
        let bundle = locale_strings_for(Language::Chinese);
        assert_eq!(bundle.language, "chinese");
        assert_eq!(bundle.entries.get("TabGeneral").copied(), Some("通用"));
        assert_eq!(bundle.entries.len(), locale::LocaleKey::ALL.len());
    }

    #[test]
    fn locale_strings_contains_every_variant() {
        let bundle = locale_strings_for(Language::English);
        for (_, name) in locale::LocaleKey::ALL {
            assert!(
                bundle.entries.contains_key(name),
                "missing key in locale bundle: {name}"
            );
        }
    }

    #[test]
    fn parse_locale_language_accepts_aliases() {
        assert!(matches!(
            parse_locale_language("en"),
            Some(Language::English)
        ));
        assert!(matches!(
            parse_locale_language("English"),
            Some(Language::English)
        ));
        assert!(matches!(
            parse_locale_language("zh"),
            Some(Language::Chinese)
        ));
        assert!(matches!(
            parse_locale_language("Chinese"),
            Some(Language::Chinese)
        ));
        assert!(matches!(
            parse_locale_language("中文"),
            Some(Language::Chinese)
        ));
        assert!(parse_locale_language("klingon").is_none());
    }
}
