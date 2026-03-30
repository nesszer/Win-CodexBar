//! Preferences window for CodexBar
//!
//! A refined settings interface inspired by Linear and Apple Settings.
//! Design principle: Precision Calm - clear hierarchy, generous spacing, subtle depth.

#![allow(dead_code)] // Legacy show_* methods kept for potential future use

use eframe::egui::{self, Color32, Rect, RichText, Rounding, Stroke, Vec2};
use std::borrow::Cow;
use std::cell::RefCell;
use std::sync::{Arc, Mutex};

use super::provider_icons::ProviderIconCache;
use super::theme::{FontSize, Radius, Spacing, Theme, provider_color, provider_icon};
use crate::browser::cookies::get_cookie_header_from_browser;
use crate::browser::detection::{BrowserDetector, BrowserType};
use crate::core::{PersonalInfoRedactor, ProviderId, WidgetSnapshot, WidgetSnapshotStore};
use crate::core::{ProviderAccountData, TokenAccount, TokenAccountStore, TokenAccountSupport};
use crate::settings::{
    ApiKeys, ManualCookies, Settings, TrayIconMode, UiLanguage, get_api_key_providers,
};
use crate::shortcuts::format_shortcut;
use std::collections::HashMap;

// Thread-local icon cache for viewport rendering
thread_local! {
    static VIEWPORT_ICON_CACHE: RefCell<ProviderIconCache> = RefCell::new(ProviderIconCache::new());
}

thread_local! {
    static CURRENT_UI_LANGUAGE: std::cell::Cell<UiLanguage> = const { std::cell::Cell::new(UiLanguage::English) };
}

fn set_current_ui_language(language: UiLanguage) {
    CURRENT_UI_LANGUAGE.with(|cell| cell.set(language));
}

fn current_ui_language() -> UiLanguage {
    CURRENT_UI_LANGUAGE.with(|cell| cell.get())
}

fn localize_text<'a>(text: &'a str) -> Cow<'a, str> {
    if current_ui_language() != UiLanguage::Chinese {
        return Cow::Borrowed(text);
    }
    match text {
        "Startup" => Cow::Borrowed("启动"),
        "Notifications" => Cow::Borrowed("通知"),
        "Info" => Cow::Borrowed("信息"),
        "Usage" => Cow::Borrowed("用量"),
        "Quick Actions" => Cow::Borrowed("快捷操作"),
        "Browser Cookie Import" => Cow::Borrowed("浏览器 Cookie 导入"),
        "Usage Display" => Cow::Borrowed("用量显示"),
        "Tray Icon" => Cow::Borrowed("托盘图标"),
        "API Keys" => Cow::Borrowed("API 密钥"),
        "Browser Cookies" => Cow::Borrowed("浏览器 Cookie"),
        "Saved Cookies" => Cow::Borrowed("已保存 Cookies"),
        "Add Manual Cookie" => Cow::Borrowed("添加手动 Cookie"),
        "Refresh" => Cow::Borrowed("刷新"),
        "Animations" => Cow::Borrowed("动画"),
        "Menu Bar" => Cow::Borrowed("菜单栏"),
        "Fun" => Cow::Borrowed("趣味"),
        "Privacy" => Cow::Borrowed("隐私"),
        "Updates" => Cow::Borrowed("更新"),
        "Keyboard Shortcuts" => Cow::Borrowed("快捷键"),
        "Appearance" => Cow::Borrowed("外观"),
        "Credits" => Cow::Borrowed("鸣谢"),
        "Links" => Cow::Borrowed("链接"),
        "Build Info" => Cow::Borrowed("构建信息"),
        "Enabled Providers" => Cow::Borrowed("已启用服务商"),
        "Start at login" => Cow::Borrowed("开机启动"),
        "Launch CodexBar when you log in" => Cow::Borrowed("登录后自动启动 CodexBar"),
        "Start minimized" => Cow::Borrowed("最小化启动"),
        "Start in the system tray" => Cow::Borrowed("启动后停留在系统托盘"),
        "Show notifications" => Cow::Borrowed("显示通知"),
        "Alert when usage thresholds are reached" => Cow::Borrowed("达到用量阈值时提醒"),
        "Sound effects" => Cow::Borrowed("声音提示"),
        "Play sound when thresholds are reached" => Cow::Borrowed("达到阈值时播放提示音"),
        "Sound volume" => Cow::Borrowed("提示音音量"),
        "Volume level for alert sounds" => Cow::Borrowed("告警提示音音量"),
        "High warning" => Cow::Borrowed("高位预警"),
        "Show warning at this usage level" => Cow::Borrowed("在该用量水平显示预警"),
        "Critical warning" => Cow::Borrowed("严重告警"),
        "Show critical warning at this level" => Cow::Borrowed("在该水平显示严重告警"),
        "Critical alert" => Cow::Borrowed("严重告警"),
        "Show critical alert at this level" => Cow::Borrowed("在该水平显示严重告警"),
        "Hide personal info" => Cow::Borrowed("隐藏个人信息"),
        "Mask emails and account names (useful for streaming)" => {
            Cow::Borrowed("遮蔽邮箱和账号名称（适合直播时使用）")
        }
        "Hide personal info " => Cow::Borrowed("隐藏个人信息"),
        "Mask emails and account names (useful for streaming) " => {
            Cow::Borrowed("遮蔽邮箱和账号名称（适合直播时使用）")
        }
        "Update channel" => Cow::Borrowed("更新通道"),
        "Choose between stable releases or beta previews" => {
            Cow::Borrowed("在稳定版与测试预览版之间选择")
        }
        "Global shortcut" => Cow::Borrowed("全局快捷键"),
        "Press this key combination to open CodexBar from anywhere" => {
            Cow::Borrowed("按下此快捷键可在任意位置打开 CodexBar")
        }
        "Format: Ctrl+Shift+Key, Alt+Ctrl+Key, etc. Restart required to apply changes." => {
            Cow::Borrowed("格式：Ctrl+Shift+Key、Alt+Ctrl+Key 等。修改后需重启生效。")
        }
        "State" => Cow::Borrowed("状态"),
        "Enabled" => Cow::Borrowed("已启用"),
        "Disabled" => Cow::Borrowed("已禁用"),
        "Source" => Cow::Borrowed("来源"),
        "Version" => Cow::Borrowed("版本"),
        "Updated" => Cow::Borrowed("更新"),
        "Status" => Cow::Borrowed("状态"),
        "All Systems Operational" => Cow::Borrowed("系统运行正常"),
        "Account" => Cow::Borrowed("账号"),
        "Not logged in" => Cow::Borrowed("未登录"),
        "Plan" => Cow::Borrowed("套餐"),
        "Unknown" => Cow::Borrowed("未知"),
        "Session" => Cow::Borrowed("会话"),
        "Weekly" => Cow::Borrowed("周度"),
        "Code review" => Cow::Borrowed("代码审查"),
        "Resetting..." => Cow::Borrowed("正在重置..."),
        "Resets in" => Cow::Borrowed("重置于"),
        "Tray Display" => Cow::Borrowed("托盘显示"),
        "Show in tray" => Cow::Borrowed("托盘显示项"),
        "Credits balance" => Cow::Borrowed("额度"),
        "left" => Cow::Borrowed("剩余"),
        "Cost" => Cow::Borrowed("费用"),
        "Today" => Cow::Borrowed("今日"),
        "Last 30 days" => Cow::Borrowed("近 30 天"),
        "tokens" => Cow::Borrowed("tokens"),
        "Settings" => Cow::Borrowed("设置"),
        "Menu bar metric" => Cow::Borrowed("菜单栏指标"),
        "Choose which window drives the menu bar percent." => {
            Cow::Borrowed("选择由哪个时间窗口驱动菜单栏百分比。")
        }
        "Usage source" => Cow::Borrowed("用量来源"),
        "Model" => Cow::Borrowed("模型"),
        "Average" => Cow::Borrowed("平均"),
        "Auto falls back to the next source if the preferred one fails." => {
            Cow::Borrowed("首选来源失败时会自动回退到下一个来源。")
        }
        "oauth + web" => Cow::Borrowed("OAuth + 网页"),
        "Automatic" => Cow::Borrowed("自动"),
        "Auto" => Cow::Borrowed("自动"),
        "OAuth" => Cow::Borrowed("OAuth"),
        "API" => Cow::Borrowed("API"),
        "just now" => Cow::Borrowed("刚刚"),
        "never" => Cow::Borrowed("从未"),
        "Updated just now" => Cow::Borrowed("刚刚更新"),
        "Never updated" => Cow::Borrowed("从未更新"),
        "Auto-refresh interval" => Cow::Borrowed("自动刷新间隔"),
        "Configure access tokens for providers that require authentication." => {
            Cow::Borrowed("为需要认证的服务商配置访问令牌。")
        }
        "Relative time" => Cow::Borrowed("相对重置时间"),
        "Show reset time as relative (3h 45m) instead of absolute" => {
            Cow::Borrowed("显示重置时间为相对格式（3h 45m）而不是绝对时间")
        }
        "Surprise animations" => Cow::Borrowed("惊喜动画"),
        "Show occasional fun animations in the tray icon" => {
            Cow::Borrowed("在托盘图标中偶尔显示趣味动画")
        }
        "Env" => Cow::Borrowed("环境变量"),
        "UI language" => Cow::Borrowed("界面语言"),
        "Language" => Cow::Borrowed("语言"),
        "Stable" => Cow::Borrowed("稳定版"),
        "Beta" => Cow::Borrowed("测试版"),
        "Never" => Cow::Borrowed("从不"),
        "30 sec" => Cow::Borrowed("30 秒"),
        "Manual" => Cow::Borrowed("手动"),
        "1 min" => Cow::Borrowed("1 分钟"),
        "2 min" => Cow::Borrowed("2 分钟"),
        "5 min" => Cow::Borrowed("5 分钟"),
        "10 min" => Cow::Borrowed("10 分钟"),
        "15 min" => Cow::Borrowed("15 分钟"),
        "Custom" => Cow::Borrowed("自定义"),
        "Provider" => Cow::Borrowed("服务商"),
        "Select..." => Cow::Borrowed("请选择..."),
        "Cookie header" => Cow::Borrowed("Cookie 头"),
        "Paste cookie header from browser dev tools" => {
            Cow::Borrowed("从浏览器开发者工具粘贴 Cookie 头")
        }
        "Save Cookie" => Cow::Borrowed("保存 Cookie"),
        "Cookies are automatically extracted from Chrome, Edge, Brave, and Firefox." => {
            Cow::Borrowed("会自动从 Chrome、Edge、Brave 和 Firefox 提取 Cookie。")
        }
        "✓ Set" => Cow::Borrowed("✓ 已设置"),
        "Needs key" => Cow::Borrowed("需要密钥"),
        "+ Add Key" => Cow::Borrowed("+ 添加密钥"),
        "Get key →" => Cow::Borrowed("获取密钥 →"),
        "Save" => Cow::Borrowed("保存"),
        "Cancel" => Cow::Borrowed("取消"),
        "Enter API Key for" => Cow::Borrowed("输入 API 密钥："),
        "Paste your API key here..." => Cow::Borrowed("在此粘贴 API 密钥..."),
        "Account switched" => Cow::Borrowed("账号已切换"),
        "Account removed" => Cow::Borrowed("账号已移除"),
        "Account added" => Cow::Borrowed("账号已添加"),
        "Add Account" => Cow::Borrowed("添加账号"),
        "+ Add Account" => Cow::Borrowed("+ 添加账号"),
        "Remove" => Cow::Borrowed("移除"),
        "Label" => Cow::Borrowed("标签"),
        "Token" => Cow::Borrowed("令牌"),
        "e.g., Work Account, Personal..." => Cow::Borrowed("例如：工作账号、个人账号..."),
        "Monitor your AI provider usage limits" => Cow::Borrowed("监控你的 AI 服务商用量限制"),
        "Created by CodexBar Contributors" => Cow::Borrowed("由 CodexBar 贡献者创建"),
        "MIT License" => Cow::Borrowed("MIT 许可证"),
        "→ View on GitHub" => Cow::Borrowed("→ 在 GitHub 查看"),
        "→ Report an Issue" => Cow::Borrowed("→ 提交问题"),
        "Commit:" => Cow::Borrowed("提交："),
        "Built:" => Cow::Borrowed("构建时间："),
        "Saved (restart to apply)" => Cow::Borrowed("已保存（重启后生效）"),
        "Invalid shortcut format" => Cow::Borrowed("快捷键格式无效"),
        _ => Cow::Borrowed(text),
    }
}

fn localize_metric_name<'a>(text: &'a str) -> Cow<'a, str> {
    if current_ui_language() != UiLanguage::Chinese {
        return Cow::Borrowed(text);
    }
    match text {
        "Credits" => Cow::Borrowed("额度"),
        _ => localize_text(text),
    }
}

/// Which preferences tab is active
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum PreferencesTab {
    #[default]
    General,
    Providers,
    Display,
    ApiKeys,
    Cookies,
    Advanced,
    About,
}

impl PreferencesTab {
    fn label(&self, language: UiLanguage) -> &'static str {
        match language {
            UiLanguage::English => match self {
                PreferencesTab::General => "General",
                PreferencesTab::Providers => "Providers",
                PreferencesTab::Display => "Display",
                PreferencesTab::ApiKeys => "API Keys",
                PreferencesTab::Cookies => "Cookies",
                PreferencesTab::Advanced => "Advanced",
                PreferencesTab::About => "About",
            },
            UiLanguage::Chinese => match self {
                PreferencesTab::General => "通用",
                PreferencesTab::Providers => "服务商",
                PreferencesTab::Display => "显示",
                PreferencesTab::ApiKeys => "API 密钥",
                PreferencesTab::Cookies => "Cookie",
                PreferencesTab::Advanced => "高级",
                PreferencesTab::About => "关于",
            },
        }
    }

    fn icon(&self) -> &'static str {
        match self {
            PreferencesTab::General => "⚙",
            PreferencesTab::Providers => "☰",
            PreferencesTab::Display => "👁",
            PreferencesTab::ApiKeys => "🔑",
            PreferencesTab::Cookies => "🍪",
            PreferencesTab::Advanced => "⚡",
            PreferencesTab::About => "ℹ",
        }
    }
}

/// Preferences window state
pub struct PreferencesWindow {
    pub is_open: bool,
    pub active_tab: PreferencesTab,
    pub settings: Settings,
    pub settings_changed: bool,
    cookies: ManualCookies,
    new_cookie_provider: String,
    new_cookie_value: String,
    cookie_status_msg: Option<(String, bool)>,
    api_keys: ApiKeys,
    new_api_key_provider: String,
    new_api_key_value: String,
    show_api_key_input: bool,
    api_key_status_msg: Option<(String, bool)>,
    // Selected provider in Providers tab (sidebar selection)
    pub selected_provider: Option<ProviderId>,
    // Browser cookie import state
    selected_browser: Option<BrowserType>,
    browser_import_status: Option<(String, bool)>, // (message, is_error)
    // Icon cache for provider SVG icons
    icon_cache: ProviderIconCache,
    // Shared state for viewport
    shared_state: Arc<Mutex<PreferencesSharedState>>,
    // Place settings viewport beside the main menu window on next show()
    needs_viewport_placement: bool,
}

/// Shared state that can be accessed from viewport
#[derive(Clone)]
struct PreferencesSharedState {
    is_open: bool,
    active_tab: PreferencesTab,
    settings: Settings,
    settings_changed: bool,
    cookies: ManualCookies,
    new_cookie_provider: String,
    new_cookie_value: String,
    cookie_status_msg: Option<(String, bool)>,
    api_keys: ApiKeys,
    new_api_key_provider: String,
    new_api_key_value: String,
    show_api_key_input: bool,
    api_key_status_msg: Option<(String, bool)>,
    selected_provider: Option<ProviderId>,
    selected_browser: Option<BrowserType>,
    browser_import_status: Option<(String, bool)>,
    refresh_requested: bool,
    cached_snapshot: Option<WidgetSnapshot>,
    // Token accounts data
    token_accounts: HashMap<ProviderId, ProviderAccountData>,
    new_account_label: String,
    new_account_token: String,
    show_add_account_input: bool,
    token_account_status_msg: Option<(String, bool)>,
    // Keyboard shortcut editing
    shortcut_input: String,
    shortcut_status_msg: Option<(String, bool)>,
}

impl Default for PreferencesWindow {
    fn default() -> Self {
        let settings = Settings::load();
        let cookies = ManualCookies::load();
        let api_keys = ApiKeys::load();
        let token_accounts = TokenAccountStore::new().load().unwrap_or_default();

        let shared_state = Arc::new(Mutex::new(PreferencesSharedState {
            is_open: false,
            active_tab: PreferencesTab::General,
            settings: settings.clone(),
            settings_changed: false,
            cookies: cookies.clone(),
            new_cookie_provider: String::new(),
            new_cookie_value: String::new(),
            cookie_status_msg: None,
            api_keys: api_keys.clone(),
            new_api_key_provider: String::new(),
            new_api_key_value: String::new(),
            show_api_key_input: false,
            api_key_status_msg: None,
            selected_provider: None,
            selected_browser: None,
            browser_import_status: None,
            refresh_requested: false,
            cached_snapshot: WidgetSnapshotStore::load(),
            token_accounts: token_accounts.clone(),
            new_account_label: String::new(),
            new_account_token: String::new(),
            show_add_account_input: false,
            token_account_status_msg: None,
            shortcut_input: settings.global_shortcut.clone(),
            shortcut_status_msg: None,
        }));

        Self {
            is_open: false,
            active_tab: PreferencesTab::General,
            settings,
            settings_changed: false,
            cookies,
            new_cookie_provider: String::new(),
            new_cookie_value: String::new(),
            cookie_status_msg: None,
            api_keys,
            new_api_key_provider: String::new(),
            new_api_key_value: String::new(),
            show_api_key_input: false,
            api_key_status_msg: None,
            selected_provider: None,
            selected_browser: None,
            browser_import_status: None,
            icon_cache: ProviderIconCache::new(),
            shared_state,
            needs_viewport_placement: false,
        }
    }
}

impl PreferencesWindow {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn open(&mut self) {
        self.is_open = true;
        self.needs_viewport_placement = true;
        self.settings = Settings::load();
        set_current_ui_language(self.settings.ui_language);
        self.cookies = ManualCookies::load();
        self.api_keys = ApiKeys::load();
        self.settings_changed = false;
        self.cookie_status_msg = None;
        self.api_key_status_msg = None;
        self.new_api_key_value.clear();
        self.show_api_key_input = false;

        // Sync to shared state and reload snapshot for fresh data after any background refreshes
        if let Ok(mut state) = self.shared_state.lock() {
            state.is_open = true;
            state.active_tab = self.active_tab;
            state.settings = self.settings.clone();
            state.cookies = self.cookies.clone();
            state.api_keys = self.api_keys.clone();
            state.settings_changed = false;
            state.cached_snapshot = WidgetSnapshotStore::load();
            state.selected_provider = self.selected_provider;
            state.shortcut_input = self.settings.global_shortcut.clone();
            state.shortcut_status_msg = None;
        }
    }

    pub fn close(&mut self) {
        // Flush any unsaved settings through the same atomic path used per-frame.
        if let Some(settings) = self.take_settings_if_changed() {
            let _ = settings.save();
        }

        self.is_open = false;
        if let Ok(mut state) = self.shared_state.lock() {
            state.is_open = false;
        }
        self.needs_viewport_placement = false;
    }

    /// Check if a refresh was requested and reset the flag
    pub fn take_refresh_requested(&mut self) -> bool {
        if let Ok(mut state) = self.shared_state.lock() {
            if state.refresh_requested {
                state.refresh_requested = false;
                return true;
            }
        }
        false
    }

    /// Atomically check if settings changed and return the new settings if so.
    /// Clears the flag in both `PreferencesWindow` and the shared viewport state
    /// so duplicate saves cannot happen across frames.
    pub fn take_settings_if_changed(&mut self) -> Option<Settings> {
        if let Ok(mut state) = self.shared_state.lock() {
            if state.settings_changed {
                state.settings_changed = false;
                self.settings = state.settings.clone();
                self.settings_changed = false;
                return Some(self.settings.clone());
            }
        }
        self.settings_changed = false;
        None
    }

    /// Reload the cached snapshot from disk (call after refresh completes)
    pub fn reload_snapshot(&mut self) {
        if let Ok(mut state) = self.shared_state.lock() {
            state.cached_snapshot = WidgetSnapshotStore::load();
        }
    }

    /// Show the preferences window as a separate native OS window
    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.is_open {
            return;
        }
        set_current_ui_language(self.settings.ui_language);

        let shared_state = Arc::clone(&self.shared_state);
        let settings_viewport_id = egui::ViewportId::from_hash_of("settings_viewport");
        let work_area = work_area_rect(ctx);
        let main_outer_rect = ctx.input(|i| i.viewport().outer_rect);

        let preferred_size = egui::vec2(720.0, 740.0);
        let default_min_size = egui::vec2(520.0, 420.0);
        let margin = 12.0;
        let settings_size = if let Some(area) = work_area {
            let max_w = (area.width() - margin * 2.0).max(360.0);
            let max_h = (area.height() - margin * 2.0).max(360.0);
            egui::vec2(preferred_size.x.min(max_w), preferred_size.y.min(max_h))
        } else {
            preferred_size
        };
        let settings_min_size = egui::vec2(
            default_min_size.x.min(settings_size.x),
            default_min_size.y.min(settings_size.y),
        );
        let settings_position = if self.needs_viewport_placement {
            match (main_outer_rect, work_area) {
                (Some(main_rect), Some(area)) => Some(settings_position_near_main_window(
                    main_rect,
                    settings_size,
                    area,
                )),
                _ => None,
            }
        } else {
            None
        };

        let title = match self.settings.ui_language {
            UiLanguage::English => "CodexBar Settings",
            UiLanguage::Chinese => "CodexBar 设置",
        };

        let mut builder = egui::ViewportBuilder::default()
            .with_title(title)
            .with_inner_size([settings_size.x, settings_size.y])
            .with_min_inner_size([settings_min_size.x, settings_min_size.y])
            .with_clamp_size_to_monitor_size(true)
            .with_resizable(true);
        if let Some(position) = settings_position {
            builder = builder.with_position(position);
        }

        ctx.show_viewport_immediate(settings_viewport_id, builder, |ctx, _class| {
            // Check if window was closed
            if ctx.input(|i| i.viewport().close_requested()) {
                if let Ok(mut state) = shared_state.lock() {
                    state.is_open = false;
                }
            }

            // Apply dark theme
            let mut style = (*ctx.style()).clone();
            style.visuals.window_fill = Theme::BG_PRIMARY;
            style.visuals.panel_fill = Theme::BG_PRIMARY;
            style.visuals.widgets.noninteractive.bg_fill = Theme::BG_SECONDARY;
            style.visuals.widgets.inactive.bg_fill = Theme::CARD_BG;
            style.visuals.widgets.hovered.bg_fill = Theme::CARD_BG_HOVER;
            style.visuals.widgets.active.bg_fill = Theme::ACCENT_PRIMARY;
            ctx.set_style(style);

            egui::CentralPanel::default()
                .frame(
                    egui::Frame::none()
                        .fill(Theme::BG_PRIMARY)
                        .inner_margin(Spacing::MD),
                )
                .show(ctx, |ui| {
                    render_settings_ui(ui, &shared_state);
                });
        });

        if settings_position.is_some() {
            ctx.send_viewport_cmd_to(settings_viewport_id, egui::ViewportCommand::Focus);
            self.needs_viewport_placement = false;
        }

        // Sync state back from shared state
        if let Ok(state) = self.shared_state.lock() {
            self.is_open = state.is_open;
            self.settings = state.settings.clone();
            self.settings_changed = state.settings_changed;
            self.active_tab = state.active_tab;
        }
    }

    fn show_general_tab(&mut self, ui: &mut egui::Ui) {
        // STARTUP section
        section_header(ui, "Startup");

        settings_card(ui, |ui| {
            let mut start_at_login = self.settings.start_at_login;
            if setting_toggle(
                ui,
                "Start at login",
                "Launch CodexBar when you log in",
                &mut start_at_login,
            ) {
                if let Err(e) = self.settings.set_start_at_login(start_at_login) {
                    tracing::error!("Failed to set start at login: {}", e);
                } else {
                    self.settings_changed = true;
                }
            }

            setting_divider(ui);

            let mut start_minimized = self.settings.start_minimized;
            if setting_toggle(
                ui,
                "Start minimized",
                "Start in the system tray",
                &mut start_minimized,
            ) {
                self.settings.start_minimized = start_minimized;
                self.settings_changed = true;
            }
        });

        ui.add_space(Spacing::LG);

        // NOTIFICATIONS section
        section_header(ui, "Notifications");

        settings_card(ui, |ui| {
            let mut show_notifications = self.settings.show_notifications;
            if setting_toggle(
                ui,
                "Show notifications",
                "Alert when usage thresholds are reached",
                &mut show_notifications,
            ) {
                self.settings.show_notifications = show_notifications;
                self.settings_changed = true;
            }

            setting_divider(ui);

            // Sound effects toggle
            let mut sound_enabled = self.settings.sound_enabled;
            if setting_toggle(
                ui,
                "Sound effects",
                "Play sound when thresholds are reached",
                &mut sound_enabled,
            ) {
                self.settings.sound_enabled = sound_enabled;
                self.settings_changed = true;
            }

            // Sound volume slider (only show if sound is enabled)
            if sound_enabled {
                setting_divider(ui);

                ui.vertical(|ui| {
                    let mut volume = self.settings.sound_volume as i32;

                    // Title row with volume badge on right
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(localize_text("Sound volume").as_ref())
                                .size(FontSize::MD)
                                .color(Theme::TEXT_PRIMARY),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            egui::Frame::none()
                                .fill(Theme::ACCENT_PRIMARY.gamma_multiply(0.15))
                                .rounding(Rounding::same(10.0))
                                .inner_margin(egui::Margin::symmetric(10.0, 3.0))
                                .show(ui, |ui| {
                                    ui.label(
                                        RichText::new(format!("{}%", volume))
                                            .size(FontSize::SM)
                                            .color(Theme::ACCENT_PRIMARY)
                                            .strong(),
                                    );
                                });
                        });
                    });

                    ui.add_space(2.0);
                    ui.label(
                        RichText::new(localize_text("Volume level for alert sounds").as_ref())
                            .size(FontSize::SM)
                            .color(Theme::TEXT_MUTED),
                    );
                    ui.add_space(6.0);

                    ui.style_mut().visuals.widgets.inactive.bg_fill = Theme::BG_TERTIARY;
                    ui.style_mut().visuals.widgets.hovered.bg_fill = Theme::CARD_BG_HOVER;
                    ui.style_mut().visuals.widgets.active.bg_fill = Theme::ACCENT_PRIMARY;

                    let slider = ui.add(
                        egui::Slider::new(&mut volume, 0..=100)
                            .show_value(false)
                            .trailing_fill(true),
                    );

                    if slider.changed() {
                        self.settings.sound_volume = volume as u8;
                        self.settings_changed = true;
                    }
                });
            }

            setting_divider(ui);

            // High warning threshold
            ui.vertical(|ui| {
                let mut threshold = self.settings.high_usage_threshold as i32;

                // Title row with percentage badge on right
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(localize_text("High warning").as_ref())
                            .size(FontSize::MD)
                            .color(Theme::TEXT_PRIMARY),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Percentage pill badge
                        egui::Frame::none()
                            .fill(Theme::ACCENT_PRIMARY.gamma_multiply(0.15))
                            .rounding(Rounding::same(10.0))
                            .inner_margin(egui::Margin::symmetric(10.0, 3.0))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(format!("{}%", threshold))
                                        .size(FontSize::SM)
                                        .color(Theme::ACCENT_PRIMARY)
                                        .strong(),
                                );
                            });
                    });
                });

                ui.add_space(2.0);
                ui.label(
                    RichText::new(localize_text("Show warning at this usage level").as_ref())
                        .size(FontSize::SM)
                        .color(Theme::TEXT_MUTED),
                );
                ui.add_space(6.0);

                // Full-width slider
                ui.style_mut().visuals.widgets.inactive.bg_fill = Theme::BG_TERTIARY;
                ui.style_mut().visuals.widgets.hovered.bg_fill = Theme::CARD_BG_HOVER;
                ui.style_mut().visuals.widgets.active.bg_fill = Theme::ACCENT_PRIMARY;

                let slider = ui.add(
                    egui::Slider::new(&mut threshold, 50..=95)
                        .show_value(false)
                        .trailing_fill(true),
                );

                if slider.changed() && threshold as f64 != self.settings.high_usage_threshold {
                    self.settings.high_usage_threshold = threshold as f64;
                    self.settings_changed = true;
                }
            });

            setting_divider(ui);

            // Critical alert threshold
            ui.vertical(|ui| {
                let mut threshold = self.settings.critical_usage_threshold as i32;
                let badge_color = Color32::from_rgb(239, 68, 68); // Red

                // Title row with percentage badge on right
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(localize_text("Critical alert").as_ref())
                            .size(FontSize::MD)
                            .color(Theme::TEXT_PRIMARY),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // Percentage pill badge - red tint for critical
                        egui::Frame::none()
                            .fill(badge_color.gamma_multiply(0.15))
                            .rounding(Rounding::same(10.0))
                            .inner_margin(egui::Margin::symmetric(10.0, 3.0))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(format!("{}%", threshold))
                                        .size(FontSize::SM)
                                        .color(badge_color)
                                        .strong(),
                                );
                            });
                    });
                });

                ui.add_space(2.0);
                ui.label(
                    RichText::new(localize_text("Show critical alert at this level").as_ref())
                        .size(FontSize::SM)
                        .color(Theme::TEXT_MUTED),
                );
                ui.add_space(6.0);

                // Full-width slider
                ui.style_mut().visuals.widgets.inactive.bg_fill = Theme::BG_TERTIARY;
                ui.style_mut().visuals.widgets.hovered.bg_fill = Theme::CARD_BG_HOVER;
                ui.style_mut().visuals.widgets.active.bg_fill = badge_color;

                let slider = ui.add(
                    egui::Slider::new(&mut threshold, 80..=100)
                        .show_value(false)
                        .trailing_fill(true),
                );

                if slider.changed() && threshold as f64 != self.settings.critical_usage_threshold {
                    self.settings.critical_usage_threshold = threshold as f64;
                    self.settings_changed = true;
                }
            });
        });
    }

    fn show_providers_tab(&mut self, ui: &mut egui::Ui, available_height: f32) {
        let providers = ProviderId::all();

        // Ensure a provider is selected
        if self.selected_provider.is_none() && !providers.is_empty() {
            self.selected_provider = Some(providers[0].clone());
        }

        // Calculate dimensions - responsive sidebar width
        let total_width = ui.available_width();
        let sidebar_width = (total_width * 0.45).min(180.0).max(140.0); // 45% of width, 140-180px range
        let gap = Spacing::SM;
        let detail_width = (total_width - sidebar_width - gap).max(150.0);
        let panel_height = available_height;

        // Side-by-side layout with explicit sizes
        ui.allocate_ui_with_layout(
            Vec2::new(total_width, panel_height),
            egui::Layout::left_to_right(egui::Align::TOP),
            |ui| {
                // LEFT SIDEBAR (fixed 200px)
                ui.allocate_ui_with_layout(
                    Vec2::new(sidebar_width, panel_height),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        self.draw_provider_sidebar(ui, providers, panel_height);
                    },
                );

                ui.add_space(gap);

                // RIGHT DETAIL PANEL (fills remaining)
                ui.allocate_ui_with_layout(
                    Vec2::new(detail_width, panel_height),
                    egui::Layout::top_down(egui::Align::LEFT),
                    |ui| {
                        self.draw_provider_detail(ui, panel_height);
                    },
                );
            },
        );
    }

    fn draw_provider_sidebar(
        &mut self,
        ui: &mut egui::Ui,
        providers: &[ProviderId],
        available_height: f32,
    ) {
        egui::Frame::none()
            .fill(Theme::BG_TERTIARY)
            .rounding(Rounding::same(Radius::MD))
            .inner_margin(Spacing::SM)
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("provider_sidebar")
                    .max_height(available_height - Spacing::SM * 2.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.style_mut().spacing.item_spacing.y = 8.0;

                        for provider_id in providers {
                            let provider_name = provider_id.cli_name();
                            let is_selected = self.selected_provider.as_ref() == Some(provider_id);
                            let is_enabled =
                                self.settings.enabled_providers.contains(provider_name);

                            // Add padding around each row
                            let frame_response = egui::Frame::none()
                                .inner_margin(egui::Margin::symmetric(8.0, 8.0))
                                .rounding(Rounding::same(Radius::MD))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        // Make the row fill available width
                                        ui.set_min_width(ui.available_width());
                                        ui.set_min_height(32.0);

                                        // Icon
                                        let icon_size = 20.0;
                                        if let Some(texture) = self.icon_cache.get_icon(
                                            ui.ctx(),
                                            provider_name,
                                            icon_size as u32,
                                        ) {
                                            ui.add(
                                                egui::Image::new(texture)
                                                    .fit_to_exact_size(Vec2::splat(icon_size)),
                                            );
                                        } else {
                                            ui.label(
                                                RichText::new(provider_icon(provider_name))
                                                    .size(FontSize::MD)
                                                    .color(provider_color(provider_name)),
                                            );
                                        }

                                        ui.add_space(8.0);

                                        // Provider name as plain label (no hover effect)
                                        let text_color = if is_selected {
                                            Theme::TEXT_PRIMARY
                                        } else if is_enabled {
                                            Theme::TEXT_SECONDARY
                                        } else {
                                            Theme::TEXT_MUTED
                                        };

                                        ui.label(
                                            RichText::new(provider_id.display_name())
                                                .size(FontSize::SM)
                                                .color(text_color),
                                        );

                                        // Spacer to push checkbox to right
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                // Checkbox
                                                let mut enabled = is_enabled;
                                                if ui.checkbox(&mut enabled, "").changed() {
                                                    if enabled {
                                                        self.settings
                                                            .enabled_providers
                                                            .insert(provider_name.to_string());
                                                    } else {
                                                        self.settings
                                                            .enabled_providers
                                                            .remove(provider_name);
                                                    }
                                                    self.settings_changed = true;
                                                }
                                            },
                                        );
                                    });
                                });

                            // Check hover and click on the frame
                            let frame_rect = frame_response.response.rect;
                            let row_response = ui.interact(
                                frame_rect,
                                ui.make_persistent_id(format!("row_{}", provider_name)),
                                egui::Sense::click(),
                            );
                            let is_hovered = row_response.hovered();

                            if row_response.clicked() {
                                self.selected_provider = Some(provider_id.clone());
                            }

                            // Draw the selection/hover ring on top
                            if is_selected || is_hovered {
                                let fill = Theme::ACCENT_PRIMARY.gamma_multiply(0.15);
                                let stroke = if is_selected {
                                    Stroke::new(1.5, Theme::ACCENT_PRIMARY)
                                } else {
                                    Stroke::new(1.0, Theme::ACCENT_PRIMARY.gamma_multiply(0.6))
                                };
                                ui.painter().rect(frame_rect, Radius::MD, fill, stroke);
                            }
                        }
                    });
            });
    }

    fn draw_provider_detail(&mut self, ui: &mut egui::Ui, available_height: f32) {
        if let Some(ref selected_id) = self.selected_provider.clone() {
            egui::Frame::none()
                .fill(Theme::BG_SECONDARY)
                .rounding(Rounding::same(Radius::LG))
                .inner_margin(Spacing::MD)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("provider_detail")
                        .max_height(available_height - Spacing::MD * 2.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.set_min_width(ui.available_width());
                            self.draw_provider_detail_panel(ui, &selected_id);
                        });
                });
        } else {
            // Placeholder
            egui::Frame::none()
                .fill(Theme::BG_SECONDARY)
                .rounding(Rounding::same(Radius::LG))
                .inner_margin(Spacing::MD)
                .show(ui, |ui| {
                    ui.set_min_height(available_height);
                    ui.vertical_centered(|ui| {
                        ui.add_space(available_height / 3.0);
                        ui.label(
                            RichText::new("Select a provider")
                                .size(FontSize::MD)
                                .color(Theme::TEXT_MUTED),
                        );
                    });
                });
        }
    }

    fn draw_provider_detail_panel(&mut self, ui: &mut egui::Ui, provider_id: &ProviderId) {
        let provider_name = provider_id.cli_name();
        let display_name = provider_id.display_name();
        let is_enabled = self.settings.enabled_providers.contains(provider_name);
        let color = provider_color(provider_name);

        // ═══════════════════════════════════════════════════════════
        // HEADER - Icon, name, enable toggle
        // ═══════════════════════════════════════════════════════════
        ui.horizontal(|ui| {
            // Large brand icon with background
            egui::Frame::none()
                .fill(color.gamma_multiply(0.15))
                .rounding(Rounding::same(Radius::MD))
                .inner_margin(Spacing::SM)
                .show(ui, |ui| {
                    let icon_size = 32.0;
                    if let Some(texture) =
                        self.icon_cache
                            .get_icon(ui.ctx(), provider_name, icon_size as u32)
                    {
                        ui.add(egui::Image::new(texture).fit_to_exact_size(Vec2::splat(icon_size)));
                    } else {
                        ui.label(
                            RichText::new(provider_icon(provider_name))
                                .size(icon_size)
                                .color(color),
                        );
                    }
                });

            ui.add_space(Spacing::SM);

            ui.vertical(|ui| {
                ui.label(
                    RichText::new(display_name)
                        .size(FontSize::XL)
                        .color(Theme::TEXT_PRIMARY)
                        .strong(),
                );
                ui.horizontal(|ui| {
                    // Status indicator dot
                    let status_color = if is_enabled {
                        Theme::GREEN
                    } else {
                        Theme::TEXT_MUTED
                    };
                    ui.label(RichText::new("●").size(FontSize::XS).color(status_color));
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(if is_enabled { "Enabled" } else { "Disabled" })
                            .size(FontSize::SM)
                            .color(status_color),
                    );
                });
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Enable/disable toggle styled as a switch
                let mut enabled = is_enabled;
                if ui.checkbox(&mut enabled, "").changed() {
                    if enabled {
                        self.settings
                            .enabled_providers
                            .insert(provider_name.to_string());
                    } else {
                        self.settings.enabled_providers.remove(provider_name);
                    }
                    self.settings_changed = true;
                }
            });
        });

        ui.add_space(Spacing::LG);

        // ═══════════════════════════════════════════════════════════
        // INFO SECTION - Provider-specific information
        // ═══════════════════════════════════════════════════════════
        section_header(ui, "Info");

        settings_card(ui, |ui| {
            // Authentication type
            let auth_type = match provider_name {
                "openai" | "gemini" | "openrouter" => "API Key",
                "claude" | "cursor" | "kimi" => "Browser Session",
                "ollama" => "Local (No Auth)",
                "windsurf" => "Browser Session",
                _ => "Browser Session",
            };
            self.draw_info_row(ui, "Authentication", auth_type);
            setting_divider(ui);

            // Data source
            let data_source = match provider_name {
                "openai" => "OpenAI API Usage Dashboard",
                "gemini" => "Google AI Studio",
                "claude" => "Anthropic Web Console",
                "cursor" => "Cursor Settings API",
                "ollama" => "Local Ollama Server",
                "openrouter" => "OpenRouter Dashboard",
                "windsurf" => "Windsurf API",
                "kimi" => "Kimi Web Console",
                _ => "Provider API",
            };
            self.draw_info_row(ui, "Data Source", data_source);
            setting_divider(ui);

            // Rate limit info
            let rate_info = match provider_name {
                "claude" => "Daily message limit",
                "cursor" => "Monthly request limit",
                "openai" => "Token usage & credits",
                "gemini" => "Requests per minute",
                _ => "Usage tracking",
            };
            self.draw_info_row(ui, "Tracks", rate_info);
        });

        ui.add_space(Spacing::LG);

        // ═══════════════════════════════════════════════════════════
        // USAGE SECTION - Link to main window
        // ═══════════════════════════════════════════════════════════
        section_header(ui, "Usage");

        settings_card(ui, |ui| {
            if is_enabled {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("📊").size(FontSize::LG));
                    ui.add_space(Spacing::SM);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("Live usage data in main window")
                                .size(FontSize::MD)
                                .color(Theme::TEXT_PRIMARY),
                        );
                        ui.label(
                            RichText::new("Click the tray icon to view real-time metrics")
                                .size(FontSize::SM)
                                .color(Theme::TEXT_MUTED),
                        );
                    });
                });
            } else {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("⏸")
                            .size(FontSize::LG)
                            .color(Theme::TEXT_MUTED),
                    );
                    ui.add_space(Spacing::SM);
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("Provider disabled")
                                .size(FontSize::MD)
                                .color(Theme::TEXT_MUTED),
                        );
                        ui.label(
                            RichText::new("Enable to start tracking usage")
                                .size(FontSize::SM)
                                .color(Theme::TEXT_DIM),
                        );
                    });
                });
            }
        });

        ui.add_space(Spacing::LG);

        // ═══════════════════════════════════════════════════════════
        // BROWSER COOKIE IMPORT - Only for cookie-based providers
        // ═══════════════════════════════════════════════════════════
        if provider_id.cookie_domain().is_some() {
            self.draw_browser_cookie_import(ui, provider_id);
            ui.add_space(Spacing::LG);
        }

        // ═══════════════════════════════════════════════════════════
        // QUICK ACTIONS
        // ═══════════════════════════════════════════════════════════
        section_header(ui, "Quick Actions");

        settings_card(ui, |ui| {
            // Provider-specific quick actions
            match provider_name {
                "openai" => {
                    if text_button(ui, "→ Open OpenAI Dashboard", Theme::ACCENT_PRIMARY) {
                        let _ = open::that("https://platform.openai.com/usage");
                    }
                }
                "claude" => {
                    if text_button(ui, "→ Open Claude Console", Theme::ACCENT_PRIMARY) {
                        let _ = open::that("https://console.anthropic.com/");
                    }
                }
                "gemini" => {
                    if text_button(ui, "→ Open Google AI Studio", Theme::ACCENT_PRIMARY) {
                        let _ = open::that("https://aistudio.google.com/");
                    }
                }
                "cursor" => {
                    if text_button(ui, "→ Open Cursor Settings", Theme::ACCENT_PRIMARY) {
                        let _ = open::that("https://www.cursor.com/settings");
                    }
                }
                "ollama" => {
                    ui.label(
                        RichText::new("Ollama runs locally - no dashboard")
                            .size(FontSize::SM)
                            .color(Theme::TEXT_MUTED),
                    );
                }
                _ => {
                    ui.label(
                        RichText::new("No quick actions available")
                            .size(FontSize::SM)
                            .color(Theme::TEXT_MUTED),
                    );
                }
            }
        });
    }

    fn draw_info_row(&self, ui: &mut egui::Ui, label: &str, value: &str) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(label)
                    .size(FontSize::SM)
                    .color(Theme::TEXT_MUTED),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(value)
                        .size(FontSize::SM)
                        .color(Theme::TEXT_SECONDARY),
                );
            });
        });
    }

    fn draw_browser_cookie_import(&mut self, ui: &mut egui::Ui, provider_id: &ProviderId) {
        section_header(ui, "Browser Cookie Import");

        settings_card(ui, |ui| {
            let domain = provider_id.cookie_domain().unwrap_or("unknown");

            ui.label(
                RichText::new(format!("Import cookies from your browser for {}", domain))
                    .size(FontSize::SM)
                    .color(Theme::TEXT_MUTED),
            );

            ui.add_space(Spacing::SM);

            // Detect available browsers
            let browsers = BrowserDetector::detect_all();

            if browsers.is_empty() {
                ui.label(
                    RichText::new("No supported browsers detected")
                        .size(FontSize::SM)
                        .color(Theme::YELLOW),
                );
            } else {
                // Browser selection dropdown
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Browser")
                            .size(FontSize::MD)
                            .color(Theme::TEXT_PRIMARY),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let selected_text = self
                            .selected_browser
                            .map(|b| b.display_name())
                            .unwrap_or("Select browser...");

                        egui::ComboBox::from_id_salt("browser_select")
                            .selected_text(selected_text)
                            .show_ui(ui, |ui| {
                                for browser in &browsers {
                                    let browser_type = browser.browser_type;
                                    if ui
                                        .selectable_label(
                                            self.selected_browser == Some(browser_type),
                                            browser_type.display_name(),
                                        )
                                        .clicked()
                                    {
                                        self.selected_browser = Some(browser_type);
                                        self.browser_import_status = None;
                                    }
                                }
                            });
                    });
                });

                ui.add_space(Spacing::MD);

                // Import button
                let can_import = self.selected_browser.is_some();
                if ui
                    .add_enabled(
                        can_import,
                        egui::Button::new(
                            RichText::new("Import Cookies").size(FontSize::SM).color(
                                if can_import {
                                    Color32::WHITE
                                } else {
                                    Theme::TEXT_MUTED
                                },
                            ),
                        )
                        .fill(if can_import {
                            Theme::ACCENT_PRIMARY
                        } else {
                            Theme::BG_TERTIARY
                        })
                        .rounding(Rounding::same(Radius::MD))
                        .min_size(Vec2::new(120.0, 36.0)),
                    )
                    .clicked()
                {
                    // Attempt to import cookies from selected browser
                    if let Some(browser_type) = self.selected_browser {
                        // Find the detected browser matching the selected type
                        let browsers = BrowserDetector::detect_all();
                        if let Some(browser) =
                            browsers.iter().find(|b| b.browser_type == browser_type)
                        {
                            match get_cookie_header_from_browser(domain, browser) {
                                Ok(cookie_header) if !cookie_header.is_empty() => {
                                    // Save the cookie
                                    self.cookies.set(provider_id.cli_name(), &cookie_header);
                                    if let Err(e) = self.cookies.save() {
                                        self.browser_import_status =
                                            Some((format!("Failed to save: {}", e), true));
                                    } else {
                                        self.browser_import_status = Some((
                                            format!(
                                                "Cookies imported for {}",
                                                provider_id.display_name()
                                            ),
                                            false,
                                        ));
                                    }
                                }
                                Ok(_) => {
                                    self.browser_import_status = Some((
                                        format!(
                                            "No cookies found for {} in {}. Make sure you're logged in.",
                                            domain,
                                            browser_type.display_name()
                                        ),
                                        true,
                                    ));
                                }
                                Err(e) => {
                                    self.browser_import_status =
                                        Some((format!("Import failed: {}", e), true));
                                }
                            }
                        } else {
                            self.browser_import_status = Some((
                                format!("Browser {} not found", browser_type.display_name()),
                                true,
                            ));
                        }
                    }
                }

                // Status message
                if let Some((msg, is_error)) = &self.browser_import_status {
                    ui.add_space(Spacing::SM);
                    let color = if *is_error { Theme::RED } else { Theme::GREEN };
                    ui.label(RichText::new(msg).size(FontSize::SM).color(color));
                }
            }
        });
    }

    fn show_display_tab(&mut self, ui: &mut egui::Ui) {
        section_header(ui, "Usage Display");

        settings_card(ui, |ui| {
            let mut show_as_used = self.settings.show_as_used;
            if setting_toggle(
                ui,
                "Show usage as used",
                "Show usage as percentage used (vs remaining)",
                &mut show_as_used,
            ) {
                self.settings.show_as_used = show_as_used;
                self.settings_changed = true;
            }

            setting_divider(ui);

            let mut reset_time_relative = self.settings.reset_time_relative;
            if setting_toggle(
                ui,
                "Relative reset times",
                "Show '2h 30m' instead of '3:00 PM'",
                &mut reset_time_relative,
            ) {
                self.settings.reset_time_relative = reset_time_relative;
                self.settings_changed = true;
            }

            setting_divider(ui);

            let mut show_credits_extra = self.settings.show_credits_extra_usage;
            if setting_toggle(
                ui,
                "Show credits + extra usage",
                "Display credit balance and extra usage information",
                &mut show_credits_extra,
            ) {
                self.settings.show_credits_extra_usage = show_credits_extra;
                self.settings_changed = true;
            }
        });

        ui.add_space(Spacing::SM);

        section_header(ui, "Tray Icon");

        settings_card(ui, |ui| {
            let mut merge_icons = self.settings.merge_tray_icons;
            if setting_toggle(
                ui,
                "Merge tray icons",
                "Show all providers in a single tray icon",
                &mut merge_icons,
            ) {
                self.settings.merge_tray_icons = merge_icons;
                self.settings_changed = true;
            }

            setting_divider(ui);

            let mut per_provider = self.settings.tray_icon_mode == TrayIconMode::PerProvider;
            if setting_toggle(
                ui,
                "Per-provider icons",
                "Show a separate tray icon for each enabled provider",
                &mut per_provider,
            ) {
                self.settings.tray_icon_mode = if per_provider {
                    TrayIconMode::PerProvider
                } else {
                    TrayIconMode::Single
                };
                self.settings_changed = true;
            }
        });
    }

    fn show_api_keys_tab(&mut self, ui: &mut egui::Ui) {
        section_header(ui, "API Keys");

        ui.label(
            RichText::new("Configure access tokens for providers that require authentication.")
                .size(FontSize::SM)
                .color(Theme::TEXT_MUTED),
        );

        ui.add_space(Spacing::MD);

        // Status message
        if let Some((msg, is_error)) = &self.api_key_status_msg {
            status_message(ui, msg, *is_error);
            ui.add_space(Spacing::SM);
        }

        // Provider cards - one per provider
        let api_key_providers = get_api_key_providers();

        for provider_info in &api_key_providers {
            let provider_id = provider_info.id.cli_name();
            let has_key = self.api_keys.has_key(provider_id);
            let is_enabled = self.settings.enabled_providers.contains(provider_id);
            let icon = provider_icon(provider_id);
            let color = provider_color(provider_id);

            // Card with left accent bar
            let accent_color = if has_key {
                Theme::GREEN
            } else if is_enabled {
                Theme::ORANGE
            } else {
                Theme::BG_TERTIARY
            };

            egui::Frame::none()
                .fill(Theme::BG_SECONDARY)
                .rounding(Rounding::same(Radius::MD))
                .inner_margin(egui::Margin::same(0.0))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        // Left accent bar - reduced height for compact layout
                        let bar_rect = Rect::from_min_size(ui.cursor().min, Vec2::new(3.0, 48.0));
                        ui.painter().rect_filled(
                            bar_rect,
                            Rounding {
                                nw: Radius::MD,
                                sw: Radius::MD,
                                ne: 0.0,
                                se: 0.0,
                            },
                            accent_color,
                        );
                        ui.add_space(3.0);

                        // Content - reduced padding for compact layout
                        ui.vertical(|ui| {
                            ui.add_space(Spacing::XS);

                            // Row 1: Icon, Name, Status badge, and Add Key button (right-aligned)
                            ui.horizontal(|ui| {
                                ui.add_space(Spacing::XS);
                                ui.label(RichText::new(icon).size(FontSize::LG).color(color));
                                ui.add_space(Spacing::XS);
                                ui.label(
                                    RichText::new(provider_info.name)
                                        .size(FontSize::MD)
                                        .color(Theme::TEXT_PRIMARY)
                                        .strong(),
                                );

                                ui.add_space(Spacing::XS);

                                if has_key {
                                    badge(ui, "✓ Set", Theme::GREEN);
                                } else if is_enabled {
                                    // Smaller pill-shaped badge with solid orange background
                                    egui::Frame::none()
                                        .fill(Theme::ORANGE)
                                        .rounding(Rounding::same(Radius::PILL))
                                        .inner_margin(egui::Margin::symmetric(Spacing::XS, 2.0))
                                        .show(ui, |ui| {
                                            ui.label(
                                                RichText::new("Needs key")
                                                    .size(FontSize::XS)
                                                    .color(Color32::BLACK),
                                            );
                                        });
                                }

                                // Right-aligned: Add Key button for providers without keys
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.add_space(Spacing::XS);
                                        if !has_key {
                                            if primary_button(ui, "+ Add Key") {
                                                self.new_api_key_provider = provider_id.to_string();
                                                self.show_api_key_input = true;
                                                self.new_api_key_value.clear();
                                            }
                                        }
                                    },
                                );
                            });

                            // Row 2: Single line with env var, masked key, and actions
                            ui.horizontal(|ui| {
                                ui.add_space(Spacing::XS);

                                // Env var info
                                if let Some(env_var) = provider_info.api_key_env_var {
                                    ui.label(
                                        RichText::new(format!(
                                            "{}: {}",
                                            localize_text("Env"),
                                            env_var
                                        ))
                                        .size(FontSize::XS)
                                        .color(Theme::TEXT_MUTED)
                                        .monospace(),
                                    );
                                }

                                if has_key {
                                    ui.add_space(Spacing::SM);
                                    // Show masked key inline
                                    if let Some(key_info) = self
                                        .api_keys
                                        .get_all_for_display()
                                        .iter()
                                        .find(|k| k.provider_id == provider_id)
                                    {
                                        ui.label(
                                            RichText::new(&key_info.masked_key)
                                                .size(FontSize::XS)
                                                .color(Theme::TEXT_MUTED)
                                                .monospace(),
                                        );
                                    }

                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.add_space(Spacing::XS);
                                            if small_button(ui, "Remove", Theme::RED) {
                                                self.api_keys.remove(provider_id);
                                                let _ = self.api_keys.save();
                                                self.api_key_status_msg = Some((
                                                    format!(
                                                        "Removed API key for {}",
                                                        provider_info.name
                                                    ),
                                                    false,
                                                ));
                                            }
                                        },
                                    );
                                } else {
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.add_space(Spacing::XS);
                                            if let Some(url) = provider_info.dashboard_url {
                                                if text_button(
                                                    ui,
                                                    "Get key →",
                                                    Theme::ACCENT_PRIMARY,
                                                ) {
                                                    let _ = open::that(url);
                                                }
                                            }
                                        },
                                    );
                                }
                            });

                            ui.add_space(Spacing::XS);
                        });
                    });
                });

            ui.add_space(Spacing::XS);
        }

        // API Key input modal
        if self.show_api_key_input {
            ui.add_space(Spacing::MD);

            let provider_name = ProviderId::from_cli_name(&self.new_api_key_provider)
                .map(|id| id.display_name())
                .unwrap_or(&self.new_api_key_provider);

            egui::Frame::none()
                .fill(Theme::BG_TERTIARY)
                .stroke(Stroke::new(1.0, Theme::ACCENT_PRIMARY.gamma_multiply(0.4)))
                .rounding(Rounding::same(Radius::LG))
                .inner_margin(Spacing::LG)
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(format!("Enter API Key for {}", provider_name))
                            .size(FontSize::MD)
                            .color(Theme::TEXT_PRIMARY)
                            .strong(),
                    );

                    ui.add_space(Spacing::SM);

                    let text_edit = egui::TextEdit::singleline(&mut self.new_api_key_value)
                        .password(true)
                        .desired_width(ui.available_width())
                        .hint_text("Paste your API key here...");
                    ui.add(text_edit);

                    ui.add_space(Spacing::MD);

                    ui.horizontal(|ui| {
                        let can_save = !self.new_api_key_value.trim().is_empty();

                        if ui
                            .add_enabled(
                                can_save,
                                egui::Button::new(
                                    RichText::new("Save")
                                        .size(FontSize::SM)
                                        .color(Color32::WHITE),
                                )
                                .fill(if can_save {
                                    Theme::GREEN
                                } else {
                                    Theme::BG_TERTIARY
                                })
                                .rounding(Rounding::same(Radius::SM))
                                .min_size(Vec2::new(80.0, 32.0)),
                            )
                            .clicked()
                        {
                            self.api_keys.set(
                                &self.new_api_key_provider,
                                self.new_api_key_value.trim(),
                                None,
                            );
                            if let Err(e) = self.api_keys.save() {
                                self.api_key_status_msg =
                                    Some((format!("Failed to save: {}", e), true));
                            } else {
                                self.api_key_status_msg =
                                    Some((format!("API key saved for {}", provider_name), false));
                                self.show_api_key_input = false;
                                self.new_api_key_value.clear();
                            }
                        }

                        ui.add_space(Spacing::XS);

                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Cancel")
                                        .size(FontSize::SM)
                                        .color(Theme::TEXT_MUTED),
                                )
                                .fill(Color32::TRANSPARENT)
                                .stroke(Stroke::new(1.0, Theme::BORDER_SUBTLE))
                                .rounding(Rounding::same(Radius::SM)),
                            )
                            .clicked()
                        {
                            self.show_api_key_input = false;
                            self.new_api_key_value.clear();
                        }
                    });
                });
        }
    }

    fn show_cookies_tab(&mut self, ui: &mut egui::Ui) {
        section_header(ui, "Browser Cookies");

        ui.label(
            RichText::new(
                "Cookies are automatically extracted from Chrome, Edge, Brave, and Firefox.",
            )
            .size(FontSize::SM)
            .color(Theme::TEXT_MUTED),
        );

        ui.add_space(Spacing::LG);

        // Status message
        if let Some((msg, is_error)) = &self.cookie_status_msg {
            status_message(ui, msg, *is_error);
            ui.add_space(Spacing::MD);
        }

        // Saved cookies
        let saved_cookies = self.cookies.get_all_for_display();

        if !saved_cookies.is_empty() {
            section_header(ui, "Saved Cookies");

            settings_card(ui, |ui| {
                let mut to_remove: Option<String> = None;
                let len = saved_cookies.len();

                for (i, cookie_info) in saved_cookies.iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(&cookie_info.provider)
                                .size(FontSize::MD)
                                .color(Theme::TEXT_PRIMARY),
                        );
                        ui.label(
                            RichText::new(format!("· {}", &cookie_info.saved_at))
                                .size(FontSize::SM)
                                .color(Theme::TEXT_MUTED),
                        );

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if small_button(ui, "Remove", Theme::RED) {
                                to_remove = Some(cookie_info.provider_id.clone());
                            }
                        });
                    });

                    if i < len - 1 {
                        setting_divider(ui);
                    }
                }

                if let Some(provider_id) = to_remove {
                    self.cookies.remove(&provider_id);
                    let _ = self.cookies.save();
                    self.cookie_status_msg =
                        Some((format!("Removed cookie for {}", provider_id), false));
                }
            });

            ui.add_space(Spacing::XL);
        }

        // Add manual cookie
        section_header(ui, "Add Manual Cookie");

        settings_card(ui, |ui| {
            // Provider selection row
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Provider")
                        .size(FontSize::MD)
                        .color(Theme::TEXT_PRIMARY),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    egui::ComboBox::from_id_salt("cookie_provider")
                        .selected_text(if self.new_cookie_provider.is_empty() {
                            "Select..."
                        } else {
                            &self.new_cookie_provider
                        })
                        .show_ui(ui, |ui| {
                            let web_providers = ["claude", "cursor", "kimi"];
                            for provider_name in web_providers {
                                if let Some(id) = ProviderId::from_cli_name(provider_name) {
                                    if ui
                                        .selectable_label(
                                            self.new_cookie_provider == provider_name,
                                            id.display_name(),
                                        )
                                        .clicked()
                                    {
                                        self.new_cookie_provider = provider_name.to_string();
                                    }
                                }
                            }
                        });
                });
            });

            ui.add_space(Spacing::MD);
            setting_divider(ui);
            ui.add_space(Spacing::SM);

            // Cookie header label
            ui.label(
                RichText::new("Cookie header")
                    .size(FontSize::MD)
                    .color(Theme::TEXT_PRIMARY),
            );
            ui.add_space(Spacing::SM);

            // Styled text input with visible border and rounded corners
            egui::Frame::none()
                .fill(Theme::INPUT_BG)
                .stroke(Stroke::new(1.0, Theme::BORDER_SUBTLE))
                .rounding(Rounding::same(Radius::SM))
                .inner_margin(Spacing::SM)
                .show(ui, |ui| {
                    let text_edit = egui::TextEdit::multiline(&mut self.new_cookie_value)
                        .desired_width(ui.available_width())
                        .desired_rows(4)
                        .frame(false)
                        .hint_text("Paste cookie header from browser dev tools");
                    ui.add(text_edit);
                });

            ui.add_space(Spacing::LG);

            // Save button - filled primary style with proper sizing
            let can_save =
                !self.new_cookie_provider.is_empty() && !self.new_cookie_value.is_empty();

            if ui
                .add_enabled(
                    can_save,
                    egui::Button::new(RichText::new("Save Cookie").size(FontSize::SM).color(
                        if can_save {
                            Color32::WHITE
                        } else {
                            Theme::TEXT_MUTED
                        },
                    ))
                    .fill(if can_save {
                        Theme::ACCENT_PRIMARY
                    } else {
                        Theme::BG_TERTIARY
                    })
                    .stroke(if can_save {
                        Stroke::NONE
                    } else {
                        Stroke::new(1.0, Theme::BORDER_SUBTLE)
                    })
                    .rounding(Rounding::same(Radius::MD))
                    .min_size(Vec2::new(120.0, 36.0)),
                )
                .clicked()
            {
                self.cookies
                    .set(&self.new_cookie_provider, &self.new_cookie_value);
                if let Err(e) = self.cookies.save() {
                    self.cookie_status_msg = Some((format!("Failed to save: {}", e), true));
                } else {
                    let provider_name = ProviderId::from_cli_name(&self.new_cookie_provider)
                        .map(|id| id.display_name().to_string())
                        .unwrap_or_else(|| self.new_cookie_provider.clone());
                    self.cookie_status_msg =
                        Some((format!("Cookie saved for {}", provider_name), false));
                    self.new_cookie_provider.clear();
                    self.new_cookie_value.clear();
                }
            }
        });
    }

    fn show_advanced_tab(&mut self, ui: &mut egui::Ui) {
        section_header(ui, "Refresh");

        settings_card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new(localize_text("Auto-refresh interval").as_ref())
                            .size(FontSize::MD)
                            .color(Theme::TEXT_PRIMARY),
                    );
                    ui.label(
                        RichText::new("How often to fetch usage data")
                            .size(FontSize::SM)
                            .color(Theme::TEXT_MUTED),
                    );
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let intervals = [
                        (0, "Manual"),
                        (60, "1 min"),
                        (120, "2 min"),
                        (300, "5 min"),
                        (600, "10 min"),
                        (900, "15 min"),
                    ];

                    egui::Frame::none()
                        .fill(Theme::BG_TERTIARY)
                        .stroke(Stroke::new(1.0, Theme::BORDER_SUBTLE))
                        .rounding(Rounding::same(Radius::SM))
                        .inner_margin(egui::Margin::symmetric(Spacing::XS, 2.0))
                        .show(ui, |ui| {
                            egui::ComboBox::from_id_salt("refresh_interval")
                                .selected_text(
                                    intervals
                                        .iter()
                                        .find(|(secs, _)| {
                                            *secs == self.settings.refresh_interval_secs
                                        })
                                        .map(|(_, label)| *label)
                                        .unwrap_or("5 min"),
                                )
                                .show_ui(ui, |ui| {
                                    for (secs, label) in intervals {
                                        if ui
                                            .selectable_value(
                                                &mut self.settings.refresh_interval_secs,
                                                secs,
                                                label,
                                            )
                                            .changed()
                                        {
                                            self.settings_changed = true;
                                        }
                                    }
                                });
                        });
                });
            });
        });

        ui.add_space(Spacing::SM);

        section_header(ui, "Animations");

        settings_card(ui, |ui| {
            let mut enable_animations = self.settings.enable_animations;
            if setting_toggle(
                ui,
                "Enable animations",
                "Animate charts and UI transitions",
                &mut enable_animations,
            ) {
                self.settings.enable_animations = enable_animations;
                self.settings_changed = true;
            }
        });

        ui.add_space(Spacing::SM);

        section_header(ui, "Menu Bar");

        settings_card(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(
                        RichText::new("Display mode")
                            .size(FontSize::MD)
                            .color(Theme::TEXT_PRIMARY),
                    );
                    ui.label(
                        RichText::new("How much detail to show in menu bar")
                            .size(FontSize::SM)
                            .color(Theme::TEXT_MUTED),
                    );
                });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let display_modes = [
                        ("minimal", "Minimal"),
                        ("compact", "Compact"),
                        ("detailed", "Detailed"),
                    ];

                    egui::Frame::none()
                        .fill(Theme::BG_TERTIARY)
                        .stroke(Stroke::new(1.0, Theme::BORDER_SUBTLE))
                        .rounding(Rounding::same(Radius::SM))
                        .inner_margin(egui::Margin::symmetric(Spacing::XS, 2.0))
                        .show(ui, |ui| {
                            egui::ComboBox::from_id_salt("display_mode")
                                .selected_text(
                                    display_modes
                                        .iter()
                                        .find(|(val, _)| {
                                            *val == self.settings.menu_bar_display_mode
                                        })
                                        .map(|(_, label)| *label)
                                        .unwrap_or("Detailed"),
                                )
                                .show_ui(ui, |ui| {
                                    for (value, label) in display_modes {
                                        if ui
                                            .selectable_value(
                                                &mut self.settings.menu_bar_display_mode,
                                                value.to_string(),
                                                label,
                                            )
                                            .changed()
                                        {
                                            self.settings_changed = true;
                                        }
                                    }
                                });
                        });
                });
            });
        });

        ui.add_space(Spacing::SM);

        section_header(ui, "Fun");

        settings_card(ui, |ui| {
            let mut surprise = self.settings.surprise_animations;
            if setting_toggle(
                ui,
                "Surprise me",
                "Random animations on tray icon",
                &mut surprise,
            ) {
                self.settings.surprise_animations = surprise;
                self.settings_changed = true;
            }
        });
    }

    fn show_about_tab(&mut self, ui: &mut egui::Ui) {
        ui.add_space(Spacing::XL);

        // App branding
        ui.vertical_centered(|ui| {
            // Logo placeholder
            egui::Frame::none()
                .fill(Theme::ACCENT_PRIMARY)
                .rounding(Rounding::same(16.0))
                .inner_margin(Spacing::MD)
                .show(ui, |ui| {
                    ui.label(RichText::new("C").size(32.0).color(Color32::WHITE).strong());
                });

            ui.add_space(Spacing::MD);

            ui.label(
                RichText::new("CodexBar")
                    .size(FontSize::XXL)
                    .color(Theme::TEXT_PRIMARY)
                    .strong(),
            );

            ui.label(
                RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION")))
                    .size(FontSize::SM)
                    .color(Theme::TEXT_MUTED),
            );
        });

        ui.add_space(Spacing::XL);

        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("A Windows port of the macOS CodexBar app.")
                    .size(FontSize::MD)
                    .color(Theme::TEXT_SECONDARY),
            );
            ui.label(
                RichText::new("Track your AI provider usage from the system tray.")
                    .size(FontSize::MD)
                    .color(Theme::TEXT_SECONDARY),
            );
        });

        ui.add_space(Spacing::XL);

        ui.vertical_centered(|ui| {
            ui.horizontal(|ui| {
                if ui.link("GitHub Repository").clicked() {
                    let _ = open::that("https://github.com/Finesssee/Win-CodexBar");
                }
                ui.label(RichText::new("·").color(Theme::TEXT_DIM));
                if ui.link("Original macOS Version").clicked() {
                    let _ = open::that("https://github.com/steipete/CodexBar");
                }
            });
        });

        ui.add_space(Spacing::LG);

        ui.vertical_centered(|ui| {
            if ui
                .add(
                    egui::Button::new(
                        RichText::new("Check for Updates")
                            .size(FontSize::SM)
                            .color(Theme::TEXT_PRIMARY),
                    )
                    .fill(Theme::BG_SECONDARY)
                    .stroke(Stroke::new(1.0, Theme::BORDER_SUBTLE))
                    .rounding(Rounding::same(Radius::SM)),
                )
                .clicked()
            {
                let _ = open::that("https://github.com/Finesssee/Win-CodexBar/releases");
            }
        });

        ui.add_space(Spacing::XXL);

        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("Built with Rust + egui")
                    .size(FontSize::XS)
                    .color(Theme::TEXT_DIM),
            );
        });
    }
}

fn settings_position_near_main_window(
    main_rect: Rect,
    settings_size: Vec2,
    monitor_size: Rect,
) -> egui::Pos2 {
    let margin = 12.0;
    let gap = 12.0;

    let right_space = monitor_size.max.x - main_rect.max.x - gap - margin;
    let left_space = main_rect.min.x - monitor_size.min.x - gap - margin;
    let bottom_space = monitor_size.max.y - main_rect.max.y - gap - margin;
    let top_space = main_rect.min.y - monitor_size.min.y - gap - margin;

    let mut best_side = "right";
    let mut best_space = right_space;
    for (side, space) in [
        ("left", left_space),
        ("bottom", bottom_space),
        ("top", top_space),
    ] {
        if space > best_space {
            best_side = side;
            best_space = space;
        }
    }

    let min_x = monitor_size.min.x + margin;
    let min_y = monitor_size.min.y + margin;
    let max_x = (monitor_size.max.x - settings_size.x - margin).max(min_x);
    let max_y = (monitor_size.max.y - settings_size.y - margin).max(min_y);
    let clamp_x = |value: f32| {
        if max_x <= min_x {
            min_x
        } else {
            value.clamp(min_x, max_x)
        }
    };
    let clamp_y = |value: f32| {
        if max_y <= min_y {
            min_y
        } else {
            value.clamp(min_y, max_y)
        }
    };

    let (x, y) = match best_side {
        "right" => (clamp_x(main_rect.max.x + gap), clamp_y(main_rect.min.y)),
        "left" => (
            clamp_x(main_rect.min.x - settings_size.x - gap),
            clamp_y(main_rect.min.y),
        ),
        "bottom" => (clamp_x(main_rect.min.x), clamp_y(main_rect.max.y + gap)),
        _ => (
            clamp_x(main_rect.min.x),
            clamp_y(main_rect.min.y - settings_size.y - gap),
        ),
    };

    egui::pos2(x, y)
}

fn work_area_rect(ctx: &egui::Context) -> Option<Rect> {
    #[cfg(target_os = "windows")]
    {
        use windows::Win32::Foundation::RECT as WinRect;
        use windows::Win32::UI::WindowsAndMessaging::{
            SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS, SystemParametersInfoW,
        };

        let mut rect = WinRect::default();
        let ok = unsafe {
            SystemParametersInfoW(
                SPI_GETWORKAREA,
                0,
                Some((&mut rect as *mut WinRect).cast()),
                SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
            )
            .is_ok()
        };

        if ok {
            let pixels_per_point = ctx.pixels_per_point().max(0.1);
            return Some(Rect::from_min_max(
                egui::pos2(
                    rect.left as f32 / pixels_per_point,
                    rect.top as f32 / pixels_per_point,
                ),
                egui::pos2(
                    rect.right as f32 / pixels_per_point,
                    rect.bottom as f32 / pixels_per_point,
                ),
            ));
        }
    }

    ctx.input(|i| {
        i.viewport()
            .monitor_size
            .map(|size| Rect::from_min_size(egui::pos2(0.0, 0.0), size))
    })
}

// ════════════════════════════════════════════════════════════════════════════════
// VIEWPORT SETTINGS UI RENDERER
// ════════════════════════════════════════════════════════════════════════════════

/// Render the settings UI inside the viewport using shared state
fn render_settings_ui(ui: &mut egui::Ui, shared_state: &Arc<Mutex<PreferencesSharedState>>) {
    // Get current tab from shared state
    let (active_tab, language) = if let Ok(state) = shared_state.lock() {
        (state.active_tab, state.settings.ui_language)
    } else {
        (PreferencesTab::General, UiLanguage::English)
    };
    set_current_ui_language(language);

    ui.vertical(|ui| {
        // ═══════════════════════════════════════════════════════════
        // TAB BAR - macOS style with icons above labels, centered
        // ═══════════════════════════════════════════════════════════
        let tabs = [
            PreferencesTab::General,
            PreferencesTab::Providers,
            PreferencesTab::Display,
            PreferencesTab::ApiKeys,
            PreferencesTab::Cookies,
            PreferencesTab::Advanced,
            PreferencesTab::About,
        ];

        let tab_width = 72.0;
        let tab_height = 56.0;
        let total_tabs_width = tabs.len() as f32 * tab_width;
        let start_x = (ui.available_width() - total_tabs_width) / 2.0;

        // Allocate the entire tab bar as one row
        let (tab_bar_rect, _) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), tab_height),
            egui::Sense::hover(),
        );

        for (i, tab) in tabs.iter().enumerate() {
            let is_selected = active_tab == *tab;

            let tab_rect = Rect::from_min_size(
                egui::pos2(
                    tab_bar_rect.min.x + start_x + i as f32 * tab_width,
                    tab_bar_rect.min.y,
                ),
                Vec2::new(tab_width, tab_height),
            );

            // Check for click
            let response = ui.interact(
                tab_rect,
                ui.id().with(format!("tab_{}", i)),
                egui::Sense::click(),
            );

            // Background for selected/hovered
            if is_selected {
                ui.painter().rect_filled(
                    tab_rect.shrink(2.0),
                    Rounding::same(Radius::MD),
                    Theme::CARD_BG,
                );
            } else if response.hovered() {
                ui.painter().rect_filled(
                    tab_rect.shrink(2.0),
                    Rounding::same(Radius::MD),
                    Theme::hover_overlay(),
                );
            }

            // Icon (centered, larger)
            let icon_color = if is_selected {
                Theme::ACCENT_PRIMARY
            } else {
                Theme::TEXT_MUTED
            };
            ui.painter().text(
                egui::pos2(tab_rect.center().x, tab_rect.min.y + 20.0),
                egui::Align2::CENTER_CENTER,
                tab.icon(),
                egui::FontId::proportional(20.0),
                icon_color,
            );

            // Label below icon
            let label_color = if is_selected {
                Theme::TEXT_PRIMARY
            } else {
                Theme::TEXT_MUTED
            };
            ui.painter().text(
                egui::pos2(tab_rect.center().x, tab_rect.min.y + 44.0),
                egui::Align2::CENTER_CENTER,
                tab.label(language),
                egui::FontId::proportional(11.0),
                label_color,
            );

            if response.clicked() {
                if let Ok(mut state) = shared_state.lock() {
                    state.active_tab = *tab;
                }
            }
        }

        // Separator line
        ui.add_space(Spacing::SM);
        let separator_rect =
            Rect::from_min_size(ui.cursor().min, Vec2::new(ui.available_width(), 1.0));
        ui.painter()
            .rect_filled(separator_rect, 0.0, Theme::SEPARATOR);
        ui.add_space(Spacing::SM);

        // ═══════════════════════════════════════════════════════════
        // TAB CONTENT
        // ═══════════════════════════════════════════════════════════
        let content_height = ui.available_height() - Spacing::SM;

        match active_tab {
            PreferencesTab::Providers => {
                // Providers tab has special sidebar + detail layout
                render_providers_tab_macos(ui, content_height, shared_state);
            }
            _ => {
                egui::ScrollArea::vertical()
                    .id_salt("settings_content_viewport")
                    .max_height(content_height)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        match active_tab {
                            PreferencesTab::General => render_general_tab(ui, shared_state),
                            PreferencesTab::Display => render_display_tab(ui, shared_state),
                            PreferencesTab::ApiKeys => render_api_keys_tab(ui, shared_state),
                            PreferencesTab::Cookies => render_cookies_tab(ui, shared_state),
                            PreferencesTab::Advanced => render_advanced_tab(ui, shared_state),
                            PreferencesTab::About => render_about_tab(ui),
                            PreferencesTab::Providers => unreachable!(),
                        }
                        ui.add_space(Spacing::LG);
                    });
            }
        }
    });
}

/// Render Providers tab with macOS-style sidebar + detail layout
fn render_providers_tab_macos(
    ui: &mut egui::Ui,
    available_height: f32,
    shared_state: &Arc<Mutex<PreferencesSharedState>>,
) {
    // macOS metrics
    let sidebar_width = 240.0; // ProviderSettingsMetrics.sidebarWidth
    let sidebar_corner_radius = 12.0; // sidebarCornerRadius
    let icon_size = 18.0; // iconSize
    let total_width = ui.available_width();
    let detail_width = (total_width - sidebar_width - Spacing::LG).min(640.0); // detailMaxWidth

    // Get selected provider
    let selected_provider = if let Ok(state) = shared_state.lock() {
        state.selected_provider.clone()
    } else {
        None
    };

    let providers = ProviderId::all();
    let selected = selected_provider.unwrap_or(providers[0]);

    // Create a horizontal layout with two fixed regions
    let sidebar_rect = ui.available_rect_before_wrap();

    // LEFT SIDEBAR - Fixed width region
    let _sidebar_response = ui.allocate_ui_with_layout(
        Vec2::new(sidebar_width, available_height),
        egui::Layout::top_down(egui::Align::LEFT),
        |ui| {
            egui::Frame::none()
                .fill(Theme::BG_SECONDARY)
                .rounding(Rounding::same(sidebar_corner_radius))
                .stroke(Stroke::new(1.0, Theme::SEPARATOR))
                .inner_margin(Spacing::SM)
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("provider_sidebar_scroll_v3")
                        .max_height(available_height - Spacing::LG * 2.0)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            for provider_id in providers {
                                let is_selected = *provider_id == selected;
                                let is_enabled = if let Ok(state) = shared_state.lock() {
                                    state
                                        .settings
                                        .enabled_providers
                                        .contains(provider_id.cli_name())
                                } else {
                                    true
                                };

                                let brand_color = provider_color(provider_id.cli_name());
                                let row_height = 44.0; // Compact rows
                                let row_width = sidebar_width - Spacing::SM * 4.0;

                                // Row with vertical padding of 2px
                                ui.add_space(2.0);

                                let (rect, response) = ui.allocate_exact_size(
                                    Vec2::new(row_width, row_height),
                                    egui::Sense::click(),
                                );

                                // Selection/hover background - use gray like macOS
                                if is_selected {
                                    ui.painter().rect_filled(
                                        rect,
                                        Rounding::same(8.0),
                                        Color32::from_rgba_unmultiplied(255, 255, 255, 15),
                                    );
                                } else if response.hovered() {
                                    ui.painter().rect_filled(
                                        rect,
                                        Rounding::same(8.0),
                                        Theme::hover_overlay(),
                                    );
                                }

                                // Layout: [drag] [icon] [name + dot] ... [checkbox]
                                let content_start = rect.min.x + 4.0;

                                // Drag handle (3x2 grid of dots, 12x12 area)
                                let dot_area_x = content_start;
                                let dot_area_center_y = rect.center().y;
                                for row in 0..3 {
                                    for col in 0..2 {
                                        let x = dot_area_x + 2.0 + col as f32 * 3.0;
                                        let y = dot_area_center_y - 4.0 + row as f32 * 4.0;
                                        ui.painter().circle_filled(
                                            egui::pos2(x, y),
                                            1.0,
                                            Theme::TEXT_MUTED,
                                        );
                                    }
                                }

                                // Provider icon (18x18) - use SVG texture if available
                                let icon_x = content_start + 16.0;
                                let icon_rect = Rect::from_center_size(
                                    egui::pos2(icon_x + icon_size / 2.0, rect.center().y),
                                    Vec2::splat(icon_size),
                                );

                                // Try to get SVG icon from cache
                                let has_svg_icon = VIEWPORT_ICON_CACHE.with(|cache| {
                                    let mut cache = cache.borrow_mut();
                                    if let Some(texture) = cache.get_icon(
                                        ui.ctx(),
                                        provider_id.cli_name(),
                                        icon_size as u32,
                                    ) {
                                        // Paint the texture
                                        ui.painter().image(
                                            texture.id(),
                                            icon_rect,
                                            Rect::from_min_max(
                                                egui::pos2(0.0, 0.0),
                                                egui::pos2(1.0, 1.0),
                                            ),
                                            Color32::WHITE,
                                        );
                                        true
                                    } else {
                                        false
                                    }
                                });

                                // Fallback to Unicode symbol if no SVG
                                if !has_svg_icon {
                                    ui.painter().text(
                                        egui::pos2(icon_x + icon_size / 2.0, rect.center().y),
                                        egui::Align2::CENTER_CENTER,
                                        provider_icon(provider_id.cli_name()),
                                        egui::FontId::proportional(icon_size),
                                        brand_color,
                                    );
                                }

                                // Text area starts after icon with 10px spacing
                                let text_x = icon_x + icon_size + 10.0;

                                // Display name (subheadline, semibold ~14px) with status dot
                                let name_text = provider_id.display_name();
                                let name_galley = ui.painter().layout_no_wrap(
                                    name_text.to_string(),
                                    egui::FontId::proportional(14.0),
                                    Theme::TEXT_PRIMARY,
                                );
                                ui.painter().galley(
                                    egui::pos2(text_x, rect.center().y - 10.0),
                                    name_galley,
                                    Theme::TEXT_PRIMARY,
                                );

                                // Status dot (small, after name) - only shown for enabled
                                if is_enabled {
                                    let dot_x = text_x + name_text.len() as f32 * 7.5 + 6.0;
                                    ui.painter().circle_filled(
                                        egui::pos2(dot_x, rect.center().y - 6.0),
                                        3.0,
                                        Theme::GREEN,
                                    );
                                }

                                // Subtitle (caption ~11px, secondary color) - 2 line height
                                // Show version/source text like macOS does
                                ui.painter().text(
                                    egui::pos2(text_x, rect.center().y + 8.0),
                                    egui::Align2::LEFT_CENTER,
                                    provider_id.cli_name(),
                                    egui::FontId::proportional(11.0),
                                    Theme::TEXT_SECONDARY,
                                );

                                // Toggle checkbox on right - use small checkbox style like macOS
                                let checkbox_x = rect.max.x - 14.0;
                                let checkbox_size = 14.0;
                                let checkbox_rect = Rect::from_center_size(
                                    egui::pos2(checkbox_x, rect.center().y),
                                    Vec2::splat(checkbox_size),
                                );

                                // Draw checkbox border
                                ui.painter().rect_stroke(
                                    checkbox_rect,
                                    Rounding::same(3.0),
                                    Stroke::new(
                                        1.0,
                                        if is_enabled {
                                            Theme::ACCENT_PRIMARY
                                        } else {
                                            Theme::TEXT_MUTED
                                        },
                                    ),
                                );

                                // Fill and checkmark if enabled
                                if is_enabled {
                                    ui.painter().rect_filled(
                                        checkbox_rect.shrink(1.0),
                                        Rounding::same(2.0),
                                        Theme::ACCENT_PRIMARY,
                                    );
                                    ui.painter().text(
                                        checkbox_rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        "✓",
                                        egui::FontId::proportional(10.0),
                                        Color32::WHITE,
                                    );
                                }

                                // Handle clicks
                                if response.clicked() {
                                    if let Ok(mut state) = shared_state.lock() {
                                        state.selected_provider = Some(*provider_id);
                                    }
                                }

                                ui.add_space(2.0);
                            }
                        });
                });
        },
    );

    // Move cursor to the right of sidebar
    let detail_rect = egui::Rect::from_min_size(
        egui::pos2(
            sidebar_rect.min.x + sidebar_width + Spacing::MD,
            sidebar_rect.min.y,
        ),
        Vec2::new(detail_width, available_height),
    );

    // RIGHT PANEL - Detail view
    ui.allocate_new_ui(egui::UiBuilder::new().max_rect(detail_rect), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("provider_detail_scroll_v3")
            .max_height(available_height - Spacing::SM)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                render_provider_detail_panel(ui, selected, shared_state);
            });
    });
}

/// Render the provider detail panel (right side)
fn render_provider_detail_panel(
    ui: &mut egui::Ui,
    provider_id: ProviderId,
    shared_state: &Arc<Mutex<PreferencesSharedState>>,
) {
    let brand_color = provider_color(provider_id.cli_name());

    let is_enabled = if let Ok(state) = shared_state.lock() {
        state
            .settings
            .enabled_providers
            .contains(provider_id.cli_name())
    } else {
        true
    };

    // Use cached snapshot data for this provider (loaded once, not every frame)
    let entry = if let Ok(state) = shared_state.lock() {
        state
            .cached_snapshot
            .as_ref()
            .and_then(|s| s.entry_for(provider_id).cloned())
    } else {
        None
    };

    // Extract data from entry or use defaults
    let account_email = entry.as_ref().and_then(|e| e.account_email.clone());
    let login_method = entry.as_ref().and_then(|e| e.login_method.clone());
    let primary_rate = entry.as_ref().and_then(|e| e.primary.clone());
    let secondary_rate = entry.as_ref().and_then(|e| e.secondary.clone());
    let tertiary_rate = entry.as_ref().and_then(|e| e.tertiary.clone());
    let credits_remaining = entry.as_ref().and_then(|e| e.credits_remaining);
    let code_review_percent = entry.as_ref().and_then(|e| e.code_review_remaining_percent);
    let token_usage = entry.as_ref().and_then(|e| e.token_usage.clone());
    let updated_at = entry.as_ref().map(|e| e.updated_at);

    // ═══════════════════════════════════════════════════════════
    // HEADER - Icon, name, version, refresh, toggle
    // ═══════════════════════════════════════════════════════════
    ui.horizontal(|ui| {
        // Large provider icon (28x28) - use SVG if available
        let icon_size = 28.0;
        let has_svg = VIEWPORT_ICON_CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            if let Some(texture) =
                cache.get_icon(ui.ctx(), provider_id.cli_name(), icon_size as u32)
            {
                ui.add(egui::Image::new(texture).fit_to_exact_size(Vec2::splat(icon_size)));
                true
            } else {
                false
            }
        });

        if !has_svg {
            ui.label(
                RichText::new(provider_icon(provider_id.cli_name()))
                    .size(icon_size)
                    .color(brand_color),
            );
        }

        ui.add_space(12.0);

        ui.vertical(|ui| {
            ui.label(
                RichText::new(provider_id.display_name())
                    .size(FontSize::LG)
                    .color(Theme::TEXT_PRIMARY)
                    .strong(),
            );
            let updated_str = if let Some(ts) = updated_at {
                let now = chrono::Utc::now();
                let diff = now - ts;
                if diff.num_seconds() < 60 {
                    localize_text("just now").to_string()
                } else if diff.num_minutes() < 60 {
                    format!("{}m ago", diff.num_minutes())
                } else if diff.num_hours() < 24 {
                    format!("{}h ago", diff.num_hours())
                } else {
                    format!("{}d ago", diff.num_days())
                }
            } else {
                localize_text("never").to_string()
            };
            ui.label(
                RichText::new(format!("{} • {}", provider_id.cli_name(), updated_str))
                    .size(FontSize::SM)
                    .color(Theme::TEXT_MUTED),
            );
        });

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Toggle switch
            let mut enabled = is_enabled;
            if switch_toggle(
                ui,
                egui::Id::new(format!("detail_toggle_{}", provider_id.cli_name())),
                &mut enabled,
            ) {
                if let Ok(mut state) = shared_state.lock() {
                    let name = provider_id.cli_name().to_string();
                    if enabled {
                        state.settings.enabled_providers.insert(name);
                    } else {
                        state.settings.enabled_providers.remove(&name);
                    }
                    state.settings_changed = true;
                }
            }

            ui.add_space(16.0);

            // Refresh button
            if ui
                .add(
                    egui::Button::new(
                        RichText::new("↻")
                            .size(FontSize::MD)
                            .color(Theme::TEXT_SECONDARY),
                    )
                    .fill(Color32::TRANSPARENT)
                    .stroke(Stroke::new(1.0, Theme::CARD_BORDER))
                    .rounding(Rounding::same(Radius::SM))
                    .min_size(Vec2::new(32.0, 32.0)),
                )
                .clicked()
            {
                if let Ok(mut state) = shared_state.lock() {
                    state.refresh_requested = true;
                }
            }
        });
    });

    ui.add_space(Spacing::LG);

    // ═══════════════════════════════════════════════════════════
    // INFO GRID - State, Source, Version, etc.
    // ═══════════════════════════════════════════════════════════
    let updated_display = if let Some(ts) = updated_at {
        let now = chrono::Utc::now();
        let diff = now - ts;
        if diff.num_seconds() < 60 {
            localize_text("Updated just now").to_string()
        } else if diff.num_minutes() < 60 {
            match current_ui_language() {
                UiLanguage::English => format!("Updated {}m ago", diff.num_minutes()),
                UiLanguage::Chinese => format!("{} 分钟前更新", diff.num_minutes()),
            }
        } else if diff.num_hours() < 24 {
            match current_ui_language() {
                UiLanguage::English => format!("Updated {}h ago", diff.num_hours()),
                UiLanguage::Chinese => format!("{} 小时前更新", diff.num_hours()),
            }
        } else {
            match current_ui_language() {
                UiLanguage::English => format!("Updated {}d ago", diff.num_days()),
                UiLanguage::Chinese => format!("{} 天前更新", diff.num_days()),
            }
        }
    } else {
        localize_text("Never updated").to_string()
    };
    let hide_personal_info = if let Ok(state) = shared_state.lock() {
        state.settings.hide_personal_info
    } else {
        false
    };
    let account_display = if account_email.is_some() {
        PersonalInfoRedactor::redact_email(account_email.as_deref(), hide_personal_info)
    } else {
        localize_text("Not logged in").to_string()
    };
    let account_display = if account_display.is_empty() {
        localize_text("Not logged in").to_string()
    } else {
        account_display
    };
    let plan_display = login_method
        .as_deref()
        .map(|s| localize_text(s).to_string())
        .unwrap_or_else(|| localize_text("Unknown").to_string());

    egui::Grid::new("provider_info_grid")
        .num_columns(2)
        .spacing([16.0, 8.0])
        .show(ui, |ui| {
            info_row(ui, "State", if is_enabled { "Enabled" } else { "Disabled" });
            info_row(ui, "Source", "oauth + web");
            info_row(ui, "Version", provider_id.cli_name());
            info_row(ui, "Updated", &updated_display);
            info_row(ui, "Status", "All Systems Operational");
            info_row(ui, "Account", &account_display);
            info_row(ui, "Plan", &plan_display);
        });

    ui.add_space(Spacing::LG);

    // ═══════════════════════════════════════════════════════════
    // USAGE SECTION
    // ═══════════════════════════════════════════════════════════
    ui.label(
        RichText::new(localize_text("Usage").as_ref())
            .size(FontSize::MD)
            .color(Theme::TEXT_PRIMARY)
            .strong(),
    );
    ui.add_space(Spacing::SM);

    // Helper to format reset time
    let format_reset = |rate: &crate::core::RateWindow| -> Option<String> {
        // First try to compute from resets_at timestamp
        if let Some(ts) = rate.resets_at {
            let now = chrono::Utc::now();
            let diff = ts - now;
            if diff.num_seconds() <= 0 {
                return Some(localize_text("Resetting...").to_string());
            } else if diff.num_hours() >= 24 {
                return Some(match current_ui_language() {
                    UiLanguage::English => {
                        format!("Resets in {}d {}h", diff.num_days(), diff.num_hours() % 24)
                    }
                    UiLanguage::Chinese => format!(
                        "{} {}d {}h",
                        localize_text("Resets in"),
                        diff.num_days(),
                        diff.num_hours() % 24
                    ),
                });
            } else {
                return Some(match current_ui_language() {
                    UiLanguage::English => {
                        format!(
                            "Resets in {}h {}m",
                            diff.num_hours(),
                            diff.num_minutes() % 60
                        )
                    }
                    UiLanguage::Chinese => format!(
                        "{} {}h {}m",
                        localize_text("Resets in"),
                        diff.num_hours(),
                        diff.num_minutes() % 60
                    ),
                });
            }
        }
        // Fall back to reset_description if available (for CLI/web sources without parsed timestamp)
        rate.reset_description.clone()
    };

    let show_as_used = if let Ok(state) = shared_state.lock() {
        state.settings.show_as_used
    } else {
        true
    };

    // Session usage bar (primary rate)
    if let Some(ref rate) = primary_rate {
        let (percent, label) = usage_display(rate.used_percent, show_as_used);
        let reset_str = format_reset(rate);
        usage_bar_row(
            ui,
            "Session",
            percent as f32,
            &label,
            reset_str.as_deref(),
            brand_color,
        );
        ui.add_space(8.0);
    }

    // Weekly usage bar (secondary rate)
    if let Some(ref rate) = secondary_rate {
        let (percent, label) = usage_display(rate.used_percent, show_as_used);
        let reset_str = format_reset(rate);
        usage_bar_row(
            ui,
            "Weekly",
            percent as f32,
            &label,
            reset_str.as_deref(),
            brand_color,
        );
        ui.add_space(8.0);
    }

    // Tertiary rate (e.g., code review)
    if let Some(ref rate) = tertiary_rate {
        let (percent, label) = usage_display(rate.used_percent, show_as_used);
        let reset_str = rate.reset_description.as_deref();
        usage_bar_row(
            ui,
            "Code review",
            percent as f32,
            &label,
            reset_str,
            brand_color,
        );
        ui.add_space(8.0);
    }

    // Code review (if available and no tertiary rate)
    // Note: code_review_remaining_percent is the REMAINING percent, so convert to used
    if tertiary_rate.is_none() {
        if let Some(remaining) = code_review_percent {
            let used = 100.0 - remaining;
            let (percent, label) = usage_display(used, show_as_used);
            usage_bar_row(ui, "Code review", percent as f32, &label, None, brand_color);
        }
    }

    ui.add_space(Spacing::LG);

    // ═══════════════════════════════════════════════════════════
    // TRAY METRIC PREFERENCE
    // ═══════════════════════════════════════════════════════════
    ui.label(
        RichText::new(localize_text("Tray Display").as_ref())
            .size(FontSize::MD)
            .color(Theme::TEXT_PRIMARY)
            .strong(),
    );
    ui.add_space(Spacing::SM);

    ui.horizontal(|ui| {
        ui.label(
            RichText::new(localize_text("Show in tray").as_ref())
                .size(FontSize::SM)
                .color(Theme::TEXT_SECONDARY),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let current_metric = if let Ok(state) = shared_state.lock() {
                state.settings.get_provider_metric(provider_id)
            } else {
                crate::settings::MetricPreference::Automatic
            };

            egui::Frame::none()
                .fill(Theme::BG_TERTIARY)
                .stroke(Stroke::new(1.0, Theme::BORDER_SUBTLE))
                .rounding(Rounding::same(Radius::SM))
                .inner_margin(egui::Margin::symmetric(Spacing::XS, 2.0))
                .show(ui, |ui| {
                    let metrics = crate::settings::MetricPreference::all();

                    let mut selected = current_metric;
                    egui::ComboBox::from_id_salt(format!("metric_pref_{}", provider_id.cli_name()))
                        .selected_text(localize_metric_name(selected.display_name()).as_ref())
                        .show_ui(ui, |ui| {
                            for metric in metrics {
                                if ui
                                    .selectable_value(
                                        &mut selected,
                                        *metric,
                                        localize_metric_name(metric.display_name()).as_ref(),
                                    )
                                    .changed()
                                {
                                    if let Ok(mut state) = shared_state.lock() {
                                        state.settings.set_provider_metric(provider_id, selected);
                                        state.settings_changed = true;
                                    }
                                }
                            }
                        });
                });
        });
    });

    ui.add_space(Spacing::LG);

    // ═══════════════════════════════════════════════════════════
    // CREDITS SECTION (if applicable)
    // ═══════════════════════════════════════════════════════════
    if let Some(credits) = credits_remaining {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(localize_text("Credits balance").as_ref())
                    .size(FontSize::SM)
                    .color(Theme::TEXT_SECONDARY),
            );
            ui.add_space(16.0);
            ui.label(
                RichText::new(match current_ui_language() {
                    UiLanguage::English => format!("{:.1} left", credits),
                    UiLanguage::Chinese => format!("{} {:.1}", localize_text("left"), credits),
                })
                .size(FontSize::SM)
                .color(Theme::TEXT_PRIMARY),
            );
        });
        ui.add_space(Spacing::SM);
    }

    // ═══════════════════════════════════════════════════════════
    // COST SECTION
    // ═══════════════════════════════════════════════════════════
    let (today_cost, today_tokens, monthly_cost, monthly_tokens) =
        if let Some(ref usage) = token_usage {
            (
                usage.session_cost_usd.unwrap_or(0.0),
                usage.session_tokens.unwrap_or(0),
                usage.last_30_days_cost_usd.unwrap_or(0.0),
                usage.last_30_days_tokens.unwrap_or(0),
            )
        } else {
            (0.0, 0, 0.0, 0)
        };

    ui.horizontal(|ui| {
        ui.label(
            RichText::new(localize_text("Cost").as_ref())
                .size(FontSize::SM)
                .color(Theme::TEXT_SECONDARY),
        );
        ui.add_space(32.0);
        ui.vertical(|ui| {
            ui.label(
                RichText::new(format!(
                    "{}: ${:.2} • {} {}",
                    localize_text("Today"),
                    today_cost,
                    today_tokens,
                    localize_text("tokens"),
                ))
                .size(FontSize::SM)
                .color(Theme::TEXT_PRIMARY),
            );
            ui.label(
                RichText::new(format!(
                    "{}: ${:.2} • {} {}",
                    localize_text("Last 30 days"),
                    monthly_cost,
                    monthly_tokens,
                    localize_text("tokens"),
                ))
                .size(FontSize::SM)
                .color(Theme::TEXT_MUTED),
            );
        });
    });

    ui.add_space(Spacing::XL);

    // ═══════════════════════════════════════════════════════════
    // SETTINGS SECTION
    // ═══════════════════════════════════════════════════════════
    ui.label(
        RichText::new(localize_text("Settings").as_ref())
            .size(FontSize::MD)
            .color(Theme::TEXT_PRIMARY)
            .strong(),
    );
    ui.add_space(Spacing::SM);

    // Menu bar metric dropdown
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(localize_text("Menu bar metric").as_ref())
                .size(FontSize::SM)
                .color(Theme::TEXT_SECONDARY),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            egui::ComboBox::from_id_salt(format!("metric_{}", provider_id.cli_name()))
                .selected_text(localize_text("Automatic").as_ref())
                .width(120.0)
                .show_ui(ui, |ui| {
                    let _ = ui.selectable_label(true, localize_text("Automatic").as_ref());
                    let _ = ui.selectable_label(false, localize_text("Session").as_ref());
                    let _ = ui.selectable_label(false, localize_text("Weekly").as_ref());
                });
        });
    });

    ui.add_space(4.0);
    ui.label(
        RichText::new(localize_text("Choose which window drives the menu bar percent.").as_ref())
            .size(FontSize::XS)
            .color(Theme::TEXT_MUTED),
    );

    ui.add_space(Spacing::MD);

    // Usage source dropdown
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(localize_text("Usage source").as_ref())
                .size(FontSize::SM)
                .color(Theme::TEXT_SECONDARY),
        );

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                RichText::new(localize_text("oauth + web").as_ref())
                    .size(FontSize::XS)
                    .color(Theme::TEXT_MUTED),
            );
            ui.add_space(8.0);
            egui::ComboBox::from_id_salt(format!("source_{}", provider_id.cli_name()))
                .selected_text(localize_text("Auto").as_ref())
                .width(100.0)
                .show_ui(ui, |ui| {
                    let _ = ui.selectable_label(true, localize_text("Auto").as_ref());
                    let _ = ui.selectable_label(false, localize_text("OAuth").as_ref());
                    let _ = ui.selectable_label(false, localize_text("API").as_ref());
                });
        });
    });

    ui.add_space(4.0);
    ui.label(
        RichText::new(
            localize_text("Auto falls back to the next source if the preferred one fails.")
                .as_ref(),
        )
        .size(FontSize::XS)
        .color(Theme::TEXT_MUTED),
    );

    // ═══════════════════════════════════════════════════════════
    // ACCOUNTS SECTION - Token account switching (only for supported providers)
    // ═══════════════════════════════════════════════════════════
    if TokenAccountSupport::is_supported(provider_id) {
        ui.add_space(Spacing::XL);
        render_accounts_section(ui, provider_id, shared_state);
    }
}

/// Helper: Info grid row
fn info_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.label(
        RichText::new(localize_text(label).as_ref())
            .size(FontSize::SM)
            .color(Theme::TEXT_SECONDARY),
    );
    ui.label(
        RichText::new(localize_text(value).as_ref())
            .size(FontSize::SM)
            .color(Theme::TEXT_PRIMARY),
    );
    ui.end_row();
}

/// Helper: Usage bar row with label, percentage, info text
fn usage_bar_row(
    ui: &mut egui::Ui,
    label: &str,
    percent: f32,
    info: &str,
    reset: Option<&str>,
    color: Color32,
) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(localize_text(label).as_ref())
                .size(FontSize::SM)
                .color(Theme::TEXT_SECONDARY),
        );
        ui.add_space(8.0);

        // Progress bar
        let bar_width = 200.0;
        let bar_height = 8.0;
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(bar_width, bar_height), egui::Sense::hover());

        ui.painter()
            .rect_filled(rect, Rounding::same(4.0), Theme::progress_track());

        let fill_width = rect.width() * (percent / 100.0).clamp(0.0, 1.0);
        if fill_width > 0.0 {
            let fill_rect = Rect::from_min_size(rect.min, Vec2::new(fill_width, bar_height));
            ui.painter()
                .rect_filled(fill_rect, Rounding::same(4.0), color);
        }

        ui.add_space(8.0);

        ui.label(
            RichText::new(info)
                .size(FontSize::XS)
                .color(Theme::TEXT_MUTED),
        );

        if let Some(reset_text) = reset {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(reset_text)
                        .size(FontSize::XS)
                        .color(Theme::TEXT_MUTED),
                );
            });
        }
    });
}

fn usage_display(used_percent: f64, show_as_used: bool) -> (f64, String) {
    let used_percent = used_percent.clamp(0.0, 100.0);
    let display_percent = if show_as_used {
        used_percent
    } else {
        100.0 - used_percent
    };

    let label = if show_as_used {
        match current_ui_language() {
            UiLanguage::English => format!("{:.0}% used", display_percent),
            UiLanguage::Chinese => format!("已使用 {:.0}%", display_percent),
        }
    } else {
        match current_ui_language() {
            UiLanguage::English => format!("{:.0}% remaining", display_percent),
            UiLanguage::Chinese => format!("剩余 {:.0}%", display_percent),
        }
    };

    (display_percent, label)
}

/// Render Accounts section for token account switching
fn render_accounts_section(
    ui: &mut egui::Ui,
    provider_id: ProviderId,
    shared_state: &Arc<Mutex<PreferencesSharedState>>,
) {
    let support = match TokenAccountSupport::for_provider(provider_id) {
        Some(s) => s,
        None => return,
    };

    ui.label(
        RichText::new(support.title)
            .size(FontSize::MD)
            .color(Theme::TEXT_PRIMARY)
            .strong(),
    );
    ui.add_space(4.0);
    ui.label(
        RichText::new(support.subtitle)
            .size(FontSize::XS)
            .color(Theme::TEXT_MUTED),
    );
    ui.add_space(Spacing::SM);

    // Get current accounts for this provider
    let (accounts_data, show_add, status_msg) = if let Ok(state) = shared_state.lock() {
        let data = state
            .token_accounts
            .get(&provider_id)
            .cloned()
            .unwrap_or_default();
        (
            data,
            state.show_add_account_input,
            state.token_account_status_msg.clone(),
        )
    } else {
        (ProviderAccountData::default(), false, None)
    };

    // Status message
    if let Some((msg, is_error)) = &status_msg {
        let color = if *is_error { Theme::RED } else { Theme::GREEN };
        ui.label(RichText::new(msg).size(FontSize::SM).color(color));
        ui.add_space(Spacing::SM);
    }

    // List existing accounts with radio buttons
    if !accounts_data.accounts.is_empty() {
        let active_idx = accounts_data.clamped_active_index();

        for (idx, account) in accounts_data.accounts.iter().enumerate() {
            let is_active = idx == active_idx;

            ui.horizontal(|ui| {
                // Radio button
                if ui.radio(is_active, "").clicked() && !is_active {
                    // Set as active
                    if let Ok(mut state) = shared_state.lock() {
                        if let Some(data) = state.token_accounts.get_mut(&provider_id) {
                            data.set_active(idx);
                        }
                        // Save to disk
                        let store = TokenAccountStore::new();
                        if let Err(e) = store.save(&state.token_accounts) {
                            state.token_account_status_msg = Some((
                                match current_ui_language() {
                                    UiLanguage::English => format!("Failed to save: {}", e),
                                    UiLanguage::Chinese => format!("保存失败：{}", e),
                                },
                                true,
                            ));
                        } else {
                            state.token_account_status_msg =
                                Some((localize_text("Account switched").to_string(), false));
                        }
                    }
                }

                ui.add_space(4.0);

                // Account label
                ui.label(
                    RichText::new(account.display_name())
                        .size(FontSize::SM)
                        .color(if is_active {
                            Theme::TEXT_PRIMARY
                        } else {
                            Theme::TEXT_SECONDARY
                        }),
                );

                // Truncated token preview
                let token_preview = if account.token.len() > 16 {
                    format!("{}...", &account.token[..12])
                } else {
                    account.token.clone()
                };
                ui.label(
                    RichText::new(token_preview)
                        .size(FontSize::XS)
                        .color(Theme::TEXT_MUTED)
                        .monospace(),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Remove button
                    let account_id = account.id;
                    if small_button(ui, "Remove", Theme::RED) {
                        if let Ok(mut state) = shared_state.lock() {
                            if let Some(data) = state.token_accounts.get_mut(&provider_id) {
                                data.remove_account(account_id);
                            }
                            // Save to disk
                            let store = TokenAccountStore::new();
                            if let Err(e) = store.save(&state.token_accounts) {
                                state.token_account_status_msg = Some((
                                    match current_ui_language() {
                                        UiLanguage::English => format!("Failed to save: {}", e),
                                        UiLanguage::Chinese => format!("保存失败：{}", e),
                                    },
                                    true,
                                ));
                            } else {
                                state.token_account_status_msg =
                                    Some((localize_text("Account removed").to_string(), false));
                            }
                        }
                    }
                });
            });

            ui.add_space(4.0);
        }

        ui.add_space(Spacing::SM);
    }

    // Add Account button or input form
    if show_add {
        // Input form for adding new account
        egui::Frame::none()
            .fill(Theme::BG_TERTIARY)
            .stroke(Stroke::new(1.0, Theme::ACCENT_PRIMARY.gamma_multiply(0.4)))
            .rounding(Rounding::same(Radius::MD))
            .inner_margin(Spacing::MD)
            .show(ui, |ui| {
                ui.label(
                    RichText::new(localize_text("Add Account").as_ref())
                        .size(FontSize::MD)
                        .color(Theme::TEXT_PRIMARY)
                        .strong(),
                );

                ui.add_space(Spacing::SM);

                // Label input
                ui.label(
                    RichText::new(localize_text("Label").as_ref())
                        .size(FontSize::SM)
                        .color(Theme::TEXT_SECONDARY),
                );
                let mut label = if let Ok(state) = shared_state.lock() {
                    state.new_account_label.clone()
                } else {
                    String::new()
                };
                let label_edit = egui::TextEdit::singleline(&mut label)
                    .desired_width(ui.available_width())
                    .hint_text(localize_text("e.g., Work Account, Personal...").as_ref());
                if ui.add(label_edit).changed() {
                    if let Ok(mut state) = shared_state.lock() {
                        state.new_account_label = label;
                    }
                }

                ui.add_space(Spacing::SM);

                // Token input
                ui.label(
                    RichText::new(localize_text("Token").as_ref())
                        .size(FontSize::SM)
                        .color(Theme::TEXT_SECONDARY),
                );
                let mut token = if let Ok(state) = shared_state.lock() {
                    state.new_account_token.clone()
                } else {
                    String::new()
                };
                let token_edit = egui::TextEdit::singleline(&mut token)
                    .password(true)
                    .desired_width(ui.available_width())
                    .hint_text(support.placeholder);
                if ui.add(token_edit).changed() {
                    if let Ok(mut state) = shared_state.lock() {
                        state.new_account_token = token;
                    }
                }

                ui.add_space(Spacing::MD);

                ui.horizontal(|ui| {
                    let (can_save, label_val, token_val) = if let Ok(state) = shared_state.lock() {
                        let can = !state.new_account_label.trim().is_empty()
                            && !state.new_account_token.trim().is_empty();
                        (
                            can,
                            state.new_account_label.clone(),
                            state.new_account_token.clone(),
                        )
                    } else {
                        (false, String::new(), String::new())
                    };

                    if ui
                        .add_enabled(
                            can_save,
                            egui::Button::new(
                                RichText::new(localize_text("Save").as_ref())
                                    .size(FontSize::SM)
                                    .color(Color32::WHITE),
                            )
                            .fill(if can_save {
                                Theme::GREEN
                            } else {
                                Theme::BG_TERTIARY
                            })
                            .rounding(Rounding::same(Radius::SM))
                            .min_size(Vec2::new(80.0, 32.0)),
                        )
                        .clicked()
                    {
                        if let Ok(mut state) = shared_state.lock() {
                            // Create new account
                            let account = TokenAccount::new(label_val.trim(), token_val.trim());

                            // Add to provider data
                            let data = state.token_accounts.entry(provider_id).or_default();
                            data.add_account(account);

                            // Save to disk
                            let store = TokenAccountStore::new();
                            if let Err(e) = store.save(&state.token_accounts) {
                                state.token_account_status_msg = Some((
                                    match current_ui_language() {
                                        UiLanguage::English => format!("Failed to save: {}", e),
                                        UiLanguage::Chinese => format!("保存失败：{}", e),
                                    },
                                    true,
                                ));
                            } else {
                                state.token_account_status_msg =
                                    Some((localize_text("Account added").to_string(), false));
                                state.new_account_label.clear();
                                state.new_account_token.clear();
                                state.show_add_account_input = false;
                            }
                        }
                    }

                    ui.add_space(Spacing::XS);

                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(localize_text("Cancel").as_ref())
                                    .size(FontSize::SM)
                                    .color(Theme::TEXT_MUTED),
                            )
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::new(1.0, Theme::BORDER_SUBTLE))
                            .rounding(Rounding::same(Radius::SM)),
                        )
                        .clicked()
                    {
                        if let Ok(mut state) = shared_state.lock() {
                            state.show_add_account_input = false;
                            state.new_account_label.clear();
                            state.new_account_token.clear();
                        }
                    }
                });
            });
    } else {
        // Add Account button
        if primary_button(ui, "+ Add Account") {
            if let Ok(mut state) = shared_state.lock() {
                state.show_add_account_input = true;
                state.new_account_label.clear();
                state.new_account_token.clear();
                state.token_account_status_msg = None;
            }
        }
    }
}

/// Render General tab for viewport
fn render_general_tab(ui: &mut egui::Ui, shared_state: &Arc<Mutex<PreferencesSharedState>>) {
    let language = if let Ok(state) = shared_state.lock() {
        state.settings.ui_language
    } else {
        UiLanguage::English
    };

    section_header(
        ui,
        match language {
            UiLanguage::English => "Language",
            UiLanguage::Chinese => "语言",
        },
    );

    settings_card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(match language {
                    UiLanguage::English => "UI language",
                    UiLanguage::Chinese => "界面语言",
                })
                .size(FontSize::MD)
                .color(Theme::TEXT_PRIMARY),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let mut selected_language = if let Ok(state) = shared_state.lock() {
                    state.settings.ui_language
                } else {
                    UiLanguage::English
                };

                egui::ComboBox::from_id_salt("ui_language")
                    .selected_text(selected_language.display_name())
                    .width(130.0)
                    .show_ui(ui, |ui| {
                        for candidate in UiLanguage::all() {
                            if ui
                                .selectable_label(
                                    selected_language == *candidate,
                                    candidate.display_name(),
                                )
                                .clicked()
                            {
                                selected_language = *candidate;
                            }
                        }
                    });

                if let Ok(mut state) = shared_state.lock() {
                    if state.settings.ui_language != selected_language {
                        state.settings.ui_language = selected_language;
                        state.settings_changed = true;
                        set_current_ui_language(selected_language);
                    }
                }
            });
        });
    });

    ui.add_space(Spacing::LG);

    section_header(ui, "Startup");

    settings_card(ui, |ui| {
        let mut start_at_login = if let Ok(state) = shared_state.lock() {
            state.settings.start_at_login
        } else {
            false
        };

        if setting_toggle(
            ui,
            "Start at login",
            "Launch CodexBar when you log in",
            &mut start_at_login,
        ) {
            if let Ok(mut state) = shared_state.lock() {
                if let Err(e) = state.settings.set_start_at_login(start_at_login) {
                    tracing::error!("Failed to set start at login: {}", e);
                } else {
                    state.settings_changed = true;
                }
            }
        }

        setting_divider(ui);

        let mut start_minimized = if let Ok(state) = shared_state.lock() {
            state.settings.start_minimized
        } else {
            false
        };

        if setting_toggle(
            ui,
            "Start minimized",
            "Start in the system tray",
            &mut start_minimized,
        ) {
            if let Ok(mut state) = shared_state.lock() {
                state.settings.start_minimized = start_minimized;
                state.settings_changed = true;
            }
        }
    });

    ui.add_space(Spacing::LG);

    section_header(ui, "Notifications");

    settings_card(ui, |ui| {
        let mut show_notifications = if let Ok(state) = shared_state.lock() {
            state.settings.show_notifications
        } else {
            true
        };

        if setting_toggle(
            ui,
            "Show notifications",
            "Alert when usage thresholds are reached",
            &mut show_notifications,
        ) {
            if let Ok(mut state) = shared_state.lock() {
                state.settings.show_notifications = show_notifications;
                state.settings_changed = true;
            }
        }

        setting_divider(ui);

        // Sound effects toggle
        let mut sound_enabled = if let Ok(state) = shared_state.lock() {
            state.settings.sound_enabled
        } else {
            true
        };

        if setting_toggle(
            ui,
            "Sound effects",
            "Play sound when thresholds are reached",
            &mut sound_enabled,
        ) {
            if let Ok(mut state) = shared_state.lock() {
                state.settings.sound_enabled = sound_enabled;
                state.settings_changed = true;
            }
        }

        // Sound volume slider (only show if sound is enabled)
        if sound_enabled {
            setting_divider(ui);

            ui.vertical(|ui| {
                let mut volume = if let Ok(state) = shared_state.lock() {
                    state.settings.sound_volume as i32
                } else {
                    100
                };

                // Title row with volume badge on right
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(localize_text("Sound volume").as_ref())
                            .size(FontSize::MD)
                            .color(Theme::TEXT_PRIMARY),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        egui::Frame::none()
                            .fill(Theme::ACCENT_PRIMARY.gamma_multiply(0.15))
                            .rounding(Rounding::same(10.0))
                            .inner_margin(egui::Margin::symmetric(10.0, 3.0))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new(format!("{}%", volume))
                                        .size(FontSize::SM)
                                        .color(Theme::ACCENT_PRIMARY)
                                        .strong(),
                                );
                            });
                    });
                });

                ui.add_space(2.0);
                ui.label(
                    RichText::new(localize_text("Volume level for alert sounds").as_ref())
                        .size(FontSize::SM)
                        .color(Theme::TEXT_MUTED),
                );
                ui.add_space(6.0);

                ui.style_mut().visuals.widgets.inactive.bg_fill = Theme::BG_TERTIARY;

                let slider = ui.add(
                    egui::Slider::new(&mut volume, 0..=100)
                        .show_value(false)
                        .trailing_fill(true),
                );

                if slider.changed() {
                    if let Ok(mut state) = shared_state.lock() {
                        state.settings.sound_volume = volume as u8;
                        state.settings_changed = true;
                    }
                }
            });
        }

        setting_divider(ui);

        // High warning threshold
        ui.vertical(|ui| {
            let mut threshold = if let Ok(state) = shared_state.lock() {
                state.settings.high_usage_threshold as i32
            } else {
                70
            };

            // Title row with percentage badge on right
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(localize_text("High warning").as_ref())
                        .size(FontSize::MD)
                        .color(Theme::TEXT_PRIMARY),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    egui::Frame::none()
                        .fill(Theme::ACCENT_PRIMARY.gamma_multiply(0.15))
                        .rounding(Rounding::same(10.0))
                        .inner_margin(egui::Margin::symmetric(10.0, 3.0))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(format!("{}%", threshold))
                                    .size(FontSize::SM)
                                    .color(Theme::ACCENT_PRIMARY)
                                    .strong(),
                            );
                        });
                });
            });

            ui.add_space(2.0);
            ui.label(
                RichText::new(localize_text("Show warning at this usage level").as_ref())
                    .size(FontSize::SM)
                    .color(Theme::TEXT_MUTED),
            );
            ui.add_space(6.0);

            ui.style_mut().visuals.widgets.inactive.bg_fill = Theme::BG_TERTIARY;

            let slider = ui.add(
                egui::Slider::new(&mut threshold, 50..=95)
                    .show_value(false)
                    .trailing_fill(true),
            );

            if slider.changed() {
                if let Ok(mut state) = shared_state.lock() {
                    state.settings.high_usage_threshold = threshold as f64;
                    state.settings_changed = true;
                }
            }
        });

        setting_divider(ui);

        // Critical alert threshold
        ui.vertical(|ui| {
            let mut threshold = if let Ok(state) = shared_state.lock() {
                state.settings.critical_usage_threshold as i32
            } else {
                90
            };

            let badge_color = Color32::from_rgb(239, 68, 68);

            // Title row with percentage badge on right
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(localize_text("Critical alert").as_ref())
                        .size(FontSize::MD)
                        .color(Theme::TEXT_PRIMARY),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    egui::Frame::none()
                        .fill(badge_color.gamma_multiply(0.15))
                        .rounding(Rounding::same(10.0))
                        .inner_margin(egui::Margin::symmetric(10.0, 3.0))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(format!("{}%", threshold))
                                    .size(FontSize::SM)
                                    .color(badge_color)
                                    .strong(),
                            );
                        });
                });
            });

            ui.add_space(2.0);
            ui.label(
                RichText::new(localize_text("Show critical alert at this level").as_ref())
                    .size(FontSize::SM)
                    .color(Theme::TEXT_MUTED),
            );
            ui.add_space(6.0);

            ui.style_mut().visuals.widgets.inactive.bg_fill = Theme::BG_TERTIARY;

            let slider = ui.add(
                egui::Slider::new(&mut threshold, 80..=100)
                    .show_value(false)
                    .trailing_fill(true),
            );

            if slider.changed() {
                if let Ok(mut state) = shared_state.lock() {
                    state.settings.critical_usage_threshold = threshold as f64;
                    state.settings_changed = true;
                }
            }
        });
    });

    ui.add_space(Spacing::LG);

    section_header(ui, "Privacy");

    settings_card(ui, |ui| {
        let mut hide_personal_info = if let Ok(state) = shared_state.lock() {
            state.settings.hide_personal_info
        } else {
            false
        };

        if setting_toggle(
            ui,
            "Hide personal info",
            "Mask emails and account names (useful for streaming)",
            &mut hide_personal_info,
        ) {
            if let Ok(mut state) = shared_state.lock() {
                state.settings.hide_personal_info = hide_personal_info;
                state.settings_changed = true;
            }
        }
    });

    ui.add_space(Spacing::LG);

    section_header(ui, "Updates");

    settings_card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(localize_text("Update channel").as_ref())
                        .size(FontSize::MD)
                        .color(Theme::TEXT_PRIMARY),
                );
                ui.label(
                    RichText::new(
                        localize_text("Choose between stable releases or beta previews").as_ref(),
                    )
                    .size(FontSize::SM)
                    .color(Theme::TEXT_MUTED),
                );
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let current_channel = if let Ok(state) = shared_state.lock() {
                    state.settings.update_channel
                } else {
                    crate::settings::UpdateChannel::Stable
                };

                egui::Frame::none()
                    .fill(Theme::BG_TERTIARY)
                    .stroke(Stroke::new(1.0, Theme::BORDER_SUBTLE))
                    .rounding(Rounding::same(Radius::SM))
                    .inner_margin(egui::Margin::symmetric(Spacing::XS, 2.0))
                    .show(ui, |ui| {
                        let channels = [
                            (crate::settings::UpdateChannel::Stable, "Stable"),
                            (crate::settings::UpdateChannel::Beta, "Beta"),
                        ];

                        let mut selected = current_channel;
                        egui::ComboBox::from_id_salt("update_channel")
                            .selected_text(
                                channels
                                    .iter()
                                    .find(|(ch, _)| *ch == selected)
                                    .map(|(_, label)| localize_text(label).to_string())
                                    .unwrap_or_else(|| localize_text("Stable").to_string()),
                            )
                            .show_ui(ui, |ui| {
                                for (channel, label) in channels {
                                    if ui
                                        .selectable_value(
                                            &mut selected,
                                            channel,
                                            localize_text(label).as_ref(),
                                        )
                                        .changed()
                                    {
                                        if let Ok(mut state) = shared_state.lock() {
                                            state.settings.update_channel = selected;
                                            state.settings_changed = true;
                                        }
                                    }
                                }
                            });
                    });
            });
        });
    });

    ui.add_space(Spacing::LG);

    section_header(ui, "Keyboard Shortcuts");

    settings_card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(
                    RichText::new(localize_text("Global shortcut").as_ref())
                        .size(FontSize::MD)
                        .color(Theme::TEXT_PRIMARY),
                );
                ui.label(
                    RichText::new(
                        localize_text("Press this key combination to open CodexBar from anywhere")
                            .as_ref(),
                    )
                    .size(FontSize::SM)
                    .color(Theme::TEXT_MUTED),
                );
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let (mut shortcut_input, status_msg) = if let Ok(state) = shared_state.lock() {
                    (
                        state.shortcut_input.clone(),
                        state.shortcut_status_msg.clone(),
                    )
                } else {
                    ("Ctrl+Shift+U".to_string(), None)
                };

                // Show status message if any
                if let Some((msg, is_error)) = &status_msg {
                    let color = if *is_error { Theme::RED } else { Theme::GREEN };
                    ui.label(RichText::new(msg).size(FontSize::XS).color(color));
                    ui.add_space(8.0);
                }

                egui::Frame::none()
                    .fill(Theme::BG_TERTIARY)
                    .stroke(Stroke::new(1.0, Theme::BORDER_SUBTLE))
                    .rounding(Rounding::same(Radius::SM))
                    .inner_margin(egui::Margin::symmetric(Spacing::SM, 4.0))
                    .show(ui, |ui| {
                        let text_edit = egui::TextEdit::singleline(&mut shortcut_input)
                            .desired_width(120.0)
                            .hint_text("e.g., Ctrl+Shift+U");
                        let response = ui.add(text_edit);

                        if response.changed() {
                            if let Ok(mut state) = shared_state.lock() {
                                state.shortcut_input = shortcut_input.clone();
                            }
                        }

                        if response.lost_focus() {
                            // Validate and save the shortcut
                            let shortcut_str = shortcut_input.trim().to_string();
                            if !shortcut_str.is_empty() {
                                // Try to parse the shortcut to validate it
                                if let Some((modifiers, key)) =
                                    crate::shortcuts::parse_shortcut(&shortcut_str)
                                {
                                    // Format it back to canonical form
                                    let formatted = format_shortcut(modifiers, key);
                                    if let Ok(mut state) = shared_state.lock() {
                                        state.settings.global_shortcut = formatted.clone();
                                        state.shortcut_input = formatted;
                                        state.settings_changed = true;
                                        state.shortcut_status_msg = Some((
                                            localize_text("Saved (restart to apply)").to_string(),
                                            false,
                                        ));
                                    }
                                } else {
                                    if let Ok(mut state) = shared_state.lock() {
                                        state.shortcut_status_msg = Some((
                                            localize_text("Invalid shortcut format").to_string(),
                                            true,
                                        ));
                                    }
                                }
                            }
                        }
                    });
            });
        });

        ui.add_space(4.0);
        ui.label(
            RichText::new(
                localize_text(
                    "Format: Ctrl+Shift+Key, Alt+Ctrl+Key, etc. Restart required to apply changes.",
                )
                .as_ref(),
            )
            .size(FontSize::XS)
            .color(Theme::TEXT_MUTED),
        );
    });
}

/// Render Display tab for viewport
fn render_display_tab(ui: &mut egui::Ui, shared_state: &Arc<Mutex<PreferencesSharedState>>) {
    section_header(ui, "Appearance");

    settings_card(ui, |ui| {
        let mut relative_time = if let Ok(state) = shared_state.lock() {
            state.settings.reset_time_relative
        } else {
            true
        };

        if setting_toggle(
            ui,
            "Relative time",
            "Show reset time as relative (3h 45m) instead of absolute",
            &mut relative_time,
        ) {
            if let Ok(mut state) = shared_state.lock() {
                state.settings.reset_time_relative = relative_time;
                state.settings_changed = true;
            }
        }

        setting_divider(ui);

        let mut surprise = if let Ok(state) = shared_state.lock() {
            state.settings.surprise_animations
        } else {
            false
        };

        if setting_toggle(
            ui,
            "Surprise animations",
            "Show occasional fun animations in the tray icon",
            &mut surprise,
        ) {
            if let Ok(mut state) = shared_state.lock() {
                state.settings.surprise_animations = surprise;
                state.settings_changed = true;
            }
        }
    });
}

/// Render API Keys tab for viewport
fn render_api_keys_tab(ui: &mut egui::Ui, shared_state: &Arc<Mutex<PreferencesSharedState>>) {
    section_header(ui, "API Keys");

    ui.label(
        RichText::new(
            localize_text("Configure access tokens for providers that require authentication.")
                .as_ref(),
        )
        .size(FontSize::SM)
        .color(Theme::TEXT_MUTED),
    );

    ui.add_space(Spacing::MD);

    // Status message
    let status_msg = if let Ok(state) = shared_state.lock() {
        state.api_key_status_msg.clone()
    } else {
        None
    };

    if let Some((msg, is_error)) = &status_msg {
        status_message(ui, msg, *is_error);
        ui.add_space(Spacing::SM);
    }

    // Get state for rendering
    let (api_keys_data, settings_data, show_input, input_provider) =
        if let Ok(state) = shared_state.lock() {
            (
                state.api_keys.clone(),
                state.settings.clone(),
                state.show_api_key_input,
                state.new_api_key_provider.clone(),
            )
        } else {
            return;
        };

    // Provider cards - one per provider
    let api_key_providers = get_api_key_providers();

    for provider_info in &api_key_providers {
        let provider_id = provider_info.id.cli_name();
        let has_key = api_keys_data.has_key(provider_id);
        let is_enabled = settings_data.enabled_providers.contains(provider_id);
        let icon = provider_icon(provider_id);
        let color = provider_color(provider_id);

        // Card with left accent bar
        let accent_color = if has_key {
            Theme::GREEN
        } else if is_enabled {
            Theme::ORANGE
        } else {
            Theme::BG_TERTIARY
        };

        egui::Frame::none()
            .fill(Theme::BG_SECONDARY)
            .rounding(Rounding::same(Radius::MD))
            .inner_margin(egui::Margin::same(0.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // Left accent bar
                    let bar_rect = Rect::from_min_size(ui.cursor().min, Vec2::new(3.0, 48.0));
                    ui.painter().rect_filled(
                        bar_rect,
                        Rounding {
                            nw: Radius::MD,
                            sw: Radius::MD,
                            ne: 0.0,
                            se: 0.0,
                        },
                        accent_color,
                    );
                    ui.add_space(3.0);

                    // Content
                    ui.vertical(|ui| {
                        ui.add_space(Spacing::XS);

                        // Row 1: Icon, Name, Status badge, and Add Key button
                        ui.horizontal(|ui| {
                            ui.add_space(Spacing::XS);

                            // Try to render SVG icon from cache
                            let icon_size = 20.0;
                            VIEWPORT_ICON_CACHE.with(|cache| {
                                if let Some(texture) = cache.borrow_mut().get_icon(
                                    ui.ctx(),
                                    provider_id,
                                    icon_size as u32,
                                ) {
                                    ui.add(
                                        egui::Image::new(texture)
                                            .fit_to_exact_size(Vec2::splat(icon_size)),
                                    );
                                } else {
                                    ui.label(RichText::new(icon).size(FontSize::LG).color(color));
                                }
                            });

                            ui.add_space(Spacing::XS);
                            ui.label(
                                RichText::new(provider_info.name)
                                    .size(FontSize::MD)
                                    .color(Theme::TEXT_PRIMARY)
                                    .strong(),
                            );

                            ui.add_space(Spacing::XS);

                            if has_key {
                                badge(ui, localize_text("✓ Set").as_ref(), Theme::GREEN);
                            } else if is_enabled {
                                egui::Frame::none()
                                    .fill(Theme::ORANGE)
                                    .rounding(Rounding::same(Radius::PILL))
                                    .inner_margin(egui::Margin::symmetric(Spacing::XS, 2.0))
                                    .show(ui, |ui| {
                                        ui.label(
                                            RichText::new(localize_text("Needs key").as_ref())
                                                .size(FontSize::XS)
                                                .color(Color32::BLACK),
                                        );
                                    });
                            }

                            // Right-aligned: Add Key button
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.add_space(Spacing::XS);
                                    if !has_key {
                                        if primary_button(ui, "+ Add Key") {
                                            if let Ok(mut state) = shared_state.lock() {
                                                state.new_api_key_provider =
                                                    provider_id.to_string();
                                                state.show_api_key_input = true;
                                                state.new_api_key_value.clear();
                                            }
                                        }
                                    }
                                },
                            );
                        });

                        // Row 2: Env var, masked key, and actions
                        ui.horizontal(|ui| {
                            ui.add_space(Spacing::XS);

                            if let Some(env_var) = provider_info.api_key_env_var {
                                ui.label(
                                    RichText::new(format!("{}: {}", localize_text("Env"), env_var))
                                        .size(FontSize::XS)
                                        .color(Theme::TEXT_MUTED)
                                        .monospace(),
                                );
                            }

                            if has_key {
                                ui.add_space(Spacing::SM);
                                if let Some(key_info) = api_keys_data
                                    .get_all_for_display()
                                    .iter()
                                    .find(|k| k.provider_id == provider_id)
                                {
                                    ui.label(
                                        RichText::new(&key_info.masked_key)
                                            .size(FontSize::XS)
                                            .color(Theme::TEXT_MUTED)
                                            .monospace(),
                                    );
                                }

                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.add_space(Spacing::XS);
                                        if small_button(ui, "Remove", Theme::RED) {
                                            if let Ok(mut state) = shared_state.lock() {
                                                state.api_keys.remove(provider_id);
                                                let _ = state.api_keys.save();
                                                state.api_key_status_msg = Some((
                                                    match current_ui_language() {
                                                        UiLanguage::English => format!(
                                                            "Removed API key for {}",
                                                            provider_info.name
                                                        ),
                                                        UiLanguage::Chinese => format!(
                                                            "已移除 {} 的 API 密钥",
                                                            provider_info.name
                                                        ),
                                                    },
                                                    false,
                                                ));
                                            }
                                        }
                                    },
                                );
                            } else {
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        ui.add_space(Spacing::XS);
                                        if let Some(url) = provider_info.dashboard_url {
                                            if text_button(
                                                ui,
                                                localize_text("Get key →").as_ref(),
                                                Theme::ACCENT_PRIMARY,
                                            ) {
                                                let _ = open::that(url);
                                            }
                                        }
                                    },
                                );
                            }
                        });

                        ui.add_space(Spacing::XS);
                    });
                });
            });

        ui.add_space(Spacing::XS);
    }

    // API Key input modal
    if show_input {
        ui.add_space(Spacing::MD);

        let provider_name = ProviderId::from_cli_name(&input_provider)
            .map(|id| id.display_name())
            .unwrap_or(&input_provider);

        egui::Frame::none()
            .fill(Theme::BG_TERTIARY)
            .stroke(Stroke::new(1.0, Theme::ACCENT_PRIMARY.gamma_multiply(0.4)))
            .rounding(Rounding::same(Radius::LG))
            .inner_margin(Spacing::LG)
            .show(ui, |ui| {
                ui.label(
                    RichText::new(match current_ui_language() {
                        UiLanguage::English => format!("Enter API Key for {}", provider_name),
                        UiLanguage::Chinese => {
                            format!("{} {}", localize_text("Enter API Key for"), provider_name)
                        }
                    })
                    .size(FontSize::MD)
                    .color(Theme::TEXT_PRIMARY)
                    .strong(),
                );

                ui.add_space(Spacing::SM);

                // Get current value for text edit
                let mut current_value = if let Ok(state) = shared_state.lock() {
                    state.new_api_key_value.clone()
                } else {
                    String::new()
                };

                let text_edit = egui::TextEdit::singleline(&mut current_value)
                    .password(true)
                    .desired_width(ui.available_width())
                    .hint_text(localize_text("Paste your API key here...").as_ref());
                let response = ui.add(text_edit);

                if response.changed() {
                    if let Ok(mut state) = shared_state.lock() {
                        state.new_api_key_value = current_value.clone();
                    }
                }

                ui.add_space(Spacing::MD);

                ui.horizontal(|ui| {
                    let can_save = !current_value.trim().is_empty();

                    if ui
                        .add_enabled(
                            can_save,
                            egui::Button::new(
                                RichText::new(localize_text("Save").as_ref())
                                    .size(FontSize::SM)
                                    .color(Color32::WHITE),
                            )
                            .fill(if can_save {
                                Theme::GREEN
                            } else {
                                Theme::BG_TERTIARY
                            })
                            .rounding(Rounding::same(Radius::SM))
                            .min_size(Vec2::new(80.0, 32.0)),
                        )
                        .clicked()
                    {
                        if let Ok(mut state) = shared_state.lock() {
                            let provider = state.new_api_key_provider.clone();
                            let value = state.new_api_key_value.trim().to_string();
                            state.api_keys.set(&provider, &value, None);
                            if let Err(e) = state.api_keys.save() {
                                state.api_key_status_msg = Some((
                                    match current_ui_language() {
                                        UiLanguage::English => format!("Failed to save: {}", e),
                                        UiLanguage::Chinese => format!("保存失败：{}", e),
                                    },
                                    true,
                                ));
                            } else {
                                state.api_key_status_msg = Some((
                                    match current_ui_language() {
                                        UiLanguage::English => {
                                            format!("API key saved for {}", provider_name)
                                        }
                                        UiLanguage::Chinese => {
                                            format!("已保存 {} 的 API 密钥", provider_name)
                                        }
                                    },
                                    false,
                                ));
                                state.show_api_key_input = false;
                                state.new_api_key_value.clear();
                            }
                        }
                    }

                    ui.add_space(Spacing::XS);

                    if ui
                        .add(
                            egui::Button::new(
                                RichText::new(localize_text("Cancel").as_ref())
                                    .size(FontSize::SM)
                                    .color(Theme::TEXT_MUTED),
                            )
                            .fill(Color32::TRANSPARENT)
                            .stroke(Stroke::new(1.0, Theme::BORDER_SUBTLE))
                            .rounding(Rounding::same(Radius::SM)),
                        )
                        .clicked()
                    {
                        if let Ok(mut state) = shared_state.lock() {
                            state.show_api_key_input = false;
                            state.new_api_key_value.clear();
                        }
                    }
                });
            });
    }
}

/// Render Cookies tab for viewport
fn render_cookies_tab(ui: &mut egui::Ui, shared_state: &Arc<Mutex<PreferencesSharedState>>) {
    section_header(ui, "Browser Cookies");

    ui.label(
        RichText::new(
            localize_text(
                "Cookies are automatically extracted from Chrome, Edge, Brave, and Firefox.",
            )
            .as_ref(),
        )
        .size(FontSize::SM)
        .color(Theme::TEXT_MUTED),
    );

    ui.add_space(Spacing::LG);

    // Status message
    let status_msg = if let Ok(state) = shared_state.lock() {
        state.cookie_status_msg.clone()
    } else {
        None
    };

    if let Some((msg, is_error)) = &status_msg {
        status_message(ui, msg, *is_error);
        ui.add_space(Spacing::MD);
    }

    // Get saved cookies
    let saved_cookies = if let Ok(state) = shared_state.lock() {
        state.cookies.get_all_for_display()
    } else {
        Vec::new()
    };

    if !saved_cookies.is_empty() {
        section_header(ui, "Saved Cookies");

        settings_card(ui, |ui| {
            let mut to_remove: Option<String> = None;
            let len = saved_cookies.len();

            for (i, cookie_info) in saved_cookies.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&cookie_info.provider)
                            .size(FontSize::MD)
                            .color(Theme::TEXT_PRIMARY),
                    );
                    ui.label(
                        RichText::new(format!("· {}", &cookie_info.saved_at))
                            .size(FontSize::SM)
                            .color(Theme::TEXT_MUTED),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if small_button(ui, "Remove", Theme::RED) {
                            to_remove = Some(cookie_info.provider_id.clone());
                        }
                    });
                });

                if i < len - 1 {
                    setting_divider(ui);
                }
            }

            if let Some(provider_id) = to_remove {
                if let Ok(mut state) = shared_state.lock() {
                    state.cookies.remove(&provider_id);
                    let _ = state.cookies.save();
                    state.cookie_status_msg = Some((
                        match current_ui_language() {
                            UiLanguage::English => format!("Removed cookie for {}", provider_id),
                            UiLanguage::Chinese => format!("已移除 {} 的 Cookie", provider_id),
                        },
                        false,
                    ));
                }
            }
        });

        ui.add_space(Spacing::XL);
    }

    // Add manual cookie
    section_header(ui, "Add Manual Cookie");

    settings_card(ui, |ui| {
        // Get current provider selection
        let current_provider = if let Ok(state) = shared_state.lock() {
            state.new_cookie_provider.clone()
        } else {
            String::new()
        };

        // Provider selection row
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(localize_text("Provider").as_ref())
                    .size(FontSize::MD)
                    .color(Theme::TEXT_PRIMARY),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let combo = egui::ComboBox::from_id_salt("cookie_provider_viewport")
                    .selected_text(if current_provider.is_empty() {
                        localize_text("Select...").to_string()
                    } else {
                        current_provider.clone()
                    })
                    .show_ui(ui, |ui| {
                        let web_providers = ["claude", "cursor", "kimi"];
                        for provider_name in web_providers {
                            if let Some(id) = ProviderId::from_cli_name(provider_name) {
                                if ui
                                    .selectable_label(
                                        current_provider == provider_name,
                                        id.display_name(),
                                    )
                                    .clicked()
                                {
                                    if let Ok(mut state) = shared_state.lock() {
                                        state.new_cookie_provider = provider_name.to_string();
                                    }
                                }
                            }
                        }
                    });
                let _ = combo;
            });
        });

        ui.add_space(Spacing::MD);
        setting_divider(ui);
        ui.add_space(Spacing::SM);

        // Cookie header label
        ui.label(
            RichText::new(localize_text("Cookie header").as_ref())
                .size(FontSize::MD)
                .color(Theme::TEXT_PRIMARY),
        );
        ui.add_space(Spacing::SM);

        // Get current cookie value
        let mut current_value = if let Ok(state) = shared_state.lock() {
            state.new_cookie_value.clone()
        } else {
            String::new()
        };

        // Styled text input
        egui::Frame::none()
            .fill(Theme::INPUT_BG)
            .stroke(Stroke::new(1.0, Theme::BORDER_SUBTLE))
            .rounding(Rounding::same(Radius::SM))
            .inner_margin(Spacing::SM)
            .show(ui, |ui| {
                let text_edit = egui::TextEdit::multiline(&mut current_value)
                    .desired_width(ui.available_width())
                    .desired_rows(4)
                    .frame(false)
                    .hint_text(
                        localize_text("Paste cookie header from browser dev tools").as_ref(),
                    );
                let response = ui.add(text_edit);

                if response.changed() {
                    if let Ok(mut state) = shared_state.lock() {
                        state.new_cookie_value = current_value.clone();
                    }
                }
            });

        ui.add_space(Spacing::LG);

        // Re-fetch current provider for save button check
        let (save_provider, save_value) = if let Ok(state) = shared_state.lock() {
            (
                state.new_cookie_provider.clone(),
                state.new_cookie_value.clone(),
            )
        } else {
            (String::new(), String::new())
        };

        let can_save = !save_provider.is_empty() && !save_value.is_empty();

        if ui
            .add_enabled(
                can_save,
                egui::Button::new(
                    RichText::new(localize_text("Save Cookie").as_ref())
                        .size(FontSize::SM)
                        .color(if can_save {
                            Color32::WHITE
                        } else {
                            Theme::TEXT_MUTED
                        }),
                )
                .fill(if can_save {
                    Theme::ACCENT_PRIMARY
                } else {
                    Theme::BG_TERTIARY
                })
                .stroke(if can_save {
                    Stroke::NONE
                } else {
                    Stroke::new(1.0, Theme::BORDER_SUBTLE)
                })
                .rounding(Rounding::same(Radius::MD))
                .min_size(Vec2::new(120.0, 36.0)),
            )
            .clicked()
        {
            if let Ok(mut state) = shared_state.lock() {
                let provider = state.new_cookie_provider.clone();
                let value = state.new_cookie_value.clone();
                state.cookies.set(&provider, &value);
                if let Err(e) = state.cookies.save() {
                    state.cookie_status_msg = Some((
                        match current_ui_language() {
                            UiLanguage::English => format!("Failed to save: {}", e),
                            UiLanguage::Chinese => format!("保存失败：{}", e),
                        },
                        true,
                    ));
                } else {
                    let provider_name = ProviderId::from_cli_name(&provider)
                        .map(|id| id.display_name().to_string())
                        .unwrap_or_else(|| provider.clone());
                    state.cookie_status_msg = Some((
                        match current_ui_language() {
                            UiLanguage::English => format!("Cookie saved for {}", provider_name),
                            UiLanguage::Chinese => format!("已保存 {} 的 Cookie", provider_name),
                        },
                        false,
                    ));
                    state.new_cookie_provider.clear();
                    state.new_cookie_value.clear();
                }
            }
        }
    });
}

/// Render Advanced tab for viewport
fn render_advanced_tab(ui: &mut egui::Ui, shared_state: &Arc<Mutex<PreferencesSharedState>>) {
    section_header(ui, "Refresh");

    settings_card(ui, |ui| {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(localize_text("Auto-refresh interval").as_ref())
                    .size(FontSize::MD)
                    .color(Theme::TEXT_PRIMARY),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let current_interval = if let Ok(state) = shared_state.lock() {
                    state.settings.refresh_interval_secs
                } else {
                    60
                };

                let intervals = [
                    (0, localize_text("Never").to_string()),
                    (30, localize_text("30 sec").to_string()),
                    (60, localize_text("1 min").to_string()),
                    (300, localize_text("5 min").to_string()),
                    (600, localize_text("10 min").to_string()),
                ];
                let custom_label = localize_text("Custom").to_string();

                let current_label = intervals
                    .iter()
                    .find(|(v, _)| *v == current_interval)
                    .map(|(_, l)| l.as_str())
                    .unwrap_or(custom_label.as_str());

                // Style combobox to match theme
                ui.style_mut().visuals.widgets.inactive.bg_fill = Theme::BG_TERTIARY;
                ui.style_mut().visuals.widgets.inactive.weak_bg_fill = Theme::BG_TERTIARY;
                ui.style_mut().visuals.widgets.hovered.bg_fill = Theme::CARD_BG_HOVER;
                ui.style_mut().visuals.widgets.active.bg_fill = Theme::CARD_BG;
                ui.style_mut().visuals.widgets.inactive.rounding = Rounding::same(Radius::SM);
                ui.style_mut().visuals.widgets.hovered.rounding = Rounding::same(Radius::SM);
                ui.style_mut().visuals.widgets.active.rounding = Rounding::same(Radius::SM);

                egui::ComboBox::from_id_salt("refresh_interval")
                    .selected_text(current_label)
                    .width(90.0)
                    .show_ui(ui, |ui| {
                        for (value, label) in &intervals {
                            if ui
                                .selectable_label(current_interval == *value, label.as_str())
                                .clicked()
                            {
                                if let Ok(mut state) = shared_state.lock() {
                                    state.settings.refresh_interval_secs = *value;
                                    state.settings_changed = true;
                                }
                            }
                        }
                    });
            });
        });
    });
}

/// Render About tab for viewport
fn render_about_tab(ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(Spacing::XL);

        // App icon
        ui.label(RichText::new("◆").size(48.0).color(Theme::ACCENT_PRIMARY));

        ui.add_space(Spacing::MD);

        // App name
        ui.label(
            RichText::new("CodexBar")
                .size(FontSize::XXL)
                .color(Theme::TEXT_PRIMARY)
                .strong(),
        );

        ui.add_space(Spacing::SM);

        // Version
        ui.label(
            RichText::new(match current_ui_language() {
                UiLanguage::English => format!("Version {}", env!("CARGO_PKG_VERSION")),
                UiLanguage::Chinese => {
                    format!("{} {}", localize_text("Version"), env!("CARGO_PKG_VERSION"))
                }
            })
            .size(FontSize::MD)
            .color(Theme::TEXT_SECONDARY),
        );

        ui.add_space(Spacing::SM);

        // Tagline
        ui.label(
            RichText::new(localize_text("Monitor your AI provider usage limits").as_ref())
                .size(FontSize::SM)
                .color(Theme::TEXT_MUTED),
        );

        ui.add_space(Spacing::XL);
    });

    // Credits section
    section_header(ui, "Credits");
    settings_card(ui, |ui| {
        ui.vertical(|ui| {
            ui.label(
                RichText::new(localize_text("Created by CodexBar Contributors").as_ref())
                    .size(FontSize::SM)
                    .color(Theme::TEXT_PRIMARY),
            );
            ui.add_space(Spacing::XS);
            ui.label(
                RichText::new(localize_text("MIT License").as_ref())
                    .size(FontSize::SM)
                    .color(Theme::TEXT_MUTED),
            );
        });
    });

    ui.add_space(Spacing::MD);

    // Links section
    section_header(ui, "Links");
    settings_card(ui, |ui| {
        ui.vertical(|ui| {
            if text_button(ui, "→ View on GitHub", Theme::ACCENT_PRIMARY) {
                let _ = open::that("https://github.com/Finesssee/Win-CodexBar");
            }
            ui.add_space(Spacing::XS);
            if text_button(ui, "→ Report an Issue", Theme::ACCENT_PRIMARY) {
                let _ = open::that("https://github.com/Finesssee/Win-CodexBar/issues");
            }
        });
    });

    ui.add_space(Spacing::MD);

    // Build info section
    section_header(ui, "Build Info");
    settings_card(ui, |ui| {
        ui.vertical(|ui| {
            let git_commit = option_env!("GIT_COMMIT").unwrap_or("dev");
            let build_date = option_env!("BUILD_DATE").unwrap_or("unknown");

            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(localize_text("Commit:").as_ref())
                        .size(FontSize::SM)
                        .color(Theme::TEXT_MUTED),
                );
                ui.label(
                    RichText::new(git_commit)
                        .size(FontSize::SM)
                        .color(Theme::TEXT_SECONDARY)
                        .monospace(),
                );
            });

            ui.add_space(Spacing::XS);

            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(localize_text("Built:").as_ref())
                        .size(FontSize::SM)
                        .color(Theme::TEXT_MUTED),
                );
                ui.label(
                    RichText::new(build_date)
                        .size(FontSize::SM)
                        .color(Theme::TEXT_SECONDARY),
                );
            });
        });
    });
}

/// Render Providers tab for viewport
fn render_providers_tab(
    ui: &mut egui::Ui,
    _available_height: f32,
    shared_state: &Arc<Mutex<PreferencesSharedState>>,
) {
    section_header(ui, "Enabled Providers");

    let providers = ProviderId::all();

    for provider_id in providers {
        let is_enabled = if let Ok(state) = shared_state.lock() {
            state
                .settings
                .enabled_providers
                .contains(provider_id.cli_name())
        } else {
            true
        };

        settings_card(ui, |ui| {
            ui.horizontal(|ui| {
                let brand_color = provider_color(provider_id.cli_name());

                ui.label(
                    RichText::new(provider_icon(provider_id.cli_name()))
                        .size(FontSize::LG)
                        .color(brand_color),
                );

                ui.add_space(8.0);

                ui.label(
                    RichText::new(provider_id.display_name())
                        .size(FontSize::MD)
                        .color(Theme::TEXT_PRIMARY),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut enabled = is_enabled;
                    if switch_toggle(
                        ui,
                        egui::Id::new(format!("provider_{}", provider_id.cli_name())),
                        &mut enabled,
                    ) {
                        if let Ok(mut state) = shared_state.lock() {
                            let name = provider_id.cli_name().to_string();
                            if enabled {
                                state.settings.enabled_providers.insert(name);
                            } else {
                                state.settings.enabled_providers.remove(&name);
                            }
                            state.settings_changed = true;
                        }
                    }
                });
            });
        });

        ui.add_space(Spacing::XS);
    }
}

// ════════════════════════════════════════════════════════════════════════════════
// HELPER COMPONENTS - Refined, reusable UI elements
// ════════════════════════════════════════════════════════════════════════════════

/// Section header - subtle, uppercase
fn section_header(ui: &mut egui::Ui, text: &str) {
    let localized = localize_text(text);
    let display = if localized.is_ascii() {
        localized.to_uppercase()
    } else {
        localized.to_string()
    };
    ui.add_space(Spacing::SM);
    ui.label(
        RichText::new(display)
            .size(FontSize::XS)
            .color(Theme::TEXT_SECTION)
            .strong(),
    );
    ui.add_space(Spacing::MD);
}

/// Settings card container - grouped settings with rounded corners and border
fn settings_card(ui: &mut egui::Ui, content: impl FnOnce(&mut egui::Ui)) {
    egui::Frame::none()
        .fill(Theme::BG_SECONDARY)
        .stroke(Stroke::new(1.0, Theme::CARD_BORDER))
        .rounding(Rounding::same(Radius::LG))
        .inner_margin(Spacing::SM)
        .show(ui, content);
}

/// Divider line between settings in a card
fn setting_divider(ui: &mut egui::Ui) {
    ui.add_space(Spacing::SM);
    let rect = Rect::from_min_size(ui.cursor().min, Vec2::new(ui.available_width(), 1.0));
    ui.painter().rect_filled(rect, 0.0, Theme::SEPARATOR);
    ui.add_space(Spacing::SM + 1.0);
}

/// iOS-style switch toggle component
/// Size: 36x20 pixels with animated knob position
fn switch_toggle(ui: &mut egui::Ui, id: impl std::hash::Hash, value: &mut bool) -> bool {
    let desired_size = Vec2::new(36.0, 20.0);
    let (rect, response) = ui.allocate_exact_size(desired_size, egui::Sense::click());

    let mut changed = false;
    if response.clicked() {
        *value = !*value;
        changed = true;
    }

    // Animate the knob position
    let animation_progress = ui.ctx().animate_bool_responsive(egui::Id::new(id), *value);

    // Track colors
    let track_color = if animation_progress > 0.5 {
        Theme::ACCENT_PRIMARY
    } else {
        Theme::BG_TERTIARY
    };

    // Draw track (rounded rectangle)
    let track_rounding = rect.height() / 2.0;
    ui.painter()
        .rect_filled(rect, Rounding::same(track_rounding), track_color);

    // Knob properties
    let knob_margin = 2.0;
    let knob_diameter = rect.height() - knob_margin * 2.0;
    let knob_travel = rect.width() - knob_diameter - knob_margin * 2.0;

    // Interpolate knob position
    let knob_x = rect.min.x + knob_margin + (knob_travel * animation_progress);
    let knob_center = egui::pos2(knob_x + knob_diameter / 2.0, rect.center().y);

    // Draw knob (white circle)
    ui.painter()
        .circle_filled(knob_center, knob_diameter / 2.0, Color32::WHITE);

    changed
}

/// Toggle setting row - iOS-style switch on right, title and subtitle on left
fn setting_toggle(ui: &mut egui::Ui, title: &str, subtitle: &str, value: &mut bool) -> bool {
    let mut changed = false;
    let title_text = localize_text(title);
    let subtitle_text = localize_text(subtitle);

    ui.horizontal(|ui| {
        // Labels on the left
        ui.vertical(|ui| {
            ui.label(
                RichText::new(title_text.as_ref())
                    .size(FontSize::MD)
                    .color(Theme::TEXT_PRIMARY),
            );
            ui.label(
                RichText::new(subtitle_text.as_ref())
                    .size(FontSize::SM)
                    .color(Theme::TEXT_MUTED),
            );
        });

        // Switch on the right
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if switch_toggle(ui, format!("switch_{}", title), value) {
                changed = true;
            }
        });
    });

    changed
}

/// Status message banner
fn status_message(ui: &mut egui::Ui, msg: &str, is_error: bool) {
    let (bg_color, text_color, icon) = if is_error {
        (
            Color32::from_rgba_unmultiplied(239, 68, 68, 15),
            Theme::RED,
            "✕",
        )
    } else {
        (
            Color32::from_rgba_unmultiplied(34, 197, 94, 15),
            Theme::GREEN,
            "✓",
        )
    };

    egui::Frame::none()
        .fill(bg_color)
        .rounding(Rounding::same(Radius::SM))
        .inner_margin(Spacing::SM)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new(icon).size(FontSize::SM).color(text_color));
                ui.add_space(Spacing::XS);
                ui.label(RichText::new(msg).size(FontSize::SM).color(text_color));
            });
        });
}

/// Small badge
fn badge(ui: &mut egui::Ui, text: &str, color: Color32) {
    egui::Frame::none()
        .fill(color.gamma_multiply(0.15))
        .rounding(Rounding::same(Radius::XS))
        .inner_margin(egui::Margin::symmetric(Spacing::XS, 2.0))
        .show(ui, |ui| {
            ui.label(RichText::new(text).size(FontSize::XS).color(color));
        });
}

/// Small text button
fn small_button(ui: &mut egui::Ui, text: &str, color: Color32) -> bool {
    let localized = localize_text(text);
    ui.add(
        egui::Button::new(
            RichText::new(localized.as_ref())
                .size(FontSize::SM)
                .color(color),
        )
        .fill(color.gamma_multiply(0.1))
        .rounding(Rounding::same(Radius::SM)),
    )
    .clicked()
}

/// Text-only button (no background)
fn text_button(ui: &mut egui::Ui, text: &str, color: Color32) -> bool {
    let localized = localize_text(text);
    ui.add(
        egui::Button::new(
            RichText::new(localized.as_ref())
                .size(FontSize::SM)
                .color(color),
        )
        .fill(Color32::TRANSPARENT)
        .stroke(Stroke::NONE),
    )
    .clicked()
}

/// Primary action button
fn primary_button(ui: &mut egui::Ui, text: &str) -> bool {
    let localized = localize_text(text);
    ui.add(
        egui::Button::new(
            RichText::new(localized.as_ref())
                .size(FontSize::SM)
                .color(Color32::WHITE),
        )
        .fill(Theme::ACCENT_PRIMARY)
        .rounding(Rounding::same(Radius::SM)),
    )
    .clicked()
}
