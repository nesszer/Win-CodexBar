use super::*;

#[test]
fn cookie_options_for_cookie_supporting_provider() {
    let opts = crate::commands::cookie_source_options_for("codex", Language::English);
    let values: Vec<_> = opts.iter().map(|o| o.value.as_str()).collect();
    assert_eq!(values, vec!["auto", "manual", "off"]);
    assert!(opts.iter().any(|o| o.label == "Automatic"));
    assert!(opts.iter().any(|o| o.label == "Manual"));
    assert!(opts.iter().any(|o| o.label == "Disabled"));
}

#[test]
fn replicate_cookie_options_allow_automatic_and_manual_sessions() {
    let opts = crate::commands::cookie_source_options_for("replicate", Language::English);
    let values: Vec<_> = opts.iter().map(|option| option.value.as_str()).collect();
    assert_eq!(values, vec!["auto", "manual"]);
}

#[test]
fn raycast_cookie_options_include_off_and_a_pinned_manual_session() {
    let opts = crate::commands::cookie_source_options_for("raycast", Language::English);
    let values: Vec<_> = opts.iter().map(|option| option.value.as_str()).collect();
    assert_eq!(values, vec!["auto", "manual", "off"]);
}

#[test]
fn cookie_options_empty_for_providers_without_picker() {
    assert!(crate::commands::cookie_source_options_for("anthropic", Language::English).is_empty());
    assert!(crate::commands::cookie_source_options_for("unknown", Language::English).is_empty());
}

#[test]
fn region_options_for_regional_provider() {
    let opts = crate::commands::region_options_for("alibaba", Language::English);
    let values: Vec<_> = opts.iter().map(|o| o.value.as_str()).collect();
    assert_eq!(values, vec!["singapore", "us", "germany", "hongkong", "cn"]);
}

#[test]
fn alibaba_token_plan_region_options() {
    let opts = crate::commands::region_options_for("alibabatokenplan", Language::English);
    let values: Vec<_> = opts.iter().map(|o| o.value.as_str()).collect();
    let labels: Vec<_> = opts.iter().map(|o| o.label.as_str()).collect();
    assert_eq!(values, vec!["cn", "intl", "cn-personal", "intl-personal"]);
    assert_eq!(
        labels,
        vec![
            "China Team",
            "International Team",
            "China Personal/Solo",
            "International Personal/Solo"
        ]
    );
}

#[test]
fn minimax_region_options_match_upstream_hosts() {
    let opts = crate::commands::region_options_for("minimax", Language::English);
    let values: Vec<_> = opts.iter().map(|o| o.value.as_str()).collect();
    let labels: Vec<_> = opts.iter().map(|o| o.label.as_str()).collect();
    assert_eq!(values, vec!["global", "cn"]);
    assert_eq!(
        labels,
        vec![
            "Global (platform.minimax.io)",
            "China mainland (platform.minimaxi.com)"
        ]
    );
}

#[test]
fn kimi_region_options_match_regional_hosts() {
    let opts = crate::commands::region_options_for("kimi", Language::English);
    let values: Vec<_> = opts.iter().map(|option| option.value.as_str()).collect();
    assert_eq!(values, vec!["china", "international"]);
}

#[test]
fn cookie_and_region_options_are_localized_for_every_provider() {
    // Every description and region label must come from the locale catalog: none may stay
    // English in another language, except the labels in ALLOWED_SAME_AS_ENGLISH, which the
    // catalogs legitimately keep identical to the English text (proper names).
    const ALLOWED_SAME_AS_ENGLISH: &[(Language, &str, &str)] = &[
        (Language::Spanish, "kimi", "china"),
        (Language::Spanish, "minimax", "global"),
        (Language::Spanish, "zai", "global"),
        (Language::PortugueseBrazil, "kimi", "china"),
        (Language::PortugueseBrazil, "minimax", "global"),
        (Language::PortugueseBrazil, "zai", "global"),
    ];
    for &lang in Language::all() {
        if lang == Language::English {
            continue;
        }
        for provider in codexbar::core::ProviderId::all() {
            let id = provider.cli_name();
            let en = crate::commands::cookie_source_options_for(id, Language::English);
            let other = crate::commands::cookie_source_options_for(id, lang);
            assert_eq!(en.len(), other.len(), "{lang:?} {id}");
            for (en, other) in en.iter().zip(&other) {
                if let Some(text) = &en.description {
                    assert_ne!(
                        Some(text),
                        other.description.as_ref(),
                        "{lang:?} {id} {} description",
                        en.value
                    );
                }
            }
            let en = crate::commands::region_options_for(id, Language::English);
            let other = crate::commands::region_options_for(id, lang);
            assert_eq!(en.len(), other.len(), "{lang:?} {id} regions");
            for (en, other) in en.iter().zip(&other) {
                if ALLOWED_SAME_AS_ENGLISH.contains(&(lang, id, en.value.as_str())) {
                    continue;
                }
                assert_ne!(en.label, other.label, "{lang:?} {id} region {}", en.value);
            }
        }
    }
}

#[test]
fn english_region_labels_match_the_provider_display_names() {
    use codexbar::providers::{AlibabaRegion, AlibabaTokenPlanRegion, KimiRegion, MiniMaxRegion};
    let labels = |id| {
        crate::commands::region_options_for(id, Language::English)
            .into_iter()
            .map(|option| option.label)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        labels("alibaba"),
        AlibabaRegion::ALL
            .iter()
            .map(|r| r.display_name())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        labels("alibabatokenplan"),
        AlibabaTokenPlanRegion::ALL
            .iter()
            .map(|r| r.display_name())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        labels("kimi"),
        KimiRegion::ALL
            .iter()
            .map(|r| r.display_name())
            .collect::<Vec<_>>()
    );
    assert_eq!(
        labels("minimax"),
        [MiniMaxRegion::Global, MiniMaxRegion::ChinaMainland].map(|r| r.display_name())
    );
}

#[test]
fn region_options_empty_for_non_regional_provider() {
    assert!(crate::commands::region_options_for("claude", Language::English).is_empty());
    assert!(crate::commands::region_options_for("codex", Language::English).is_empty());
}

#[test]
fn cookie_source_option_roundtrips_serde() {
    let opt = crate::commands::CookieSourceOption {
        value: "auto".to_string(),
        label: "Automatic".to_string(),
        description: Some("Imports browser cookies.".to_string()),
    };
    let json = serde_json::to_string(&opt).unwrap();
    let back: crate::commands::CookieSourceOption = serde_json::from_str(&json).unwrap();
    assert_eq!(opt, back);
}

#[test]
fn region_option_roundtrips_serde() {
    let opt = crate::commands::RegionOption {
        value: "intl".to_string(),
        label: "International".to_string(),
    };
    let json = serde_json::to_string(&opt).unwrap();
    let back: crate::commands::RegionOption = serde_json::from_str(&json).unwrap();
    assert_eq!(opt, back);
}
