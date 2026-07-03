use super::*;

impl LocaleKey {
    pub(super) fn korean(self) -> &'static str {
        match self {
            // Tab names
            LocaleKey::TabGeneral => "일반",
            LocaleKey::TabProviders => "제공업체",
            LocaleKey::TabDisplay => "디스플레이",
            LocaleKey::TabApiKeys => "API 키",
            LocaleKey::TabCookies => "쿠키",
            LocaleKey::TabAdvanced => "고급",
            LocaleKey::TabAbout => "정보",
            LocaleKey::TabShortcuts => "단축키",

            // General settings
            LocaleKey::InterfaceLanguage => "인터페이스 언어",
            LocaleKey::StartupSettings => "시스템",
            LocaleKey::StartAtLogin => "로그인 시 자동 실행",
            LocaleKey::StartMinimized => "최소화 상태로 시작",
            LocaleKey::StartAtLoginHelper => "시스템 시작 시 자동으로 앱을 실행합니다",
            LocaleKey::StartMinimizedHelper => "시스템 트레이로 최소화된 상태로 시작합니다",

            // Notification settings
            LocaleKey::ShowNotifications => "알림 표시",
            LocaleKey::ShowNotificationsHelper => "사용량 임계값에 도달하면 알림",
            LocaleKey::SoundEnabled => "소리 알림",
            LocaleKey::SoundEnabledHelper => "임계값에 도달하면 소리 재생",
            LocaleKey::SoundVolume => "알림 볼륨",
            LocaleKey::HighUsageThreshold => "높은 사용량 임계값",
            LocaleKey::HighUsageThresholdHelper => "이 사용량 수준에서 경고 표시",
            LocaleKey::HighUsageAlert => "높은 사용량 경고",
            LocaleKey::CriticalUsageThreshold => "위험 사용량 임계값",
            LocaleKey::CriticalUsageThresholdHelper => "이 수준에서 위험 경고 표시",
            LocaleKey::CriticalUsageAlert => "위험 경고",

            // Display settings
            LocaleKey::UsageDisplay => "사용량 표시",
            LocaleKey::ShowUsageAsUsed => "사용된 양으로 표시",
            LocaleKey::ShowUsageAsUsedHelper => "남은 양 대신 사용된 백분율로 표시",
            LocaleKey::ResetTimeRelative => "상대적인 초기화 시간",
            LocaleKey::ResetTimeRelativeHelper => "\"3:00 PM\" 대신 \"2시간 30분\"으로 표시",
            LocaleKey::TrayIcon => "트레이 아이콘",
            LocaleKey::MergeTrayIcons => "트레이 아이콘 병합",
            LocaleKey::MergeTrayIconsHelper => "모든 제공업체를 단일 트레이 아이콘에 표시",
            LocaleKey::PerProviderTrayIcons => "제공업체별 아이콘",
            LocaleKey::PerProviderTrayIconsHelper => {
                "활성화된 각 제공업체에 대해 별도의 트레이 아이콘 표시"
            }

            // Provider settings
            LocaleKey::ProviderEnabled => "활성화됨",
            LocaleKey::ProviderDisabled => "비활성화됨",
            LocaleKey::ProviderInfo => "정보",
            LocaleKey::ProviderUsage => "사용량",
            LocaleKey::AuthType => "인증",
            LocaleKey::DataSource => "데이터 소스",
            LocaleKey::ProviderNotDetected => "감지되지 않음",
            LocaleKey::ProviderLastFetchFailed => "최근 가져오기 실패",
            LocaleKey::ProviderUsageNotFetchedYet => "아직 사용량을 가져오지 않음",
            LocaleKey::ProviderNotFetchedYetTitle => "아직 가져오지 않음",
            LocaleKey::ProviderDisabledNoRecentData => "비활성화됨 — 최근 데이터 없음",
            LocaleKey::ProviderSourceAutoShort => "자동",
            LocaleKey::ProviderSourceWebShort => "웹",
            LocaleKey::ProviderSourceCliShort => "CLI",
            LocaleKey::ProviderSourceOauthShort => "OAuth",
            LocaleKey::ProviderSourceApiShort => "API",
            LocaleKey::ProviderSourceGithubApiShort => "GitHub API",
            LocaleKey::ProviderSourceLocalShort => "로컬",
            LocaleKey::ProviderSourceKiroEnvShort => "Kiro Env",
            LocaleKey::TrackingItem => "추적 대상",
            LocaleKey::MainWindowLiveUsageData => "기본 창의 실시간 사용량 데이터",
            LocaleKey::StartTrackingUsage => "사용량 추적을 시작하려면 활성화",
            LocaleKey::ClickTrayIconForMetrics => {
                "실시간 메트릭을 보려면 트레이 아이콘을 클릭하세요"
            }

            // Browser cookie import
            LocaleKey::BrowserCookieImport => "브라우저 쿠키 가져오기",
            LocaleKey::ImportFromBrowser => "브라우저에서 {} 쿠키 가져오기",
            LocaleKey::NoCookiesFoundInBrowser => {
                "{}에서 쿠키를 찾을 수 없습니다. 먼저 로그인해 주세요."
            }
            LocaleKey::SelectBrowser => "브라우저 선택...",
            LocaleKey::ImportCookies => "쿠키 가져오기",
            LocaleKey::ImportSuccess => "{} 쿠키를 가져왔습니다",
            LocaleKey::ImportFailed => "가져오기 실패: {}",
            LocaleKey::SaveFailed => "저장 실패: {}",
            LocaleKey::CookiesAutoImport => {
                "Chrome, Edge, Brave, Firefox에서 쿠키를 자동으로 가져옵니다"
            }
            LocaleKey::QuickActions => "빠른 작업",
            LocaleKey::OpenProviderDashboard => "{} 대시보드 열기",
            LocaleKey::OllamaNoDashboard => "Ollama는 로컬에서 실행되며 대시보드가 없습니다",

            // API Keys tab
            LocaleKey::ApiKeysTitle => "API 키",
            LocaleKey::ApiKeysDescription => "인증이 필요한 제공업체의 액세스 토큰을 구성합니다.",
            LocaleKey::AddKey => "+ 키 추가",
            LocaleKey::KeySet => "설정됨",
            LocaleKey::KeyRequired => "키 필요",
            LocaleKey::Remove => "제거",
            LocaleKey::GetKey => "키 받기 →",

            // Cookies tab
            LocaleKey::SavedCookies => "저장된 쿠키",
            LocaleKey::AddManualCookie => "수동 쿠키 추가",
            LocaleKey::CookieHeader => "쿠키 헤더",
            LocaleKey::PasteHere => "여기에 붙여넣기...",
            LocaleKey::DeleteCookie => "삭제",
            LocaleKey::CookieSaved => "{}개 쿠키 저장됨",
            LocaleKey::CookieDeleted => "{} 쿠키가 삭제되었습니다",

            // Advanced tab
            LocaleKey::RefreshSettings => "새로고침",
            LocaleKey::Animations => "애니메이션",
            LocaleKey::MenuBar => "메뉴 바",
            LocaleKey::Fun => "재미",
            LocaleKey::GlobalShortcut => "글로벌 단축키",
            LocaleKey::Privacy => "개인정보 보호",
            LocaleKey::Updates => "업데이트",
            LocaleKey::UpdateChannel => "업데이트 채널",
            LocaleKey::UpdateChannelStable => "안정(Stable)",
            LocaleKey::UpdateChannelBeta => "베타(Beta)",
            LocaleKey::Never => "안 함",
            LocaleKey::LastUpdated => "최근 업데이트",
            LocaleKey::MinutesAgo => "{}분 전",
            LocaleKey::HoursAgo => "{}시간 전",
            LocaleKey::DaysAgo => "{}일 전",
            LocaleKey::BuiltWithRust => "Rust + egui로 구축됨",
            LocaleKey::OriginalMacOSVersion => "원본 macOS 버전",
            LocaleKey::Links => "링크",
            LocaleKey::BuildInfo => "빌드 정보",
            LocaleKey::EnabledProviders => "활성화된 제공업체",
            LocaleKey::Appearance => "테마",
            LocaleKey::ThemeSelection => "테마",
            LocaleKey::LightMode => "라이트",
            LocaleKey::DarkMode => "다크",

            // About
            LocaleKey::AboutTitle => "CodexBar 정보",
            LocaleKey::Version => "버전",

            // Main popup - Header actions
            LocaleKey::ActionRefreshAll => "모두 새로고침",
            LocaleKey::ActionSettings => "설정",
            LocaleKey::ActionClose => "✕",

            // Main popup - Provider section
            LocaleKey::ProviderAccount => "계정",
            LocaleKey::ProviderSession => "세션",
            LocaleKey::ProviderWeekly => "주간",
            LocaleKey::ProviderMonthly => "30일",
            LocaleKey::ProviderModel => "모델",
            LocaleKey::ProviderPlan => "요금제",
            LocaleKey::ProviderNextReset => "다음 초기화",
            LocaleKey::ProviderNoRecentUsage => "최근 사용량 없음",
            LocaleKey::ProviderNotSignedIn => "로그인하지 않음",
            LocaleKey::SummaryTab => "요약",

            // Main popup - Loading/Empty/Error states
            LocaleKey::StateLoadingProviders => "제공업체 로딩 중...",
            LocaleKey::StateNoProviderData => "제공업체 데이터가 없습니다.",
            LocaleKey::StateNoProviderSelected => "선택된 제공업체가 없습니다.",
            LocaleKey::StateSummaryRefreshPending => "모든 제공업체 새로고침 완료 후 업데이트 중",
            LocaleKey::StateError => "오류",
            LocaleKey::StateRetry => "재시도",
            LocaleKey::StateDownload => "다운로드",
            LocaleKey::StateRestartAndUpdate => "재시작 및 업데이트",

            // Main popup - Credits
            LocaleKey::CreditsTitle => "크레딧",

            // Main popup - Update banner (non-happy-path)
            LocaleKey::UpdateRestartAndUpdate => "재시작 및 업데이트",
            LocaleKey::UpdateRetry => "재시도",
            LocaleKey::UpdateDownload => "다운로드",
            LocaleKey::UpdateDownloading => "다운로드 중",
            LocaleKey::UpdateReady => "설치 준비 완료",
            LocaleKey::UpdateFailed => "업데이트 실패",

            // Main popup - Settings button
            LocaleKey::ButtonOpenProviderSettings => "제공업체 설정 열기",

            // Main popup - Bottom menu (Actions)
            LocaleKey::MenuSettings => "설정...",
            LocaleKey::MenuAbout => "CodexBar 정보",
            LocaleKey::MenuQuit => "종료",

            // Main popup - Status strings
            LocaleKey::StatusJustUpdated => "방금 업데이트됨",
            LocaleKey::StatusUnableToGetUsage => "사용량을 가져올 수 없음",

            // Main popup - Provider detail actions
            LocaleKey::ActionRefresh => "새로고침",
            LocaleKey::ActionSwitchAccount => "계정 전환...",
            LocaleKey::ActionUsageDashboard => "사용량 대시보드",
            LocaleKey::ActionStatusPage => "상태 페이지",
            LocaleKey::ActionCopyError => "오류 복사",
            LocaleKey::ActionBuyCredits => "크레딧 구매...",

            // Main popup - Pace status
            LocaleKey::PaceOnTrack => "정상 진행",
            LocaleKey::PaceBehind => "지연됨",

            // Main popup - Reset prefix
            LocaleKey::MetricResetsIn => "초기화까지",

            // Main popup - Section titles
            LocaleKey::SectionUsageBreakdown => "사용량 내역",
            LocaleKey::SectionCost => "비용",

            // Tray - Single icon mode
            LocaleKey::TrayOpenCodexBar => "대시보드 띄우기",
            LocaleKey::TrayPopOutDashboard => "대시보드 띄우기",
            LocaleKey::TrayRefreshAll => "모두 새로고침",
            LocaleKey::TrayProviders => "제공업체",
            LocaleKey::TraySettings => "설정...",
            LocaleKey::TrayCheckForUpdates => "업데이트 확인",
            LocaleKey::TrayQuit => "종료",
            LocaleKey::TrayLoading => "CodexBar - 로딩 중...",
            LocaleKey::TrayNoProviders => "CodexBar - 사용 가능한 제공업체 없음",
            LocaleKey::TraySessionPercent => "세션 {}%",
            LocaleKey::TrayWeeklyPercent => "주간 {}%",
            LocaleKey::TrayStatusError => " (오류)",
            LocaleKey::TrayStatusStale => " (이전 데이터)",
            LocaleKey::TrayStatusIncident => " (장애)",
            LocaleKey::TrayStatusPartial => " (부분 장애)",
            LocaleKey::TrayWeeklyExhausted => "주간 할당량 모두 사용함",
            LocaleKey::TrayCreditsRemaining => "남은 크레딧 {}%",
            LocaleKey::TrayStatusRowLoading => "로딩 중...",
            LocaleKey::TrayStatusRowError => "오류",
            LocaleKey::TrayCreditsRow => "크레딧 {}%",

            // Main popup - Usage/reset labels
            LocaleKey::ResetInProgress => "초기화 중...",
            LocaleKey::TomorrowAt => "내일 {}",
            LocaleKey::UsedPercent => "{:.0}% 사용됨",
            LocaleKey::RemainingPercent => "{:.0}% 남음",
            LocaleKey::RemainingAmount => "{:.2} 남음",
            LocaleKey::Tokens1K => "1K 토큰",
            LocaleKey::TodayCost => "오늘: ${:.2}",
            LocaleKey::Last30DaysCost => "최근 30일: ${:.2}",
            LocaleKey::StatusLabel => "상태: {}",

            // Main popup - Update banner messages
            LocaleKey::UpdateAvailableMessage => "사용 가능한 업데이트: {}",
            LocaleKey::UpdateReadyMessage => "{} 설치 준비 완료",
            LocaleKey::UpdateFailedMessage => "업데이트 실패: {}",
            LocaleKey::UpdateDownloadingMessage => "{} 다운로드 중 ({:.0}%)",

            // Tray - Per-provider mode
            LocaleKey::TrayProviderPopOut => "대시보드 띄우기",
            LocaleKey::TrayProviderRefresh => "새로고침",
            LocaleKey::TrayProviderSettings => "설정...",
            LocaleKey::TrayProviderQuit => "종료",

            // Provider settings - Live renderer specific
            LocaleKey::State => "상태",
            LocaleKey::Source => "소스",
            LocaleKey::Updated => "업데이트됨",
            LocaleKey::NeverUpdated => "업데이트된 적 없음",
            LocaleKey::UpdatedJustNow => "방금 업데이트됨",
            LocaleKey::UpdatedMinutesAgo => "{}분 전",
            LocaleKey::UpdatedHoursAgo => "{}시간 전",
            LocaleKey::UpdatedDaysAgo => "{}일 전",
            LocaleKey::Status => "상태",
            LocaleKey::AllSystemsOperational => "모든 시스템 정상 작동 중",
            LocaleKey::Plan => "요금제",
            LocaleKey::Account => "계정",

            // Provider detail - Usage section
            LocaleKey::ProviderSessionLabel => "세션",
            LocaleKey::ProviderWeeklyLabel => "주간",
            LocaleKey::ProviderCodeReviewLabel => "코드 리뷰",
            LocaleKey::ResetsInShort => "초기화까지",
            LocaleKey::ResetsInDaysHours => "초기화까지 {}일 {}시간",
            LocaleKey::ResetsInHoursMinutes => "초기화까지 {}시간 {}분",

            // Provider detail - Tray Display
            LocaleKey::TrayDisplayTitle => "트레이 표시",
            LocaleKey::ShowInTray => "트레이에 표시",

            // Provider detail - Credits
            LocaleKey::CreditsLabel => "크레딧",
            LocaleKey::CreditsLeft => "{:.1} 남음",

            // Provider detail - Cost
            LocaleKey::CostTitle => "비용",
            LocaleKey::TodayCostFull => "오늘: ${:.2} • {} 토큰",
            LocaleKey::Last30DaysCostFull => "최근 30일: ${:.2} • {} 토큰",

            // Provider detail - Settings section
            LocaleKey::ProviderSettingsTitle => "설정",
            LocaleKey::ProviderAccountsTitle => "계정",
            LocaleKey::ProviderOptionsTitle => "옵션",
            LocaleKey::MenuBarMetric => "메뉴 바 메트릭",
            LocaleKey::MenuBarMetricHelper => "메뉴 바 백분율을 결정할 창을 선택합니다.",
            LocaleKey::UsageSource => "사용량 소스",
            LocaleKey::ProviderNoCodexAccountsDetected => "감지된 Codex 계정이 아직 없습니다.",
            LocaleKey::ProviderCodexAutoImportHelp => {
                "대시보드 추가 기능을 위해 브라우저 쿠키를 자동으로 가져옵니다."
            }
            LocaleKey::ProviderCodexHistoryHelp => {
                "사용 속도 예측을 개인화하기 위해 로컬 Codex 사용 기록(8주)을 저장합니다."
            }
            LocaleKey::ProviderOpenAiCookies => "OpenAI 쿠키",
            LocaleKey::ProviderHistoricalTracking => "기록 추적",
            LocaleKey::ProviderOpenAiWebExtras => "OpenAI 웹 추가 기능",
            LocaleKey::ProviderOpenAiWebExtrasHelp => {
                "chatgpt.com을 통해 상세 사용량, 크레딧 내역 및 코드 리뷰를 보여줍니다."
            }
            LocaleKey::ProviderCodexCreditsUnavailable => {
                "크레딧을 사용할 수 없습니다. 새로 고치려면 Codex를 계속 실행해 두세요."
            }
            LocaleKey::ProviderCodexLastFetchFailedTitle => "최근 Codex 가져오기 실패:",
            LocaleKey::ProviderCodexNotRunningHelp => {
                "Codex가 실행 중이 아닙니다. 먼저 Codex 명령을 실행해 보세요."
            }
            LocaleKey::ProviderCookieSource => "쿠키 소스",
            LocaleKey::CookieSourceManual => "수동",
            LocaleKey::ProviderRegion => "지역",
            LocaleKey::ProviderClaudeCookies => "Claude 쿠키",
            LocaleKey::ProviderClaudeCookiesHelp => {
                "브라우저 쿠키/`sessionKey`가 Claude의 설정 사용량 페이지와 일치하므로 권장됩니다."
            }
            LocaleKey::ProviderClaudeAvoidKeychainPrompts => "키체인 메시지 표시 방지",
            LocaleKey::ProviderClaudeAvoidKeychainPromptsHelp => {
                "Claude 자격 증명을 읽을 때 `/usr/bin/security`를 사용하여 CodexBar 키체인 권한 창이 뜨지 않도록 합니다."
            }
            LocaleKey::ProviderCursorCookieSourceHelp => {
                "브라우저 쿠키 또는 저장된 세션을 자동으로 가져옵니다."
            }
            LocaleKey::ProviderCursorCreditsHelp => "기본 요금제 한도를 초과하는 온디맨드 사용량.",
            LocaleKey::AutoFallbackHelp => "기본 소스가 실패하면 자동으로 다음 소스로 전환합니다.",
            LocaleKey::ProviderSourceOauthWeb => "OAuth + 웹",
            LocaleKey::Automatic => "자동",
            LocaleKey::Average => "평균",
            LocaleKey::ExtraUsage => "추가 사용량",
            LocaleKey::OAuth => "OAuth",
            LocaleKey::Api => "API",
            LocaleKey::Web => "웹",

            // General tab sections
            LocaleKey::PrivacyTitle => "개인정보 보호",
            LocaleKey::HidePersonalInfo => "개인 정보 숨기기",
            LocaleKey::HidePersonalInfoHelper => "이메일 및 계정 이름 마스킹 (스트리밍 시 유용)",
            LocaleKey::UpdatesTitle => "업데이트",
            LocaleKey::UpdateChannelChoice => "업데이트 채널",
            LocaleKey::UpdateChannelChoiceHelper => "안정 버전과 베타 미리보기 버전 중 선택",
            LocaleKey::AutoDownloadUpdates => "업데이트 자동 확인",
            LocaleKey::AutoDownloadUpdatesHelper => {
                "새 릴리스가 발견되면 백그라운드에서 설치 프로그램 업데이트 다운로드"
            }
            LocaleKey::InstallUpdatesOnQuit => "종료 시 업데이트 설치",
            LocaleKey::InstallUpdatesOnQuitHelper => {
                "CodexBar를 종료할 때 준비된 설치 프로그램을 자동으로 실행합니다."
            }

            // Keyboard shortcuts
            LocaleKey::KeyboardShortcutsTitle => "키보드 단축키",
            LocaleKey::GlobalShortcutLabel => "글로벌 단축키",
            LocaleKey::GlobalShortcutHelper => "어디서나 이 단축키를 눌러 CodexBar를 엽니다",
            LocaleKey::ShortcutFormatHint => {
                "형식: Ctrl+Shift+Key, Alt+Ctrl+Key 등. 변경 사항을 적용하려면 재시작이 필요합니다."
            }
            LocaleKey::Saved => "저장됨 (적용하려면 재시작)",
            LocaleKey::InvalidFormat => "유효하지 않은 단축키 형식",
            LocaleKey::ShortcutHintPlaceholder => "예: Ctrl+Shift+U",

            // Display/Preferences helpers
            LocaleKey::SelectProvider => "제공업체 선택",

            // Refresh interval labels
            LocaleKey::RefreshInterval30Sec => "30초",
            LocaleKey::RefreshInterval1Min => "1분",
            LocaleKey::RefreshInterval5Min => "5분",
            LocaleKey::RefreshInterval10Min => "10분",

            // Cookies tab
            LocaleKey::BrowserCookiesTitle => "브라우저 쿠키",
            LocaleKey::CookieImport => "쿠키 가져오기",
            LocaleKey::Provider => "제공업체",
            LocaleKey::SelectPlaceholder => "선택...",
            LocaleKey::AutoRefreshInterval => "자동 새로고침 간격",

            // About tab
            LocaleKey::AboutDescription => "오리지널 macOS 버전의 Windows 이식 버전입니다.",
            LocaleKey::AboutDescriptionLine2 => {
                "시스템 트레이에서 AI 제공업체의 사용량을 추적하세요."
            }
            LocaleKey::ViewOnGitHub => "→ GitHub에서 보기",
            LocaleKey::SubmitIssue => "→ 이슈 제출",
            LocaleKey::MaintainedBy => "CodexBar 기여자들에 의해 유지 관리됨",
            LocaleKey::CommitLabel => "커밋",
            LocaleKey::BuildDateLabel => "빌드 날짜",

            // Shared form controls
            LocaleKey::Save => "저장",
            LocaleKey::Cancel => "취소",
            LocaleKey::Label => "라벨",
            LocaleKey::Token => "토큰",
            LocaleKey::AddAccount => "계정 추가",
            LocaleKey::AccountAdded => "계정이 추가되었습니다",
            LocaleKey::AccountRemoved => "계정이 제거되었습니다",
            LocaleKey::AccountSwitched => "계정이 전환되었습니다",
            LocaleKey::AccountLabelHint => "예: 회사 계정, 개인 계정...",
            LocaleKey::EnterApiKeyFor => "{}의 API 키 입력",
            LocaleKey::PasteApiKeyHere => "여기에 API 키 붙여넣기...",
            LocaleKey::ApiKeySaved => "{}의 API 키 저장됨",
            LocaleKey::ApiKeyRemoved => "{}의 API 키 제거됨",
            LocaleKey::EnvironmentVariable => "환경 변수",
            LocaleKey::CookieSavedForProvider => "{}의 쿠키 저장됨",
            LocaleKey::CookieRemovedForProvider => "{}의 쿠키 제거됨",

            // Usage helper functions
            LocaleKey::ShowUsedPercent => "{:.0}% 사용됨",
            LocaleKey::ShowRemainingPercent => "{:.0}% 남음",

            // Tauri desktop shell — Settings section headings
            LocaleKey::TabTokenAccounts => "토큰",
            LocaleKey::SectionRefresh => "자동화",
            LocaleKey::SectionNotifications => "알림",
            LocaleKey::SectionUsageThresholds => "사용량 임계값",
            LocaleKey::SectionKeyboard => "키보드",
            LocaleKey::SectionUsageRendering => "사용량 렌더링",
            LocaleKey::SectionTime => "시간",
            LocaleKey::SectionLanguage => "언어",
            LocaleKey::SectionCredentialsSecurity => "자격 증명 및 보안",
            LocaleKey::SectionDebug => "디버그",
            LocaleKey::SectionApiKeys => "API 키",
            LocaleKey::SectionSavedCookies => "저장된 쿠키",
            LocaleKey::SectionImportFromBrowser => "브라우저에서 가져오기",
            LocaleKey::SectionAddCookieManually => "수동으로 쿠키 추가",
            LocaleKey::SectionTokenAccounts => "토큰 계정",
            LocaleKey::SectionSavedAccounts => "저장된 계정",
            LocaleKey::SectionAddAccount => "계정 추가",

            // Tauri desktop shell — General tab fields
            LocaleKey::RefreshIntervalLabel => "새로고침 간격",
            LocaleKey::RefreshIntervalHelper => {
                "제공업체 자동 새로고침 간격(초) (0으로 설정 시 수동 새로고침)."
            }
            LocaleKey::RefreshAllProvidersOnMenuOpen => "메뉴를 열 때 새로고침",
            LocaleKey::RefreshAllProvidersOnMenuOpenHelper => {
                "트레이 메뉴를 열 때마다 활성화된 제공업체를 강제로 새로고침합니다."
            }
            LocaleKey::SoundVolumeHelper => "임계값 경고 소리 볼륨 (0~100).",
            LocaleKey::HighUsageWarningHelper => "사용량이 이 백분율을 초과하면 경고를 표시합니다.",
            LocaleKey::CriticalUsageWarningHelper => {
                "사용량이 이 백분율을 초과하면 위험 경고를 표시합니다."
            }
            LocaleKey::GlobalShortcutFieldLabel => "글로벌 단축키",
            LocaleKey::GlobalShortcutToggleHelper => "트레이 패널을 토글하는 키 조합입니다.",
            LocaleKey::ShortcutRecordButton => "입력",
            LocaleKey::ShortcutRecordingLabel => "입력 중…",
            LocaleKey::ShortcutRecordingHint => {
                "조합 키 + 단축키를 누르세요. Esc는 취소, Backspace는 지우기입니다."
            }
            LocaleKey::ShortcutClearButton => "지우기",
            LocaleKey::ShortcutEmptyPlaceholder => "설정되지 않음",
            LocaleKey::NotificationTestSound => "소리 테스트",
            LocaleKey::NotificationTestSoundPlaying => "재생 중…",

            // Tauri desktop shell — Display tab fields
            LocaleKey::TrayIconModeLabel => "트레이 아이콘 모드",
            LocaleKey::TrayIconModeHelper => {
                "단일 통합 아이콘 또는 활성화된 제공업체당 하나의 아이콘."
            }
            LocaleKey::TrayIconModeSingle => "단일",
            LocaleKey::TrayIconModePerProvider => "제공업체별",
            LocaleKey::ShowProviderIcons => "제공업체 아이콘 표시",
            LocaleKey::ShowProviderIconsHelper => "트레이 메뉴에 제공업체 아이콘을 표시합니다.",
            LocaleKey::PreferHighestUsage => "가장 높은 사용량 우선",
            LocaleKey::PreferHighestUsageHelper => {
                "병합된 트레이 아이콘에 사용 한도에 가장 가까운 제공업체를 표시합니다."
            }
            LocaleKey::ShowPercentInTray => "트레이에 백분율 표시",
            LocaleKey::ShowPercentInTrayHelper => {
                "사용량 표시줄을 제공업체 브랜드 및 백분율 텍스트로 대체합니다."
            }
            LocaleKey::DisplayModeLabel => "디스플레이 모드",
            LocaleKey::DisplayModeHelper => "메뉴 바 라벨에 표시할 세부 정보 수준입니다.",
            LocaleKey::DisplayModeDetailed => "상세",
            LocaleKey::DisplayModeCompact => "압축",
            LocaleKey::DisplayModeMinimal => "최소",
            LocaleKey::WindowScaleLabel => "창 크기 조절",
            LocaleKey::WindowScaleHelper => {
                "팝아웃 대시보드 콘텐츠의 크기를 조절합니다. 창 자체는 자유롭게 크기를 변경할 수 있습니다."
            }
            LocaleKey::WindowScaleAriaLabel => "창 크기 조절",
            LocaleKey::WindowMinimize => "최소화",
            LocaleKey::WindowMaximize => "최대화",
            LocaleKey::WindowRestore => "이전 크기로 복원",
            LocaleKey::WindowClose => "닫기",
            LocaleKey::ShowAsUsedLabel => "사용된 양으로 표시",
            LocaleKey::ShowAsUsedHelper => "사용량 표시줄을 남은 양 대신 소비된 양으로 표시합니다.",
            LocaleKey::ShowAllTokenAccountsLabel => "모든 토큰 계정 표시",
            LocaleKey::ShowAllTokenAccountsHelper => {
                "제공업체 메뉴에서 토큰 계정을 축소하지 않고 모두 나열합니다."
            }
            LocaleKey::EnableAnimationsLabel => "애니메이션 활성화",
            LocaleKey::EnableAnimationsHelper => {
                "부드러운 전환 효과 및 애니메이션 진행 표시줄을 사용합니다."
            }

            // Advanced tab fields
            LocaleKey::UpdateChannelStableOption => "안정(Stable)",
            LocaleKey::UpdateChannelBetaOption => "베타(Beta)",
            LocaleKey::AvoidKeychainPromptsLabel => "키체인 창 표시 방지 (Claude)",
            LocaleKey::AvoidKeychainPromptsHelper => {
                "OS 권한 대화 상자가 나타나지 않도록 Claude의 키체인 자격 증명 읽기를 건너뜁니다."
            }
            LocaleKey::DisableAllKeychainLabel => "모든 키체인 액세스 비활성화",
            LocaleKey::DisableAllKeychainHelper => {
                "모든 제공업체에 대한 자격 증명/키체인 읽기를 비활성화합니다. 위의 Claude 옵션도 활성화됩니다."
            }

            // Theme (Phase 12)
            LocaleKey::SectionTheme => "테마",
            LocaleKey::ThemeLabel => "테마",
            LocaleKey::ThemeHelper => {
                "시스템 테마 설정을 자동으로 따릅니다. 라이트 또는 다크 모드는 이 설정을 무시합니다."
            }
            LocaleKey::ThemeAutoOption => "자동 (시스템)",
            LocaleKey::ThemeLightOption => "라이트",
            LocaleKey::ThemeDarkOption => "다크",

            // settings status / common
            LocaleKey::SettingsStatusSaving => "저장 중…",
            LocaleKey::ApiKeysTabHint => {
                "토큰 기반 인증을 사용하는 제공업체의 API 키를 구성합니다. 키는 로컬에 저장되며 절대 전송되지 않습니다."
            }

            // tray / popout
            LocaleKey::FetchingProviderData => "제공업체 데이터 가져오는 중…",
            LocaleKey::NoProvidersConfigured => "구성된 제공업체가 없습니다.",
            LocaleKey::EnableProvidersHint => {
                "사용량 데이터를 보려면 설정에서 제공업체를 활성화하세요."
            }
            LocaleKey::OpenSettingsButton => "설정 열기",
            LocaleKey::TooltipRefresh => "새로고침",
            LocaleKey::TooltipSettings => "설정",
            LocaleKey::TooltipPopOut => "대시보드 띄우기",
            LocaleKey::TooltipBackToTray => "트레이로 복귀",
            LocaleKey::TrayCardErrorBadge => "오류",
            LocaleKey::SummaryProvidersLabel => "제공업체",
            LocaleKey::SummaryRefreshing => "새로고침 중…",
            LocaleKey::SummaryFailed => "실패",
            LocaleKey::SummaryWithErrors => "오류 있음",

            // provider detail
            LocaleKey::DetailBackButton => "뒤로",
            LocaleKey::DetailWindowPrimary => "기본",
            LocaleKey::DetailWindowSecondary => "보조",
            LocaleKey::DetailWindowModelSpecific => "모델별",
            LocaleKey::DetailWindowTertiary => "3차",
            LocaleKey::DetailWindowMinutesSuffix => "분 창",
            LocaleKey::DetailWindowExhausted => "모두 사용됨",
            LocaleKey::DetailPaceTitle => "사용 속도",
            LocaleKey::DetailPaceOnTrack => "정상 진행",
            LocaleKey::DetailPaceSlightlyAhead => "조금 빠름",
            LocaleKey::DetailPaceAhead => "빠름",
            LocaleKey::DetailPaceFarAhead => "매우 빠름",
            LocaleKey::DetailPaceSlightlyBehind => "조금 느림",
            LocaleKey::DetailPaceBehind => "느림",
            LocaleKey::DetailPaceFarBehind => "매우 느림",
            LocaleKey::DetailPaceRunsOutIn => "소진까지",
            LocaleKey::DetailPaceWillLastToReset => "초기화까지 유지 예상",
            LocaleKey::DetailCostTitle => "비용",
            LocaleKey::DetailCostUsed => "사용량",
            LocaleKey::DetailCostLimit => "한도",
            LocaleKey::DetailCostRemaining => "남음",
            LocaleKey::DetailCostResets => "초기화",
            LocaleKey::DetailChartCost => "비용 (30일)",
            LocaleKey::DetailChartCredits => "사용 크레딧 (30일)",
            LocaleKey::DetailChartUsageBreakdown => "서비스별 사용량 (30일)",
            LocaleKey::DetailChartEmpty => "아직 차트 데이터가 없습니다.",
            LocaleKey::DetailUpdatedPrefix => "업데이트됨",

            // update banner
            LocaleKey::BannerCheckingForUpdates => "업데이트 확인 중…",
            LocaleKey::BannerUpdateAvailablePrefix => "업데이트",
            LocaleKey::BannerDownloadButton => "다운로드",
            LocaleKey::BannerViewRelease => "릴리스 보기",
            LocaleKey::BannerDismiss => "닫기",
            LocaleKey::BannerDownloadingPrefix => "업데이트 다운로드 중",
            LocaleKey::BannerReadyToInstallSuffix => "설치 준비 완료",
            LocaleKey::BannerInstallRestart => "설치 및 재시작",
            LocaleKey::BannerUpdateFailedPrefix => "업데이트 실패",
            LocaleKey::BannerRetry => "재시도",

            // providers sidebar (Phase 6a)
            LocaleKey::ProviderSidebarSearch => "검색",
            LocaleKey::ProviderSidebarClearSearch => "제공업체 검색 지우기",
            LocaleKey::ProviderSidebarNoMatches => "일치하는 제공업체 없음",
            LocaleKey::ProviderSidebarReorderHint => "끌어서 순서 변경",
            LocaleKey::ProviderSidebarMoveUp => "위로 이동",
            LocaleKey::ProviderSidebarMoveDown => "아래로 이동",
            LocaleKey::ProviderStatusOk => "최신 상태",
            LocaleKey::ProviderStatusStale => "이전 데이터",
            LocaleKey::ProviderStatusError => "오류",
            LocaleKey::ProviderStatusLoading => "로딩 중",
            LocaleKey::ProviderStatusDisabled => "비활성화됨",
            LocaleKey::ProviderDetailPlaceholder => "세부 정보 패널은 6b단계에 제공될 예정입니다",
            LocaleKey::ProviderIssueNeedsSignIn => "로그인이 필요합니다",
            LocaleKey::ProviderIssueFetchNeedsAttention => {
                "제공업체 데이터 가져오기에 확인이 필요합니다"
            }
            LocaleKey::ProviderIssueCopy => "복사",
            LocaleKey::ProviderIssueUnsupportedSourceModePrefix => {
                "이 제공업체는 선택한 소스 모드를 지원하지 않습니다"
            }
            LocaleKey::CredentialStorageTitle => "자격 증명 저장소",
            LocaleKey::CredentialRevokeStored => "저장된 자격 증명 해제",
            LocaleKey::CredentialApiKeys => "API 키",
            LocaleKey::CredentialManualCookies => "수동 쿠키",
            LocaleKey::CredentialTokenAccounts => "토큰 계정",
            LocaleKey::CredentialProtectedPrefix => "보호됨",
            LocaleKey::CredentialStatusNotCreated => "생성되지 않음",
            LocaleKey::CredentialStatusPlaintext => "일반 텍스트",
            LocaleKey::CredentialStatusUnavailable => "사용할 수 없음",
            LocaleKey::CredentialStatusUnreadable => "읽을 수 없음",
            LocaleKey::BrowserCookiesSectionTitle => "브라우저 쿠키",
            LocaleKey::BrowserCookieNoneSaved => "저장된 쿠키가 없습니다.",
            LocaleKey::BrowserCookieSavedBadge => "저장됨",
            LocaleKey::BrowserCookieRemove => "삭제",
            LocaleKey::BrowserCookieImportSuccess => "쿠키를 가져왔습니다.",
            LocaleKey::BrowserCookieImportFromBrowser => "브라우저에서 가져오기",
            LocaleKey::BrowserCookieProfileSingular => "프로필",
            LocaleKey::BrowserCookieProfilePlural => "프로필",
            LocaleKey::BrowserCookiePlaceholderDefault => "쿠키 헤더 값을 붙여넣으세요...",
            LocaleKey::BrowserCookiePlaceholderOllama => {
                "전체 Cookie 헤더 또는 __Secure-session 값만 붙여넣으세요..."
            }
            LocaleKey::BrowserCookiePlaceholderCurl => {
                "Cookie 헤더 또는 브라우저 cURL 요청 전체를 붙여넣으세요..."
            }
            LocaleKey::BrowserCookieSave => "쿠키 저장",

            // Phase 6d — credential detection
            LocaleKey::CredentialsSectionTitle => "자격 증명",
            LocaleKey::CredsStatusAuthenticated => "인증됨",
            LocaleKey::CredsStatusNotSignedIn => "로그인하지 않음",
            LocaleKey::CredsStatusDetected => "감지됨",
            LocaleKey::CredsStatusNotDetected => "감지되지 않음",
            LocaleKey::CredsStatusAvailable => "사용 가능",
            LocaleKey::CredsStatusUnavailable => "사용 불가능",
            LocaleKey::CredsOpenFolderAction => "자격 증명 폴더 열기",
            LocaleKey::CredsRefreshDetectionAction => "감지 새로고침",
            LocaleKey::CredsSavePathAction => "경로 저장",
            LocaleKey::CredsBrowseAction => "찾아보기…",
            LocaleKey::CredsGeminiCliLabel => "Gemini CLI",
            LocaleKey::CredsGeminiCliHelperPrefix => "다음 경로의 OAuth 자격 증명 사용:",
            LocaleKey::CredsGeminiCliSetupAction => "Gemini CLI 설정",
            LocaleKey::CredsGeminiCliSetupHelp => {
                "Gemini CLI를 설치하고 `gemini auth login`을 실행하여 로그인하십시오."
            }
            LocaleKey::CredsVertexAiLabel => "Google Cloud",
            LocaleKey::CredsVertexAiHelperPrefix => "다음 경로의 Google Cloud 자격 증명 사용:",
            LocaleKey::CredsVertexAiSetupAction => "Google Cloud 인증 설정",
            LocaleKey::CredsVertexAiSetupHelp => {
                "`gcloud auth application-default login`을 실행하여 자격 증명을 생성하십시오."
            }
            LocaleKey::CredsJetBrainsLabel => "JetBrains IDE",
            LocaleKey::CredsJetBrainsHelperDetectedPrefix => "다음 경로의 감지된 IDE 구성 사용:",
            LocaleKey::CredsJetBrainsHelperCustomPrefix => "사용자 정의 IDE 기본 경로 사용:",
            LocaleKey::CredsJetBrainsHelperMissing => {
                "AI Assistant가 활성화된 JetBrains IDE를 설치한 후 CodexBar를 새로 고치십시오."
            }
            LocaleKey::CredsJetBrainsCustomPathLabel => "사용자 정의 경로",
            LocaleKey::CredsJetBrainsCustomPathPlaceholder => "%APPDATA%/JetBrains/IntelliJIdea...",
            LocaleKey::CredsJetBrainsSelectLabel => "모니터링할 JetBrains IDE를 선택하십시오.",
            LocaleKey::CredsJetBrainsAutoDetectOption => "자동 감지",
            LocaleKey::CredsKiroLabel => "Kiro CLI",
            LocaleKey::CredsKiroHelperAvailablePrefix => "감지된 위치:",
            LocaleKey::CredsKiroHelperMissing => {
                "`kiro-cli`: PATH 또는 알려진 설치 위치에서 찾을 수 없습니다."
            }
            LocaleKey::CredsOpenAiHistoryHelp => {
                "시간 경과에 따른 사용량을 확인하려면 기록 추적을 활성화하세요."
            }

            // Token accounts (Phase 6e, review)
            LocaleKey::TokenAccountActive => "활성",
            LocaleKey::TokenAccountSetActive => "활성으로 설정",
            LocaleKey::TokenAccountRemove => "제거",
            LocaleKey::TokenAccountAddButton => "계정 추가",
            LocaleKey::TokenAccountGithubLoginButton => "GitHub로 로그인",
            LocaleKey::TokenAccountEmpty => "이 제공업체에 대해 저장된 계정이 없습니다.",
            LocaleKey::TokenAccountLabelPlaceholder => "라벨 (예: 업무용, 개인용)…",
            LocaleKey::TokenAccountProviderLabel => "제공업체",
            LocaleKey::TokenAccountProviderPlaceholder => "제공업체 선택…",
            LocaleKey::TokenAccountAddedPrefix => "추가됨",
            LocaleKey::TokenAccountUsedPrefix => "사용됨",
            LocaleKey::TokenAccountTabHint => {
                "제공업체별로 여러 세션 토큰 또는 API 토큰을 관리합니다. 활성 계정이 모든 데이터 가져오기에 사용됩니다. 수동 토큰이 필요한 제공업체만 여기에 표시됩니다."
            }
            LocaleKey::TokenAccountNoSupported => "현재 토큰 계정을 지원하는 제공업체가 없습니다.",
            LocaleKey::TokenAccountInlineSummary => "토큰 계정",

            // Phase 9 - Tray / pop-out pace badges + countdowns
            LocaleKey::TrayPaceBadgeSlow => "느림",
            LocaleKey::TrayPaceBadgeSteady => "안정적",
            LocaleKey::TrayPaceBadgeRacing => "빠름",
            LocaleKey::TrayPaceBadgeBurning => "매우 빠름",
            LocaleKey::TrayResetsInLabel => "초기화까지 {}",
            LocaleKey::TrayResetsDueNow => "초기화 중…",
        }
    }
}
