use std::collections::HashSet;

use crate::commands::ProviderCatalogEntry;
use codexbar::locale::{self, LocaleKey};
use codexbar::settings::Language;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TrayMenuEntry {
    pub(crate) id: Option<String>,
    pub(crate) label: String,
    pub(crate) children: Vec<Self>,
    pub(crate) is_separator: bool,
    pub(crate) disabled: bool,
    /// When `Some`, this entry renders as a check/checkbox item.
    /// `true` = checked (enabled), `false` = unchecked (disabled).
    pub(crate) checked: Option<bool>,
}

impl TrayMenuEntry {
    pub(crate) fn item(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: Some(id.into()),
            label: label.into(),
            children: Vec::new(),
            is_separator: false,
            disabled: false,
            checked: None,
        }
    }

    /// A checkbox menu item. `checked` mirrors the provider's enabled state.
    pub(crate) fn check_item(
        id: impl Into<String>,
        label: impl Into<String>,
        checked: bool,
    ) -> Self {
        Self {
            id: Some(id.into()),
            label: label.into(),
            children: Vec::new(),
            is_separator: false,
            disabled: false,
            checked: Some(checked),
        }
    }

    pub(crate) fn submenu(
        id: impl Into<String>,
        label: impl Into<String>,
        children: Vec<Self>,
    ) -> Self {
        Self {
            id: Some(id.into()),
            label: label.into(),
            children,
            is_separator: false,
            disabled: false,
            checked: None,
        }
    }

    pub(crate) fn separator() -> Self {
        Self {
            id: None,
            label: String::new(),
            children: Vec::new(),
            is_separator: true,
            disabled: false,
            checked: None,
        }
    }

    pub(crate) fn status_row(id: impl Into<String>, label: impl Into<String>) -> Self {
        Self {
            id: Some(id.into()),
            label: label.into(),
            children: Vec::new(),
            is_separator: false,
            disabled: true,
            checked: None,
        }
    }
}

/// Live app state the menu mirrors beyond the provider catalog.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct MenuState {
    pub(crate) float_bar_enabled: bool,
    /// Stay Awake currently holds an idle-sleep prevention.
    pub(crate) stay_awake_held: bool,
}

#[cfg(test)]
pub(crate) fn build_tray_menu(
    providers: &[ProviderCatalogEntry],
    status_labels: &[(String, String)],
    enabled_providers: &HashSet<String>,
) -> Vec<TrayMenuEntry> {
    build_tray_menu_with(
        providers,
        status_labels,
        enabled_providers,
        MenuState::default(),
        Language::English,
    )
}

pub(crate) fn build_tray_menu_with(
    providers: &[ProviderCatalogEntry],
    status_labels: &[(String, String)],
    enabled_providers: &HashSet<String>,
    state: MenuState,
    lang: Language,
) -> Vec<TrayMenuEntry> {
    let mut menu: Vec<TrayMenuEntry> = Vec::new();
    let text = |key| locale::get_text(lang, key);

    // Status rows (one per enabled provider with live usage).
    for (id, label) in status_labels {
        menu.push(TrayMenuEntry::status_row(format!("status_{id}"), label));
    }
    if state.stay_awake_held {
        menu.push(TrayMenuEntry::status_row(
            "stay_awake_active",
            text(LocaleKey::TrayStayAwakeActive),
        ));
    }
    if !status_labels.is_empty() || state.stay_awake_held {
        menu.push(TrayMenuEntry::separator());
    }

    menu.push(TrayMenuEntry::item(
        "refresh",
        text(LocaleKey::TrayRefreshAll),
    ));
    menu.push(TrayMenuEntry::item(
        "pop_out",
        text(LocaleKey::TrayPopOutDashboard),
    ));
    menu.push(TrayMenuEntry::check_item(
        "toggle_float_bar",
        text(LocaleKey::TrayShowFloatBar),
        state.float_bar_enabled,
    ));
    menu.push(TrayMenuEntry::separator());

    if !providers.is_empty() {
        menu.push(TrayMenuEntry::submenu(
            "providers",
            text(LocaleKey::TrayProviders),
            providers
                .iter()
                .map(|provider| {
                    let is_enabled = enabled_providers.contains(&provider.id);
                    TrayMenuEntry::check_item(
                        format!("toggle_provider:{}", provider.id),
                        &provider.display_name,
                        is_enabled,
                    )
                })
                .collect(),
        ));
        menu.push(TrayMenuEntry::separator());
    }

    menu.push(TrayMenuEntry::item(
        "settings",
        text(LocaleKey::TraySettings),
    ));
    menu.push(TrayMenuEntry::item(
        "check_for_updates",
        text(LocaleKey::TrayCheckForUpdates),
    ));
    menu.push(TrayMenuEntry::item("about", text(LocaleKey::MenuAbout)));
    menu.push(TrayMenuEntry::separator());
    menu.push(TrayMenuEntry::item("quit", text(LocaleKey::MenuQuit)));

    menu
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn menu_contains(menu: &[TrayMenuEntry], id: &str) -> bool {
        menu.iter().any(|entry| {
            entry.id.as_deref() == Some(id)
                || (!entry.children.is_empty() && menu_contains(&entry.children, id))
        })
    }

    fn sample_provider_catalog() -> Vec<ProviderCatalogEntry> {
        vec![
            ProviderCatalogEntry {
                id: "codex".into(),
                display_name: "Codex".into(),
                cookie_domain: None,
            },
            ProviderCatalogEntry {
                id: "claude".into(),
                display_name: "Claude".into(),
                cookie_domain: None,
            },
        ]
    }

    fn both_enabled() -> HashSet<String> {
        ["codex".to_string(), "claude".to_string()]
            .into_iter()
            .collect()
    }

    #[test]
    fn tray_menu_includes_about_and_provider_entries() {
        let menu = build_tray_menu(&sample_provider_catalog(), &[], &both_enabled());
        assert!(menu_contains(&menu, "about"));
        assert!(menu_contains(&menu, "toggle_provider:codex"));
        assert!(menu_contains(&menu, "quit"));
    }

    #[test]
    fn check_for_updates_item_is_present() {
        let menu = build_tray_menu(&sample_provider_catalog(), &[], &both_enabled());
        assert!(menu_contains(&menu, "check_for_updates"));
    }

    #[test]
    fn tray_menu_offers_only_the_tray_panel_window_entry() {
        // The legacy "Show Window" entry opened the retired PopOut layout on
        // `main`; "Pop Out Dashboard" (the tray-panel flyout) is the only
        // window entry now.
        let menu = build_tray_menu(&sample_provider_catalog(), &[], &both_enabled());
        assert!(menu_contains(&menu, "pop_out"));
        assert!(!menu_contains(&menu, "show_panel"));
    }

    #[test]
    fn provider_check_items_reflect_enabled_state() {
        let menu = build_tray_menu(
            &sample_provider_catalog(),
            &[],
            &["claude".to_string()].into_iter().collect(),
        );
        let providers_submenu = menu
            .iter()
            .find(|e| e.id.as_deref() == Some("providers"))
            .expect("providers submenu");

        let claude_item = providers_submenu
            .children
            .iter()
            .find(|e| e.id.as_deref() == Some("toggle_provider:claude"))
            .expect("claude item");
        let codex_item = providers_submenu
            .children
            .iter()
            .find(|e| e.id.as_deref() == Some("toggle_provider:codex"))
            .expect("codex item");

        assert_eq!(claude_item.checked, Some(true), "Claude should be checked");
        assert_eq!(codex_item.checked, Some(false), "Codex should be unchecked");
    }

    #[test]
    fn float_bar_toggle_reflects_state() {
        let menu_on = build_tray_menu_with(
            &sample_provider_catalog(),
            &[],
            &both_enabled(),
            MenuState {
                float_bar_enabled: true,
                ..MenuState::default()
            },
            Language::English,
        );
        let toggle = menu_on
            .iter()
            .find(|e| e.id.as_deref() == Some("toggle_float_bar"))
            .expect("float bar toggle present");
        assert_eq!(toggle.checked, Some(true));
        assert_eq!(toggle.label, "Show Float Bar");

        let menu_off = build_tray_menu_with(
            &sample_provider_catalog(),
            &[],
            &both_enabled(),
            MenuState::default(),
            Language::English,
        );
        let toggle = menu_off
            .iter()
            .find(|e| e.id.as_deref() == Some("toggle_float_bar"))
            .expect("float bar toggle present");
        assert_eq!(toggle.checked, Some(false));
    }

    #[test]
    fn tray_menu_static_labels_follow_language_but_provider_names_stay_raw() {
        let menu = build_tray_menu_with(
            &sample_provider_catalog(),
            &[],
            &both_enabled(),
            MenuState::default(),
            Language::Japanese,
        );
        fn label_for<'a>(menu: &'a [TrayMenuEntry], id: &'a str) -> &'a str {
            menu.iter()
                .find(|e| e.id.as_deref() == Some(id))
                .map(|e| e.label.as_str())
                .expect(id)
        }

        assert_eq!(label_for(&menu, "refresh"), "すべて更新");
        assert_eq!(label_for(&menu, "pop_out"), "ダッシュボードを開く");
        assert_eq!(label_for(&menu, "settings"), "設定...");
        assert_eq!(label_for(&menu, "quit"), "終了");

        let providers = menu
            .iter()
            .find(|e| e.id.as_deref() == Some("providers"))
            .expect("providers submenu");
        let provider_labels: Vec<&str> = providers
            .children
            .iter()
            .map(|e| e.label.as_str())
            .collect();
        assert_eq!(provider_labels, vec!["Codex", "Claude"]);
    }

    #[test]
    fn status_rows_appear_at_top_with_separator() {
        let labels = vec![
            ("claude".to_string(), "Claude 60%".to_string()),
            ("codex".to_string(), "Codex 30%".to_string()),
        ];
        let menu = build_tray_menu(&sample_provider_catalog(), &labels, &both_enabled());
        // First two items should be disabled status rows.
        assert_eq!(menu[0].id.as_deref(), Some("status_claude"));
        assert!(menu[0].disabled);
        assert_eq!(menu[1].id.as_deref(), Some("status_codex"));
        assert!(menu[1].disabled);
        // Third item should be a separator.
        assert!(menu[2].is_separator);
    }

    #[test]
    fn stay_awake_row_appears_only_while_held() {
        let build = |stay_awake_held| {
            build_tray_menu_with(
                &sample_provider_catalog(),
                &[],
                &both_enabled(),
                MenuState {
                    stay_awake_held,
                    ..MenuState::default()
                },
                Language::English,
            )
        };
        assert!(!menu_contains(&build(false), "stay_awake_active"));

        let held = build(true);
        assert_eq!(held[0].id.as_deref(), Some("stay_awake_active"));
        assert_eq!(held[0].label, "Stay Awake: local agent session is live");
        assert!(held[0].disabled);
        assert!(held[1].is_separator);
    }
}
