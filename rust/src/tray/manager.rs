//! System tray manager with dynamic usage bar icon
//!
//! Creates a system tray icon that shows session and weekly usage as two horizontal bars

#![allow(dead_code)]

use image::{ImageBuffer, Rgba, RgbaImage};
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};
use tray_icon::{
    Icon, TrayIcon, TrayIconBuilder,
    menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu},
};

use super::icon::{LoadingPattern, UsageLevel};
use crate::core::ProviderId;
use crate::settings::{Settings, TrayIconMode, UiLanguage};
use crate::status::IndicatorStatusLevel;

const ICON_SIZE: u32 = 32;

/// Surprise animation types (matching macOS CodexBar)
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SurpriseAnimation {
    /// No animation
    #[allow(dead_code)]
    None,
    /// Bars flash bright white briefly (like eyes blinking)
    Blink,
    /// Bars wiggle left/right (Claude arms/legs effect)
    Wiggle,
    /// Bars pulse in intensity
    Pulse,
    /// Rainbow color sweep
    Rainbow,
    /// Icon tilts slightly (Codex hat tilt effect)
    Tilt,
}

impl SurpriseAnimation {
    /// Get a random animation type
    pub fn random() -> Self {
        use rand::Rng;
        let mut rng = rand::rng();
        match rng.random_range(0..5) {
            0 => SurpriseAnimation::Blink,
            1 => SurpriseAnimation::Wiggle,
            2 => SurpriseAnimation::Pulse,
            3 => SurpriseAnimation::Rainbow,
            _ => SurpriseAnimation::Tilt,
        }
    }

    /// Duration of the animation in frames (at ~60fps)
    pub fn duration_frames(&self) -> u32 {
        match self {
            SurpriseAnimation::None => 0,
            SurpriseAnimation::Blink => 8,    // Quick flash
            SurpriseAnimation::Wiggle => 20,  // Shake back and forth
            SurpriseAnimation::Pulse => 30,   // Slow pulse
            SurpriseAnimation::Rainbow => 40, // Color sweep
            SurpriseAnimation::Tilt => 24,    // Tilt and return
        }
    }
}

/// Icon overlay types for status indicators
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Hash)]
pub enum IconOverlay {
    /// No overlay - normal display
    #[default]
    None,
    /// Error state - grayed out icon with X
    Error,
    /// Stale data - dim icon with clock indicator
    #[allow(dead_code)]
    Stale,
    /// Incident - warning badge overlay
    Incident,
    /// Partial outage - orange badge
    Partial,
}

/// Provider usage data for merged icon mode
#[derive(Clone, Debug)]
pub struct ProviderUsage {
    pub name: String,
    pub session_percent: f64,
    #[allow(dead_code)]
    pub weekly_percent: f64,
}

/// System tray manager
pub struct TrayManager {
    tray_icon: TrayIcon,
    /// Provider menu items for updating with status prefixes
    provider_menu_items: HashMap<ProviderId, CheckMenuItem>,
    last_usage_signature: Cell<Option<u64>>,
    last_merged_signature: Cell<Option<u64>>,
    language: UiLanguage,
}

impl TrayManager {
    fn text(&self, en: &'static str, zh: &'static str) -> &'static str {
        match self.language {
            UiLanguage::English => en,
            UiLanguage::Chinese => zh,
        }
    }

    /// Create a new tray manager with default icon
    pub fn new() -> anyhow::Result<Self> {
        let settings = Settings::load();
        let menu = Menu::new();
        let language = settings.ui_language;

        let text = |en: &'static str, zh: &'static str| match language {
            UiLanguage::English => en,
            UiLanguage::Chinese => zh,
        };

        // Open CodexBar
        let open_item =
            MenuItem::with_id("open", text("Open CodexBar", "打开 CodexBar"), true, None);
        menu.append(&open_item)?;

        // Separator
        menu.append(&PredefinedMenuItem::separator())?;

        // Refresh All
        let refresh_item =
            MenuItem::with_id("refresh", text("Refresh All", "全部刷新"), true, None);
        menu.append(&refresh_item)?;

        // Separator
        menu.append(&PredefinedMenuItem::separator())?;

        // Providers submenu with check items
        // Build submenu items first, then add to parent menu to avoid Windows duplication bug
        let providers_submenu = Submenu::new(text("Providers", "服务商"), true);
        let mut provider_menu_items = HashMap::new();
        for provider_id in ProviderId::all() {
            let cli_name = provider_id.cli_name();
            let display_name = provider_id.display_name();
            let is_enabled = settings.is_provider_enabled(*provider_id);
            let item_id = format!("provider_{}", cli_name);
            let check_item = CheckMenuItem::with_id(&item_id, display_name, true, is_enabled, None);
            providers_submenu.append(&check_item)?;
            provider_menu_items.insert(*provider_id, check_item);
        }
        menu.append(&providers_submenu)?;

        // Separator
        menu.append(&PredefinedMenuItem::separator())?;

        // Settings
        let settings_item =
            MenuItem::with_id("settings", text("Settings...", "设置..."), true, None);
        menu.append(&settings_item)?;

        // Check for Updates
        let updates_item =
            MenuItem::with_id("updates", text("Check for Updates", "检查更新"), true, None);
        menu.append(&updates_item)?;

        // Separator
        menu.append(&PredefinedMenuItem::separator())?;

        // Quit
        let quit_item = MenuItem::with_id("quit", text("Quit", "退出"), true, None);
        menu.append(&quit_item)?;

        let icon = create_bar_icon(0.0, 0.0, IconOverlay::None);

        let tray_icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip(text("CodexBar - Loading...", "CodexBar - 加载中..."))
            .with_icon(icon)
            .build()?;

        Ok(Self {
            tray_icon,
            provider_menu_items,
            last_usage_signature: Cell::new(None),
            last_merged_signature: Cell::new(None),
            language,
        })
    }

    /// Update the tray icon based on usage percentages (single provider mode)
    pub fn update_usage(&self, session_percent: f64, weekly_percent: f64, provider_name: &str) {
        let tooltip = match self.language {
            UiLanguage::English => format!(
                "{}: Session {}% | Weekly {}%",
                provider_name, session_percent as i32, weekly_percent as i32
            ),
            UiLanguage::Chinese => format!(
                "{}：会话 {}% | 周度 {}%",
                provider_name, session_percent as i32, weekly_percent as i32
            ),
        };
        let _ = self.tray_icon.set_tooltip(Some(&tooltip));

        if !self.should_update_usage(
            session_percent,
            weekly_percent,
            provider_name,
            IconOverlay::None,
        ) {
            return;
        }

        let icon = create_bar_icon(session_percent, weekly_percent, IconOverlay::None);
        let _ = self.tray_icon.set_icon(Some(icon));
    }

    /// Update the tray icon with an overlay (error, stale, incident)
    pub fn update_usage_with_overlay(
        &self,
        session_percent: f64,
        weekly_percent: f64,
        provider_name: &str,
        overlay: IconOverlay,
    ) {
        let status_suffix = match overlay {
            IconOverlay::None => "",
            IconOverlay::Error => self.text(" (Error)", "（错误）"),
            IconOverlay::Stale => self.text(" (Stale)", "（数据过期）"),
            IconOverlay::Incident => self.text(" (Incident)", "（故障）"),
            IconOverlay::Partial => self.text(" (Partial Outage)", "（部分中断）"),
        };

        let tooltip = match self.language {
            UiLanguage::English => format!(
                "{}: Session {}% | Weekly {}%{}",
                provider_name, session_percent as i32, weekly_percent as i32, status_suffix
            ),
            UiLanguage::Chinese => format!(
                "{}：会话 {}% | 周度 {}%{}",
                provider_name, session_percent as i32, weekly_percent as i32, status_suffix
            ),
        };
        let _ = self.tray_icon.set_tooltip(Some(&tooltip));

        if !self.should_update_usage(session_percent, weekly_percent, provider_name, overlay) {
            return;
        }

        let icon = create_bar_icon(session_percent, weekly_percent, overlay);
        let _ = self.tray_icon.set_icon(Some(icon));
    }

    /// Show error state on the tray icon
    #[allow(dead_code)]
    pub fn show_error(&self, provider_name: &str, error_msg: &str) {
        let icon = create_bar_icon(0.0, 0.0, IconOverlay::Error);
        let _ = self.tray_icon.set_icon(Some(icon));
        let tooltip = format!("{}: {}", provider_name, error_msg);
        let _ = self.tray_icon.set_tooltip(Some(&tooltip));
    }

    /// Show stale data indicator
    #[allow(dead_code)]
    pub fn show_stale(
        &self,
        session_percent: f64,
        weekly_percent: f64,
        provider_name: &str,
        age_minutes: u64,
    ) {
        let icon = create_bar_icon(session_percent, weekly_percent, IconOverlay::Stale);
        let _ = self.tray_icon.set_icon(Some(icon));

        let tooltip = match self.language {
            UiLanguage::English => format!(
                "{}: Session {}% | Weekly {}% (data {}m old)",
                provider_name, session_percent as i32, weekly_percent as i32, age_minutes
            ),
            UiLanguage::Chinese => format!(
                "{}：会话 {}% | 周度 {}%（数据 {} 分钟前）",
                provider_name, session_percent as i32, weekly_percent as i32, age_minutes
            ),
        };
        let _ = self.tray_icon.set_tooltip(Some(&tooltip));
    }

    /// Update the tray icon showing credits mode (thicker bar when weekly exhausted)
    /// This shows a thick credits bar when weekly quota is exhausted but credits remain
    pub fn update_credits_mode(&self, credits_percent: f64, provider_name: &str) {
        let icon = create_credits_icon(credits_percent);
        let _ = self.tray_icon.set_icon(Some(icon));

        let tooltip = match self.language {
            UiLanguage::English => format!(
                "{}: Weekly quota exhausted | {:.0}% credits remaining",
                provider_name, credits_percent
            ),
            UiLanguage::Chinese => {
                format!(
                    "{}：周额度已用尽 | 剩余额度 {:.0}%",
                    provider_name, credits_percent
                )
            }
        };
        let _ = self.tray_icon.set_tooltip(Some(&tooltip));
    }

    /// Update the tray icon showing multiple providers (merged mode)
    pub fn update_merged(&self, providers: &[ProviderUsage]) {
        if providers.is_empty() {
            let icon = create_bar_icon(0.0, 0.0, IconOverlay::None);
            let _ = self.tray_icon.set_icon(Some(icon));
            let _ = self.tray_icon.set_tooltip(Some(
                self.text("CodexBar - No providers", "CodexBar - 无可用服务商"),
            ));
            return;
        }

        let signature = Self::merged_signature(providers);
        if self.last_merged_signature.get() == Some(signature) {
            // Still update tooltip to reflect current data even if icon unchanged
            let tooltip_lines: Vec<String> = providers
                .iter()
                .take(4)
                .map(|p| format!("{}: {}%", p.name, p.session_percent as i32))
                .collect();
            let tooltip = format!("CodexBar\n{}", tooltip_lines.join("\n"));
            let _ = self.tray_icon.set_tooltip(Some(&tooltip));
            return;
        }

        let icon = create_merged_icon(providers);
        let _ = self.tray_icon.set_icon(Some(icon));

        // Build tooltip with all providers
        let tooltip_lines: Vec<String> = providers
            .iter()
            .take(4) // Limit tooltip length
            .map(|p| format!("{}: {}%", p.name, p.session_percent as i32))
            .collect();
        let tooltip = format!("CodexBar\n{}", tooltip_lines.join("\n"));
        let _ = self.tray_icon.set_tooltip(Some(&tooltip));

        self.last_merged_signature.set(Some(signature));
    }

    /// Show loading animation on the tray icon
    pub fn show_loading(&self, pattern: LoadingPattern, phase: f64) {
        let primary = pattern.value(phase);
        let secondary = pattern.value(phase + pattern.secondary_offset());

        // Clear signatures so the next real update isn't skipped
        self.last_usage_signature.set(None);
        self.last_merged_signature.set(None);

        let icon = create_loading_icon(primary, secondary);
        let _ = self.tray_icon.set_icon(Some(icon));
        let _ = self.tray_icon.set_tooltip(Some(
            self.text("CodexBar - Loading...", "CodexBar - 加载中..."),
        ));
    }

    /// Show morph animation on the tray icon (Unbraid effect)
    /// Progress goes from 0.0 (knot/logo) to 1.0 (usage bars)
    pub fn show_morph(&self, progress: f64, session_percent: f64, weekly_percent: f64) {
        let icon = create_morph_icon(progress, session_percent, weekly_percent);
        let _ = self.tray_icon.set_icon(Some(icon));
        let _ = self.tray_icon.set_tooltip(Some(
            self.text("CodexBar - Loading...", "CodexBar - 加载中..."),
        ));
    }

    /// Show a surprise animation frame
    pub fn show_surprise(
        &self,
        animation: SurpriseAnimation,
        frame: u32,
        session_percent: f64,
        weekly_percent: f64,
    ) {
        let icon = create_surprise_icon(animation, frame, session_percent, weekly_percent);
        let _ = self.tray_icon.set_icon(Some(icon));
    }

    /// Update provider menu item labels with status prefixes (colored dots)
    ///
    /// Takes a map of provider IDs to their current status levels and updates
    /// the corresponding menu item labels to show status dots for non-operational providers.
    pub fn update_provider_statuses(&self, statuses: &HashMap<ProviderId, IndicatorStatusLevel>) {
        for (provider_id, check_item) in &self.provider_menu_items {
            let base_name = provider_id.display_name();
            if let Some(status_level) = statuses.get(provider_id) {
                let prefix = status_level.status_prefix();
                let new_label = format!("{}{}", prefix, base_name);
                check_item.set_text(&new_label);
            } else {
                // No status info, show plain name
                check_item.set_text(base_name);
            }
        }
    }

    /// Update a single provider's menu item label with status prefix
    pub fn update_provider_status(
        &self,
        provider_id: ProviderId,
        status_level: IndicatorStatusLevel,
    ) {
        if let Some(check_item) = self.provider_menu_items.get(&provider_id) {
            let base_name = provider_id.display_name();
            let prefix = status_level.status_prefix();
            let new_label = format!("{}{}", prefix, base_name);
            check_item.set_text(&new_label);
        }
    }

    /// Clear status prefix from a provider's menu item (revert to plain name)
    pub fn clear_provider_status(&self, provider_id: ProviderId) {
        if let Some(check_item) = self.provider_menu_items.get(&provider_id) {
            check_item.set_text(provider_id.display_name());
        }
    }

    /// Check for menu events
    pub fn check_events() -> Option<TrayMenuAction> {
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            let id_str = event.id.0.as_str();
            if id_str == "quit" {
                return Some(TrayMenuAction::Quit);
            } else if id_str == "open" {
                return Some(TrayMenuAction::Open);
            } else if id_str == "refresh" {
                return Some(TrayMenuAction::Refresh);
            } else if id_str == "settings" {
                return Some(TrayMenuAction::Settings);
            } else if id_str == "updates" {
                return Some(TrayMenuAction::CheckForUpdates);
            } else if let Some(provider_name) = id_str.strip_prefix("provider_") {
                return Some(TrayMenuAction::ToggleProvider(provider_name.to_string()));
            }
        }
        None
    }
}

impl TrayManager {
    fn usage_signature(
        session_percent: f64,
        weekly_percent: f64,
        provider_name: &str,
        overlay: IconOverlay,
    ) -> u64 {
        let mut hasher = DefaultHasher::new();
        let session_tenths = (session_percent * 10.0).round() as i32;
        let weekly_tenths = (weekly_percent * 10.0).round() as i32;
        session_tenths.hash(&mut hasher);
        weekly_tenths.hash(&mut hasher);
        provider_name.hash(&mut hasher);
        overlay.hash(&mut hasher);
        hasher.finish()
    }

    fn should_update_usage(
        &self,
        session_percent: f64,
        weekly_percent: f64,
        provider_name: &str,
        overlay: IconOverlay,
    ) -> bool {
        let signature =
            Self::usage_signature(session_percent, weekly_percent, provider_name, overlay);
        if self.last_usage_signature.get() == Some(signature) {
            return false;
        }
        self.last_usage_signature.set(Some(signature));
        true
    }

    fn merged_signature(providers: &[ProviderUsage]) -> u64 {
        let mut hasher = DefaultHasher::new();
        providers.len().hash(&mut hasher);
        for p in providers.iter().take(8) {
            p.name.hash(&mut hasher);
            let session_tenths = (p.session_percent * 10.0).round() as i32;
            let weekly_tenths = (p.weekly_percent * 10.0).round() as i32;
            session_tenths.hash(&mut hasher);
            weekly_tenths.hash(&mut hasher);
        }
        hasher.finish()
    }
}

/// Tray menu actions
#[derive(Debug, Clone)]
pub enum TrayMenuAction {
    Open,
    Refresh,
    Settings,
    CheckForUpdates,
    ToggleProvider(String),
    Quit,
}

/// Multi-provider tray manager for per-provider icon mode
/// Creates and manages one tray icon per enabled provider
pub struct MultiTrayManager {
    /// Map of provider ID to their individual tray icon
    provider_icons: HashMap<ProviderId, TrayIcon>,
    provider_signatures: RefCell<HashMap<ProviderId, u64>>,
    language: UiLanguage,
}

impl MultiTrayManager {
    /// Create a new multi-tray manager with icons for enabled providers
    pub fn new() -> anyhow::Result<Self> {
        let settings = Settings::load();
        Ok(Self {
            provider_icons: HashMap::new(),
            provider_signatures: RefCell::new(HashMap::new()),
            language: settings.ui_language,
        })
    }

    /// Sync tray icons with enabled providers
    /// Adds icons for newly enabled providers and removes icons for disabled ones
    pub fn sync_providers(&mut self, enabled_providers: &[ProviderId]) -> anyhow::Result<()> {
        // Remove icons for providers that are no longer enabled
        let enabled_set: std::collections::HashSet<_> = enabled_providers.iter().collect();
        self.provider_icons.retain(|id, _| enabled_set.contains(id));
        self.provider_signatures
            .borrow_mut()
            .retain(|id, _| enabled_set.contains(id));

        // Add icons for newly enabled providers
        for provider_id in enabled_providers {
            if !self.provider_icons.contains_key(provider_id) {
                if let Ok(icon) = self.create_provider_icon(*provider_id) {
                    self.provider_icons.insert(*provider_id, icon);
                }
            }
        }

        Ok(())
    }

    /// Create a tray icon for a specific provider
    fn create_provider_icon(&self, provider_id: ProviderId) -> anyhow::Result<TrayIcon> {
        let menu = Menu::new();
        let text = |en: &'static str, zh: &'static str| match self.language {
            UiLanguage::English => en,
            UiLanguage::Chinese => zh,
        };

        // Provider name header (disabled menu item)
        let header = MenuItem::with_id(
            &format!("header_{}", provider_id.cli_name()),
            provider_id.display_name(),
            false,
            None,
        );
        menu.append(&header)?;

        menu.append(&PredefinedMenuItem::separator())?;

        // Open CodexBar
        let open_item =
            MenuItem::with_id("open", text("Open CodexBar", "打开 CodexBar"), true, None);
        menu.append(&open_item)?;

        // Refresh
        let refresh_item = MenuItem::with_id(
            &format!("refresh_{}", provider_id.cli_name()),
            text("Refresh", "刷新"),
            true,
            None,
        );
        menu.append(&refresh_item)?;

        menu.append(&PredefinedMenuItem::separator())?;

        // Settings
        let settings_item =
            MenuItem::with_id("settings", text("Settings...", "设置..."), true, None);
        menu.append(&settings_item)?;

        menu.append(&PredefinedMenuItem::separator())?;

        // Quit
        let quit_item = MenuItem::with_id("quit", text("Quit", "退出"), true, None);
        menu.append(&quit_item)?;

        let icon = create_bar_icon(0.0, 0.0, IconOverlay::None);
        let tooltip = match self.language {
            UiLanguage::English => format!("{} - Loading...", provider_id.display_name()),
            UiLanguage::Chinese => format!("{} - 加载中...", provider_id.display_name()),
        };

        let tray_icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip(&tooltip)
            .with_icon(icon)
            .build()?;

        Ok(tray_icon)
    }

    /// Update a specific provider's tray icon
    pub fn update_provider(
        &self,
        provider_id: ProviderId,
        session_percent: f64,
        weekly_percent: f64,
    ) {
        if let Some(tray_icon) = self.provider_icons.get(&provider_id) {
            let signature = TrayManager::usage_signature(
                session_percent,
                weekly_percent,
                provider_id.display_name(),
                IconOverlay::None,
            );
            let mut sigs = self.provider_signatures.borrow_mut();
            if sigs.get(&provider_id) != Some(&signature) {
                sigs.insert(provider_id, signature);
            }

            let icon = create_bar_icon(session_percent, weekly_percent, IconOverlay::None);
            let _ = tray_icon.set_icon(Some(icon));

            let tooltip = match self.language {
                UiLanguage::English => format!(
                    "{}: Session {}% | Weekly {}%",
                    provider_id.display_name(),
                    session_percent as i32,
                    weekly_percent as i32
                ),
                UiLanguage::Chinese => format!(
                    "{}：会话 {}% | 周度 {}%",
                    provider_id.display_name(),
                    session_percent as i32,
                    weekly_percent as i32
                ),
            };
            let _ = tray_icon.set_tooltip(Some(&tooltip));
        }
    }

    /// Update a specific provider's tray icon with an overlay
    pub fn update_provider_with_overlay(
        &self,
        provider_id: ProviderId,
        session_percent: f64,
        weekly_percent: f64,
        overlay: IconOverlay,
    ) {
        if let Some(tray_icon) = self.provider_icons.get(&provider_id) {
            let signature = TrayManager::usage_signature(
                session_percent,
                weekly_percent,
                provider_id.display_name(),
                overlay,
            );
            let mut sigs = self.provider_signatures.borrow_mut();
            if sigs.get(&provider_id) != Some(&signature) {
                sigs.insert(provider_id, signature);
            }

            let icon = create_bar_icon(session_percent, weekly_percent, overlay);
            let _ = tray_icon.set_icon(Some(icon));

            let status_suffix = match overlay {
                IconOverlay::None => "",
                IconOverlay::Error => match self.language {
                    UiLanguage::English => " (Error)",
                    UiLanguage::Chinese => "（错误）",
                },
                IconOverlay::Stale => match self.language {
                    UiLanguage::English => " (Stale)",
                    UiLanguage::Chinese => "（数据过期）",
                },
                IconOverlay::Incident => match self.language {
                    UiLanguage::English => " (Incident)",
                    UiLanguage::Chinese => "（故障）",
                },
                IconOverlay::Partial => match self.language {
                    UiLanguage::English => " (Partial Outage)",
                    UiLanguage::Chinese => "（部分中断）",
                },
            };

            let tooltip = match self.language {
                UiLanguage::English => format!(
                    "{}: Session {}% | Weekly {}%{}",
                    provider_id.display_name(),
                    session_percent as i32,
                    weekly_percent as i32,
                    status_suffix
                ),
                UiLanguage::Chinese => format!(
                    "{}：会话 {}% | 周度 {}%{}",
                    provider_id.display_name(),
                    session_percent as i32,
                    weekly_percent as i32,
                    status_suffix
                ),
            };
            let _ = tray_icon.set_tooltip(Some(&tooltip));
        }
    }

    /// Show loading state for a specific provider
    pub fn show_provider_loading(
        &self,
        provider_id: ProviderId,
        pattern: LoadingPattern,
        phase: f64,
    ) {
        if let Some(tray_icon) = self.provider_icons.get(&provider_id) {
            let primary = pattern.value(phase);
            let secondary = pattern.value(phase + pattern.secondary_offset());

            let icon = create_loading_icon(primary, secondary);
            let _ = tray_icon.set_icon(Some(icon));
            let tooltip = match self.language {
                UiLanguage::English => format!("{} - Loading...", provider_id.display_name()),
                UiLanguage::Chinese => format!("{} - 加载中...", provider_id.display_name()),
            };
            let _ = tray_icon.set_tooltip(Some(&tooltip));
        }
    }

    /// Show error state for a specific provider
    pub fn show_provider_error(&self, provider_id: ProviderId, error_msg: &str) {
        if let Some(tray_icon) = self.provider_icons.get(&provider_id) {
            let icon = create_bar_icon(0.0, 0.0, IconOverlay::Error);
            let _ = tray_icon.set_icon(Some(icon));
            let tooltip = format!("{}: {}", provider_id.display_name(), error_msg);
            let _ = tray_icon.set_tooltip(Some(&tooltip));
        }
    }

    /// Get the number of active provider icons
    pub fn icon_count(&self) -> usize {
        self.provider_icons.len()
    }

    /// Check if a provider has an icon
    pub fn has_provider(&self, provider_id: ProviderId) -> bool {
        self.provider_icons.contains_key(&provider_id)
    }
}

/// Unified tray icon manager that supports both single and per-provider modes
pub enum UnifiedTrayManager {
    /// Single icon mode (original behavior)
    Single(TrayManager),
    /// Per-provider icon mode
    PerProvider(MultiTrayManager),
}

impl UnifiedTrayManager {
    /// Create a new unified tray manager based on settings
    pub fn new(settings: &Settings) -> anyhow::Result<Self> {
        match settings.tray_icon_mode {
            TrayIconMode::Single => Ok(UnifiedTrayManager::Single(TrayManager::new()?)),
            TrayIconMode::PerProvider => {
                let mut multi = MultiTrayManager::new()?;
                let enabled = settings.get_enabled_provider_ids();
                multi.sync_providers(&enabled)?;
                Ok(UnifiedTrayManager::PerProvider(multi))
            }
        }
    }

    /// Check if we need to recreate the manager due to mode change
    pub fn needs_mode_switch(&self, new_mode: TrayIconMode) -> bool {
        match (self, new_mode) {
            (UnifiedTrayManager::Single(_), TrayIconMode::PerProvider) => true,
            (UnifiedTrayManager::PerProvider(_), TrayIconMode::Single) => true,
            _ => false,
        }
    }

    /// Check for menu events (delegates to TrayManager's static method)
    pub fn check_events() -> Option<TrayMenuAction> {
        TrayManager::check_events()
    }

    /// Show loading animation
    pub fn show_loading(&self, pattern: LoadingPattern, phase: f64) {
        match self {
            UnifiedTrayManager::Single(tm) => tm.show_loading(pattern, phase),
            UnifiedTrayManager::PerProvider(_) => {
                // In per-provider mode, we could animate all icons or skip
                // For now, skip loading animation in per-provider mode
            }
        }
    }

    /// Show surprise animation
    pub fn show_surprise(&self, anim: SurpriseAnimation, frame: u32, session: f64, weekly: f64) {
        match self {
            UnifiedTrayManager::Single(tm) => tm.show_surprise(anim, frame, session, weekly),
            UnifiedTrayManager::PerProvider(_) => {
                // Skip surprise in per-provider mode
            }
        }
    }

    /// Update usage for a single provider display
    pub fn update_usage(&self, session_percent: f64, weekly_percent: f64, tooltip_name: &str) {
        match self {
            UnifiedTrayManager::Single(tm) => {
                tm.update_usage(session_percent, weekly_percent, tooltip_name)
            }
            UnifiedTrayManager::PerProvider(_) => {
                // Per-provider mode doesn't use single update
            }
        }
    }

    /// Update merged display for all providers
    pub fn update_merged(&self, usages: &[ProviderUsage]) {
        match self {
            UnifiedTrayManager::Single(tm) => tm.update_merged(usages),
            UnifiedTrayManager::PerProvider(multi) => {
                // Update each provider's individual icon
                for usage in usages {
                    if let Some(id) = crate::core::ProviderId::from_cli_name(&usage.name) {
                        multi.update_provider(id, usage.session_percent, usage.weekly_percent);
                    }
                }
            }
        }
    }
}

/// Create a bar icon showing session and weekly usage with optional overlay
fn create_bar_icon(session_percent: f64, weekly_percent: f64, overlay: IconOverlay) -> Icon {
    let mut img: RgbaImage = ImageBuffer::new(ICON_SIZE, ICON_SIZE);

    // Fill with transparent background
    for pixel in img.pixels_mut() {
        *pixel = Rgba([0, 0, 0, 0]);
    }

    // Draw rounded background - dimmed if error/stale
    let bg_alpha = match overlay {
        IconOverlay::Error | IconOverlay::Stale => 180,
        _ => 255,
    };
    let bg_color = Rgba([60, 60, 70, bg_alpha]);
    for y in 2..ICON_SIZE - 2 {
        for x in 2..ICON_SIZE - 2 {
            img.put_pixel(x, y, bg_color);
        }
    }

    // Bar dimensions
    let bar_left = 4u32;
    let bar_right = ICON_SIZE - 4;
    let bar_width = bar_right - bar_left;

    // Color adjustment for error/stale states
    let color_adjust = |r: u8, g: u8, b: u8| -> (u8, u8, u8) {
        match overlay {
            IconOverlay::Error => {
                // Grayscale
                let gray = ((r as u16 + g as u16 + b as u16) / 3) as u8;
                (gray, gray, gray)
            }
            IconOverlay::Stale => {
                // Dim colors by 40%
                (
                    (r as f32 * 0.6) as u8,
                    (g as f32 * 0.6) as u8,
                    (b as f32 * 0.6) as u8,
                )
            }
            _ => (r, g, b),
        }
    };

    // Session bar (top, thicker) - y: 8 to 14
    let session_level = UsageLevel::from_percent(session_percent);
    let (sr, sg, sb) = session_level.color();
    let (sr, sg, sb) = color_adjust(sr, sg, sb);
    let session_fill = ((session_percent / 100.0) * bar_width as f64) as u32;

    // Track (gray)
    for y in 8..15 {
        for x in bar_left..bar_right {
            img.put_pixel(x, y, Rgba([80, 80, 90, 255]));
        }
    }
    // Fill (colored)
    for y in 8..15 {
        for x in bar_left..(bar_left + session_fill).min(bar_right) {
            img.put_pixel(x, y, Rgba([sr, sg, sb, 255]));
        }
    }

    // Weekly bar (bottom, thinner) - y: 18 to 22
    let weekly_level = UsageLevel::from_percent(weekly_percent);
    let (wr, wg, wb) = weekly_level.color();
    let (wr, wg, wb) = color_adjust(wr, wg, wb);
    let weekly_fill = ((weekly_percent / 100.0) * bar_width as f64) as u32;

    // Track (gray)
    for y in 18..23 {
        for x in bar_left..bar_right {
            img.put_pixel(x, y, Rgba([80, 80, 90, 255]));
        }
    }
    // Fill (colored)
    for y in 18..23 {
        for x in bar_left..(bar_left + weekly_fill).min(bar_right) {
            img.put_pixel(x, y, Rgba([wr, wg, wb, 255]));
        }
    }

    // Draw overlay badge
    draw_overlay_badge(&mut img, overlay);

    let rgba = img.into_raw();
    Icon::from_rgba(rgba, ICON_SIZE, ICON_SIZE).expect("Failed to create icon")
}

/// Draw overlay badge on the icon (bottom-right corner)
fn draw_overlay_badge(img: &mut RgbaImage, overlay: IconOverlay) {
    match overlay {
        IconOverlay::None => {}
        IconOverlay::Error => {
            // Red X in bottom-right corner
            let badge_color = Rgba([255, 60, 60, 255]);
            // Draw a small X (6x6 pixels in corner)
            for i in 0..6 {
                // Diagonal line \
                let x = ICON_SIZE - 8 + i;
                let y = ICON_SIZE - 8 + i;
                if x < ICON_SIZE && y < ICON_SIZE {
                    img.put_pixel(x, y, badge_color);
                }
                // Diagonal line /
                let x2 = ICON_SIZE - 3 - i;
                let y2 = ICON_SIZE - 8 + i;
                if x2 < ICON_SIZE && y2 < ICON_SIZE {
                    img.put_pixel(x2, y2, badge_color);
                }
            }
        }
        IconOverlay::Stale => {
            // Clock indicator - small dot in corner
            let badge_color = Rgba([180, 180, 180, 255]);
            // Draw a small circle (clock symbol)
            for dy in 0..4 {
                for dx in 0..4 {
                    let x = ICON_SIZE - 6 + dx;
                    let y = ICON_SIZE - 6 + dy;
                    if x < ICON_SIZE && y < ICON_SIZE {
                        img.put_pixel(x, y, badge_color);
                    }
                }
            }
        }
        IconOverlay::Incident => {
            // Red warning badge
            let badge_color = Rgba([244, 67, 54, 255]);
            // Draw filled circle in corner
            for dy in 0..6 {
                for dx in 0..6 {
                    let x = ICON_SIZE - 8 + dx;
                    let y = ICON_SIZE - 8 + dy;
                    if x < ICON_SIZE && y < ICON_SIZE {
                        img.put_pixel(x, y, badge_color);
                    }
                }
            }
        }
        IconOverlay::Partial => {
            // Orange warning badge
            let badge_color = Rgba([255, 152, 0, 255]);
            // Draw filled circle in corner
            for dy in 0..6 {
                for dx in 0..6 {
                    let x = ICON_SIZE - 8 + dx;
                    let y = ICON_SIZE - 8 + dy;
                    if x < ICON_SIZE && y < ICON_SIZE {
                        img.put_pixel(x, y, badge_color);
                    }
                }
            }
        }
    }
}

/// Create a credits icon showing a thick single bar for credits mode
/// Used when weekly quota is exhausted but paid credits remain
fn create_credits_icon(credits_percent: f64) -> Icon {
    let mut img: RgbaImage = ImageBuffer::new(ICON_SIZE, ICON_SIZE);

    // Fill with transparent background
    for pixel in img.pixels_mut() {
        *pixel = Rgba([0, 0, 0, 0]);
    }

    // Draw rounded background
    let bg_color = Rgba([60, 60, 70, 255]);
    for y in 2..ICON_SIZE - 2 {
        for x in 2..ICON_SIZE - 2 {
            img.put_pixel(x, y, bg_color);
        }
    }

    // Bar dimensions - thick bar for credits (16px like macOS version)
    let bar_left = 4u32;
    let bar_right = ICON_SIZE - 4;
    let bar_width = bar_right - bar_left;

    // Credits bar - centered and thick (y: 8 to 24)
    let bar_y_start = 8u32;
    let bar_y_end = 24u32;

    // Cyan/blue color for credits
    let credits_color = Rgba([64, 196, 255, 255]);
    let credits_fill = ((credits_percent / 100.0) * bar_width as f64) as u32;

    // Track (gray)
    for y in bar_y_start..bar_y_end {
        for x in bar_left..bar_right {
            img.put_pixel(x, y, Rgba([80, 80, 90, 255]));
        }
    }
    // Fill (cyan)
    for y in bar_y_start..bar_y_end {
        for x in bar_left..(bar_left + credits_fill).min(bar_right) {
            img.put_pixel(x, y, credits_color);
        }
    }

    let rgba = img.into_raw();
    Icon::from_rgba(rgba, ICON_SIZE, ICON_SIZE).expect("Failed to create icon")
}

/// Create a merged icon showing multiple providers stacked
fn create_merged_icon(providers: &[ProviderUsage]) -> Icon {
    let mut img: RgbaImage = ImageBuffer::new(ICON_SIZE, ICON_SIZE);

    // Fill with transparent background
    for pixel in img.pixels_mut() {
        *pixel = Rgba([0, 0, 0, 0]);
    }

    // Draw rounded background
    let bg_color = Rgba([60, 60, 70, 255]);
    for y in 2..ICON_SIZE - 2 {
        for x in 2..ICON_SIZE - 2 {
            img.put_pixel(x, y, bg_color);
        }
    }

    // Bar dimensions
    let bar_left = 4u32;
    let bar_right = ICON_SIZE - 4;
    let bar_width = bar_right - bar_left;

    // Calculate bar positions based on provider count
    let provider_count = providers.len().min(4); // Max 4 bars
    if provider_count == 0 {
        let rgba = img.into_raw();
        return Icon::from_rgba(rgba, ICON_SIZE, ICON_SIZE).expect("Failed to create icon");
    }

    // Calculate bar height and spacing to fit within icon
    let total_height = ICON_SIZE - 8; // Leave margin
    let bar_height = (total_height / provider_count as u32).min(6);
    let spacing = if provider_count > 1 {
        (total_height - (bar_height * provider_count as u32)) / (provider_count as u32 - 1).max(1)
    } else {
        0
    };

    for (i, provider) in providers.iter().take(4).enumerate() {
        let y_start = 4 + (i as u32 * (bar_height + spacing));
        let y_end = (y_start + bar_height).min(ICON_SIZE - 4);

        let level = UsageLevel::from_percent(provider.session_percent);
        let (r, g, b) = level.color();
        let fill_width = ((provider.session_percent / 100.0) * bar_width as f64) as u32;

        // Draw track (gray)
        for y in y_start..y_end {
            for x in bar_left..bar_right {
                img.put_pixel(x, y, Rgba([80, 80, 90, 255]));
            }
        }

        // Draw fill (colored)
        for y in y_start..y_end {
            for x in bar_left..(bar_left + fill_width).min(bar_right) {
                img.put_pixel(x, y, Rgba([r, g, b, 255]));
            }
        }
    }

    let rgba = img.into_raw();
    Icon::from_rgba(rgba, ICON_SIZE, ICON_SIZE).expect("Failed to create icon")
}

/// Create a loading animation icon with animated bars
fn create_loading_icon(primary_percent: f64, secondary_percent: f64) -> Icon {
    let mut img: RgbaImage = ImageBuffer::new(ICON_SIZE, ICON_SIZE);

    // Fill with transparent background
    for pixel in img.pixels_mut() {
        *pixel = Rgba([0, 0, 0, 0]);
    }

    // Draw rounded background
    let bg_color = Rgba([60, 60, 70, 255]);
    for y in 2..ICON_SIZE - 2 {
        for x in 2..ICON_SIZE - 2 {
            img.put_pixel(x, y, bg_color);
        }
    }

    // Bar dimensions
    let bar_left = 4u32;
    let bar_right = ICON_SIZE - 4;
    let bar_width = bar_right - bar_left;

    // Loading color - cyan/blue gradient
    let loading_color = Rgba([64, 196, 255, 255]);

    // Primary bar (top) - y: 8 to 14
    let primary_fill = ((primary_percent / 100.0) * bar_width as f64) as u32;
    for y in 8..15 {
        for x in bar_left..bar_right {
            img.put_pixel(x, y, Rgba([80, 80, 90, 255]));
        }
    }
    for y in 8..15 {
        for x in bar_left..(bar_left + primary_fill).min(bar_right) {
            img.put_pixel(x, y, loading_color);
        }
    }

    // Secondary bar (bottom) - y: 18 to 22
    let secondary_fill = ((secondary_percent / 100.0) * bar_width as f64) as u32;
    for y in 18..23 {
        for x in bar_left..bar_right {
            img.put_pixel(x, y, Rgba([80, 80, 90, 255]));
        }
    }
    for y in 18..23 {
        for x in bar_left..(bar_left + secondary_fill).min(bar_right) {
            img.put_pixel(x, y, loading_color);
        }
    }

    let rgba = img.into_raw();
    Icon::from_rgba(rgba, ICON_SIZE, ICON_SIZE).expect("Failed to create icon")
}

/// Create a surprise animation icon frame
fn create_surprise_icon(
    animation: SurpriseAnimation,
    frame: u32,
    session_percent: f64,
    weekly_percent: f64,
) -> Icon {
    let mut img: RgbaImage = ImageBuffer::new(ICON_SIZE, ICON_SIZE);

    // Fill with transparent background
    for pixel in img.pixels_mut() {
        *pixel = Rgba([0, 0, 0, 0]);
    }

    // Draw rounded background
    let bg_color = Rgba([60, 60, 70, 255]);
    for y in 2..ICON_SIZE - 2 {
        for x in 2..ICON_SIZE - 2 {
            img.put_pixel(x, y, bg_color);
        }
    }

    // Bar dimensions
    let bar_left = 4u32;
    let bar_right = ICON_SIZE - 4;
    let bar_width = bar_right - bar_left;

    // Calculate animation parameters
    let total_frames = animation.duration_frames().max(1);
    let progress = frame as f64 / total_frames as f64;

    // Color and position modifiers based on animation type
    let (color_mod, x_offset, y_offset) = match animation {
        SurpriseAnimation::None => ((1.0, 1.0, 1.0), 0i32, 0i32),
        SurpriseAnimation::Blink => {
            // Flash to white and back
            let flash = if progress < 0.5 {
                progress * 2.0 // Fade to white
            } else {
                (1.0 - progress) * 2.0 // Fade back
            };
            let blend = 1.0 + flash * 0.8; // Boost brightness
            ((blend, blend, blend), 0, 0)
        }
        SurpriseAnimation::Wiggle => {
            // Shake left and right
            let shake = (progress * std::f64::consts::PI * 6.0).sin(); // 3 full oscillations
            let offset = (shake * 2.0) as i32; // +/- 2 pixels
            ((1.0, 1.0, 1.0), offset, 0)
        }
        SurpriseAnimation::Pulse => {
            // Gentle pulse - grow and shrink brightness
            let pulse = (progress * std::f64::consts::PI * 2.0).sin(); // One full cycle
            let intensity = 1.0 + pulse * 0.3; // +/- 30% brightness
            ((intensity, intensity, intensity), 0, 0)
        }
        SurpriseAnimation::Rainbow => {
            // Sweep through rainbow colors
            let hue = progress * 360.0;
            let (r, g, b) = hsv_to_rgb(hue, 0.8, 1.0);
            (
                (
                    r as f64 / 255.0 * 2.0,
                    g as f64 / 255.0 * 2.0,
                    b as f64 / 255.0 * 2.0,
                ),
                0,
                0,
            )
        }
        SurpriseAnimation::Tilt => {
            // Tilt effect - slight diagonal shift that returns
            let tilt = (progress * std::f64::consts::PI).sin(); // 0 -> 1 -> 0
            let x_off = (tilt * 2.0) as i32; // +2 pixels at peak
            let y_off = (tilt * 1.0) as i32; // +1 pixel at peak (slight diagonal)
            ((1.0, 1.0, 1.0), x_off, y_off)
        }
    };

    // Session bar (top) - y: 8 to 14
    let session_level = UsageLevel::from_percent(session_percent);
    let (sr, sg, sb) = session_level.color();
    let sr = ((sr as f64 * color_mod.0).min(255.0)) as u8;
    let sg = ((sg as f64 * color_mod.1).min(255.0)) as u8;
    let sb = ((sb as f64 * color_mod.2).min(255.0)) as u8;
    let session_fill = ((session_percent / 100.0) * bar_width as f64) as u32;

    // Track (gray)
    for y in 8..15 {
        for x in bar_left..bar_right {
            let adjusted_x = (x as i32 + x_offset)
                .max(bar_left as i32)
                .min(bar_right as i32 - 1) as u32;
            let adjusted_y = (y as i32 + y_offset).max(4).min(ICON_SIZE as i32 - 4) as u32;
            img.put_pixel(adjusted_x, adjusted_y, Rgba([80, 80, 90, 255]));
        }
    }
    // Fill (colored with animation)
    for y in 8..15 {
        for x in bar_left..(bar_left + session_fill).min(bar_right) {
            let adjusted_x = (x as i32 + x_offset)
                .max(bar_left as i32)
                .min(bar_right as i32 - 1) as u32;
            let adjusted_y = (y as i32 + y_offset).max(4).min(ICON_SIZE as i32 - 4) as u32;
            img.put_pixel(adjusted_x, adjusted_y, Rgba([sr, sg, sb, 255]));
        }
    }

    // Weekly bar (bottom) - y: 18 to 22
    let weekly_level = UsageLevel::from_percent(weekly_percent);
    let (wr, wg, wb) = weekly_level.color();
    let wr = ((wr as f64 * color_mod.0).min(255.0)) as u8;
    let wg = ((wg as f64 * color_mod.1).min(255.0)) as u8;
    let wb = ((wb as f64 * color_mod.2).min(255.0)) as u8;
    let weekly_fill = ((weekly_percent / 100.0) * bar_width as f64) as u32;

    // Track (gray)
    for y in 18..23 {
        for x in bar_left..bar_right {
            let adjusted_x = (x as i32 + x_offset)
                .max(bar_left as i32)
                .min(bar_right as i32 - 1) as u32;
            let adjusted_y = (y as i32 + y_offset).max(4).min(ICON_SIZE as i32 - 4) as u32;
            img.put_pixel(adjusted_x, adjusted_y, Rgba([80, 80, 90, 255]));
        }
    }
    // Fill (colored with animation)
    for y in 18..23 {
        for x in bar_left..(bar_left + weekly_fill).min(bar_right) {
            let adjusted_x = (x as i32 + x_offset)
                .max(bar_left as i32)
                .min(bar_right as i32 - 1) as u32;
            let adjusted_y = (y as i32 + y_offset).max(4).min(ICON_SIZE as i32 - 4) as u32;
            img.put_pixel(adjusted_x, adjusted_y, Rgba([wr, wg, wb, 255]));
        }
    }

    let rgba = img.into_raw();
    Icon::from_rgba(rgba, ICON_SIZE, ICON_SIZE).expect("Failed to create icon")
}

/// Create a morph animation icon frame (logo/knot -> bars transition)
/// This is the "Unbraid" animation from Swift - morphs from interlaced ribbons to usage bars
fn create_morph_icon(progress: f64, session_percent: f64, weekly_percent: f64) -> Icon {
    let mut img: RgbaImage = ImageBuffer::new(ICON_SIZE, ICON_SIZE);
    let t = progress.clamp(0.0, 1.0) as f32;

    // Fill with transparent background
    for pixel in img.pixels_mut() {
        *pixel = Rgba([0, 0, 0, 0]);
    }

    // Draw rounded background
    let bg_color = Rgba([60, 60, 70, 255]);
    for y in 2..ICON_SIZE - 2 {
        for x in 2..ICON_SIZE - 2 {
            img.put_pixel(x, y, bg_color);
        }
    }

    let center_x = ICON_SIZE as f32 / 2.0;
    let center_y = ICON_SIZE as f32 / 2.0;
    let ribbon_color = Rgba([200, 200, 210, 255]);

    // Morphing segments - three ribbons that transform into two bars
    // Segment 1: Upper ribbon -> top bar
    let seg1_start_y = center_y + 2.0;
    let seg1_end_y = 11.0; // Final top bar position
    let seg1_y = lerp(seg1_start_y, seg1_end_y, t);
    let seg1_start_angle = -30.0_f32;
    let seg1_end_angle = 0.0_f32;
    let seg1_angle = lerp(seg1_start_angle, seg1_end_angle, t);
    let seg1_start_len = 16.0_f32;
    let seg1_end_len = 24.0_f32;
    let seg1_len = lerp(seg1_start_len, seg1_end_len, t);
    let seg1_thickness = lerp(3.5, 7.0, t);

    draw_rotated_ribbon(
        &mut img,
        center_x,
        seg1_y,
        seg1_len,
        seg1_thickness,
        seg1_angle,
        ribbon_color,
    );

    // Segment 2: Lower ribbon -> bottom bar
    let seg2_start_y = center_y - 2.0;
    let seg2_end_y = 20.0; // Final bottom bar position
    let seg2_y = lerp(seg2_start_y, seg2_end_y, t);
    let seg2_start_angle = 210.0_f32 - 180.0; // Normalize to -30 to 30 range
    let seg2_end_angle = 0.0_f32;
    let seg2_angle = lerp(seg2_start_angle, seg2_end_angle, t);
    let seg2_start_len = 16.0_f32;
    let seg2_end_len = 24.0_f32;
    let seg2_len = lerp(seg2_start_len, seg2_end_len, t);
    let seg2_thickness = lerp(3.5, 5.0, t);

    draw_rotated_ribbon(
        &mut img,
        center_x,
        seg2_y,
        seg2_len,
        seg2_thickness,
        seg2_angle,
        ribbon_color,
    );

    // Segment 3: Side ribbon that fades out
    let seg3_alpha = ((1.0 - t * 1.1).max(0.0) * 255.0) as u8;
    if seg3_alpha > 10 {
        let seg3_y = lerp(center_y, center_y - 6.0, t);
        let seg3_angle = lerp(90.0, 0.0, t);
        let seg3_len = lerp(16.0, 8.0, t);
        let seg3_thickness = lerp(3.5, 1.8, t);
        let fading_color = Rgba([200, 200, 210, seg3_alpha]);
        draw_rotated_ribbon(
            &mut img,
            center_x,
            seg3_y,
            seg3_len,
            seg3_thickness,
            seg3_angle,
            fading_color,
        );
    }

    // Cross-fade in colored fill bars near the end of the morph
    if t > 0.55 {
        let bar_t = ((t - 0.55) / 0.45).min(1.0);
        let bar_alpha = (bar_t * 200.0) as u8;

        // Bar dimensions
        let bar_left = 4u32;
        let bar_right = ICON_SIZE - 4;
        let bar_width = bar_right - bar_left;

        // Session bar fill color
        let session_level = UsageLevel::from_percent(session_percent);
        let (sr, sg, sb) = session_level.color();
        let session_fill = ((session_percent / 100.0) * bar_width as f64) as u32;

        // Draw session bar fill with alpha
        for y in 8..15 {
            for x in bar_left..(bar_left + session_fill).min(bar_right) {
                let existing = img.get_pixel(x, y);
                let blended = blend_alpha(existing, &Rgba([sr, sg, sb, bar_alpha]));
                img.put_pixel(x, y, blended);
            }
        }

        // Weekly bar fill color
        let weekly_level = UsageLevel::from_percent(weekly_percent);
        let (wr, wg, wb) = weekly_level.color();
        let weekly_fill = ((weekly_percent / 100.0) * bar_width as f64) as u32;

        // Draw weekly bar fill with alpha
        for y in 18..23 {
            for x in bar_left..(bar_left + weekly_fill).min(bar_right) {
                let existing = img.get_pixel(x, y);
                let blended = blend_alpha(existing, &Rgba([wr, wg, wb, bar_alpha]));
                img.put_pixel(x, y, blended);
            }
        }
    }

    let rgba = img.into_raw();
    Icon::from_rgba(rgba, ICON_SIZE, ICON_SIZE).expect("Failed to create icon")
}

/// Draw a rotated ribbon/rounded rectangle
fn draw_rotated_ribbon(
    img: &mut RgbaImage,
    cx: f32,
    cy: f32,
    length: f32,
    thickness: f32,
    angle_deg: f32,
    color: Rgba<u8>,
) {
    let angle_rad = angle_deg.to_radians();
    let cos_a = angle_rad.cos();
    let sin_a = angle_rad.sin();

    let half_len = length / 2.0;
    let half_thick = thickness / 2.0;

    // Draw the ribbon by iterating over a bounding box and checking if pixels are inside
    let bound = (half_len + half_thick) as i32 + 2;

    for dy in -bound..=bound {
        for dx in -bound..=bound {
            // Rotate point back to ribbon-local coordinates
            let px = dx as f32 * cos_a + dy as f32 * sin_a;
            let py = -dx as f32 * sin_a + dy as f32 * cos_a;

            // Check if inside rounded rectangle
            let in_length = px.abs() <= half_len;
            let in_thickness = py.abs() <= half_thick;

            // Rounded ends
            let in_left_cap = (px + half_len).powi(2) + py.powi(2) <= half_thick.powi(2);
            let in_right_cap = (px - half_len).powi(2) + py.powi(2) <= half_thick.powi(2);

            if (in_length && in_thickness) || in_left_cap || in_right_cap {
                let final_x = (cx + dx as f32) as i32;
                let final_y = (cy + dy as f32) as i32;

                if final_x >= 0
                    && final_x < ICON_SIZE as i32
                    && final_y >= 0
                    && final_y < ICON_SIZE as i32
                {
                    let existing = img.get_pixel(final_x as u32, final_y as u32);
                    let blended = blend_alpha(existing, &color);
                    img.put_pixel(final_x as u32, final_y as u32, blended);
                }
            }
        }
    }
}

/// Linear interpolation
fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Blend two colors with alpha
fn blend_alpha(base: &Rgba<u8>, overlay: &Rgba<u8>) -> Rgba<u8> {
    let oa = overlay[3] as f32 / 255.0;
    let ba = base[3] as f32 / 255.0;

    if oa < 0.01 {
        return *base;
    }

    let out_a = oa + ba * (1.0 - oa);
    if out_a < 0.01 {
        return Rgba([0, 0, 0, 0]);
    }

    let r = (overlay[0] as f32 * oa + base[0] as f32 * ba * (1.0 - oa)) / out_a;
    let g = (overlay[1] as f32 * oa + base[1] as f32 * ba * (1.0 - oa)) / out_a;
    let b = (overlay[2] as f32 * oa + base[2] as f32 * ba * (1.0 - oa)) / out_a;

    Rgba([r as u8, g as u8, b as u8, (out_a * 255.0) as u8])
}

/// Convert HSV to RGB (h: 0-360, s: 0-1, v: 0-1)
fn hsv_to_rgb(h: f64, s: f64, v: f64) -> (u8, u8, u8) {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;

    let (r, g, b) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };

    (
        ((r + m) * 255.0) as u8,
        ((g + m) * 255.0) as u8,
        ((b + m) * 255.0) as u8,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_bar_icon() {
        // Just verify it doesn't panic
        let _icon = create_bar_icon(50.0, 25.0, IconOverlay::None);
        let _icon = create_bar_icon(0.0, 0.0, IconOverlay::None);
        let _icon = create_bar_icon(100.0, 100.0, IconOverlay::None);
    }

    #[test]
    fn test_create_bar_icon_with_overlays() {
        // Test all overlay types
        let _icon = create_bar_icon(50.0, 25.0, IconOverlay::Error);
        let _icon = create_bar_icon(50.0, 25.0, IconOverlay::Stale);
        let _icon = create_bar_icon(50.0, 25.0, IconOverlay::Incident);
        let _icon = create_bar_icon(50.0, 25.0, IconOverlay::Partial);
    }

    #[test]
    fn test_usage_signature_detects_changes() {
        let sig1 = TrayManager::usage_signature(10.0, 20.0, "Claude", IconOverlay::None);
        let sig1_repeat = TrayManager::usage_signature(10.0, 20.0, "Claude", IconOverlay::None);
        let sig_overlay = TrayManager::usage_signature(10.0, 20.0, "Claude", IconOverlay::Error);
        let sig_provider = TrayManager::usage_signature(10.0, 20.0, "Codex", IconOverlay::None);
        let sig_values = TrayManager::usage_signature(11.0, 20.0, "Claude", IconOverlay::None);

        assert_eq!(sig1, sig1_repeat, "same inputs should yield same signature");
        assert_ne!(sig1, sig_overlay, "overlay changes should change signature");
        assert_ne!(
            sig1, sig_provider,
            "provider name changes should change signature"
        );
        assert_ne!(
            sig1, sig_values,
            "usage values changes should change signature"
        );
    }

    #[test]
    fn test_merged_signature_tracks_list_content() {
        let providers_a = vec![
            ProviderUsage {
                name: "Claude".into(),
                session_percent: 10.0,
                weekly_percent: 20.0,
            },
            ProviderUsage {
                name: "Codex".into(),
                session_percent: 30.0,
                weekly_percent: 40.0,
            },
        ];
        let providers_b = vec![
            ProviderUsage {
                name: "Claude".into(),
                session_percent: 10.0,
                weekly_percent: 20.0,
            },
            ProviderUsage {
                name: "Codex".into(),
                session_percent: 30.0,
                weekly_percent: 50.0,
            },
        ];
        let providers_c = vec![ProviderUsage {
            name: "Claude".into(),
            session_percent: 10.0,
            weekly_percent: 20.0,
        }];

        let sig_a1 = TrayManager::merged_signature(&providers_a);
        let sig_a2 = TrayManager::merged_signature(&providers_a);
        let sig_b = TrayManager::merged_signature(&providers_b);
        let sig_c = TrayManager::merged_signature(&providers_c);

        assert_eq!(
            sig_a1, sig_a2,
            "same provider list should yield stable signature"
        );
        assert_ne!(sig_a1, sig_b, "value change should alter signature");
        assert_ne!(
            sig_a1, sig_c,
            "length/content change should alter signature"
        );
    }
}
