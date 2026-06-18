use super::*;

impl LocaleKey {
    pub(super) fn chinese(self) -> &'static str {
        match self {
            // Tab names
            LocaleKey::TabGeneral => "通用",
            LocaleKey::TabProviders => "服务商",
            LocaleKey::TabDisplay => "显示",
            LocaleKey::TabApiKeys => "API 密钥",
            LocaleKey::TabCookies => "Cookie",
            LocaleKey::TabAdvanced => "高级",
            LocaleKey::TabAbout => "关于",
            LocaleKey::TabShortcuts => "快捷键",

            // General settings
            LocaleKey::InterfaceLanguage => "界面语言",
            LocaleKey::StartupSettings => "系统",
            LocaleKey::StartAtLogin => "开机启动",
            LocaleKey::StartMinimized => "最小化启动",
            LocaleKey::StartAtLoginHelper => "登录后自动启动 CodexBar",
            LocaleKey::StartMinimizedHelper => "启动后停留在系统托盘",

            // Notification settings
            LocaleKey::ShowNotifications => "显示通知",
            LocaleKey::ShowNotificationsHelper => "达到用量阈值时提醒",
            LocaleKey::SoundEnabled => "声音提示",
            LocaleKey::SoundEnabledHelper => "达到阈值时播放提示音",
            LocaleKey::SoundVolume => "提示音音量",
            LocaleKey::HighUsageThreshold => "高用量阈值",
            LocaleKey::HighUsageThresholdHelper => "在该用量水平显示预警",
            LocaleKey::HighUsageAlert => "高位预警",
            LocaleKey::CriticalUsageThreshold => "紧急用量阈值",
            LocaleKey::CriticalUsageThresholdHelper => "在该水平显示严重告警",
            LocaleKey::CriticalUsageAlert => "严重告警",

            // Display settings
            LocaleKey::ShowUsageAsUsed => "显示已用用量",
            LocaleKey::ShowUsageAsUsedHelper => "显示为已使用百分比（而非剩余）",
            LocaleKey::ResetTimeRelative => "相对重置时间",
            LocaleKey::ResetTimeRelativeHelper => "显示\"2h 30m\"而不是\"3:00 PM\"",
            LocaleKey::ShowCreditsExtra => "显示额度与扩展用量",
            LocaleKey::ShowCreditsExtraHelper => "显示额度余额和额外用量信息",
            LocaleKey::UsageDisplay => "用量显示",
            LocaleKey::TrayIcon => "托盘图标",
            LocaleKey::MergeTrayIcons => "合并托盘图标",
            LocaleKey::MergeTrayIconsHelper => "将所有服务商显示在一个托盘图标中",
            LocaleKey::PerProviderTrayIcons => "按服务商分图标",
            LocaleKey::PerProviderTrayIconsHelper => "每个启用的服务商显示独立托盘图标",

            // Provider settings
            LocaleKey::ProviderEnabled => "已启用",
            LocaleKey::ProviderDisabled => "已禁用",
            LocaleKey::ProviderInfo => "信息",
            LocaleKey::ProviderUsage => "用量",
            LocaleKey::AuthType => "认证方式",
            LocaleKey::DataSource => "数据来源",
            LocaleKey::ProviderNotDetected => "未检测到",
            LocaleKey::ProviderLastFetchFailed => "上次获取失败",
            LocaleKey::ProviderUsageNotFetchedYet => "尚未获取用量",
            LocaleKey::ProviderNotFetchedYetTitle => "尚未获取",
            LocaleKey::ProviderDisabledNoRecentData => "已禁用 — 没有最近数据",
            LocaleKey::ProviderSourceAutoShort => "自动",
            LocaleKey::ProviderSourceWebShort => "网页",
            LocaleKey::ProviderSourceCliShort => "CLI",
            LocaleKey::ProviderSourceOauthShort => "OAuth",
            LocaleKey::ProviderSourceApiShort => "API",
            LocaleKey::ProviderSourceGithubApiShort => "GitHub API",
            LocaleKey::ProviderSourceLocalShort => "本地",
            LocaleKey::ProviderSourceKiroEnvShort => "Kiro 环境",
            LocaleKey::TrackingItem => "追踪项",
            LocaleKey::MainWindowLiveUsageData => "主窗口实时用量数据",
            LocaleKey::StartTrackingUsage => "启用后开始追踪用量",
            LocaleKey::ClickTrayIconForMetrics => "点击托盘图标查看实时指标",

            // Browser cookie import
            LocaleKey::BrowserCookieImport => "浏览器 Cookie 导入",
            LocaleKey::ImportFromBrowser => "从浏览器导入 {} 的 Cookies",
            LocaleKey::NoCookiesFoundInBrowser => "在 {} 的 {} 中未找到 Cookies。请先确认已登录",
            LocaleKey::SelectBrowser => "请选择浏览器...",
            LocaleKey::ImportCookies => "导入 Cookies",
            LocaleKey::ImportSuccess => "已为 {} 导入 Cookies",
            LocaleKey::ImportFailed => "导入失败：{}",
            LocaleKey::SaveFailed => "保存失败：{}",
            LocaleKey::CookiesAutoImport => {
                "Cookies 会自动从 Chrome、Edge、Brave 和 Firefox 中提取"
            }
            LocaleKey::QuickActions => "快捷操作",
            LocaleKey::OpenProviderDashboard => "→ 打开 {} 仪表盘",
            LocaleKey::OllamaNoDashboard => "Ollama 在本地运行，无仪表盘",

            // API Keys tab
            LocaleKey::ApiKeysTitle => "API 密钥",
            LocaleKey::ApiKeysDescription => "为需要认证的服务商配置访问令牌。",
            LocaleKey::AddKey => "+ 添加密钥",
            LocaleKey::KeySet => "✓ 已设置",
            LocaleKey::KeyRequired => "需要密钥",
            LocaleKey::Remove => "移除",
            LocaleKey::GetKey => "获取密钥 →",

            // Cookies tab
            LocaleKey::SavedCookies => "已保存的 Cookies",
            LocaleKey::AddManualCookie => "添加手动 Cookie",
            LocaleKey::CookieHeader => "Cookie 头",
            LocaleKey::PasteHere => "在这里粘贴...",
            LocaleKey::DeleteCookie => "删除",
            LocaleKey::CookieSaved => "已保存 {} 个 Cookies",
            LocaleKey::CookieDeleted => "已删除 {} 的 Cookies",

            // Advanced tab
            LocaleKey::RefreshSettings => "刷新",
            LocaleKey::Animations => "动画",
            LocaleKey::MenuBar => "菜单栏",
            LocaleKey::Fun => "趣味",
            LocaleKey::GlobalShortcut => "全局快捷键",
            LocaleKey::Privacy => "隐私",
            LocaleKey::Updates => "更新",
            LocaleKey::UpdateChannel => "更新通道",
            LocaleKey::UpdateChannelStable => "稳定版",
            LocaleKey::UpdateChannelBeta => "测试预览版",
            LocaleKey::Never => "从不",
            LocaleKey::LastUpdated => "上次更新",
            LocaleKey::MinutesAgo => "{} 分钟前更新",
            LocaleKey::HoursAgo => "{} 小时前更新",
            LocaleKey::DaysAgo => "{} 天前更新",
            LocaleKey::BuiltWithRust => "基于 Rust + egui 构建",
            LocaleKey::OriginalMacOSVersion => "原始 macOS 版本",
            LocaleKey::Links => "链接",
            LocaleKey::BuildInfo => "构建信息",
            LocaleKey::EnabledProviders => "已启用服务商",
            LocaleKey::Appearance => "外观",
            LocaleKey::ThemeSelection => "主题",
            LocaleKey::LightMode => "浅色",
            LocaleKey::DarkMode => "深色",

            // About
            LocaleKey::AboutTitle => "关于 CodexBar",
            LocaleKey::Version => "版本",

            // Main popup - Header actions
            LocaleKey::ActionRefreshAll => "刷新全部",
            LocaleKey::ActionSettings => "设置",
            LocaleKey::ActionClose => "✕",

            // Main popup - Provider section
            LocaleKey::ProviderAccount => "账号",
            LocaleKey::ProviderSession => "本次会话",
            LocaleKey::ProviderWeekly => "本周",
            LocaleKey::ProviderMonthly => "30天",
            LocaleKey::ProviderModel => "模型",
            LocaleKey::ProviderPlan => "套餐",
            LocaleKey::ProviderNextReset => "下次重置",
            LocaleKey::ProviderNoRecentUsage => "暂无用量",
            LocaleKey::ProviderNotSignedIn => "未登录",
            LocaleKey::SummaryTab => "汇总",

            // Main popup - Loading/Empty/Error states
            LocaleKey::StateLoadingProviders => "正在加载服务商...",
            LocaleKey::StateNoProviderData => "暂无服务商数据。",
            LocaleKey::StateNoProviderSelected => "尚未选择服务商。",
            LocaleKey::StateSummaryRefreshPending => "将在全部服务商刷新完成后更新",
            LocaleKey::StateError => "错误",
            LocaleKey::StateRetry => "重试",
            LocaleKey::StateDownload => "下载",
            LocaleKey::StateRestartAndUpdate => "重启并更新",

            // Main popup - Credits
            LocaleKey::CreditsTitle => "额度",

            // Main popup - Update banner (non-happy-path)
            LocaleKey::UpdateRestartAndUpdate => "重启并更新",
            LocaleKey::UpdateRetry => "重试",
            LocaleKey::UpdateDownload => "下载",
            LocaleKey::UpdateDownloading => "下载中",
            LocaleKey::UpdateReady => "准备安装",
            LocaleKey::UpdateFailed => "更新失败",

            // Main popup - Settings button
            LocaleKey::ButtonOpenProviderSettings => "打开服务商设置",

            // Main popup - Bottom menu (Actions)
            LocaleKey::MenuSettings => "设置...",
            LocaleKey::MenuAbout => "关于 CodexBar",
            LocaleKey::MenuQuit => "退出",

            // Main popup - Status strings
            LocaleKey::StatusJustUpdated => "刚刚更新",
            LocaleKey::StatusUnableToGetUsage => "无法获取用量",

            // Main popup - Provider detail actions
            LocaleKey::ActionRefresh => "刷新",
            LocaleKey::ActionSwitchAccount => "切换账号...",
            LocaleKey::ActionUsageDashboard => "用量仪表盘",
            LocaleKey::ActionStatusPage => "状态页面",
            LocaleKey::ActionCopyError => "复制错误",
            LocaleKey::ActionBuyCredits => "购买额度...",

            // Main popup - Pace status
            LocaleKey::PaceOnTrack => "进度正常",
            LocaleKey::PaceBehind => "进度滞后",

            // Main popup - Reset prefix
            LocaleKey::MetricResetsIn => "重置于",

            // Main popup - Section titles
            LocaleKey::SectionUsageBreakdown => "用量明细",
            LocaleKey::SectionCost => "费用",

            // Tray - Single icon mode
            LocaleKey::TrayOpenCodexBar => "弹出仪表盘",
            LocaleKey::TrayPopOutDashboard => "弹出仪表盘",
            LocaleKey::TrayRefreshAll => "刷新全部",
            LocaleKey::TrayProviders => "服务商",
            LocaleKey::TraySettings => "设置...",
            LocaleKey::TrayCheckForUpdates => "检查更新",
            LocaleKey::TrayQuit => "退出",
            LocaleKey::TrayLoading => "CodexBar - 加载中...",
            LocaleKey::TrayNoProviders => "CodexBar - 无可用服务商",
            LocaleKey::TraySessionPercent => "本次会话 {}%",
            LocaleKey::TrayWeeklyPercent => "本周 {}%",
            LocaleKey::TrayStatusError => "（错误）",
            LocaleKey::TrayStatusStale => "（数据过期）",
            LocaleKey::TrayStatusIncident => "（故障）",
            LocaleKey::TrayStatusPartial => "（部分中断）",
            LocaleKey::TrayWeeklyExhausted => "周额度已用尽",
            LocaleKey::TrayCreditsRemaining => "剩余额度 {}%",
            LocaleKey::TrayStatusRowLoading => "加载中...",
            LocaleKey::TrayStatusRowError => "错误",
            LocaleKey::TrayCreditsRow => "额度 {}%",

            // Main popup - Usage/reset labels
            LocaleKey::ResetInProgress => "正在重置...",
            LocaleKey::TomorrowAt => "明天 {}",
            LocaleKey::UsedPercent => "已使用 {:.0}%",
            LocaleKey::RemainingPercent => "剩余 {:.0}%",
            LocaleKey::RemainingAmount => "剩余 {:.2}",
            LocaleKey::Tokens1K => "1K tokens",
            LocaleKey::TodayCost => "今日：${:.2}",
            LocaleKey::Last30DaysCost => "近 30 天：${:.2}",
            LocaleKey::StatusLabel => "状态：{}",

            // Main popup - Update banner messages
            LocaleKey::UpdateAvailableMessage => "有可用更新：{}",
            LocaleKey::UpdateReadyMessage => "{} 准备安装",
            LocaleKey::UpdateFailedMessage => "更新失败：{}",
            LocaleKey::UpdateDownloadingMessage => "正在下载 {} ({:.0}%)",

            // Tray - Per-provider mode
            LocaleKey::TrayProviderPopOut => "弹出仪表盘",
            LocaleKey::TrayProviderRefresh => "刷新",
            LocaleKey::TrayProviderSettings => "设置...",
            LocaleKey::TrayProviderQuit => "退出",

            // Provider settings - Live renderer specific
            LocaleKey::State => "状态",
            LocaleKey::Source => "来源",
            LocaleKey::Updated => "更新时间",
            LocaleKey::NeverUpdated => "从未更新",
            LocaleKey::UpdatedJustNow => "刚刚更新",
            LocaleKey::UpdatedMinutesAgo => "{} 分钟前更新",
            LocaleKey::UpdatedHoursAgo => "{} 小时前更新",
            LocaleKey::UpdatedDaysAgo => "{} 天前更新",
            LocaleKey::Status => "状态",
            LocaleKey::AllSystemsOperational => "系统运行正常",
            LocaleKey::Plan => "套餐",
            LocaleKey::Account => "账号",

            // Provider detail - Usage section
            LocaleKey::ProviderSessionLabel => "本次会话",
            LocaleKey::ProviderWeeklyLabel => "本周",
            LocaleKey::ProviderCodeReviewLabel => "代码审查",
            LocaleKey::ResetsInShort => "重置于",
            LocaleKey::ResetsInDaysHours => "{} 天 {} 小时后重置",
            LocaleKey::ResetsInHoursMinutes => "{} 小时 {} 分钟后重置",

            // Provider detail - Tray Display
            LocaleKey::TrayDisplayTitle => "托盘显示",
            LocaleKey::ShowInTray => "在托盘中显示",

            // Provider detail - Credits
            LocaleKey::CreditsLabel => "额度",
            LocaleKey::CreditsLeft => "剩余 {:.1}",

            // Provider detail - Cost
            LocaleKey::CostTitle => "费用",
            LocaleKey::TodayCostFull => "今日：${:.2} • {} tokens",
            LocaleKey::Last30DaysCostFull => "近 30 天：${:.2} • {} tokens",

            // Provider detail - Settings section
            LocaleKey::ProviderSettingsTitle => "设置",
            LocaleKey::ProviderAccountsTitle => "账号",
            LocaleKey::ProviderOptionsTitle => "选项",
            LocaleKey::MenuBarMetric => "菜单栏指标",
            LocaleKey::MenuBarMetricHelper => "选择由哪个窗口驱动菜单栏百分比。",
            LocaleKey::UsageSource => "用量来源",
            LocaleKey::ProviderNoCodexAccountsDetected => "尚未检测到 Codex 账号。",
            LocaleKey::ProviderCodexAutoImportHelp => "自动导入浏览器 Cookie 以补充仪表盘信息。",
            LocaleKey::ProviderCodexHistoryHelp => {
                "在本地保存 Codex 用量历史（8 周），用于个性化 Pace 预测。"
            }
            LocaleKey::ProviderOpenAiCookies => "OpenAI Cookie",
            LocaleKey::ProviderHistoricalTracking => "历史追踪",
            LocaleKey::ProviderOpenAiWebExtras => "OpenAI 网页扩展",
            LocaleKey::ProviderOpenAiWebExtrasHelp => {
                "通过 chatgpt.com 显示用量明细、额度历史和代码审查信息。"
            }
            LocaleKey::ProviderCodexCreditsUnavailable => {
                "额度暂不可用；保持 Codex 运行后会自动刷新。"
            }
            LocaleKey::ProviderCodexLastFetchFailedTitle => "上次 Codex 获取失败：",
            LocaleKey::ProviderCodexNotRunningHelp => "Codex 未运行。先运行一次 Codex 命令再试。",
            LocaleKey::ProviderCookieSource => "Cookie 来源",
            LocaleKey::CookieSourceManual => "手动",
            LocaleKey::ProviderRegion => "地区",
            LocaleKey::ProviderClaudeCookies => "Claude Cookie",
            LocaleKey::ProviderClaudeCookiesHelp => {
                "优先使用浏览器 Cookie/sessionKey，因为它与 Claude 设置页的用量一致。"
            }
            LocaleKey::ProviderClaudeAvoidKeychainPrompts => "避免钥匙串提示",
            LocaleKey::ProviderClaudeAvoidKeychainPromptsHelp => {
                "使用 /usr/bin/security 读取 Claude 凭据，避免 CodexBar 的钥匙串提示。"
            }
            LocaleKey::ProviderCursorCookieSourceHelp => "自动导入浏览器 Cookie 或已保存会话。",
            LocaleKey::ProviderCursorCreditsHelp => "包含计划额度之外的按量计费用量。",
            LocaleKey::AutoFallbackHelp => "当首选来源失败时自动回退到下一个来源。",
            LocaleKey::ProviderSourceOauthWeb => "OAuth + 网页",
            LocaleKey::Automatic => "自动",
            LocaleKey::Average => "平均",
            LocaleKey::ExtraUsage => "额外用量",
            LocaleKey::OAuth => "OAuth",
            LocaleKey::Api => "API",
            LocaleKey::Web => "网页",

            // General tab sections
            LocaleKey::PrivacyTitle => "隐私",
            LocaleKey::HidePersonalInfo => "隐藏个人信息",
            LocaleKey::HidePersonalInfoHelper => "遮蔽邮箱和账号名称（适合直播时使用）",
            LocaleKey::UpdatesTitle => "更新",
            LocaleKey::UpdateChannelChoice => "更新通道",
            LocaleKey::UpdateChannelChoiceHelper => "在稳定版与测试预览版之间选择",
            LocaleKey::AutoDownloadUpdates => "自动检查更新",
            LocaleKey::AutoDownloadUpdatesHelper => "发现新版本后在后台下载安装器更新",
            LocaleKey::InstallUpdatesOnQuit => "退出时安装更新",
            LocaleKey::InstallUpdatesOnQuitHelper => "退出 CodexBar 时自动启动已准备好的安装器",

            // Keyboard shortcuts
            LocaleKey::KeyboardShortcutsTitle => "快捷键",
            LocaleKey::GlobalShortcutLabel => "全局快捷键",
            LocaleKey::GlobalShortcutHelper => "按此快捷键可从任何位置打开 CodexBar",
            LocaleKey::ShortcutFormatHint => {
                "格式：Ctrl+Shift+Key、Alt+Ctrl+Key 等。需重启以应用更改。"
            }
            LocaleKey::Saved => "已保存（需重启以应用）",
            LocaleKey::InvalidFormat => "无效的快捷键格式",
            LocaleKey::ShortcutHintPlaceholder => "例如：Ctrl+Shift+U",

            // Display/Preferences helpers
            LocaleKey::SurpriseAnimationsHelper => "在托盘图标中偶尔显示趣味动画",
            LocaleKey::SelectProvider => "请选择服务商",

            // Refresh interval labels
            LocaleKey::RefreshInterval30Sec => "30 秒",
            LocaleKey::RefreshInterval1Min => "1 分钟",
            LocaleKey::RefreshInterval5Min => "5 分钟",
            LocaleKey::RefreshInterval10Min => "10 分钟",

            // Cookies tab
            LocaleKey::BrowserCookiesTitle => "浏览器 Cookie",
            LocaleKey::CookieImport => "Cookie 导入",
            LocaleKey::Provider => "服务商",
            LocaleKey::SelectPlaceholder => "请选择...",
            LocaleKey::AutoRefreshInterval => "自动刷新间隔",

            // About tab
            LocaleKey::AboutDescription => "CodexBar 的 Windows 移植版本。",
            LocaleKey::AboutDescriptionLine2 => "在系统托盘中追踪 AI 服务商用量。",
            LocaleKey::ViewOnGitHub => "→ 查看 GitHub",
            LocaleKey::SubmitIssue => "→ 提交问题",
            LocaleKey::MaintainedBy => "由 CodexBar 贡献者维护",
            LocaleKey::CommitLabel => "提交",
            LocaleKey::BuildDateLabel => "构建",

            // Shared form controls
            LocaleKey::Save => "保存",
            LocaleKey::Cancel => "取消",
            LocaleKey::Label => "标签",
            LocaleKey::Token => "令牌",
            LocaleKey::AddAccount => "添加账号",
            LocaleKey::AccountAdded => "账号已添加",
            LocaleKey::AccountRemoved => "账号已移除",
            LocaleKey::AccountSwitched => "账号已切换",
            LocaleKey::AccountLabelHint => "例如：工作账号、个人账号...",
            LocaleKey::EnterApiKeyFor => "为 {} 输入 API Key",
            LocaleKey::PasteApiKeyHere => "在这里粘贴 API key...",
            LocaleKey::ApiKeySaved => "已保存 {} 的 API key",
            LocaleKey::ApiKeyRemoved => "已移除 {} 的 API key",
            LocaleKey::EnvironmentVariable => "环境变量",
            LocaleKey::CookieSavedForProvider => "已保存 {} 的 Cookie",
            LocaleKey::CookieRemovedForProvider => "已移除 {} 的 Cookie",

            // Usage helper functions
            LocaleKey::ShowUsedPercent => "已使用 {:.0}%",
            LocaleKey::ShowRemainingPercent => "剩余 {:.0}%",

            // Tauri desktop shell — Settings section headings
            LocaleKey::TabTokenAccounts => "令牌",
            LocaleKey::SectionRefresh => "自动化",
            LocaleKey::SectionNotifications => "通知",
            LocaleKey::SectionUsageThresholds => "用量阈值",
            LocaleKey::SectionKeyboard => "键盘",
            LocaleKey::SectionUsageRendering => "用量展示",
            LocaleKey::SectionTime => "时间",
            LocaleKey::SectionLanguage => "语言",
            LocaleKey::SectionCredentialsSecurity => "凭据与安全",
            LocaleKey::SectionDebug => "调试",
            LocaleKey::SectionApiKeys => "API 密钥",
            LocaleKey::SectionSavedCookies => "已保存的 Cookies",
            LocaleKey::SectionImportFromBrowser => "从浏览器导入",
            LocaleKey::SectionAddCookieManually => "手动添加 Cookie",
            LocaleKey::SectionTokenAccounts => "令牌账户",
            LocaleKey::SectionSavedAccounts => "已保存账户",
            LocaleKey::SectionAddAccount => "添加账户",

            // Tauri desktop shell — General tab fields
            LocaleKey::RefreshIntervalLabel => "刷新间隔",
            LocaleKey::RefreshIntervalHelper => "两次自动刷新之间的秒数（0 = 手动）。",
            LocaleKey::SoundVolumeHelper => "阈值告警音量（0–100）。",
            LocaleKey::HighUsageWarningHelper => "当用量超过该百分比时显示预警。",
            LocaleKey::CriticalUsageWarningHelper => "当用量超过该百分比时显示严重告警。",
            LocaleKey::GlobalShortcutFieldLabel => "全局快捷键",
            LocaleKey::GlobalShortcutToggleHelper => "用于切换托盘面板的组合键。",
            // REVIEW-i18n: Phase 7 shortcut-capture + notification test labels.
            LocaleKey::ShortcutRecordButton => "录制",
            LocaleKey::ShortcutRecordingLabel => "录制中…",
            LocaleKey::ShortcutRecordingHint => "按下修饰键 + 任意键。Esc 取消，Backspace 清除。",
            LocaleKey::ShortcutClearButton => "清除",
            LocaleKey::ShortcutEmptyPlaceholder => "未设置",
            LocaleKey::NotificationTestSound => "测试声音",
            LocaleKey::NotificationTestSoundPlaying => "播放中…",

            // Tauri desktop shell — Display tab fields
            LocaleKey::TrayIconModeLabel => "托盘图标模式",
            LocaleKey::TrayIconModeHelper => "使用单一合并图标，或为每个已启用服务商显示独立图标。",
            LocaleKey::TrayIconModeSingle => "合并",
            LocaleKey::TrayIconModePerProvider => "按服务商",
            LocaleKey::ShowProviderIcons => "显示服务商图标",
            LocaleKey::ShowProviderIconsHelper => "在托盘切换器中显示服务商图标。",
            LocaleKey::PreferHighestUsage => "优先显示最高用量",
            LocaleKey::PreferHighestUsageHelper => "在合并托盘显示中优先展示最接近限额的服务商。",
            LocaleKey::ShowPercentInTray => "在托盘中显示百分比",
            LocaleKey::ShowPercentInTrayHelper => "使用服务商标识与百分比文字替代用量条。",
            LocaleKey::DisplayModeLabel => "显示模式",
            LocaleKey::DisplayModeHelper => "菜单栏标签显示的详细程度。",
            LocaleKey::DisplayModeDetailed => "详细",
            LocaleKey::DisplayModeCompact => "紧凑",
            LocaleKey::DisplayModeMinimal => "最简",
            LocaleKey::ShowAsUsedLabel => "显示为已用",
            LocaleKey::ShowAsUsedHelper => "以已使用百分比而非剩余显示用量条。",
            LocaleKey::ShowAllTokenAccountsLabel => "显示所有令牌账户",
            LocaleKey::ShowAllTokenAccountsHelper => {
                "在服务商菜单中列出所有令牌账户，而不是折叠显示。"
            }
            LocaleKey::EnableAnimationsLabel => "启用动画",
            LocaleKey::EnableAnimationsHelper => "平滑过渡与动画进度条。",
            LocaleKey::SurpriseAnimationsLabel => "惊喜动画",

            // Tauri desktop shell — Advanced tab fields
            LocaleKey::UpdateChannelStableOption => "稳定版",
            LocaleKey::UpdateChannelBetaOption => "测试预览版",
            LocaleKey::AvoidKeychainPromptsLabel => "避免钥匙串弹窗（Claude）",
            LocaleKey::AvoidKeychainPromptsHelper => {
                "跳过 Claude 的钥匙串凭据读取，避免系统权限弹窗。"
            }
            LocaleKey::DisableAllKeychainLabel => "禁用所有钥匙串访问",
            LocaleKey::DisableAllKeychainHelper => {
                "关闭所有服务商的凭据/钥匙串读取。同时启用上方的 Claude 选项。"
            }
            LocaleKey::ShowDebugSettingsLabel => "显示调试设置",
            LocaleKey::ShowDebugSettingsHelper => "在界面中显示故障排查和开发者相关选项。",
            LocaleKey::LanguageEnglishOption => "English",
            LocaleKey::LanguageChineseOption => "中文",
            LocaleKey::LanguageJapaneseOption => "日本語",

            // Tauri desktop shell — Theme (Phase 12)
            // REVIEW-i18n: ZH translations for Phase 12 theme labels.
            LocaleKey::SectionTheme => "外观",
            LocaleKey::ThemeLabel => "主题",
            LocaleKey::ThemeHelper => "自动跟随系统配色方案；浅色/深色可手动覆盖。",
            LocaleKey::ThemeAutoOption => "自动（跟随系统）",
            LocaleKey::ThemeLightOption => "浅色",
            LocaleKey::ThemeDarkOption => "深色",

            // Tauri desktop shell — settings status / common
            LocaleKey::SettingsStatusSaving => "保存中…",
            LocaleKey::ApiKeysTabHint => {
                "为使用令牌认证的服务商配置 API 密钥。密钥仅存储在本地，不会上传。"
            }

            // Tauri desktop shell — tray / popout
            LocaleKey::FetchingProviderData => "正在获取服务商数据…",
            LocaleKey::NoProvidersConfigured => "尚未配置任何服务商。",
            LocaleKey::EnableProvidersHint => "请在设置中启用服务商以查看用量数据。",
            LocaleKey::OpenSettingsButton => "打开设置",
            LocaleKey::TooltipRefresh => "刷新",
            LocaleKey::TooltipSettings => "设置",
            LocaleKey::TooltipPopOut => "弹出",
            LocaleKey::TooltipBackToTray => "返回托盘",
            LocaleKey::TrayCardErrorBadge => "错误",
            LocaleKey::SummaryProvidersLabel => "服务商",
            LocaleKey::SummaryRefreshing => "正在刷新…",
            LocaleKey::SummaryFailed => "失败",
            LocaleKey::SummaryWithErrors => "存在错误",

            // Tauri desktop shell — provider detail
            LocaleKey::DetailBackButton => "返回",
            LocaleKey::DetailWindowPrimary => "主要",
            LocaleKey::DetailWindowSecondary => "次要",
            LocaleKey::DetailWindowModelSpecific => "模型专属",
            LocaleKey::DetailWindowTertiary => "第三",
            LocaleKey::DetailWindowMinutesSuffix => "分钟窗口",
            LocaleKey::DetailWindowExhausted => "已用尽",
            LocaleKey::DetailPaceTitle => "进度",
            LocaleKey::DetailPaceOnTrack => "正常",
            LocaleKey::DetailPaceSlightlyAhead => "略超前",
            LocaleKey::DetailPaceAhead => "超前",
            LocaleKey::DetailPaceFarAhead => "远超前",
            LocaleKey::DetailPaceSlightlyBehind => "略落后",
            LocaleKey::DetailPaceBehind => "落后",
            LocaleKey::DetailPaceFarBehind => "远落后",
            LocaleKey::DetailPaceRunsOutIn => "预计耗尽时间",
            LocaleKey::DetailPaceWillLastToReset => "足以支撑到重置",
            LocaleKey::DetailCostTitle => "费用",
            LocaleKey::DetailCostUsed => "已用",
            LocaleKey::DetailCostLimit => "限额",
            LocaleKey::DetailCostRemaining => "剩余",
            LocaleKey::DetailCostResets => "重置",
            LocaleKey::DetailChartCost => "费用（30 天）",
            LocaleKey::DetailChartCredits => "已用额度（30 天）",
            LocaleKey::DetailChartUsageBreakdown => "按服务划分的用量（30 天）",
            // REVIEW-i18n
            LocaleKey::DetailChartEmpty => "暂无图表数据。",
            LocaleKey::DetailUpdatedPrefix => "更新于",

            // Tauri desktop shell — update banner
            LocaleKey::BannerCheckingForUpdates => "正在检查更新…",
            LocaleKey::BannerUpdateAvailablePrefix => "更新",
            LocaleKey::BannerDownloadButton => "下载",
            LocaleKey::BannerViewRelease => "查看发布",
            LocaleKey::BannerDismiss => "忽略",
            LocaleKey::BannerDownloadingPrefix => "正在下载更新",
            LocaleKey::BannerReadyToInstallSuffix => "已准备好安装",
            LocaleKey::BannerInstallRestart => "安装并重启",
            LocaleKey::BannerUpdateFailedPrefix => "更新失败",
            LocaleKey::BannerRetry => "重试",

            // Tauri desktop shell — providers sidebar (Phase 6a)
            LocaleKey::ProviderSidebarSearch => "搜索",
            LocaleKey::ProviderSidebarClearSearch => "清除提供商搜索",
            LocaleKey::ProviderSidebarNoMatches => "没有匹配的提供商",
            LocaleKey::ProviderSidebarReorderHint => "拖动以重新排序",
            LocaleKey::ProviderSidebarMoveUp => "上移",
            LocaleKey::ProviderSidebarMoveDown => "下移",
            LocaleKey::ProviderStatusOk => "已更新",
            LocaleKey::ProviderStatusStale => "已过期",
            LocaleKey::ProviderStatusError => "错误",
            LocaleKey::ProviderStatusLoading => "加载中",
            LocaleKey::ProviderStatusDisabled => "已禁用",
            LocaleKey::ProviderDetailPlaceholder => "详细面板将在 6b 阶段推出",

            // Phase 6d — credential detection
            LocaleKey::CredentialsSectionTitle => "凭据",
            LocaleKey::CredsStatusAuthenticated => "已认证",
            LocaleKey::CredsStatusNotSignedIn => "未登录",
            LocaleKey::CredsStatusDetected => "已检测到",
            LocaleKey::CredsStatusNotDetected => "未检测到",
            LocaleKey::CredsStatusAvailable => "可用",
            LocaleKey::CredsStatusUnavailable => "不可用",
            LocaleKey::CredsOpenFolderAction => "打开凭据文件夹",
            LocaleKey::CredsRefreshDetectionAction => "刷新检测",
            LocaleKey::CredsSavePathAction => "保存路径",
            LocaleKey::CredsBrowseAction => "浏览…",
            LocaleKey::CredsGeminiCliLabel => "Gemini CLI",
            LocaleKey::CredsGeminiCliHelperPrefix => "使用的 OAuth 凭据来自",
            LocaleKey::CredsGeminiCliSetupAction => "安装 Gemini CLI",
            LocaleKey::CredsGeminiCliSetupHelp => {
                "安装 Gemini CLI 并运行 `gemini auth login` 进行登录。"
            }
            LocaleKey::CredsVertexAiLabel => "Google Cloud",
            LocaleKey::CredsVertexAiHelperPrefix => "使用的 Google Cloud 凭据来自",
            LocaleKey::CredsVertexAiSetupAction => "配置 Google Cloud 身份",
            LocaleKey::CredsVertexAiSetupHelp => {
                "运行 `gcloud auth application-default login` 创建凭据。"
            }
            LocaleKey::CredsJetBrainsLabel => "JetBrains IDE",
            LocaleKey::CredsJetBrainsHelperDetectedPrefix => "使用检测到的 IDE 配置：",
            LocaleKey::CredsJetBrainsHelperCustomPrefix => "使用自定义 IDE 基础路径：",
            LocaleKey::CredsJetBrainsHelperMissing => {
                "请安装启用了 AI Assistant 的 JetBrains IDE，然后刷新 CodexBar。"
            }
            LocaleKey::CredsJetBrainsCustomPathLabel => "自定义路径",
            LocaleKey::CredsJetBrainsCustomPathPlaceholder => "%APPDATA%/JetBrains/IntelliJIdea...",
            LocaleKey::CredsJetBrainsSelectLabel => "选择要监控的 JetBrains IDE。",
            LocaleKey::CredsJetBrainsAutoDetectOption => "自动检测",
            LocaleKey::CredsKiroLabel => "Kiro CLI",
            LocaleKey::CredsKiroHelperAvailablePrefix => "检测到于",
            LocaleKey::CredsKiroHelperMissing => "kiro-cli：未在 PATH 或常见安装位置找到。",
            LocaleKey::CredsOpenAiHistoryHelp => "启用历史跟踪以查看一段时间内的使用情况。",

            // Tauri desktop shell — Token accounts (Phase 6e, review)
            LocaleKey::TokenAccountActive => "活动",
            LocaleKey::TokenAccountSetActive => "设为活动",
            LocaleKey::TokenAccountRemove => "移除",
            LocaleKey::TokenAccountAddButton => "添加账户",
            LocaleKey::TokenAccountGithubLoginButton => "使用 GitHub 登录",
            LocaleKey::TokenAccountEmpty => "该服务商尚未保存任何账户。",
            LocaleKey::TokenAccountLabelPlaceholder => "标签（如工作、个人）…",
            LocaleKey::TokenAccountProviderLabel => "服务商",
            LocaleKey::TokenAccountProviderPlaceholder => "选择服务商…",
            LocaleKey::TokenAccountAddedPrefix => "添加于",
            LocaleKey::TokenAccountUsedPrefix => "上次使用",
            LocaleKey::TokenAccountTabHint => {
                "按服务商管理多个会话令牌或 API 令牌。所有数据拉取都会使用活动账户。仅需要手动令牌的服务商会显示在此处。"
            }
            LocaleKey::TokenAccountNoSupported => "当前没有支持令牌账户的服务商。",
            LocaleKey::TokenAccountInlineSummary => "令牌账户",

            // Phase 9 - Tray / pop-out pace badges + countdowns
            // REVIEW-i18n: short badge labels for usage pace categories
            LocaleKey::TrayPaceBadgeSlow => "缓慢",
            LocaleKey::TrayPaceBadgeSteady => "稳定",
            LocaleKey::TrayPaceBadgeRacing => "加速",
            LocaleKey::TrayPaceBadgeBurning => "超速",
            LocaleKey::TrayResetsInLabel => "{} 后重置",
            LocaleKey::TrayResetsDueNow => "正在重置…",
        }
    }
}
