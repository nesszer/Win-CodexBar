use std::collections::HashSet;

use crate::commands::ProviderCatalogEntry;

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
    fn item(id: impl Into<String>, label: impl Into<String>) -> Self {
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
    fn check_item(id: impl Into<String>, label: impl Into<String>, checked: bool) -> Self {
        Self {
            id: Some(id.into()),
            label: label.into(),
            children: Vec::new(),
            is_separator: false,
            disabled: false,
            checked: Some(checked),
        }
    }

    fn submenu(label: impl Into<String>, children: Vec<Self>) -> Self {
        Self {
            id: None,
            label: label.into(),
            children,
            is_separator: false,
            disabled: false,
            checked: None,
        }
    }

    fn separator() -> Self {
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

    fn path_segment(&self) -> Option<String> {
        if self.is_separator {
            return None;
        }

        Some(
            self.id
                .clone()
                .unwrap_or_else(|| self.label.to_ascii_lowercase().replace(' ', "_")),
        )
    }
}

pub(crate) fn build_tray_menu(
    providers: &[ProviderCatalogEntry],
    status_labels: &[(String, String)],
    enabled_providers: &HashSet<String>,
) -> Vec<TrayMenuEntry> {
    build_tray_menu_with(providers, status_labels, enabled_providers, false)
}

pub(crate) fn build_tray_menu_with(
    providers: &[ProviderCatalogEntry],
    status_labels: &[(String, String)],
    enabled_providers: &HashSet<String>,
    float_bar_enabled: bool,
) -> Vec<TrayMenuEntry> {
    let mut menu: Vec<TrayMenuEntry> = Vec::new();

    // Status rows (one per enabled provider with live usage).
    for (id, label) in status_labels {
        menu.push(TrayMenuEntry::status_row(format!("status_{id}"), label));
    }
    if !status_labels.is_empty() {
        menu.push(TrayMenuEntry::separator());
    }

    menu.push(TrayMenuEntry::item("refresh", "Refresh All"));
    menu.push(TrayMenuEntry::item("pop_out", "Pop Out Dashboard"));
    menu.push(TrayMenuEntry::item("show_panel", "Show Panel"));
    menu.push(TrayMenuEntry::check_item(
        "toggle_float_bar",
        "Show Float Bar",
        float_bar_enabled,
    ));
    menu.push(TrayMenuEntry::separator());

    if !providers.is_empty() {
        menu.push(TrayMenuEntry::submenu(
            "Providers",
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

    menu.push(TrayMenuEntry::item("settings", "Settings"));
    menu.push(TrayMenuEntry::item(
        "check_for_updates",
        "Check for Updates",
    ));
    menu.push(TrayMenuEntry::item("about", "About"));
    menu.push(TrayMenuEntry::separator());
    menu.push(TrayMenuEntry::item("quit", "Quit CodexBar"));

    menu
}

pub(crate) fn proof_menu_items(entries: &[TrayMenuEntry], menu_path: &str) -> Option<Vec<String>> {
    proof_menu_entries(entries, menu_path).map(|visible_entries| {
        visible_entries
            .iter()
            .filter(|entry| !entry.is_separator)
            .map(|entry| entry.label.clone())
            .collect()
    })
}

pub(crate) fn proof_menu_context_for_item(
    entries: &[TrayMenuEntry],
    item_id: &str,
) -> Option<(String, Vec<String>)> {
    proof_menu_context_for_item_inner(entries, item_id, "tray")
}

fn proof_menu_context_for_item_inner(
    entries: &[TrayMenuEntry],
    item_id: &str,
    menu_path: &str,
) -> Option<(String, Vec<String>)> {
    for entry in entries {
        if entry.is_separator {
            continue;
        }

        if entry.id.as_deref() == Some(item_id) {
            return proof_menu_items(entries, menu_path)
                .map(|items| (menu_path.to_string(), items));
        }

        if entry.children.is_empty() {
            continue;
        }

        let next_path = format!("{menu_path}/{}", entry.path_segment()?);
        if let Some(context) =
            proof_menu_context_for_item_inner(&entry.children, item_id, &next_path)
        {
            return Some(context);
        }
    }

    None
}

fn proof_menu_entries<'a>(
    entries: &'a [TrayMenuEntry],
    menu_path: &str,
) -> Option<&'a [TrayMenuEntry]> {
    let mut segments = menu_path.split('/');
    if segments.next()? != "tray" {
        return None;
    }

    let mut current = entries;
    for segment in segments {
        let submenu = current.iter().find(|entry| {
            !entry.is_separator
                && !entry.children.is_empty()
                && entry.path_segment().as_deref() == Some(segment)
        })?;
        current = &submenu.children;
    }

    Some(current)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn menu_contains(menu: &[TrayMenuEntry], id: &str) -> bool {
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
    fn proof_menu_items_follow_current_context() {
        let items = proof_menu_items(
            &build_tray_menu(&sample_provider_catalog(), &[], &both_enabled()),
            "tray",
        )
        .unwrap();

        assert_eq!(
            items,
            vec![
                "Refresh All",
                "Pop Out Dashboard",
                "Show Panel",
                "Show Float Bar",
                "Providers",
                "Settings",
                "Check for Updates",
                "About",
                "Quit CodexBar",
            ]
        );
    }

    #[test]
    fn proof_menu_items_follow_submenu_context() {
        let items = proof_menu_items(
            &build_tray_menu(&sample_provider_catalog(), &[], &both_enabled()),
            "tray/providers",
        )
        .unwrap();

        assert_eq!(items, vec!["Codex", "Claude"]);
    }

    #[test]
    fn proof_menu_context_for_leaf_item_returns_parent_menu() {
        let (menu_path, items) = proof_menu_context_for_item(
            &build_tray_menu(&sample_provider_catalog(), &[], &both_enabled()),
            "about",
        )
        .unwrap();

        assert_eq!(menu_path, "tray");
        assert!(items.iter().any(|item| item == "About"));
    }

    #[test]
    fn check_for_updates_item_is_present() {
        let menu = build_tray_menu(&sample_provider_catalog(), &[], &both_enabled());
        assert!(menu_contains(&menu, "check_for_updates"));
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
            .find(|e| e.label == "Providers")
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
            /* float_bar_enabled = */ true,
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
            /* float_bar_enabled = */ false,
        );
        let toggle = menu_off
            .iter()
            .find(|e| e.id.as_deref() == Some("toggle_float_bar"))
            .expect("float bar toggle present");
        assert_eq!(toggle.checked, Some(false));
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
}
