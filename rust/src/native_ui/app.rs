//! Main egui application - Modern refined menubar popup
//! Clean, spacious design with rich visual hierarchy

use eframe::egui::{
    self, Color32, FontData, FontDefinitions, FontFamily, Rect, RichText, Rounding, Stroke, Vec2,
};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use super::charts::{
    ChartPoint, CostHistoryChart, CreditsHistoryChart, ServiceUsage, UsageBreakdownChart,
    UsageBreakdownPoint,
};
use super::preferences::PreferencesWindow;
use super::provider_icons::ProviderIconCache;
use super::theme::{FontSize, Radius, Spacing, Theme, provider_color, status_color};
use crate::browser::cookies::get_cookie_header;
use crate::core::{
    FetchContext, OpenAIDashboardCacheStore, PersonalInfoRedactor, Provider, ProviderFetchResult,
    ProviderId, RateWindow,
};
use crate::core::{TokenAccountStore, TokenAccountSupport};
use crate::cost_scanner::get_daily_cost_history;
use crate::locale::{LocaleKey, get_text as locale_text};
use crate::login::LoginPhase;
use crate::notifications::NotificationManager;
use crate::providers::*;
use crate::settings::{ApiKeys, Language, ManualCookies, Settings, UpdateChannel};
use crate::shortcuts::{ShortcutManager, parse_shortcut};
use crate::status::{StatusLevel, fetch_provider_status, get_status_page_url};
use crate::tray::{
    LoadingPattern, ProviderUsage, SurpriseAnimation, TrayMenuAction, UnifiedTrayManager,
};
use crate::updater::{self, UpdateInfo, UpdateState};

#[cfg(windows)]
fn restore_main_window() {
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, IsIconic, SW_RESTORE, SW_SHOW, SetForegroundWindow, ShowWindow,
    };
    use windows::core::w;

    unsafe {
        if let Ok(hwnd) = FindWindowW(None, w!("CodexBar"))
            && !hwnd.is_invalid()
        {
            if IsIconic(hwnd).as_bool() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            } else {
                let _ = ShowWindow(hwnd, SW_SHOW);
            }
            let _ = SetForegroundWindow(hwnd);
        }
    }
}

#[cfg(windows)]
fn show_main_window_no_focus() {
    use windows::Win32::UI::WindowsAndMessaging::{FindWindowW, SW_SHOWNOACTIVATE, ShowWindow};
    use windows::core::w;

    unsafe {
        if let Ok(hwnd) = FindWindowW(None, w!("CodexBar"))
            && !hwnd.is_invalid()
        {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        }
    }
}

#[cfg(not(windows))]
fn show_main_window_no_focus() {}

#[cfg(not(windows))]
fn restore_main_window() {}

#[cfg(windows)]
fn is_remote_session() -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_REMOTESESSION};

    unsafe { GetSystemMetrics(SM_REMOTESESSION) != 0 }
}

#[cfg_attr(not(windows), allow(dead_code))]
fn remote_session_error_message() -> &'static str {
    "CodexBar can't render its native window inside a Windows Remote Desktop session on this machine.\n\nRun it from the local desktop session instead, or use the CLI while connected over RDP:\n\n  codexbar usage -p claude\n\nThe startup log is written to %TEMP%\\codexbar_launch.log."
}

#[cfg(windows)]
fn show_remote_session_error_dialog() {
    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    use windows::core::{HSTRING, w};

    let message = HSTRING::from(remote_session_error_message());

    unsafe {
        let _ = MessageBoxW(None, &message, w!("CodexBar"), MB_OK | MB_ICONERROR);
    }
}

fn build_native_options() -> eframe::NativeOptions {
    let viewport = egui::ViewportBuilder::default()
        .with_inner_size([360.0, 500.0])
        .with_min_inner_size([320.0, 320.0])
        .with_clamp_size_to_monitor_size(true)
        .with_resizable(true)
        .with_decorations(true)
        .with_transparent(false)
        .with_always_on_top()
        .with_title("CodexBar");

    eframe::NativeOptions {
        viewport,
        persist_window: false, // Don't persist window state
        ..Default::default()
    }
}

#[derive(Clone, Debug)]
pub struct ProviderData {
    pub name: String,
    pub display_name: String,
    pub account: Option<String>, // Account email for display
    pub session_percent: Option<f64>,
    pub session_reset: Option<String>,
    pub weekly_percent: Option<f64>,
    pub weekly_reset: Option<String>,
    pub monthly_percent: Option<f64>, // Tertiary (30-day) usage for Infini
    pub monthly_reset: Option<String>,
    pub model_percent: Option<f64>,
    pub model_name: Option<String>,
    pub plan: Option<String>,
    pub error: Option<String>,
    pub dashboard_url: Option<String>,
    pub pace_percent: Option<f64>,
    pub pace_lasts_to_reset: bool,
    pub cost_used: Option<String>,
    pub credits_remaining: Option<f64>,
    pub credits_percent: Option<f64>,
    pub status_level: StatusLevel,
    pub status_description: Option<String>,
    pub cost_history: Vec<(String, f64)>,
    pub credits_history: Vec<(String, f64)>,
    pub usage_breakdown: Vec<UsageBreakdownPoint>,
}

impl ProviderData {
    fn placeholder(id: ProviderId) -> Self {
        Self {
            name: id.cli_name().to_string(),
            display_name: id.display_name().to_string(),
            account: None,
            session_percent: None,
            session_reset: None,
            weekly_percent: None,
            weekly_reset: None,
            monthly_percent: None,
            monthly_reset: None,
            model_percent: None,
            model_name: None,
            plan: None,
            error: None,
            dashboard_url: None,
            pace_percent: None,
            pace_lasts_to_reset: false,
            cost_used: None,
            credits_remaining: None,
            credits_percent: None,
            status_level: StatusLevel::Unknown,
            status_description: None,
            cost_history: Vec::new(),
            credits_history: Vec::new(),
            usage_breakdown: Vec::new(),
        }
    }

    fn from_result(
        id: ProviderId,
        result: &ProviderFetchResult,
        metadata: &crate::core::ProviderMetadata,
        reset_time_relative: bool,
        ui_language: Language,
    ) -> Self {
        let snapshot = &result.usage;
        let (pace_percent, pace_lasts) = calculate_pace(&snapshot.primary);

        let (cost_used, credits_remaining, credits_percent) = if let Some(ref cost) = result.cost {
            if cost.period == "Credits" {
                // Use the limit from the cost snapshot if available, otherwise default to 1000
                let scale = cost.limit.unwrap_or(1000.0);
                let remaining = cost.used;
                let percent = if scale > 0.0 {
                    (remaining / scale * 100.0).clamp(0.0, 100.0)
                } else {
                    0.0
                };
                (None, Some(remaining), Some(percent))
            } else {
                (Some(cost.format_used()), None, None)
            }
        } else {
            (None, None, None)
        };

        Self {
            name: id.cli_name().to_string(),
            display_name: id.display_name().to_string(),
            account: snapshot.account_email.clone(), // Account email if available
            session_percent: Some(snapshot.primary.used_percent),
            session_reset: snapshot
                .primary
                .resets_at
                .map(|t| format_reset_time(t, reset_time_relative, ui_language)),
            weekly_percent: snapshot.secondary.as_ref().map(|s| s.used_percent),
            weekly_reset: snapshot.secondary.as_ref().and_then(|s| {
                s.resets_at
                    .map(|t| format_reset_time(t, reset_time_relative, ui_language))
            }),
            monthly_percent: snapshot.tertiary.as_ref().map(|t| t.used_percent),
            monthly_reset: snapshot.tertiary.as_ref().and_then(|t| {
                t.resets_at
                    .map(|r| format_reset_time(r, reset_time_relative, ui_language))
            }),
            model_percent: snapshot.model_specific.as_ref().map(|m| m.used_percent),
            model_name: snapshot
                .model_specific
                .as_ref()
                .and_then(|m| m.reset_description.clone()),
            plan: snapshot.login_method.clone(),
            error: None,
            dashboard_url: metadata.dashboard_url.map(|s| s.to_string()),
            pace_percent,
            pace_lasts_to_reset: pace_lasts,
            cost_used,
            credits_remaining,
            credits_percent,
            status_level: StatusLevel::Unknown,
            status_description: None,
            cost_history: Vec::new(),
            credits_history: Vec::new(),
            usage_breakdown: Vec::new(),
        }
    }

    fn from_error(id: ProviderId, error: String) -> Self {
        Self {
            name: id.cli_name().to_string(),
            display_name: id.display_name().to_string(),
            account: None,
            session_percent: None,
            session_reset: None,
            weekly_percent: None,
            weekly_reset: None,
            monthly_percent: None,
            monthly_reset: None,
            model_percent: None,
            model_name: None,
            plan: None,
            error: Some(error),
            dashboard_url: None,
            pace_percent: None,
            pace_lasts_to_reset: false,
            cost_used: None,
            credits_remaining: None,
            credits_percent: None,
            status_level: StatusLevel::Unknown,
            status_description: None,
            cost_history: Vec::new(),
            credits_history: Vec::new(),
            usage_breakdown: Vec::new(),
        }
    }

    /// Get the preferred metric percent based on the MetricPreference setting
    pub fn get_preferred_metric(&self, pref: crate::settings::MetricPreference) -> f64 {
        match pref {
            crate::settings::MetricPreference::Session => self.session_percent.unwrap_or(0.0),
            crate::settings::MetricPreference::Weekly => self
                .weekly_percent
                .unwrap_or_else(|| self.session_percent.unwrap_or(0.0)),
            crate::settings::MetricPreference::Model => self
                .model_percent
                .unwrap_or_else(|| self.session_percent.unwrap_or(0.0)),
            crate::settings::MetricPreference::Credits => {
                // For credits, we show the credits_percent (remaining as percentage of full scale)
                self.credits_percent
                    .unwrap_or_else(|| self.session_percent.unwrap_or(0.0))
            }
            crate::settings::MetricPreference::Average => {
                // Average of all available metrics
                let mut sum = 0.0;
                let mut count = 0;
                if let Some(v) = self.session_percent {
                    sum += v;
                    count += 1;
                }
                if let Some(v) = self.weekly_percent {
                    sum += v;
                    count += 1;
                }
                if let Some(v) = self.model_percent {
                    sum += v;
                    count += 1;
                }
                if count > 0 { sum / count as f64 } else { 0.0 }
            }
            crate::settings::MetricPreference::Automatic => {
                // Automatic: prefer the highest available metric (most concerning)
                let session = self.session_percent.unwrap_or(0.0);
                let weekly = self.weekly_percent.unwrap_or(0.0);
                let model = self.model_percent.unwrap_or(0.0);
                session.max(weekly).max(model)
            }
        }
    }
}

fn provider_metric_labels(provider_name: &str) -> (String, String) {
    ProviderId::from_cli_name(provider_name)
        .map(|id| {
            let metadata = create_provider(id).metadata().clone();
            (
                metadata.session_label.to_string(),
                metadata.weekly_label.to_string(),
            )
        })
        .unwrap_or_else(|| {
            (
                locale_text(Language::English, LocaleKey::ProviderSessionLabel).to_string(),
                locale_text(Language::English, LocaleKey::ProviderWeeklyLabel).to_string(),
            )
        })
}

fn should_show_provider(provider: &ProviderData) -> bool {
    provider.session_percent.is_some()
        || provider.weekly_percent.is_some()
        || provider.monthly_percent.is_some()
        || provider.model_percent.is_some()
        || provider.error.is_some()
}

fn format_reset_time(
    reset: chrono::DateTime<chrono::Utc>,
    relative: bool,
    lang: Language,
) -> String {
    if relative {
        let now = chrono::Utc::now();
        let diff = reset - now;

        if diff.num_seconds() <= 0 {
            return locale_text(lang, LocaleKey::ResetInProgress).to_string();
        }

        let hours = diff.num_hours();
        let minutes = (diff.num_minutes() % 60).abs();

        if hours >= 24 {
            let days = hours / 24;
            let remaining_hours = hours % 24;
            format!("{}d {}h", days, remaining_hours)
        } else {
            format!("{}h {}m", hours, minutes)
        }
    } else {
        // Absolute time format using local timezone
        // Include date if not today
        use chrono::Local;
        let local_time = reset.with_timezone(&Local);
        let today = Local::now().date_naive();
        let reset_date = local_time.date_naive();

        if reset_date == today {
            local_time.format("%I:%M %p").to_string()
        } else if reset_date == today + chrono::Days::new(1) {
            let tomorrow_template = locale_text(lang, LocaleKey::TomorrowAt);
            tomorrow_template
                .replace("{}", &local_time.format("%I:%M %p").to_string())
                .to_string()
        } else {
            local_time.format("%b %d, %I:%M %p").to_string()
        }
    }
}

fn calculate_pace(rate_window: &RateWindow) -> (Option<f64>, bool) {
    let Some(window_minutes) = rate_window.window_minutes else {
        return (None, false);
    };
    let Some(resets_at) = rate_window.resets_at else {
        return (None, false);
    };

    let now = chrono::Utc::now();
    let time_remaining = resets_at - now;
    let remaining_minutes = time_remaining.num_minutes() as f64;

    if remaining_minutes <= 0.0 {
        return (None, false);
    }

    let total_minutes = window_minutes as f64;
    let elapsed_minutes = total_minutes - remaining_minutes;

    if elapsed_minutes <= 0.0 {
        return (None, false);
    }

    let expected_percent = (elapsed_minutes / total_minutes) * 100.0;
    let actual_percent = rate_window.used_percent;
    let pace = actual_percent - expected_percent;
    let lasts_to_reset = actual_percent <= expected_percent;

    (Some(pace), lasts_to_reset)
}

fn usage_display_percent(used_percent: f64, show_as_used: bool) -> f64 {
    let used_percent = used_percent.clamp(0.0, 100.0);
    if show_as_used {
        used_percent
    } else {
        100.0 - used_percent
    }
}

fn usage_display_label(display_percent: f64, show_as_used: bool, lang: Language) -> String {
    if show_as_used {
        locale_text(lang, LocaleKey::UsedPercent)
            .replace("{:.0}", &format!("{:.0}", display_percent))
    } else {
        locale_text(lang, LocaleKey::RemainingPercent)
            .replace("{:.0}", &format!("{:.0}", display_percent))
    }
}

fn load_usage_breakdown_points(
    provider_id: ProviderId,
    account_email: Option<&str>,
) -> Vec<UsageBreakdownPoint> {
    if provider_id != ProviderId::Codex {
        return Vec::new();
    }

    // Require account_email to validate cache belongs to current account
    // Without it, we risk showing stale data from a different account
    let Some(account_email) = account_email else {
        return Vec::new();
    };

    let Some(cache) = OpenAIDashboardCacheStore::load() else {
        return Vec::new();
    };

    // Verify cache belongs to current account
    if !cache.account_email.eq_ignore_ascii_case(account_email) {
        return Vec::new();
    }

    cache
        .snapshot
        .usage_breakdown
        .iter()
        .map(|point| {
            let services = point
                .services
                .iter()
                .map(|service| ServiceUsage {
                    service: service.service.clone(),
                    credits_used: service.credits_used,
                })
                .collect();
            UsageBreakdownPoint::new(point.day.clone(), services)
        })
        .collect()
}

fn load_credits_history_points(
    provider_id: ProviderId,
    account_email: Option<&str>,
) -> Vec<(String, f64)> {
    if provider_id != ProviderId::Codex {
        return Vec::new();
    }

    let Some(account_email) = account_email else {
        return Vec::new();
    };

    let Some(cache) = OpenAIDashboardCacheStore::load() else {
        return Vec::new();
    };

    if !cache.account_email.eq_ignore_ascii_case(account_email) {
        return Vec::new();
    }

    let breakdown = if !cache.snapshot.daily_breakdown.is_empty() {
        &cache.snapshot.daily_breakdown
    } else if !cache.snapshot.usage_breakdown.is_empty() {
        &cache.snapshot.usage_breakdown
    } else {
        return Vec::new();
    };

    let mut points: Vec<(String, f64)> = breakdown
        .iter()
        .map(|day| (day.day.clone(), day.total_credits_used))
        .collect();

    points.sort_by(|a, b| a.0.cmp(&b.0));
    points
}

fn random_surprise_delay() -> Duration {
    use rand::Rng;
    let mut rng = rand::rng();
    let secs = rng.random_range(30..300);
    Duration::from_secs(secs)
}

struct SharedState {
    providers: Vec<ProviderData>,
    summary_providers: Vec<ProviderData>,
    selected_tab: SelectedTab,
    last_refresh: Instant,
    is_refreshing: bool,
    loading_pattern: LoadingPattern,
    loading_phase: f64,
    surprise_animation: Option<SurpriseAnimation>,
    surprise_frame: u32,
    next_surprise_time: Instant,
    update_available: Option<UpdateInfo>,
    update_checked: bool,
    update_dismissed: bool,
    update_check_in_progress: bool,
    update_state: UpdateState,
    login_provider: Option<String>,
    login_phase: LoginPhase,
    login_message: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelectedTab {
    Summary,
    Provider(ProviderId),
}

fn open_update_destination(update: &UpdateInfo) {
    let _ = open::that(&update.download_url);
}

fn start_update_download(state: Arc<Mutex<SharedState>>, update: UpdateInfo) {
    std::thread::spawn(move || {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                tracing::error!("Failed to create runtime for update download: {}", e);
                if let Ok(mut s) = state.lock() {
                    s.update_state = UpdateState::Failed(e.to_string());
                }
                return;
            }
        };

        rt.block_on(async move {
            if let Ok(mut s) = state.lock() {
                s.update_state = UpdateState::Downloading(0.0);
            }
            let (progress_tx, _) = tokio::sync::watch::channel(UpdateState::Available);
            match updater::download_update(&update, progress_tx).await {
                Ok(path) => {
                    if let Ok(mut s) = state.lock() {
                        s.update_state = UpdateState::Ready(path);
                    }
                }
                Err(e) => {
                    if let Ok(mut s) = state.lock() {
                        s.update_state = UpdateState::Failed(e);
                    }
                }
            }
        });
    });
}

fn start_update_check(
    state: Arc<Mutex<SharedState>>,
    update_channel: UpdateChannel,
    auto_download: bool,
) {
    if let Ok(mut s) = state.lock() {
        if s.update_check_in_progress {
            return;
        }
        s.update_check_in_progress = true;
    } else {
        return;
    }

    std::thread::spawn(move || {
        let rt = match tokio::runtime::Runtime::new() {
            Ok(rt) => rt,
            Err(e) => {
                tracing::error!("Failed to create tokio runtime for update check: {}", e);
                if let Ok(mut s) = state.lock() {
                    s.update_check_in_progress = false;
                }
                return;
            }
        };

        rt.block_on(async move {
            if let Some(update) = updater::check_for_updates_with_channel(update_channel).await {
                let should_download = {
                    if let Ok(mut s) = state.lock() {
                        s.update_available = Some(update.clone());
                        s.update_checked = true;
                        s.update_dismissed = false;
                        s.update_state = UpdateState::Available;
                        auto_download && update.supports_auto_download()
                    } else {
                        false
                    }
                };

                if should_download {
                    let (progress_tx, mut progress_rx) =
                        tokio::sync::watch::channel(UpdateState::Available);
                    let state_clone = Arc::clone(&state);

                    if let Ok(mut s) = state_clone.lock() {
                        s.update_state = UpdateState::Downloading(0.0);
                    }

                    let progress_state = Arc::clone(&state_clone);
                    tokio::spawn(async move {
                        while progress_rx.changed().await.is_ok() {
                            let new_state = progress_rx.borrow().clone();
                            if let Ok(mut s) = progress_state.lock() {
                                s.update_state = new_state;
                            }
                        }
                    });

                    match updater::download_update(&update, progress_tx).await {
                        Ok(path) => {
                            if let Ok(mut s) = state_clone.lock() {
                                s.update_state = UpdateState::Ready(path);
                            }
                        }
                        Err(e) => {
                            tracing::error!("Failed to download update: {}", e);
                            if let Ok(mut s) = state_clone.lock() {
                                s.update_state = UpdateState::Failed(e);
                            }
                        }
                    }
                }
            } else if let Ok(mut s) = state.lock() {
                s.update_checked = true;
            }

            if let Ok(mut s) = state.lock() {
                s.update_check_in_progress = false;
            }
        });
    });
}

pub struct CodexBarApp {
    state: Arc<Mutex<SharedState>>,
    settings: Settings,
    tray_manager: Option<UnifiedTrayManager>,
    tray_action_rx: Option<Receiver<TrayMenuAction>>,
    preferences_window: PreferencesWindow,
    shortcut_manager: Option<ShortcutManager>,
    icon_cache: ProviderIconCache,
    was_refreshing: bool, // Track previous frame's refresh state
    pending_main_window_layout: bool,
    anchor_main_window_to_pointer: bool,
    notification_manager: Arc<Mutex<NotificationManager>>,
    #[cfg(debug_assertions)]
    test_input_queue: super::test_server::TestInputQueue,
}

fn prepend_font(fonts: &mut FontDefinitions, family: FontFamily, font_name: &str) {
    let entries = fonts.families.entry(family).or_default();
    if !entries.iter().any(|existing| existing == font_name) {
        entries.insert(0, font_name.to_owned());
    }
}

fn add_font_if_present(fonts: &mut FontDefinitions, font_name: &str, path: &str) {
    if let Ok(font_data) = std::fs::read(path) {
        fonts
            .font_data
            .insert(font_name.to_owned(), FontData::from_owned(font_data).into());
        prepend_font(fonts, FontFamily::Proportional, font_name);
        prepend_font(fonts, FontFamily::Monospace, font_name);
    }
}

#[cfg(windows)]
fn cjk_font_candidates() -> &'static [(&'static str, &'static str)] {
    &[
        ("msyh", "C:\\Windows\\Fonts\\msyh.ttc"),
        ("msyhbd", "C:\\Windows\\Fonts\\msyhbd.ttc"),
        ("simsun", "C:\\Windows\\Fonts\\simsun.ttc"),
        ("simhei", "C:\\Windows\\Fonts\\simhei.ttf"),
        ("deng", "C:\\Windows\\Fonts\\Deng.ttf"),
        (
            "wqy_zenhei",
            "Z:\\usr\\share\\fonts\\truetype\\wqy\\wqy-zenhei.ttc",
        ),
        (
            "droid_fallback",
            "Z:\\usr\\share\\fonts\\truetype\\droid\\DroidSansFallbackFull.ttf",
        ),
    ]
}

#[cfg(not(windows))]
fn cjk_font_candidates() -> &'static [(&'static str, &'static str)] {
    &[
        ("wqy_zenhei", "/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc"),
        (
            "droid_fallback",
            "/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf",
        ),
    ]
}

impl CodexBarApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Load Windows symbol + CJK fallback fonts so Chinese UI text renders correctly.
        let mut fonts = FontDefinitions::default();
        if let Ok(font_data) = std::fs::read("C:\\Windows\\Fonts\\seguisym.ttf") {
            fonts.font_data.insert(
                "segoe_symbols".to_owned(),
                FontData::from_owned(font_data).into(),
            );
            fonts
                .families
                .entry(FontFamily::Proportional)
                .or_default()
                .push("segoe_symbols".to_owned());
        }

        // Add common Chinese fonts in priority order (if present).
        // We insert them at the front so CJK glyph lookup hits these first.
        for (name, path) in cjk_font_candidates() {
            add_font_if_present(&mut fonts, name, path);
        }
        cc.egui_ctx.set_fonts(fonts);

        let settings = Settings::load();
        let enabled_ids = settings.get_enabled_provider_ids();

        let placeholders: Vec<ProviderData> = enabled_ids
            .iter()
            .map(|&id| ProviderData::placeholder(id))
            .collect();

        let state = Arc::new(Mutex::new(SharedState {
            providers: placeholders.clone(),
            summary_providers: placeholders,
            selected_tab: SelectedTab::Summary,
            last_refresh: Instant::now() - Duration::from_secs(999),
            is_refreshing: false,
            loading_pattern: LoadingPattern::random(),
            loading_phase: 0.0,
            surprise_animation: None,
            surprise_frame: 0,
            next_surprise_time: Instant::now() + random_surprise_delay(),
            update_available: None,
            update_checked: false,
            update_dismissed: false,
            update_check_in_progress: false,
            update_state: UpdateState::Idle,
            login_provider: None,
            login_phase: LoginPhase::Idle,
            login_message: None,
        }));

        // Initialize system tray based on settings
        let tray_manager = match UnifiedTrayManager::new(&settings) {
            Ok(tm) => Some(tm),
            Err(e) => {
                tracing::warn!("Failed to create tray manager: {}", e);
                None
            }
        };
        let tray_action_rx = if tray_manager.is_some() {
            let (tx, rx) = mpsc::channel::<TrayMenuAction>();
            let repaint_ctx = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                loop {
                    if let Some(action) = UnifiedTrayManager::check_events() {
                        if matches!(action, TrayMenuAction::Open | TrayMenuAction::Refresh) {
                            // Egui viewport commands alone can be ignored while minimized.
                            // Force a native restore first so the update loop wakes up.
                            restore_main_window();
                            repaint_ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
                            repaint_ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        } else if matches!(action, TrayMenuAction::Settings) {
                            // Show main window so update() runs (needed to spawn the
                            // settings child viewport), but don't steal focus.
                            show_main_window_no_focus();
                            repaint_ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                        }
                        if tx.send(action).is_err() {
                            break;
                        }
                        repaint_ctx.request_repaint();
                    } else {
                        std::thread::sleep(Duration::from_millis(50));
                    }
                }
            });
            Some(rx)
        } else {
            None
        };

        // Check for updates in background (using configured update channel)
        start_update_check(
            Arc::clone(&state),
            settings.update_channel,
            settings.auto_download_updates,
        );

        // Initialize keyboard shortcuts with custom shortcut from settings
        let shortcut_manager = match ShortcutManager::new() {
            Ok(mut sm) => {
                // Apply custom shortcut from settings if configured
                if let Some((modifiers, key)) = parse_shortcut(&settings.global_shortcut) {
                    if let Err(e) = sm.set_open_menu_shortcut(modifiers, key) {
                        tracing::warn!(
                            "Failed to set custom shortcut '{}': {}",
                            settings.global_shortcut,
                            e
                        );
                    } else {
                        tracing::info!(
                            "Keyboard shortcut registered: {}",
                            settings.global_shortcut
                        );
                    }
                } else {
                    tracing::info!("Keyboard shortcut registered: Ctrl+Shift+U (default)");
                }
                Some(sm)
            }
            Err(e) => {
                tracing::warn!("Failed to register keyboard shortcuts: {}", e);
                None
            }
        };

        // Initialize test input queue and start server (debug builds only)
        #[cfg(debug_assertions)]
        let test_input_queue = {
            let q = super::test_server::create_queue();
            super::test_server::start_server(q.clone());
            q
        };

        Self {
            state,
            settings,
            tray_manager,
            tray_action_rx,
            preferences_window: PreferencesWindow::new(),
            shortcut_manager,
            icon_cache: ProviderIconCache::new(),
            was_refreshing: false,
            pending_main_window_layout: true,
            anchor_main_window_to_pointer: false,
            notification_manager: Arc::new(Mutex::new(NotificationManager::new())),
            #[cfg(debug_assertions)]
            test_input_queue,
        }
    }

    fn pending_update_to_install_on_quit(&self) -> Option<PathBuf> {
        let effective_settings = self.preferences_window.current_settings();
        if !effective_settings.install_updates_on_quit {
            return None;
        }

        let ready_update = self.state.lock().ok().and_then(|state| {
            if let UpdateState::Ready(path) = &state.update_state {
                Some(path.clone())
            } else {
                None
            }
        });

        ready_update.or_else(updater::get_pending_update)
    }

    fn quit_application(&self) -> ! {
        if let Some(installer_path) = self.pending_update_to_install_on_quit()
            && let Err(e) = updater::apply_update(&installer_path)
        {
            tracing::error!("Failed to apply pending update on quit: {}", e);
        }

        std::process::exit(0);
    }

    fn layout_main_window(&mut self, ctx: &egui::Context, anchor_to_pointer: bool) {
        let Some(outer_rect) = ctx.input(|i| i.viewport().outer_rect) else {
            return;
        };
        let Some(work_area) = work_area_rect(ctx) else {
            return;
        };

        let margin = 12.0;
        let gap = 10.0;
        let min_size = egui::vec2(320.0, 320.0);
        let max_w = (work_area.width() - margin * 2.0).max(min_size.x);
        let max_h = (work_area.height() - margin * 2.0).max(min_size.y);
        let target_size = egui::vec2(360.0_f32.min(max_w), 500.0_f32.min(max_h));
        ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(target_size));

        let anchor = if anchor_to_pointer {
            ctx.input(|i| i.pointer.latest_pos())
        } else {
            None
        }
        .unwrap_or_else(|| outer_rect.center());

        // For tray/shortcut opens, keep the popup on the left side and vertically centered
        // so it doesn't appear pinned to the taskbar area.
        let (target_x, target_y) = if anchor_to_pointer {
            let center_x = work_area.min.x + work_area.width() * 0.22;
            (
                center_x - target_size.x * 0.5,
                work_area.min.y + (work_area.height() - target_size.y) * 0.5,
            )
        } else {
            let space_above = anchor.y - work_area.min.y - margin;
            let space_below = work_area.max.y - anchor.y - margin;
            let x = anchor.x - target_size.x * 0.5;
            let y = if space_below >= target_size.y + gap || space_below >= space_above {
                anchor.y + gap
            } else {
                anchor.y - target_size.y - gap
            };
            (x, y)
        };

        let min_x = work_area.min.x + margin;
        let min_y = work_area.min.y + margin;
        let max_x = (work_area.max.x - target_size.x - margin).max(min_x);
        let max_y = (work_area.max.y - target_size.y - margin).max(min_y);
        let x = if max_x <= min_x {
            min_x
        } else {
            target_x.clamp(min_x, max_x)
        };
        let y = if max_y <= min_y {
            min_y
        } else {
            target_y.clamp(min_y, max_y)
        };

        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(x, y)));
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(egui::ViewportCommand::Focus);

        self.pending_main_window_layout = false;
        self.anchor_main_window_to_pointer = false;
    }

    fn refresh_providers(&self) {
        let state = Arc::clone(&self.state);
        let enabled_ids = self.settings.get_enabled_provider_ids();
        let manual_cookies = ManualCookies::load();
        let api_keys = ApiKeys::load();
        let reset_time_relative = self.settings.reset_time_relative;
        let ui_language = self.settings.ui_language;
        // Load token accounts for account switching support
        let token_accounts = TokenAccountStore::new().load().unwrap_or_default();
        // Notification manager for usage alerts
        let notification_manager = Arc::clone(&self.notification_manager);
        let settings = self.settings.clone();

        std::thread::spawn(move || {
            if let Ok(mut s) = state.lock() {
                s.is_refreshing = true;
                s.loading_pattern = LoadingPattern::random();
                s.loading_phase = 0.0;
                s.providers = enabled_ids
                    .iter()
                    .map(|&id| ProviderData::placeholder(id))
                    .collect();
                if let SelectedTab::Provider(id) = s.selected_tab
                    && !enabled_ids.contains(&id)
                {
                    s.selected_tab = SelectedTab::Summary;
                }
            }

            let rt = match tokio::runtime::Runtime::new() {
                Ok(rt) => rt,
                Err(e) => {
                    tracing::error!("Failed to create tokio runtime: {}", e);
                    if let Ok(mut s) = state.lock() {
                        s.is_refreshing = false;
                    }
                    return;
                }
            };
            rt.block_on(async {
                // Clear any stale OAuth env vars at the start of refresh
                // This ensures account switches take effect immediately
                const OAUTH_ENV_KEYS: &[&str] = &[
                    "CODEXBAR_CLAUDE_OAUTH_TOKEN",
                    "ZED_API_TOKEN",
                    "ZAI_API_TOKEN",
                ];
                for key in OAUTH_ENV_KEYS {
                    // SAFETY: no concurrent threads are reading these env vars at this point
                    unsafe { std::env::remove_var(key) };
                }

                let handles: Vec<_> = enabled_ids
                    .iter()
                    .enumerate()
                    .map(|(idx, &id)| {
                        // Check for active token account first
                        let active_token = token_accounts
                            .get(&id)
                            .and_then(|data| data.active_account())
                            .map(|account| account.token.clone());

                        // Check for environment override from token account (e.g., for Zai/Claude OAuth)
                        let env_override = active_token
                            .as_ref()
                            .and_then(|token| TokenAccountSupport::env_override(id, token));

                        // Set env override if present - providers will read from env vars
                        // Note: We don't clear env vars here to avoid race conditions with
                        // concurrent provider fetches. Instead, we pass api_key directly
                        // through FetchContext for providers that support it.
                        if let Some(ref env_vars) = env_override {
                            for (key, value) in env_vars {
                                // SAFETY: called sequentially before spawning provider fetch tasks
                                unsafe { std::env::set_var(key, value) };
                            }
                        }

                        // Determine cookie header: active token account > manual cookie > browser extraction
                        let cookie_header = if env_override.is_some() {
                            None
                        } else if let Some(ref token) = active_token {
                            // Use active account's token, normalized for this provider
                            Some(TokenAccountSupport::normalized_cookie_header(id, token))
                        } else {
                            // Fallback to manual cookie or browser extraction
                            let manual_cookie =
                                manual_cookies.get(id.cli_name()).map(|s| s.to_string());
                            manual_cookie.or_else(|| {
                                // Try browser cookie extraction if no manual cookie
                                id.cookie_domain().and_then(|domain| {
                                    get_cookie_header(domain).ok().filter(|h| !h.is_empty())
                                })
                            })
                        };

                        let api_key = if env_override.is_some() {
                            // If we have env override, extract API key from it
                            env_override
                                .as_ref()
                                .and_then(|env| env.values().next().cloned())
                        } else {
                            api_keys.get(id.cli_name()).map(|s| s.to_string())
                        };

                        let ctx = FetchContext {
                            manual_cookie_header: cookie_header,
                            api_key,
                            ..FetchContext::default()
                        };
                        let state = Arc::clone(&state);
                        tokio::spawn(async move {
                            let provider = create_provider(id);
                            let metadata = provider.metadata().clone();
                            let provider_name = id.cli_name().to_string();

                            let (usage_result, status_result) = tokio::join!(
                                async {
                                    tokio::time::timeout(
                                        std::time::Duration::from_secs(5),
                                        provider.fetch_usage(&ctx),
                                    )
                                    .await
                                },
                                async {
                                    tokio::time::timeout(
                                        std::time::Duration::from_secs(5),
                                        fetch_provider_status(&provider_name),
                                    )
                                    .await
                                }
                            );

                            let mut result = match usage_result {
                                Ok(Ok(result)) => ProviderData::from_result(
                                    id,
                                    &result,
                                    &metadata,
                                    reset_time_relative,
                                    ui_language,
                                ),
                                Ok(Err(e)) => ProviderData::from_error(id, e.to_string()),
                                Err(_) => ProviderData::from_error(id, "Timeout".to_string()),
                            };

                            if let Ok(Some(status)) = status_result {
                                result.status_level = status.level;
                                result.status_description = Some(status.description);
                            }

                            if result.error.is_none() {
                                result.usage_breakdown =
                                    load_usage_breakdown_points(id, result.account.as_deref());
                            }

                            let provider_name_lower = provider_name.to_lowercase();
                            if provider_name_lower == "codex" || provider_name_lower == "claude" {
                                result.cost_history =
                                    get_daily_cost_history(&provider_name_lower, 30);
                            }

                            if result.error.is_none() {
                                result.credits_history =
                                    load_credits_history_points(id, result.account.as_deref());
                            }

                            if let Ok(mut s) = state.lock()
                                && idx < s.providers.len()
                            {
                                s.providers[idx] = result;
                            }
                        })
                    })
                    .collect();

                for handle in handles {
                    let _ = handle.await;
                }
            });

            if let Ok(mut s) = state.lock() {
                s.summary_providers = s.providers.clone();
                s.last_refresh = Instant::now();
                s.is_refreshing = false;

                // Check for usage alerts and send notifications
                for provider in &s.providers {
                    if let Some(provider_id) = ProviderId::from_cli_name(&provider.name) {
                        // Check primary session usage
                        if let Some(session_percent) = provider.session_percent
                            && let Ok(mut nm) = notification_manager.lock()
                        {
                            nm.check_and_notify(provider_id, session_percent, &settings);
                            nm.check_session_transition(provider_id, session_percent, &settings);
                        }
                        // Note: Infini alerts are based on the highest usage across all windows
                        // The primary (5-hour) window is used as the main indicator
                    }
                }
            }
        });
    }

    /// Get an animated value that smoothly transitions to the target over 300ms.
    ///
    /// This helper provides consistent animation behavior for progress bar fills
    /// and other numeric value transitions.
    ///
    /// # Arguments
    /// * `ctx` - The egui context for animation state
    /// * `id` - A unique identifier for tracking this animation
    /// * `target` - The target value to animate towards
    ///
    /// # Returns
    /// The current animated value, which will smoothly approach the target
    #[allow(dead_code)]
    fn get_animated_value(ctx: &egui::Context, id: egui::Id, target: f32) -> f32 {
        ctx.animate_value_with_time(id, target, 0.3)
    }
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

fn create_provider(id: ProviderId) -> Box<dyn Provider> {
    match id {
        ProviderId::Claude => Box::new(ClaudeProvider::new()),
        ProviderId::Codex => Box::new(CodexProvider::new()),
        ProviderId::Cursor => Box::new(CursorProvider::new()),
        ProviderId::Gemini => Box::new(GeminiProvider::new()),
        ProviderId::Copilot => Box::new(CopilotProvider::new()),
        ProviderId::Antigravity => Box::new(AntigravityProvider::new()),
        ProviderId::Factory => Box::new(FactoryProvider::new()),
        ProviderId::Zai => Box::new(ZaiProvider::new()),
        ProviderId::Kiro => Box::new(KiroProvider::new()),
        ProviderId::VertexAI => Box::new(VertexAIProvider::new()),
        ProviderId::Augment => Box::new(AugmentProvider::new()),
        ProviderId::MiniMax => Box::new(MiniMaxProvider::new()),
        ProviderId::OpenCode => Box::new(OpenCodeProvider::new()),
        ProviderId::Kimi => Box::new(KimiProvider::new()),
        ProviderId::KimiK2 => Box::new(KimiK2Provider::new()),
        ProviderId::Amp => Box::new(AmpProvider::new()),
        ProviderId::Warp => Box::new(WarpProvider::new()),
        ProviderId::Ollama => Box::new(OllamaProvider::new()),
        ProviderId::OpenRouter => Box::new(OpenRouterProvider::new()),
        ProviderId::Synthetic => Box::new(SyntheticProvider::new()),
        ProviderId::JetBrains => Box::new(JetBrainsProvider::new()),
        ProviderId::Alibaba => Box::new(AlibabaProvider::new()),
        ProviderId::NanoGPT => Box::new(NanoGPTProvider::new()),
        ProviderId::Infini => Box::new(InfiniProvider::default()),
    }
}

impl eframe::App for CodexBarApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Intercept window close: hide to tray instead of exiting
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

        if self.pending_main_window_layout {
            self.layout_main_window(ctx, self.anchor_main_window_to_pointer);
        }

        // Check keyboard shortcuts without holding an immutable borrow of self
        // while triggering layout changes.
        let mut shortcut_triggered = false;
        if let Some(shortcut_mgr) = self.shortcut_manager.as_ref() {
            while shortcut_mgr.check_events() {
                shortcut_triggered = true;
            }
        }
        if shortcut_triggered {
            tracing::info!("Keyboard shortcut triggered - focusing window");
            self.pending_main_window_layout = true;
            self.anchor_main_window_to_pointer = true;
            self.layout_main_window(ctx, true);
        }

        // Process test input queue (debug builds only - for automated testing without moving real cursor)
        #[cfg(debug_assertions)]
        if let Ok(mut queue) = self.test_input_queue.lock() {
            let mut had_input = false;
            for input in queue.drain(..) {
                had_input = true;
                match input {
                    super::test_server::TestInput::Click { x, y } => {
                        let pos = egui::pos2(x, y);
                        ctx.input_mut(|i| {
                            // Move pointer to position first (required for hover detection)
                            i.events.push(egui::Event::PointerMoved(pos));
                            // Then click
                            i.events.push(egui::Event::PointerButton {
                                pos,
                                button: egui::PointerButton::Primary,
                                pressed: true,
                                modifiers: egui::Modifiers::NONE,
                            });
                            i.events.push(egui::Event::PointerButton {
                                pos,
                                button: egui::PointerButton::Primary,
                                pressed: false,
                                modifiers: egui::Modifiers::NONE,
                            });
                        });
                        tracing::debug!("Injected test click at ({}, {})", x, y);
                    }
                    super::test_server::TestInput::DoubleClick { x, y } => {
                        let pos = egui::pos2(x, y);
                        ctx.input_mut(|i| {
                            // Move pointer to position first
                            i.events.push(egui::Event::PointerMoved(pos));
                            for _ in 0..2 {
                                i.events.push(egui::Event::PointerButton {
                                    pos,
                                    button: egui::PointerButton::Primary,
                                    pressed: true,
                                    modifiers: egui::Modifiers::NONE,
                                });
                                i.events.push(egui::Event::PointerButton {
                                    pos,
                                    button: egui::PointerButton::Primary,
                                    pressed: false,
                                    modifiers: egui::Modifiers::NONE,
                                });
                            }
                        });
                        tracing::debug!("Injected test double-click at ({}, {})", x, y);
                    }
                    super::test_server::TestInput::RightClick { x, y } => {
                        let pos = egui::pos2(x, y);
                        ctx.input_mut(|i| {
                            // Move pointer to position first
                            i.events.push(egui::Event::PointerMoved(pos));
                            i.events.push(egui::Event::PointerButton {
                                pos,
                                button: egui::PointerButton::Secondary,
                                pressed: true,
                                modifiers: egui::Modifiers::NONE,
                            });
                            i.events.push(egui::Event::PointerButton {
                                pos,
                                button: egui::PointerButton::Secondary,
                                pressed: false,
                                modifiers: egui::Modifiers::NONE,
                            });
                        });
                        tracing::debug!("Injected test right-click at ({}, {})", x, y);
                    }
                }
            }
            if had_input {
                ctx.request_repaint();
            }
        }

        // Auto-refresh check
        let should_refresh = {
            if self.settings.refresh_interval_secs == 0 {
                false
            } else if let Ok(state) = self.state.lock() {
                !state.is_refreshing
                    && state.last_refresh.elapsed()
                        > Duration::from_secs(self.settings.refresh_interval_secs)
            } else {
                false
            }
        };
        if should_refresh {
            self.refresh_providers();
        }

        // Get state
        let (
            providers,
            summary_providers,
            selected_tab,
            is_refreshing,
            loading_pattern,
            loading_phase,
            surprise_state,
            update_info,
            update_download_state,
            login_state,
        ) = {
            if let Ok(mut state) = self.state.lock() {
                if state.is_refreshing {
                    state.loading_phase += 0.05;
                    if state.loading_phase > 1.0 {
                        state.loading_phase -= 1.0;
                    }
                }

                let surprise = if self.settings.surprise_animations && !state.is_refreshing {
                    if let Some(anim) = state.surprise_animation {
                        state.surprise_frame += 1;
                        if state.surprise_frame >= anim.duration_frames() {
                            state.surprise_animation = None;
                            state.surprise_frame = 0;
                            state.next_surprise_time = Instant::now() + random_surprise_delay();
                            None
                        } else {
                            Some((anim, state.surprise_frame))
                        }
                    } else if Instant::now() >= state.next_surprise_time {
                        let anim = SurpriseAnimation::random();
                        state.surprise_animation = Some(anim);
                        state.surprise_frame = 0;
                        Some((anim, 0))
                    } else {
                        None
                    }
                } else {
                    None
                };

                let update = if state.update_dismissed {
                    None
                } else {
                    state.update_available.clone()
                };

                let update_download_state = state.update_state.clone();

                let login_state = (
                    state.login_provider.clone(),
                    state.login_phase,
                    state.login_message.clone(),
                );

                (
                    state.providers.clone(),
                    state.summary_providers.clone(),
                    state.selected_tab,
                    state.is_refreshing,
                    state.loading_pattern,
                    state.loading_phase,
                    surprise,
                    update,
                    update_download_state,
                    login_state,
                )
            } else {
                (
                    Vec::new(),
                    Vec::new(),
                    SelectedTab::Summary,
                    false,
                    LoadingPattern::default(),
                    0.0,
                    None,
                    None,
                    UpdateState::Idle,
                    (None, LoginPhase::Idle, None),
                )
            }
        };

        let (_login_provider, login_phase, _login_message) = login_state;
        let is_logging_in = _login_provider.is_some() && login_phase != LoginPhase::Idle;

        ctx.request_repaint_after(
            if is_refreshing || surprise_state.is_some() || is_logging_in {
                Duration::from_millis(50)
            } else {
                Duration::from_millis(200)
            },
        );

        // Update tray icon
        if let Some(ref tray) = self.tray_manager {
            if is_refreshing {
                tray.show_loading(loading_pattern, loading_phase);
            } else if let Some((anim, frame)) = surprise_state {
                // Use first provider with data for surprise animation
                if let Some(provider) = providers.iter().find(|p| p.session_percent.is_some()) {
                    let session = provider.session_percent.unwrap_or(0.0);
                    let weekly = provider.weekly_percent.unwrap_or(session);
                    tray.show_surprise(anim, frame, session, weekly);
                }
            } else {
                // Respect menu_bar_display_mode setting
                // Use per-provider metric preferences from settings
                let provider_usages: Vec<ProviderUsage> = providers
                    .iter()
                    .filter(|p| p.session_percent.is_some())
                    .map(|p| {
                        // Get the metric preference for this provider
                        let metric_pref = crate::core::ProviderId::from_cli_name(&p.name)
                            .map(|id| self.settings.get_provider_metric(id))
                            .unwrap_or_default();
                        let preferred_percent = p.get_preferred_metric(metric_pref);
                        // For credits metric, convert from "remaining" to "used" for consistent tray behavior
                        // Credits are stored as remaining %, but tray expects used % for severity coloring
                        let used_percent =
                            if metric_pref == crate::settings::MetricPreference::Credits {
                                100.0 - preferred_percent // Convert remaining to used
                            } else {
                                preferred_percent // Already used %
                            };
                        // Weekly percent is always usage-based (not credits)
                        let weekly_percent = p.weekly_percent.unwrap_or(used_percent);
                        ProviderUsage {
                            name: p.display_name.clone(),
                            session_percent: used_percent, // Always use "used %" for tray severity
                            weekly_percent,
                        }
                    })
                    .collect();

                match self.settings.menu_bar_display_mode.as_str() {
                    "minimal" => {
                        // Minimal: show only the highest-usage provider's session bar
                        if let Some(p) = provider_usages.iter().max_by(|a, b| {
                            a.session_percent
                                .partial_cmp(&b.session_percent)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        }) {
                            tray.update_usage(p.session_percent, p.weekly_percent, &p.name);
                        }
                    }
                    "compact" => {
                        // Compact: merged icon but shorter tooltip (first provider only)
                        if let Some(p) = provider_usages.first() {
                            tray.update_usage(p.session_percent, p.weekly_percent, &p.name);
                        }
                    }
                    _ => {
                        // Detailed (default): show all providers merged
                        tray.update_merged(&provider_usages);
                    }
                }
            }

            let mut tray_actions = Vec::new();
            if let Some(ref action_rx) = self.tray_action_rx {
                while let Ok(action) = action_rx.try_recv() {
                    tray_actions.push(action);
                }
            }
            for action in tray_actions {
                match action {
                    TrayMenuAction::Quit => self.quit_application(),
                    TrayMenuAction::Open => {
                        self.pending_main_window_layout = true;
                        self.anchor_main_window_to_pointer = true;
                        self.layout_main_window(ctx, true);
                    }
                    TrayMenuAction::Refresh => {
                        if !is_refreshing {
                            self.refresh_providers();
                            // Ensure animations advance even if window is hidden/minimized.
                            ctx.request_repaint();
                        }
                    }
                    TrayMenuAction::Settings => {
                        self.preferences_window.open();
                        // Move main window off-screen so only settings viewport is visible.
                        ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2(
                            -10000.0, -10000.0,
                        )));
                    }
                    TrayMenuAction::CheckForUpdates => {
                        let effective_settings = self.preferences_window.current_settings();
                        start_update_check(
                            Arc::clone(&self.state),
                            effective_settings.update_channel,
                            effective_settings.auto_download_updates,
                        );
                    }
                    TrayMenuAction::ToggleProvider(provider_name) => {
                        // Toggle provider enabled state
                        if let Some(provider_id) = ProviderId::from_cli_name(&provider_name) {
                            self.settings.toggle_provider(provider_id);
                            if let Err(e) = self.settings.save() {
                                tracing::error!("Failed to save settings: {}", e);
                            }
                            // Refresh to update the UI with new provider list
                            self.refresh_providers();
                        }
                    }
                }
            }
        }

        // Apply refined style
        let mut style = (*ctx.style()).clone();
        style.visuals.window_fill = Theme::BG_PRIMARY;
        style.visuals.panel_fill = Theme::BG_PRIMARY;
        style.visuals.widgets.noninteractive.bg_fill = Theme::BG_SECONDARY;
        style.visuals.widgets.inactive.bg_fill = Theme::CARD_BG;
        style.visuals.widgets.hovered.bg_fill = Theme::CARD_BG_HOVER;
        style.visuals.widgets.active.bg_fill = Theme::ACCENT_PRIMARY;
        style.visuals.selection.bg_fill = Theme::selection_overlay();
        style.visuals.selection.stroke = Stroke::new(1.0, Theme::ACCENT_PRIMARY);
        ctx.set_style(style);

        // Handle keyboard shortcuts
        ctx.input(|i| {
            if i.modifiers.ctrl && i.key_pressed(egui::Key::Comma) {
                self.preferences_window.open();
            }
        });

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(Theme::BG_PRIMARY).inner_margin(Spacing::SM))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                    // ════════════════════════════════════════════════════════════
                    // UPDATE BANNER
                    // ════════════════════════════════════════════════════════════
                    if let Some(ref update) = update_info {
                        egui::Frame::none()
                            .fill(Theme::ACCENT_PRIMARY)
                            .rounding(Rounding::same(Radius::LG))
                            .inner_margin(Spacing::MD)
                            .show(ui, |ui| {
                                ui.vertical(|ui| {
                                    ui.horizontal(|ui| {
                                        // Icon based on state
                                        let icon = match &update_download_state {
                                            UpdateState::Ready(_) => "✓",
                                            UpdateState::Failed(_) => "⚠",
                                            UpdateState::Downloading(_) => "↓",
                                            _ => "🎉",
                                        };
                                        ui.label(RichText::new(icon).size(FontSize::MD).color(Color32::WHITE));
                                        ui.add_space(Spacing::XS);

                                        // Message based on state
                                        let ui_lang = self.settings.ui_language;
                                        let message = match &update_download_state {
                                            UpdateState::Downloading(progress) => {
                                                let template = locale_text(ui_lang, LocaleKey::UpdateDownloadingMessage);
                                                template.replace("{}", &update.version).replace("{:.0}", &format!("{:.0}", progress * 100.0))
                                            }
                                            UpdateState::Ready(_) => {
                                                let template = locale_text(ui_lang, LocaleKey::UpdateReadyMessage);
                                                template.replace("{}", &update.version)
                                            }
                                            UpdateState::Failed(e) => {
                                                let template = locale_text(ui_lang, LocaleKey::UpdateFailedMessage);
                                                template.replace("{}", e)
                                            }
                                            _ => {
                                                let template = locale_text(ui_lang, LocaleKey::UpdateAvailableMessage);
                                                template.replace("{}", &update.version)
                                            }
                                        };
                                        ui.label(
                                            RichText::new(message)
                                                .size(FontSize::BASE)
                                                .color(Color32::WHITE)
                                                .strong(),
                                        );

                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            // Dismiss button
                                        if ui.add(
                                            egui::Button::new(RichText::new("✕").size(FontSize::SM).color(Color32::WHITE))
                                                .fill(Color32::TRANSPARENT)
                                                .stroke(Stroke::NONE)
                                        ).clicked()
                                            && let Ok(mut s) = self.state.lock() {
                                                s.update_dismissed = true;
                                            }

                                            // Action button based on state
                                            match &update_download_state {
                                                UpdateState::Ready(path) => {
                                                    if update.supports_auto_apply() {
                                                        let installer_path = path.clone();
                                                        if ui.add(
                                                            egui::Button::new(
                                                                RichText::new(locale_text(ui_lang, LocaleKey::UpdateRestartAndUpdate))
                                                                    .size(FontSize::SM)
                                                                    .color(Theme::ACCENT_PRIMARY),
                                                            )
                                                            .fill(Color32::WHITE)
                                                            .rounding(Rounding::same(Radius::SM))
                                                        ).clicked()
                                                            && let Err(e) = updater::apply_update(&installer_path) {
                                                                tracing::error!("Failed to apply update: {}", e);
                                                            }
                                                    } else if ui.add(
                                                        egui::Button::new(
                                                            RichText::new(locale_text(ui_lang, LocaleKey::UpdateDownload))
                                                                .size(FontSize::SM)
                                                                .color(Theme::ACCENT_PRIMARY),
                                                        )
                                                        .fill(Color32::WHITE)
                                                        .rounding(Rounding::same(Radius::SM))
                                                    ).clicked() {
                                                        open_update_destination(update);
                                                    }
                                                }
                                                UpdateState::Downloading(_) => {
                                                    ui.spinner();
                                                }
                                                UpdateState::Failed(_) => {
                                                    if update.supports_auto_download()
                                                        && ui.add(
                                                            egui::Button::new(
                                                                RichText::new(locale_text(ui_lang, LocaleKey::UpdateRetry))
                                                                    .size(FontSize::SM)
                                                                    .color(Theme::ACCENT_PRIMARY),
                                                            )
                                                            .fill(Color32::WHITE)
                                                            .rounding(Rounding::same(Radius::SM))
                                                        ).clicked() {
                                                            start_update_download(
                                                                Arc::clone(&self.state),
                                                                update.clone(),
                                                            );
                                                        }
                                                    if ui.add(
                                                        egui::Button::new(
                                                            RichText::new(locale_text(ui_lang, LocaleKey::UpdateDownload))
                                                                .size(FontSize::SM)
                                                                .color(Color32::WHITE),
                                                        )
                                                        .fill(Color32::TRANSPARENT)
                                                        .stroke(Stroke::new(1.0, Color32::WHITE))
                                                        .rounding(Rounding::same(Radius::SM))
                                                    ).clicked() {
                                                        open_update_destination(update);
                                                    }
                                                }
                                                _ => {
                                                    if ui.add(
                                                        egui::Button::new(
                                                            RichText::new(locale_text(ui_lang, LocaleKey::UpdateDownload))
                                                                .size(FontSize::SM)
                                                                .color(Theme::ACCENT_PRIMARY),
                                                        )
                                                        .fill(Color32::WHITE)
                                                        .rounding(Rounding::same(Radius::SM))
                                                    ).clicked() {
                                                        if update.supports_auto_download() {
                                                            start_update_download(
                                                                Arc::clone(&self.state),
                                                                update.clone(),
                                                            );
                                                        } else {
                                                            open_update_destination(update);
                                                        }
                                                    }
                                                }
                                            }
                                        });
                                    });

                                    // Show download progress bar when downloading
                                    if let UpdateState::Downloading(progress) = &update_download_state {
                                        ui.add_space(Spacing::XS);
                                        let bar_width = ui.available_width();
                                        let bar_height = 4.0;
                                        let (rect, _) = ui.allocate_exact_size(Vec2::new(bar_width, bar_height), egui::Sense::hover());

                                        // Track (semi-transparent white)
                                        ui.painter().rect_filled(rect, Rounding::same(2.0), Color32::from_rgba_unmultiplied(255, 255, 255, 80));

                                        // Fill (solid white)
                                        let fill_w = rect.width() * progress.clamp(0.0, 1.0);
                                        if fill_w > 0.0 {
                                            let fill_rect = Rect::from_min_size(rect.min, Vec2::new(fill_w, bar_height));
                                            ui.painter().rect_filled(fill_rect, Rounding::same(2.0), Color32::WHITE);
                                        }
                                    }
                                });
                            });
                        ui.add_space(Spacing::MD);
                    }

                    // ════════════════════════════════════════════════════════════
                    // PROVIDER ICON GRID - macOS style with icons + names
                    // ════════════════════════════════════════════════════════════
                    if !providers.is_empty() {
                        let visible_providers: Vec<(usize, &ProviderData)> = providers.iter()
                            .enumerate()
                            .filter(|(_, p)| should_show_provider(p))
                            .collect();

                        if !visible_providers.is_empty() || !summary_providers.is_empty() {
                            // Provider grid - 4 columns with icons and names (compact)
                            let columns = 4;
                            let available_width = ui.available_width();
                            let cell_width = available_width / columns as f32;
                            let cell_height = 44.0; // Compact: icon + small name

                            egui::Grid::new("provider_grid")
                                .num_columns(columns)
                                .spacing([0.0, 2.0])
                                .show(ui, |ui| {
                                    let summary_selected = matches!(selected_tab, SelectedTab::Summary);
                                    let (rect, response) = ui.allocate_exact_size(
                                        Vec2::new(cell_width, cell_height),
                                        egui::Sense::click(),
                                    );
                                    let summary_color = Theme::ACCENT_PRIMARY;

                                    if summary_selected {
                                        ui.painter().rect_filled(
                                            rect,
                                            Rounding::same(Radius::SM),
                                            summary_color,
                                        );
                                    } else if response.hovered() {
                                        ui.painter().rect_filled(
                                            rect,
                                            Rounding::same(Radius::SM),
                                            Theme::CARD_BG_HOVER,
                                        );
                                    }

                                    let summary_text_color = if summary_selected {
                                        Color32::WHITE
                                    } else {
                                        summary_color
                                    };
                                    ui.painter().text(
                                        rect.center(),
                                        egui::Align2::CENTER_CENTER,
                                        locale_text(self.settings.ui_language, LocaleKey::SummaryTab),
                                        egui::FontId::proportional(10.0),
                                        summary_text_color,
                                    );

                                    if response.clicked()
                                        && let Ok(mut state) = self.state.lock()
                                    {
                                        state.selected_tab = SelectedTab::Summary;
                                    }

                                    for (i, (_, provider)) in visible_providers.iter().enumerate() {
                                        let is_selected = matches!(
                                            selected_tab,
                                            SelectedTab::Provider(selected_id)
                                                if ProviderId::from_cli_name(&provider.name)
                                                    == Some(selected_id)
                                        );
                                        let brand_color = provider_color(&provider.name);

                                        let (rect, response) = ui.allocate_exact_size(
                                            Vec2::new(cell_width, cell_height),
                                            egui::Sense::click(),
                                        );

                                        if is_selected {
                                            ui.painter().rect_filled(
                                                rect,
                                                Rounding::same(Radius::SM),
                                                brand_color,
                                            );
                                        } else if response.hovered() {
                                            ui.painter().rect_filled(
                                                rect,
                                                Rounding::same(Radius::SM),
                                                Theme::CARD_BG_HOVER,
                                            );
                                        }

                                        let icon_color = if is_selected { Color32::WHITE } else { brand_color };
                                        let icon_size = 18.0;
                                        let icon_center_y = rect.min.y + 14.0;
                                        let icon_min = egui::pos2(
                                            rect.center().x - icon_size / 2.0,
                                            icon_center_y - icon_size / 2.0,
                                        );

                                        if let Some(texture) = self.icon_cache.get_icon(
                                            ui.ctx(),
                                            &provider.name,
                                            icon_size as u32,
                                        ) {
                                            let img_rect =
                                                Rect::from_min_size(icon_min, Vec2::splat(icon_size));
                                            ui.painter().image(
                                                texture.id(),
                                                img_rect,
                                                Rect::from_min_max(
                                                    egui::pos2(0.0, 0.0),
                                                    egui::pos2(1.0, 1.0),
                                                ),
                                                icon_color,
                                            );
                                        } else {
                                            let letter = provider
                                                .display_name
                                                .chars()
                                                .next()
                                                .unwrap_or('?')
                                                .to_string();
                                            ui.painter().text(
                                                egui::pos2(rect.center().x, icon_center_y),
                                                egui::Align2::CENTER_CENTER,
                                                letter,
                                                egui::FontId::proportional(14.0),
                                                icon_color,
                                            );
                                        }

                                        let text_color = if is_selected {
                                            Color32::WHITE
                                        } else {
                                            Theme::TEXT_SECONDARY
                                        };
                                        let name_y = rect.min.y + 32.0;
                                        let display_name = if provider.display_name.len() > 7 {
                                            format!("{}…", &provider.display_name[..6])
                                        } else {
                                            provider.display_name.clone()
                                        };
                                        ui.painter().text(
                                            egui::pos2(rect.center().x, name_y),
                                            egui::Align2::CENTER_CENTER,
                                            display_name,
                                            egui::FontId::proportional(9.0),
                                            text_color,
                                        );

                                        if provider.status_level != StatusLevel::Operational
                                            && provider.status_level != StatusLevel::Unknown
                                        {
                                            let dot_radius = 4.0;
                                            let dot_center = egui::pos2(
                                                rect.max.x - dot_radius - 4.0,
                                                rect.min.y + dot_radius + 4.0,
                                            );
                                            let dot_color = status_color(provider.status_level);
                                            ui.painter()
                                                .circle_filled(dot_center, dot_radius, dot_color);
                                        }

                                        if response.clicked()
                                            && let Ok(mut state) = self.state.lock()
                                            && let Some(provider_id) =
                                                ProviderId::from_cli_name(&provider.name)
                                        {
                                            state.selected_tab = SelectedTab::Provider(provider_id);
                                        }

                                        if (i + 2) % columns == 0 {
                                            ui.end_row();
                                        }
                                    }
                                });

                            // Divider between grid and detail card
                            ui.add_space(2.0);
                            let sep_rect = ui.available_rect_before_wrap();
                            ui.painter().hline(
                                sep_rect.x_range(),
                                sep_rect.top(),
                                Stroke::new(1.0, Theme::SEPARATOR),
                            );
                            ui.add_space(2.0);

                            // ════════════════════════════════════════════════════════════
                            // SELECTED PROVIDER DETAIL CARD
                            // ════════════════════════════════════════════════════════════
                            let mut manual_refresh_requested = false;
                            let mut account_switch_provider: Option<String> = None;
                            let show_credits = self.settings.show_credits_extra_usage;
                            let show_as_used = self.settings.show_as_used;
                            let hide_personal_info = self.settings.hide_personal_info;
                            let ui_language = self.settings.ui_language;
                            if matches!(selected_tab, SelectedTab::Summary) {
                                manual_refresh_requested = draw_summary_card(
                                    ui,
                                    &summary_providers,
                                    is_refreshing,
                                    show_as_used,
                                    ui_language,
                                );
                            } else if let Some((_, selected_provider)) = visible_providers
                                .iter()
                                .find(|(_, provider)| {
                                    matches!(
                                        selected_tab,
                                        SelectedTab::Provider(selected_id)
                                            if ProviderId::from_cli_name(&provider.name)
                                                == Some(selected_id)
                                    )
                                })
                            {
                                let (refresh, switch) = draw_provider_detail_card(
                                    ui,
                                    selected_provider,
                                    &mut self.icon_cache,
                                    show_credits,
                                    show_as_used,
                                    hide_personal_info,
                                    ui_language,
                                );
                                manual_refresh_requested = refresh;
                                account_switch_provider = switch;
                            } else if let Some((_, first_provider)) = visible_providers.first() {
                                // Fallback to first if selected isn't visible
                                let (refresh, switch) = draw_provider_detail_card(
                                    ui,
                                    first_provider,
                                    &mut self.icon_cache,
                                    show_credits,
                                    show_as_used,
                                    hide_personal_info,
                                    ui_language,
                                );
                                manual_refresh_requested = refresh;
                                account_switch_provider = switch;
                            }

                            // Trigger manual refresh if requested
                            if manual_refresh_requested && !is_refreshing {
                                self.refresh_providers();
                            }

                            // Handle account switch request - open preferences to Providers tab with provider selected
                            if let Some(provider_name) = account_switch_provider
                                && let Some(provider_id) = ProviderId::from_cli_name(&provider_name) {
                                    self.preferences_window.active_tab = super::preferences::PreferencesTab::Providers;
                                    self.preferences_window.selected_provider = Some(provider_id);
                                    self.preferences_window.open();
                                }
                        } else if is_refreshing {
                            egui::Frame::none()
                                .fill(Theme::CARD_BG)
                                .rounding(Rounding::same(Radius::LG))
                                .inner_margin(Spacing::XXL)
                                .stroke(Stroke::new(1.0, Theme::CARD_BORDER))
                                .show(ui, |ui| {
                                    ui.vertical_centered(|ui| {
                                        ui.spinner();
                                        ui.add_space(Spacing::SM);
                                        ui.label(
                                            RichText::new(locale_text(self.settings.ui_language, LocaleKey::StateLoadingProviders))
                                                .size(FontSize::BASE)
                                                .color(Theme::TEXT_MUTED),
                                        );
                                    });
                                });
                        } else {
                            egui::Frame::none()
                                .fill(Theme::CARD_BG)
                                .rounding(Rounding::same(Radius::LG))
                                .inner_margin(Spacing::XXL)
                                .stroke(Stroke::new(1.0, Theme::CARD_BORDER))
                                .show(ui, |ui| {
                                    ui.vertical_centered(|ui| {
                                        ui.label(
                                            RichText::new(locale_text(self.settings.ui_language, LocaleKey::StateNoProviderData))
                                                .size(FontSize::BASE)
                                                .color(Theme::TEXT_MUTED),
                                        );
                                    });
                                });
                        }
                    } else {
                        let has_enabled_providers = !self.settings.get_enabled_provider_ids().is_empty();
                        egui::Frame::none()
                            .fill(Theme::CARD_BG)
                            .rounding(Rounding::same(Radius::LG))
                            .inner_margin(Spacing::XXL)
                            .stroke(Stroke::new(1.0, Theme::CARD_BORDER))
                            .show(ui, |ui| {
                                ui.vertical_centered(|ui| {
                                    if has_enabled_providers {
                                        ui.spinner();
                                        ui.add_space(Spacing::SM);
                                        ui.label(
                                            RichText::new(locale_text(self.settings.ui_language, LocaleKey::StateLoadingProviders))
                                                .size(FontSize::BASE)
                                                .color(Theme::TEXT_MUTED),
                                        );
                                    } else {
                                        ui.label(
                                            RichText::new(locale_text(self.settings.ui_language, LocaleKey::StateNoProviderSelected))
                                                .size(FontSize::BASE)
                                                .color(Theme::TEXT_MUTED),
                                        );
                                        ui.add_space(Spacing::SM);
                                        if ui.button(locale_text(self.settings.ui_language, LocaleKey::ButtonOpenProviderSettings)).clicked() {
                                            self.preferences_window.active_tab = super::preferences::PreferencesTab::Providers;
                                            self.preferences_window.open();
                                        }
                                    }
                                });
                            });
                    }

                    ui.add_space(4.0);

                    // ════════════════════════════════════════════════════════════
                    // BOTTOM MENU - macOS style vertical text items
                    // ════════════════════════════════════════════════════════════
                    draw_horizontal_separator(ui, 0.0);
                    ui.add_space(4.0);

                    if draw_text_menu_item(ui, locale_text(self.settings.ui_language, LocaleKey::MenuSettings), self.settings.ui_language) {
                        self.preferences_window.open();
                    }
                    if draw_text_menu_item(ui, locale_text(self.settings.ui_language, LocaleKey::MenuAbout), self.settings.ui_language) {
                        self.preferences_window.active_tab = super::preferences::PreferencesTab::About;
                        self.preferences_window.open();
                    }
                    if draw_text_menu_item(ui, locale_text(self.settings.ui_language, LocaleKey::MenuQuit), self.settings.ui_language) {
                        self.quit_application();
                    }
                }); // end ScrollArea
            });

        // Show preferences window
        self.preferences_window.show(ctx);

        let mut refresh_requested = self.preferences_window.take_refresh_requested();
        let previous_enabled_provider_ids = self.settings.get_enabled_provider_ids();
        let previous_ui_language = self.settings.ui_language;

        // Atomically consume settings changes so the flag is cleared in both
        // PreferencesWindow and the shared viewport state in one shot.
        if let Some(new_settings) = self.preferences_window.take_settings_if_changed() {
            let language_changed = new_settings.ui_language != previous_ui_language;
            self.settings = new_settings;
            if let Err(e) = self.settings.save() {
                tracing::error!("Failed to save settings: {}", e);
            }
            if previous_enabled_provider_ids != self.settings.get_enabled_provider_ids() {
                refresh_requested = true;
            }
            // If language changed, refresh the tray menu/tooltip to update localized strings
            if language_changed && let Some(ref tray_manager) = self.tray_manager {
                tray_manager.refresh_language();
            }
        }

        // Check if preferences window requested a provider refresh
        if refresh_requested {
            self.refresh_providers();
        }

        // Reload preferences snapshot when refresh completes (is_refreshing transitions to false)
        if self.preferences_window.is_open {
            let is_refreshing = self.state.lock().map(|s| s.is_refreshing).unwrap_or(false);
            if self.was_refreshing && !is_refreshing {
                // Refresh just completed - reload the snapshot
                self.preferences_window.reload_snapshot();
            }
            self.was_refreshing = is_refreshing;
        }
    }
}

fn summary_metric_text(
    label: &str,
    percent: Option<f64>,
    reset_text: Option<&str>,
    show_as_used: bool,
    ui_language: Language,
) -> Option<String> {
    percent.map(|used_percent| {
        let display_percent = usage_display_percent(used_percent, show_as_used);
        let mut text = format!(
            "{} {}",
            label,
            usage_display_label(display_percent, show_as_used, ui_language)
        );

        if let Some(reset) = reset_text {
            text.push_str(&format!(
                " • {} {}",
                locale_text(ui_language, LocaleKey::MetricResetsIn),
                reset
            ));
        }

        text
    })
}

struct SummaryMetricDisplay<'a> {
    label: &'a str,
    used_percent: f64,
    detail_text: String,
}

fn summary_metric_display<'a>(
    label: &'a str,
    percent: Option<f64>,
    reset_text: Option<&str>,
    show_as_used: bool,
    ui_language: Language,
) -> Option<SummaryMetricDisplay<'a>> {
    percent.map(|used_percent| SummaryMetricDisplay {
        label,
        used_percent,
        detail_text: summary_metric_text(
            label,
            Some(used_percent),
            reset_text,
            show_as_used,
            ui_language,
        )
        .unwrap_or_default(),
    })
}

fn draw_summary_metric_bar(ui: &mut egui::Ui, metric: &SummaryMetricDisplay<'_>, color: Color32) {
    ui.label(
        RichText::new(metric.label)
            .size(FontSize::XS)
            .color(Theme::TEXT_PRIMARY)
            .strong(),
    );
    ui.add_space(2.0);

    let bar_width = ui.available_width();
    let bar_height = 6.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(bar_width, bar_height), egui::Sense::hover());

    ui.painter()
        .rect_filled(rect, Rounding::same(3.0), Theme::progress_track());

    let fill_width = rect.width() * (metric.used_percent as f32 / 100.0).clamp(0.0, 1.0);
    if fill_width > 0.0 {
        let fill_rect = Rect::from_min_size(rect.min, Vec2::new(fill_width, bar_height));
        ui.painter()
            .rect_filled(fill_rect, Rounding::same(3.0), color);
    }

    ui.add_space(2.0);
    ui.label(
        RichText::new(&metric.detail_text)
            .size(FontSize::XS)
            .color(Theme::TEXT_SECONDARY),
    );
}

fn draw_summary_card(
    ui: &mut egui::Ui,
    providers: &[ProviderData],
    is_refreshing: bool,
    show_as_used: bool,
    ui_language: Language,
) -> bool {
    let mut refresh_requested = false;

    ui.vertical(|ui| {
        if is_refreshing {
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                ui.label(
                    RichText::new(locale_text(
                        ui_language,
                        LocaleKey::StateSummaryRefreshPending,
                    ))
                    .size(FontSize::XS)
                    .color(Theme::TEXT_SECONDARY),
                );
            });
            ui.add_space(4.0);
        }

        draw_horizontal_separator(ui, 0.0);
        ui.add_space(4.0);

        ui.horizontal(|ui| {
            ui.add_space(16.0);
            ui.vertical(|ui| {
                let visible_providers: Vec<&ProviderData> = providers
                    .iter()
                    .filter(|provider| should_show_provider(provider))
                    .collect();

                if visible_providers.is_empty() {
                    ui.label(
                        RichText::new(locale_text(ui_language, LocaleKey::StateNoProviderData))
                            .size(FontSize::SM)
                            .color(Theme::TEXT_SECONDARY),
                    );
                } else {
                    for (index, provider) in visible_providers.iter().enumerate() {
                        if index > 0 {
                            ui.add_space(8.0);
                            draw_horizontal_separator(ui, 0.0);
                            ui.add_space(8.0);
                        }

                        let brand_color = provider_color(&provider.name);
                        let (session_label, weekly_label) = provider_metric_labels(&provider.name);
                        let session_metric = summary_metric_display(
                            &session_label,
                            provider.session_percent,
                            provider.session_reset.as_deref(),
                            show_as_used,
                            ui_language,
                        );
                        let weekly_metric = summary_metric_display(
                            &weekly_label,
                            provider.weekly_percent,
                            provider.weekly_reset.as_deref(),
                            show_as_used,
                            ui_language,
                        );

                        ui.label(
                            RichText::new(&provider.display_name)
                                .size(FontSize::BASE)
                                .color(brand_color)
                                .strong(),
                        );

                        if let Some(error) = &provider.error {
                            ui.add_space(2.0);
                            ui.label(RichText::new(error).size(FontSize::XS).color(Theme::RED));
                        } else {
                            if let Some(session_metric) = session_metric {
                                ui.add_space(2.0);
                                draw_summary_metric_bar(ui, &session_metric, brand_color);
                            }

                            if let Some(weekly_metric) = weekly_metric {
                                ui.add_space(6.0);
                                draw_summary_metric_bar(ui, &weekly_metric, brand_color);
                            }
                        }

                        if let Some(plan) = &provider.plan {
                            ui.add_space(2.0);
                            ui.label(
                                RichText::new(plan)
                                    .size(FontSize::XS)
                                    .color(Theme::TEXT_SECONDARY),
                            );
                        }
                    }
                }
            });
        });

        ui.add_space(8.0);
        draw_horizontal_separator(ui, 0.0);
        ui.add_space(6.0);

        if draw_menu_item(ui, "↻", locale_text(ui_language, LocaleKey::ActionRefresh)) {
            refresh_requested = true;
        }

        refresh_requested
    })
    .inner
}

/// Draw a provider detail card - macOS UsageMenuCardView style
/// Structure: Header -> Divider -> Metrics (Session, Weekly, Model) -> Credits -> Cost
/// Returns (refresh_requested, account_switch_provider_name)
fn draw_provider_detail_card(
    ui: &mut egui::Ui,
    provider: &ProviderData,
    _icon_cache: &mut ProviderIconCache,
    show_credits_extra: bool,
    show_as_used: bool,
    hide_personal_info: bool,
    ui_language: Language,
) -> (bool, Option<String>) {
    let mut refresh_requested = false;
    let mut account_switch_requested: Option<String> = None;
    let brand_color = provider_color(&provider.name);
    // Main VStack with spacing: 4 (compact)
    ui.vertical(|ui| {
        ui.add_space(1.0); // Top padding

        // ═══════════════════════════════════════════════════════════════════
        // HEADER SECTION - UsageMenuCardHeaderView
        // ═══════════════════════════════════════════════════════════════════
        ui.horizontal(|ui| {
            ui.add_space(16.0); // Left padding

            ui.vertical(|ui| {
                // Row 1: Provider name (left) | Email (right)
                ui.horizontal(|ui| {
                    // Provider name - .headline, .semibold
                    ui.label(
                        RichText::new(&provider.display_name)
                            .size(FontSize::BASE) // Slightly smaller
                            .color(Theme::TEXT_PRIMARY)
                            .strong(),
                    );

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(16.0); // Right padding
                        // Email - .subheadline, secondary color (redacted if privacy mode enabled)
                        if let Some(account) = &provider.account {
                            let display_account = PersonalInfoRedactor::redact_email(
                                Some(account.as_str()),
                                hide_personal_info,
                            );
                            if !display_account.is_empty() {
                                ui.label(
                                    RichText::new(&display_account)
                                        .size(FontSize::XS) // Smaller
                                        .color(Theme::TEXT_SECONDARY),
                                );
                            }
                        }
                    });
                });

                ui.add_space(1.0); // VStack spacing in header

                // Row 2: Subtitle/Error (left) | Plan (right)
                ui.horizontal(|ui| {
                    // Subtitle - .footnote
                    if let Some(error) = &provider.error {
                        ui.label(
                            RichText::new(error)
                                .size(FontSize::XS) // 11px footnote
                                .color(Theme::RED),
                        );
                    } else {
                        ui.label(
                            RichText::new(locale_text(ui_language, LocaleKey::StatusJustUpdated))
                                .size(FontSize::XS)
                                .color(Theme::TEXT_SECONDARY),
                        );
                    }

                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.add_space(16.0); // Right padding
                        // Plan badge - .footnote, secondary
                        if let Some(plan) = &provider.plan {
                            ui.label(
                                RichText::new(plan)
                                    .size(FontSize::XS)
                                    .color(Theme::TEXT_SECONDARY),
                            );
                        }
                    });
                });

                // Row 3: Status description (if non-Operational)
                match &provider.status_description {
                    Some(status_desc)
                        if provider.status_level != StatusLevel::Operational
                            && provider.status_level != StatusLevel::Unknown =>
                    {
                        ui.add_space(2.0);
                        ui.horizontal(|ui| {
                            let status_col = status_color(provider.status_level);
                            let status_template = locale_text(ui_language, LocaleKey::StatusLabel);
                            ui.label(
                                RichText::new(status_template.replace("{}", status_desc))
                                    .size(FontSize::XS)
                                    .color(status_col),
                            );
                        });
                    }
                    _ => {}
                }
            });
        });

        // ═══════════════════════════════════════════════════════════════════
        // DIVIDER - only if we have metrics
        // ═══════════════════════════════════════════════════════════════════
        let has_metrics = provider.session_percent.is_some()
            || provider.weekly_percent.is_some()
            || provider.monthly_percent.is_some();
        let has_credits = provider.credits_remaining.is_some();
        let has_cost = provider.cost_used.is_some();
        let has_usage_breakdown = !provider.usage_breakdown.is_empty();

        if has_metrics || provider.error.is_some() || has_credits || has_cost || has_usage_breakdown
        {
            ui.add_space(4.0);
            draw_horizontal_separator(ui, 0.0);
        }

        // ═══════════════════════════════════════════════════════════════════
        // METRICS SECTION - macOS style with 12px spacing between metrics
        // ═══════════════════════════════════════════════════════════════════
        if has_metrics {
            ui.add_space(10.0);

            // Session metric (primary) - no pace indicator for session
            if let Some(session_pct) = provider.session_percent {
                draw_metric_row(
                    ui,
                    MetricRow {
                        title: locale_text(ui_language, LocaleKey::ProviderSession),
                        percent: session_pct,
                        show_as_used,
                        reset_text: provider.session_reset.as_deref(),
                        color: brand_color,
                        pace_percent: None, // No pace for session
                        pace_lasts_to_reset: false,
                        ui_language,
                    },
                );
            }

            // Weekly metric (secondary) - includes pace indicator
            if let Some(weekly_pct) = provider.weekly_percent {
                ui.add_space(12.0);

                draw_metric_row(
                    ui,
                    MetricRow {
                        title: locale_text(ui_language, LocaleKey::ProviderWeekly),
                        percent: weekly_pct,
                        show_as_used,
                        reset_text: provider.weekly_reset.as_deref(),
                        color: brand_color,
                        pace_percent: provider.pace_percent,
                        pace_lasts_to_reset: provider.pace_lasts_to_reset,
                        ui_language,
                    },
                );
            }

            // Monthly metric (tertiary) - for Infini 30-day quota
            if let Some(monthly_pct) = provider.monthly_percent {
                ui.add_space(12.0);

                draw_metric_row(
                    ui,
                    MetricRow {
                        title: locale_text(ui_language, LocaleKey::ProviderMonthly),
                        percent: monthly_pct,
                        show_as_used,
                        reset_text: provider.monthly_reset.as_deref(),
                        color: brand_color,
                        pace_percent: None, // No pace for monthly
                        pace_lasts_to_reset: false,
                        ui_language,
                    },
                );
            }

            // Model-specific metric
            if let Some(model_pct) = provider.model_percent {
                ui.add_space(12.0);

                let model_label = provider
                    .model_name
                    .as_deref()
                    .unwrap_or(locale_text(ui_language, LocaleKey::ProviderModel));
                draw_metric_row(
                    ui,
                    MetricRow {
                        title: model_label,
                        percent: model_pct,
                        show_as_used,
                        reset_text: None,
                        color: brand_color,
                        pace_percent: None, // No pace for model
                        pace_lasts_to_reset: false,
                        ui_language,
                    },
                );
            }

            ui.add_space(2.0);
        } else if provider.error.is_some() {
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.add_space(16.0);
                ui.label(
                    RichText::new(locale_text(ui_language, LocaleKey::StatusUnableToGetUsage))
                        .size(FontSize::SM)
                        .color(Theme::TEXT_SECONDARY),
                );
            });
            ui.add_space(2.0);
        }

        // ═══════════════════════════════════════════════════════════════════
        // CREDITS SECTION - macOS CreditsBarContent style
        // ═══════════════════════════════════════════════════════════════════
        if show_credits_extra && let Some(credits) = provider.credits_remaining {
            if has_metrics {
                draw_horizontal_separator(ui, 0.0);
            }
            ui.add_space(12.0);

            let bar_width = ui.available_width();

            // Title: "Credits" - .font(.body).fontWeight(.medium)
            ui.label(
                RichText::new(locale_text(ui_language, LocaleKey::CreditsTitle))
                    .size(FontSize::BASE)
                    .color(Theme::TEXT_PRIMARY)
                    .strong(),
            );

            // Progress bar
            if let Some(credits_pct) = provider.credits_percent {
                ui.add_space(6.0);
                let bar_height = 8.0;
                let (rect, _) =
                    ui.allocate_exact_size(Vec2::new(bar_width, bar_height), egui::Sense::hover());

                ui.painter()
                    .rect_filled(rect, Rounding::same(4.0), Theme::progress_track());

                let fill_w = rect.width() * (credits_pct as f32 / 100.0).clamp(0.0, 1.0);
                if fill_w > 0.0 {
                    let fill_rect = Rect::from_min_size(rect.min, Vec2::new(fill_w, bar_height));
                    ui.painter()
                        .rect_filled(fill_rect, Rounding::same(4.0), brand_color);
                }
            }

            // Info row: X left (left) | 1K tokens (right) - .font(.caption)
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let remaining_template = locale_text(ui_language, LocaleKey::RemainingAmount);
                ui.label(
                    RichText::new(remaining_template.replace("{:.2}", &format!("{:.2}", credits)))
                        .size(FontSize::XS)
                        .color(Theme::TEXT_PRIMARY),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        RichText::new(locale_text(ui_language, LocaleKey::Tokens1K))
                            .size(FontSize::XS)
                            .color(Theme::TEXT_SECONDARY),
                    );
                });
            });

            // Buy Credits link
            ui.add_space(6.0);
            if draw_menu_item(
                ui,
                "⊕",
                locale_text(ui_language, LocaleKey::ActionBuyCredits),
            ) && let Some(ref url) = provider.dashboard_url
            {
                let _ = open::that(url);
            }

            // Credits history chart
            if !provider.credits_history.is_empty() {
                ui.add_space(8.0);
                let chart_points: Vec<ChartPoint> = provider
                    .credits_history
                    .iter()
                    .map(|(date, value)| ChartPoint::new(date.clone(), *value))
                    .collect();
                let mut chart = CreditsHistoryChart::new(chart_points);
                chart.show(ui);
            }
        }

        // ═══════════════════════════════════════════════════════════════════
        // USAGE BREAKDOWN SECTION - stacked service credits chart
        // ═══════════════════════════════════════════════════════════════════
        if has_usage_breakdown {
            if has_metrics || has_credits {
                draw_horizontal_separator(ui, 0.0);
            }
            ui.add_space(12.0);

            ui.label(
                RichText::new(locale_text(ui_language, LocaleKey::SectionUsageBreakdown))
                    .size(FontSize::BASE)
                    .color(Theme::TEXT_PRIMARY)
                    .strong(),
            );
            ui.add_space(6.0);

            let mut chart = UsageBreakdownChart::new(provider.usage_breakdown.clone());
            chart.show(ui);
        }

        // ═══════════════════════════════════════════════════════════════════
        // COST SECTION - macOS TokenUsageSection style
        // ═══════════════════════════════════════════════════════════════════
        if show_credits_extra && (provider.cost_used.is_some() || !provider.cost_history.is_empty())
        {
            if has_metrics || has_credits || has_usage_breakdown {
                draw_horizontal_separator(ui, 0.0);
            }
            ui.add_space(12.0);

            // Title: "Cost" - .font(.body).fontWeight(.medium)
            ui.label(
                RichText::new(locale_text(ui_language, LocaleKey::SectionCost))
                    .size(FontSize::BASE)
                    .color(Theme::TEXT_PRIMARY)
                    .strong(),
            );

            ui.add_space(6.0);

            // Cost details - Today and Last 30 days - .font(.caption)
            if !provider.cost_history.is_empty() {
                let total_30d: f64 = provider.cost_history.iter().map(|(_, cost)| cost).sum();
                let today_cost: f64 = provider.cost_history.last().map(|(_, c)| *c).unwrap_or(0.0);

                let today_template = locale_text(ui_language, LocaleKey::TodayCost);
                let total30d_template = locale_text(ui_language, LocaleKey::Last30DaysCost);
                ui.label(
                    RichText::new(today_template.replace("{:.2}", &format!("{:.2}", today_cost)))
                        .size(FontSize::XS)
                        .color(Theme::TEXT_PRIMARY),
                );
                ui.label(
                    RichText::new(total30d_template.replace("{:.2}", &format!("{:.2}", total_30d)))
                        .size(FontSize::XS)
                        .color(Theme::TEXT_PRIMARY),
                );
            } else if let Some(cost_used) = &provider.cost_used {
                ui.label(
                    RichText::new(cost_used)
                        .size(FontSize::XS)
                        .color(Theme::TEXT_PRIMARY),
                );
            }

            // Cost history chart
            if !provider.cost_history.is_empty() {
                ui.add_space(8.0);
                let chart_points: Vec<ChartPoint> = provider
                    .cost_history
                    .iter()
                    .map(|(date, cost)| ChartPoint::new(date.clone(), *cost))
                    .collect();
                let mut chart = CostHistoryChart::new(chart_points, brand_color);
                chart.show(ui);
            }
        }

        // ═══════════════════════════════════════════════════════════════════
        // ACTION LINKS SECTION - macOS style vertical list
        // ═══════════════════════════════════════════════════════════════════
        let has_dashboard = provider.dashboard_url.is_some();
        let has_status_issue = provider.status_level != StatusLevel::Operational;
        let has_error = provider.error.is_some();

        if has_dashboard || has_status_issue || has_error {
            if has_metrics || has_credits || has_cost || has_usage_breakdown {
                draw_horizontal_separator(ui, 0.0);
            }
            ui.add_space(6.0);

            // Vertical action links like macOS
            // Refresh button - first action
            if draw_menu_item(ui, "↻", locale_text(ui_language, LocaleKey::ActionRefresh)) {
                refresh_requested = true;
            }

            // Switch Account link - only show for providers that support token accounts
            if TokenAccountSupport::is_supported(
                ProviderId::from_cli_name(&provider.name).unwrap_or(ProviderId::Claude),
            ) && draw_menu_item(
                ui,
                "->",
                locale_text(ui_language, LocaleKey::ActionSwitchAccount),
            ) {
                account_switch_requested = Some(provider.name.clone());
            }

            // Usage Dashboard link
            if let Some(ref url) = provider.dashboard_url {
                let dashboard_url = url.clone();
                if draw_menu_item(
                    ui,
                    "📊",
                    locale_text(ui_language, LocaleKey::ActionUsageDashboard),
                ) {
                    let _ = open::that(&dashboard_url);
                }
            }

            // Status Page link
            if let Some(status_url) = get_status_page_url(&provider.name)
                && draw_menu_item(
                    ui,
                    "⚡",
                    locale_text(ui_language, LocaleKey::ActionStatusPage),
                )
            {
                let _ = open::that(status_url);
            }

            // Copy Error link
            if let Some(ref error) = provider.error {
                let error_text = error.clone();
                if draw_menu_item(
                    ui,
                    "📋",
                    locale_text(ui_language, LocaleKey::ActionCopyError),
                ) && let Ok(mut clipboard) = arboard::Clipboard::new()
                {
                    let _ = clipboard.set_text(&error_text);
                }
            }

            ui.add_space(4.0);
        }

        (refresh_requested, account_switch_requested)
    })
    .inner
}

/// Draw a horizontal separator with left padding
fn draw_horizontal_separator(ui: &mut egui::Ui, left_padding: f32) {
    ui.horizontal(|ui| {
        ui.add_space(left_padding);
        let sep_rect = ui.available_rect_before_wrap();
        let sep_width = sep_rect.width() - left_padding;
        ui.painter().hline(
            sep_rect.left()..=(sep_rect.left() + sep_width),
            sep_rect.top(),
            Stroke::new(1.0, Theme::SEPARATOR),
        );
    });
}

/// Draw a text-only menu item (Settings, About, Quit style)
fn draw_text_menu_item(ui: &mut egui::Ui, label: &str, _ui_language: Language) -> bool {
    let available_width = ui.available_width();

    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(available_width, 24.0), egui::Sense::click());

    let is_hovered = response.hovered();

    if is_hovered {
        ui.painter()
            .rect_filled(rect, Rounding::same(Radius::SM), Theme::menu_hover());
    }

    let text_color = if is_hovered {
        Theme::TEXT_PRIMARY
    } else {
        Theme::TEXT_SECONDARY
    };

    // Label
    let label_pos = egui::pos2(rect.min.x + 4.0, rect.center().y);
    ui.painter().text(
        label_pos,
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(FontSize::SM),
        text_color,
    );

    response.clicked()
}

/// Draw a single metric row - macOS style matching SwiftUI MetricRow.
/// Structure: Title (.body.medium) → Progress bar (with optional pace marker) →
/// X% used | Pace status | Resets in Xh (.footnote)
struct MetricRow<'a> {
    title: &'a str,
    percent: f64,
    show_as_used: bool,
    reset_text: Option<&'a str>,
    color: Color32,
    /// Difference between actual and expected usage. Positive means ahead of expected, negative means behind.
    pace_percent: Option<f64>,
    /// Whether current usage will last until reset (on track or ahead).
    pace_lasts_to_reset: bool,
    ui_language: Language,
}

fn draw_metric_row(ui: &mut egui::Ui, metric: MetricRow<'_>) {
    let MetricRow {
        title,
        percent,
        show_as_used,
        reset_text,
        color,
        pace_percent,
        pace_lasts_to_reset,
        ui_language,
    } = metric;

    // Title - .font(.body).fontWeight(.medium)
    ui.label(
        RichText::new(title)
            .size(FontSize::BASE)
            .color(Theme::TEXT_PRIMARY)
            .strong(),
    );

    ui.add_space(6.0);

    let display_percent = usage_display_percent(percent, show_as_used);
    let display_pace_percent = pace_percent.map(|pace| if show_as_used { pace } else { -pace });

    // Progress bar row - 8px height like macOS
    let bar_width = ui.available_width();
    let bar_height = 8.0;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(bar_width, bar_height), egui::Sense::hover());

    // Track
    ui.painter()
        .rect_filled(rect, Rounding::same(4.0), Theme::progress_track());

    // Fill
    let fill_w = rect.width() * (display_percent as f32 / 100.0).clamp(0.0, 1.0);
    if fill_w > 0.0 {
        let fill_rect = Rect::from_min_size(rect.min, Vec2::new(fill_w, bar_height));
        ui.painter()
            .rect_filled(fill_rect, Rounding::same(4.0), color);
    }

    // Pace marker - thin vertical line showing expected usage position
    if let Some(pace_diff) = display_pace_percent {
        // pace_percent is the difference (actual - expected), so expected = actual - pace_diff
        let expected_position = (display_percent - pace_diff).clamp(0.0, 100.0);
        let marker_x = rect.min.x + rect.width() * (expected_position as f32 / 100.0);

        // Draw 2px wide vertical line in a contrasting color (white with transparency)
        let marker_color = Color32::from_rgba_unmultiplied(255, 255, 255, 180);
        let marker_width = 2.0;
        let marker_rect = Rect::from_min_size(
            egui::pos2(marker_x - marker_width / 2.0, rect.min.y),
            Vec2::new(marker_width, bar_height),
        );
        ui.painter()
            .rect_filled(marker_rect, Rounding::same(1.0), marker_color);
    }

    ui.add_space(6.0);

    // Info row: X% used (left) | Pace status | Resets in Xh (right) - .font(.footnote)
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(usage_display_label(
                display_percent,
                show_as_used,
                ui_language,
            ))
            .size(FontSize::XS)
            .color(Theme::TEXT_PRIMARY),
        );

        // Pace status indicator
        if display_pace_percent.is_some() {
            ui.add_space(8.0);
            let (pace_text, pace_color) = if pace_lasts_to_reset {
                (
                    locale_text(ui_language, LocaleKey::PaceOnTrack),
                    Theme::GREEN,
                )
            } else {
                (
                    locale_text(ui_language, LocaleKey::PaceBehind),
                    Theme::YELLOW,
                )
            };
            ui.label(
                RichText::new(pace_text)
                    .size(FontSize::XS)
                    .color(pace_color),
            );
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(reset) = reset_text {
                ui.label(
                    RichText::new(format!(
                        "{} {}",
                        locale_text(ui_language, LocaleKey::MetricResetsIn),
                        reset
                    ))
                    .size(FontSize::XS)
                    .color(Theme::TEXT_SECONDARY),
                );
            }
        });
    });
}

/// Draw a menu item button - macOS style compact
fn draw_menu_item(ui: &mut egui::Ui, icon: &str, label: &str) -> bool {
    let available_width = ui.available_width();

    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(available_width, 32.0), // Slightly larger height
        egui::Sense::click(),
    );

    let is_hovered = response.hovered();

    if is_hovered {
        ui.painter()
            .rect_filled(rect, Rounding::same(Radius::SM), Theme::menu_hover());
    }

    let text_color = if is_hovered {
        Theme::TEXT_PRIMARY
    } else {
        Theme::TEXT_SECONDARY
    };

    // Icon
    let icon_pos = egui::pos2(rect.min.x + Spacing::SM, rect.center().y);
    ui.painter().text(
        icon_pos,
        egui::Align2::LEFT_CENTER,
        icon,
        egui::FontId::proportional(FontSize::MD),
        text_color,
    );

    // Label
    let label_pos = egui::pos2(rect.min.x + Spacing::SM + 22.0, rect.center().y);
    ui.painter().text(
        label_pos,
        egui::Align2::LEFT_CENTER,
        label,
        egui::FontId::proportional(FontSize::SM),
        text_color,
    );

    response.clicked()
}

/// Run the application
pub fn run() -> anyhow::Result<()> {
    #[cfg(windows)]
    if is_remote_session() {
        show_remote_session_error_dialog();
        anyhow::bail!(remote_session_error_message());
    }

    // Delete any corrupted window state
    if let Some(data_dir) = dirs::data_dir() {
        let state_file = data_dir.join("CodexBar").join("data").join("app.ron");
        if state_file.exists() {
            let _ = std::fs::remove_file(&state_file);
        }
    }

    let options = build_native_options();

    eframe::run_native(
        "CodexBar",
        options,
        Box::new(|cc| Ok(Box::new(CodexBarApp::new(cc)))),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {}", e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        ProviderData, remote_session_error_message, should_show_provider, summary_metric_display,
        summary_metric_text,
    };
    use crate::settings::Language;
    use crate::status::StatusLevel;

    fn test_provider() -> ProviderData {
        ProviderData {
            name: "codex".to_string(),
            display_name: "Codex".to_string(),
            account: None,
            session_percent: None,
            session_reset: None,
            weekly_percent: None,
            weekly_reset: None,
            monthly_percent: None,
            monthly_reset: None,
            model_percent: None,
            model_name: None,
            plan: None,
            error: None,
            dashboard_url: None,
            pace_percent: None,
            pace_lasts_to_reset: false,
            cost_used: None,
            credits_remaining: None,
            credits_percent: None,
            status_level: StatusLevel::Unknown,
            status_description: None,
            cost_history: Vec::new(),
            credits_history: Vec::new(),
            usage_breakdown: Vec::new(),
        }
    }

    #[test]
    fn remote_session_error_mentions_cli_and_log() {
        let message = remote_session_error_message();

        assert!(message.contains("Remote Desktop"));
        assert!(message.contains("codexbar usage -p claude"));
        assert!(message.contains("%TEMP%\\codexbar_launch.log"));
    }

    #[test]
    fn should_show_provider_when_any_usage_exists() {
        let mut provider = test_provider();
        provider.weekly_percent = Some(42.0);

        assert!(should_show_provider(&provider));
    }

    #[test]
    fn should_show_provider_when_error_exists() {
        let mut provider = test_provider();
        provider.error = Some("Auth required".to_string());

        assert!(should_show_provider(&provider));
    }

    #[test]
    fn should_hide_provider_without_usage_or_error() {
        let provider = test_provider();

        assert!(!should_show_provider(&provider));
    }

    #[test]
    fn summary_metric_text_uses_remaining_mode() {
        let text = summary_metric_text(
            "Weekly",
            Some(20.0),
            Some("3h 0m"),
            false,
            Language::English,
        )
        .expect("metric text");

        assert!(text.contains("Weekly"));
        assert!(text.contains("80%"));
        assert!(text.contains("3h 0m"));
    }

    #[test]
    fn summary_metric_display_preserves_used_percent_for_bar_fill() {
        let display = summary_metric_display(
            "Session",
            Some(65.0),
            Some("1h 30m"),
            false,
            Language::English,
        )
        .expect("metric display");

        assert_eq!(display.label, "Session");
        assert!((display.used_percent - 65.0).abs() < f64::EPSILON);
        assert!(display.detail_text.contains("35%"));
    }
}
