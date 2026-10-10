use super::*;

/// Build a compact tray status label from a raw snapshot using the current language.
/// Localization is done at render time so cached snapshots stay language-neutral.
pub(crate) fn compact_tray_status_label(
    window: &RateWindowSnapshot,
    lang: codexbar::settings::Language,
) -> String {
    if window.is_informational {
        return window
            .reset_description
            .clone()
            .unwrap_or_else(|| locale::get_text(lang, locale::LocaleKey::ProviderTextUnavailable));
    }

    let pct = format!("{:.0}%", window.used_percent);
    if let Some(reset) = compact_reset_description(window, lang) {
        format!("{pct} • {reset}")
    } else {
        pct
    }
}

fn compact_reset_description(
    window: &RateWindowSnapshot,
    lang: codexbar::settings::Language,
) -> Option<String> {
    if let Some(ref resets_at) = window.resets_at {
        let dt = super::parse_utc(resets_at)?;
        return Some(format_compact_reset_countdown(dt, lang));
    }

    if window.description_is_detail {
        return None;
    }

    window
        .reset_description
        .as_deref()
        .map(|desc| normalize_reset_description(desc, lang))
        .filter(|desc| !desc.is_empty())
}

fn format_compact_reset_countdown(
    resets_at: chrono::DateTime<chrono::Utc>,
    lang: codexbar::settings::Language,
) -> String {
    let now = chrono::Utc::now();
    if resets_at <= now {
        return locale::get_text(lang, locale::LocaleKey::ResetInProgress);
    }

    let total_minutes = (resets_at - now).num_minutes().max(0);
    let days = total_minutes / 1440;
    let hours = (total_minutes % 1440) / 60;
    let minutes = total_minutes % 60;

    if days > 0 {
        locale::format_locale(
            lang,
            locale::LocaleKey::ResetsInDaysHours,
            &[&days.to_string(), &hours.to_string()],
        )
    } else {
        locale::format_locale(
            lang,
            locale::LocaleKey::ResetsInHoursMinutes,
            &[&hours.to_string(), &format!("{minutes:02}")],
        )
    }
}

fn normalize_reset_description(desc: &str, lang: codexbar::settings::Language) -> String {
    let trimmed = desc.trim();
    let lower = trimmed.to_ascii_lowercase();
    let prefix_len = ["resets in ", "reset in ", "in "]
        .iter()
        .find(|&&p| lower.starts_with(p))
        .map(|p| p.len())
        .unwrap_or(0);
    let body = trimmed[prefix_len..].trim_start();
    if let Some(countdown) = localized_countdown(body, lang) {
        return countdown;
    }
    if prefix_len == 0 {
        // "Resets Apr 3, 2pm" / "Resets at 23:30": a clock time, not a countdown.
        for prefix in ["resets ", "reset "] {
            if lower.starts_with(prefix) {
                let rest = trimmed[prefix.len()..].trim_start();
                return match rest.get(..3) {
                    Some(at) if at.eq_ignore_ascii_case("at ") => locale::format_locale(
                        lang,
                        locale::LocaleKey::ResetsAtTime,
                        &[rest[3..].trim_start()],
                    ),
                    _ => locale::format_locale(lang, locale::LocaleKey::ResetsAtLabel, &[rest]),
                };
            }
        }
    }
    format!(
        "{} {body}",
        locale::get_text(lang, locale::LocaleKey::ResetsInShort)
    )
}

/// Localize an English countdown body ("2h 10m", "12 hours", "30 seconds").
fn localized_countdown(body: &str, lang: codexbar::settings::Language) -> Option<String> {
    let (mut days, mut hours, mut minutes, mut seconds) = (0u64, 0u64, 0u64, 0u64);
    let mut tokens = body.split_whitespace().peekable();
    let mut any = false;
    let mut pad_minutes = false;
    while let Some(token) = tokens.next() {
        let lower = token.to_ascii_lowercase();
        let digits = lower.trim_end_matches(|c: char| c.is_ascii_alphabetic());
        let value: u64 = digits.parse().ok()?;
        let unit = if digits.len() < lower.len() {
            lower[digits.len()..].to_string()
        } else {
            tokens.next()?.to_ascii_lowercase()
        };
        match unit.as_str() {
            "d" | "day" | "days" => days += value,
            "h" | "hour" | "hours" => hours += value,
            "m" | "min" | "minute" | "minutes" => {
                pad_minutes = digits.len() == 2 && digits.starts_with('0');
                minutes += value
            }
            "s" | "second" | "seconds" => seconds += value,
            _ => return None,
        }
        any = true;
    }
    if !any {
        return None;
    }
    if seconds > 0 {
        minutes += seconds.div_ceil(60);
    }
    let fmt = |key, args: &[&str]| locale::format_locale(lang, key, args);
    let m = if pad_minutes && hours > 0 {
        format!("{minutes:02}")
    } else {
        minutes.to_string()
    };
    let (d, h) = (days.to_string(), hours.to_string());
    Some(match (days, hours, minutes) {
        (0, 0, _) => fmt(locale::LocaleKey::ResetsInMinutes, &[&m]),
        (0, _, 0) => fmt(locale::LocaleKey::ResetsInHoursOnly, &[&h]),
        (0, _, _) => fmt(locale::LocaleKey::ResetsInHoursMinutes, &[&h, &m]),
        (_, 0, _) => fmt(locale::LocaleKey::ResetsInDaysOnly, &[&d]),
        _ => fmt(locale::LocaleKey::ResetsInDaysHours, &[&d, &h]),
    })
}

pub(crate) fn friendly_provider_error(id: ProviderId, error: &str) -> String {
    if id != ProviderId::Claude {
        return error.to_string();
    }

    let trimmed = error.trim();
    let lower = trimmed.to_lowercase();

    if lower.contains("swift.cancellationerror")
        || lower.contains("the operation couldn't be completed")
        || lower.contains("the operation could not be completed")
    {
        return "Claude usage fetch was cancelled before usage data was returned. Refresh Claude, or re-authenticate with Claude Code and try again.".to_string();
    }

    if lower.contains("claude oauth credentials not found") {
        return "Claude sign-in was not found. Run `claude` once to authenticate, then refresh Claude in Win-CodexBar.".to_string();
    }

    if lower.contains("oauth token expired") || lower.contains("token invalid or expired") {
        return "Claude sign-in expired. Run `claude` to refresh your Claude Code login, then refresh Claude in Win-CodexBar.".to_string();
    }

    if trimmed == "Authentication required" {
        return "Claude needs sign-in before Win-CodexBar can read usage. Run `claude` once, or add Claude cookies in Provider settings.".to_string();
    }

    if lower.starts_with("claude usage failed from all configured sources.") {
        return trimmed
            .replace(
                "OAuth: OAuth error: Claude OAuth credentials not found. Run `claude` to authenticate.",
                "OAuth: sign-in not found",
            )
            .replace(
                "Web: No cookies available for web API",
                "Web: no Claude cookies available",
            )
            .replace(
                "CLI: Provider not installed:",
                "CLI: not installed:",
            );
    }

    trimmed.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use codexbar::core::RateWindow;
    use codexbar::settings::Language;

    #[test]
    fn detail_backed_description_is_never_a_tray_reset_label() {
        let window = RateWindowSnapshot::from_rate_window(
            &RateWindow::with_details(
                13.0,
                None,
                None,
                Some("34.07 EUR / 255.00 EUR · 220.93 EUR remaining".to_string()),
            )
            .with_description_as_detail(),
        );

        assert!(window.description_is_detail);
        assert_eq!(compact_tray_status_label(&window, Language::English), "13%");
    }

    #[test]
    fn detail_backed_window_keeps_its_countdown_when_reset_is_known() {
        let window = RateWindowSnapshot::from_rate_window(
            &RateWindow::with_details(
                13.0,
                None,
                Some(chrono::Utc::now() + chrono::Duration::minutes(125)),
                Some("34.07 EUR / 255.00 EUR · 220.93 EUR remaining".to_string()),
            )
            .with_description_as_detail(),
        );

        let label = compact_tray_status_label(&window, Language::English);

        assert!(label.starts_with("13% • "), "{label}");
        assert!(!label.contains("EUR"), "{label}");
    }

    #[test]
    fn english_countdowns_follow_the_ui_language() {
        let ru = Language::Russian;
        assert_eq!(
            normalize_reset_description("Resets in 12 hours", ru),
            "Сброс через 12 ч"
        );
        assert_eq!(
            normalize_reset_description("Resets in 2h 10m", ru),
            "Сброс через 2 ч 10 мин"
        );
        assert_eq!(
            normalize_reset_description("Resets in 30 seconds", ru),
            "Сброс через 1 мин"
        );
        assert_eq!(
            normalize_reset_description("in 5 days", Language::English),
            "Resets in 5d"
        );
        assert_eq!(
            normalize_reset_description("Resets Apr 3, 2pm", Language::English),
            "Resets Apr 3, 2pm"
        );
        assert_eq!(
            normalize_reset_description("Resets at 23:30 (UTC)", ru),
            "Сброс в 23:30 (UTC)"
        );
    }
}
