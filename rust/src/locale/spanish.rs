use super::*;

impl LocaleKey {
    pub(super) fn spanish(self) -> &'static str {
        match self {
            // Tab names
            LocaleKey::TabGeneral => "General",
            LocaleKey::TabProviders => "Proveedores",
            LocaleKey::TabDisplay => "Pantalla",
            LocaleKey::TabApiKeys => "Claves API",
            LocaleKey::TabCookies => "Cookies",
            LocaleKey::TabAdvanced => "Avanzado",
            LocaleKey::TabAbout => "Acerca de",
            LocaleKey::TabShortcuts => "Atajos",

            // General settings
            LocaleKey::InterfaceLanguage => "Idioma de la interfaz",
            LocaleKey::StartupSettings => "Sistema",
            LocaleKey::StartAtLogin => "Iniciar al arrancar",
            LocaleKey::StartMinimized => "Iniciar minimizado",
            LocaleKey::StartAtLoginHelper => {
                "Iniciar sesión automáticamente al arrancar el sistema"
            }
            LocaleKey::StartMinimizedHelper => "Iniciar minimizado en la bandeja del sistema",

            // Notification settings
            LocaleKey::ShowNotifications => "Mostrar notificaciones",
            LocaleKey::ShowNotificationsHelper => "Alertar cuando se alcancen los umbrales de uso",
            LocaleKey::SoundEnabled => "Alertas sonoras",
            LocaleKey::SoundEnabledHelper => "Reproducir sonido cuando se alcancen los umbrales",
            LocaleKey::SoundVolume => "Volumen de alerta",
            LocaleKey::HighUsageThreshold => "Umbral de uso alto",
            LocaleKey::HighUsageThresholdHelper => "Mostrar advertencia en este nivel de uso",
            LocaleKey::HighUsageAlert => "Alerta de uso alto",
            LocaleKey::CriticalUsageThreshold => "Umbral de uso crítico",
            LocaleKey::CriticalUsageThresholdHelper => "Mostrar alerta crítica en este nivel",
            LocaleKey::CriticalUsageAlert => "Alerta crítica",

            // Display settings
            LocaleKey::UsageDisplay => "Visualización de uso",
            LocaleKey::ShowUsageAsUsed => "Mostrar como usado",
            LocaleKey::ShowUsageAsUsedHelper => {
                "Mostrar como porcentaje usado en lugar de restante"
            }
            LocaleKey::ResetTimeRelative => "Tiempo relativo de reinicio",
            LocaleKey::ResetTimeRelativeHelper => "Mostrar \"2h 30m\" en lugar de \"3:00 PM\"",
            LocaleKey::TrayIcon => "Ícono de bandeja",
            LocaleKey::MergeTrayIcons => "Combinar íconos de bandeja",
            LocaleKey::MergeTrayIconsHelper => {
                "Mostrar todos los proveedores en un solo ícono de bandeja"
            }
            LocaleKey::PerProviderTrayIcons => "Íconos por proveedor",
            LocaleKey::PerProviderTrayIconsHelper => {
                "Mostrar un ícono de bandeja separado por cada proveedor habilitado"
            }

            // Provider settings
            LocaleKey::ProviderEnabled => "Habilitado",
            LocaleKey::ProviderDisabled => "Deshabilitado",
            LocaleKey::ProviderInfo => "Información",
            LocaleKey::ProviderUsage => "Uso",
            LocaleKey::AuthType => "Autenticación",
            LocaleKey::DataSource => "Fuente de datos",
            LocaleKey::ProviderNotDetected => "no detectado",
            LocaleKey::ProviderLastFetchFailed => "última consulta fallida",
            LocaleKey::ProviderUsageNotFetchedYet => "uso no consultado aún",
            LocaleKey::ProviderNotFetchedYetTitle => "Aún no consultado",
            LocaleKey::ProviderDisabledNoRecentData => "Deshabilitado — sin datos recientes",
            LocaleKey::ProviderSourceAutoShort => "auto",
            LocaleKey::ProviderSourceWebShort => "web",
            LocaleKey::ProviderSourceCliShort => "cli",
            LocaleKey::ProviderSourceOauthShort => "oauth",
            LocaleKey::ProviderSourceApiShort => "api",
            LocaleKey::ProviderSourceGithubApiShort => "api de github",
            LocaleKey::ProviderSourceLocalShort => "local",
            LocaleKey::ProviderSourceKiroEnvShort => "entorno kiro",
            LocaleKey::TrackingItem => "Elemento rastreado",
            LocaleKey::MainWindowLiveUsageData => "Datos de uso en vivo en ventana principal",
            LocaleKey::StartTrackingUsage => "Habilitar para comenzar a rastrear el uso",
            LocaleKey::ClickTrayIconForMetrics => "Clic en ícono de bandeja para métricas en vivo",

            // Browser cookie import
            LocaleKey::BrowserCookieImport => "Importación de cookies del navegador",
            LocaleKey::ImportFromBrowser => "Importar cookies de {} desde el navegador",
            LocaleKey::NoCookiesFoundInBrowser => {
                "No se encontraron cookies en {}. Inicie sesión primero."
            }
            LocaleKey::SelectBrowser => "Seleccionar navegador...",
            LocaleKey::ImportCookies => "Importar cookies",
            LocaleKey::ImportSuccess => "Cookies importadas para {}",
            LocaleKey::ImportFailed => "Importación fallida: {}",
            LocaleKey::SaveFailed => "Guardado fallido: {}",
            LocaleKey::CookiesAutoImport => {
                "Las cookies se importan automáticamente desde Chrome, Edge, Brave y Firefox"
            }
            LocaleKey::QuickActions => "Acciones rápidas",
            LocaleKey::OpenProviderDashboard => "Abrir panel de {}",
            LocaleKey::OllamaNoDashboard => "Ollama se ejecuta localmente, sin panel",

            // API Keys tab
            LocaleKey::ApiKeysTitle => "Claves API",
            LocaleKey::ApiKeysDescription => {
                "Configurar tokens de acceso para proveedores que requieren autenticación."
            }
            LocaleKey::AddKey => "+ Agregar clave",
            LocaleKey::KeySet => "Establecida",
            LocaleKey::KeyRequired => "Requiere clave",
            LocaleKey::Remove => "Eliminar",
            LocaleKey::GetKey => "Obtener clave →",

            // Cookies tab
            LocaleKey::SavedCookies => "Cookies guardadas",
            LocaleKey::AddManualCookie => "Agregar cookie manual",
            LocaleKey::CookieHeader => "Encabezado de cookie",
            LocaleKey::PasteHere => "Pegar aquí...",
            LocaleKey::DeleteCookie => "Eliminar",
            LocaleKey::CookieSaved => "{} cookies guardadas",
            LocaleKey::CookieDeleted => "Cookies eliminadas para {}",

            // Advanced tab
            LocaleKey::RefreshSettings => "Actualizar",
            LocaleKey::Animations => "Animaciones",
            LocaleKey::MenuBar => "Barra de menú",
            LocaleKey::Fun => "Diversión",
            LocaleKey::GlobalShortcut => "Atajo global",
            LocaleKey::Privacy => "Privacidad",
            LocaleKey::Updates => "Actualizaciones",
            LocaleKey::UpdateChannel => "Canal de actualización",
            LocaleKey::UpdateChannelStable => "Estable",
            LocaleKey::UpdateChannelBeta => "Beta",
            LocaleKey::Never => "Nunca",
            LocaleKey::LastUpdated => "Actualizado",
            LocaleKey::MinutesAgo => "Hace {} minutos",
            LocaleKey::HoursAgo => "Hace {} horas",
            LocaleKey::DaysAgo => "Hace {} días",
            LocaleKey::BuiltWithRust => "Construido con Rust + egui",
            LocaleKey::OriginalMacOSVersion => "Versión original de macOS",
            LocaleKey::Links => "Enlaces",
            LocaleKey::BuildInfo => "Información de compilación",
            LocaleKey::EnabledProviders => "Proveedores habilitados",
            LocaleKey::Appearance => "Apariencia",
            LocaleKey::ThemeSelection => "Tema",
            LocaleKey::LightMode => "Claro",
            LocaleKey::DarkMode => "Oscuro",

            // About
            LocaleKey::AboutTitle => "Acerca de CodexBar",
            LocaleKey::Version => "Versión",

            // Main popup - Header actions
            LocaleKey::ActionRefreshAll => "Actualizar todo",
            LocaleKey::ActionSettings => "Configuración",
            LocaleKey::ActionClose => "✕",

            // Main popup - Provider section
            LocaleKey::ProviderAccount => "Cuenta",
            LocaleKey::ProviderSession => "Sesión",
            LocaleKey::ProviderWeekly => "Semanal",
            LocaleKey::ProviderMonthly => "30 días",
            LocaleKey::ProviderModel => "Modelo",
            LocaleKey::ProviderPlan => "Plan",
            LocaleKey::ProviderNextReset => "Próximo reinicio",
            LocaleKey::ProviderNoRecentUsage => "Sin uso reciente",
            LocaleKey::ProviderNotSignedIn => "Sin iniciar sesión",
            LocaleKey::SummaryTab => "Resumen",

            // Main popup - Loading/Empty/Error states
            LocaleKey::StateLoadingProviders => "Cargando proveedores...",
            LocaleKey::StateNoProviderData => "Sin datos de proveedores.",
            LocaleKey::StateNoProviderSelected => "Ningún proveedor seleccionado.",
            LocaleKey::StateSummaryRefreshPending => {
                "Actualizando después de que terminen todas las consultas"
            }
            LocaleKey::StateError => "Error",
            LocaleKey::StateRetry => "Reintentar",
            LocaleKey::StateDownload => "Descargar",
            LocaleKey::StateRestartAndUpdate => "Reiniciar y actualizar",

            // Main popup - Credits
            LocaleKey::CreditsTitle => "Créditos",

            // Main popup - Update banner (non-happy-path)
            LocaleKey::UpdateRestartAndUpdate => "Reiniciar y actualizar",
            LocaleKey::UpdateRetry => "Reintentar",
            LocaleKey::UpdateDownload => "Descargar",
            LocaleKey::UpdateDownloading => "Descargando",
            LocaleKey::UpdateReady => "Listo para instalar",
            LocaleKey::UpdateFailed => "Actualización fallida",

            // Main popup - Settings button
            LocaleKey::ButtonOpenProviderSettings => "Abrir configuración del proveedor",

            // Main popup - Bottom menu (Actions)
            LocaleKey::MenuSettings => "Configuración...",
            LocaleKey::MenuAbout => "Acerca de CodexBar",
            LocaleKey::MenuQuit => "Salir",

            // Main popup - Status strings
            LocaleKey::StatusJustUpdated => "Recién actualizado",
            LocaleKey::StatusUnableToGetUsage => "No se pudo obtener el uso",

            // Main popup - Provider detail actions
            LocaleKey::ActionRefresh => "Actualizar",
            LocaleKey::ActionSwitchAccount => "Cambiar cuenta...",
            LocaleKey::ActionUsageDashboard => "Panel de uso",
            LocaleKey::ActionStatusPage => "Página de estado",
            LocaleKey::ActionCopyError => "Copiar error",
            LocaleKey::ActionBuyCredits => "Comprar créditos...",

            // Main popup - Pace status
            LocaleKey::PaceOnTrack => "En ritmo",
            LocaleKey::PaceBehind => "Atrasado",

            // Main popup - Reset prefix
            LocaleKey::MetricResetsIn => "Reinicia en",

            // Main popup - Section titles
            LocaleKey::SectionUsageBreakdown => "Desglose de uso",
            LocaleKey::SectionCost => "Costo",

            // Tray - Single icon mode
            LocaleKey::TrayOpenCodexBar => "Abrir panel",
            LocaleKey::TrayPopOutDashboard => "Abrir panel",
            LocaleKey::TrayRefreshAll => "Actualizar todo",
            LocaleKey::TrayProviders => "Proveedores",
            LocaleKey::TraySettings => "Configuración...",
            LocaleKey::TrayCheckForUpdates => "Buscar actualizaciones",
            LocaleKey::TrayQuit => "Salir",
            LocaleKey::TrayLoading => "CodexBar - Cargando...",
            LocaleKey::TrayNoProviders => "CodexBar - Sin proveedores disponibles",
            LocaleKey::TraySessionPercent => "Sesión {}%",
            LocaleKey::TrayWeeklyPercent => "Semanal {}%",
            LocaleKey::TrayStatusError => " (Error)",
            LocaleKey::TrayStatusStale => " (Datos desactualizados)",
            LocaleKey::TrayStatusIncident => " (Incidente)",
            LocaleKey::TrayStatusPartial => " (Interrupción parcial)",
            LocaleKey::TrayWeeklyExhausted => "Límite semanal agotado",
            LocaleKey::TrayCreditsRemaining => "Créditos restantes {}%",
            LocaleKey::TrayStatusRowLoading => "Cargando...",
            LocaleKey::TrayStatusRowError => "Error",
            LocaleKey::TrayCreditsRow => "Créditos {}%",

            // Main popup - Usage/reset labels
            LocaleKey::ResetInProgress => "Reiniciando...",
            LocaleKey::TomorrowAt => "Mañana a las {}",
            LocaleKey::UsedPercent => "{:.0}% usado",
            LocaleKey::RemainingPercent => "{:.0}% restante",
            LocaleKey::RemainingAmount => "{:.2} restante",
            LocaleKey::Tokens1K => "1K tokens",
            LocaleKey::TodayCost => "Hoy: ${:.2}",
            LocaleKey::Last30DaysCost => "Últimos 30 días: ${:.2}",
            LocaleKey::StatusLabel => "Estado: {}",

            // Main popup - Update banner messages
            LocaleKey::UpdateAvailableMessage => "Actualización disponible: {}",
            LocaleKey::UpdateReadyMessage => "{} lista para instalar",
            LocaleKey::UpdateFailedMessage => "Actualización fallida: {}",
            LocaleKey::UpdateDownloadingMessage => "Descargando {} ({:.0}%)",

            // Tray - Per-provider mode
            LocaleKey::TrayProviderPopOut => "Abrir panel",
            LocaleKey::TrayProviderRefresh => "Actualizar",
            LocaleKey::TrayProviderSettings => "Configuración...",
            LocaleKey::TrayProviderQuit => "Salir",

            // Provider settings - Live renderer specific
            LocaleKey::State => "Estado",
            LocaleKey::Source => "Fuente",
            LocaleKey::Updated => "Actualizado",
            LocaleKey::NeverUpdated => "Nunca actualizado",
            LocaleKey::UpdatedJustNow => "Recién actualizado",
            LocaleKey::UpdatedMinutesAgo => "Hace {} minutos",
            LocaleKey::UpdatedHoursAgo => "Hace {} horas",
            LocaleKey::UpdatedDaysAgo => "Hace {} días",
            LocaleKey::Status => "Estado",
            LocaleKey::AllSystemsOperational => "Todos los sistemas funcionando",
            LocaleKey::Plan => "Plan",
            LocaleKey::Account => "Cuenta",

            // Provider detail - Usage section
            LocaleKey::ProviderSessionLabel => "Sesión",
            LocaleKey::ProviderWeeklyLabel => "Semanal",
            LocaleKey::ProviderCodeReviewLabel => "Revisión de código",
            LocaleKey::ResetsInShort => "Reinicia en",
            LocaleKey::ResetsInDaysHours => "Reinicia en {}d {}h",
            LocaleKey::ResetsInHoursMinutes => "Reinicia en {}h {}m",

            // Provider detail - Tray Display
            LocaleKey::TrayDisplayTitle => "Pantalla de bandeja",
            LocaleKey::ShowInTray => "Mostrar en bandeja",

            // Provider detail - Credits
            LocaleKey::CreditsLabel => "Créditos",
            LocaleKey::CreditsLeft => "{:.1} restantes",

            // Provider detail - Cost
            LocaleKey::CostTitle => "Costo",
            LocaleKey::TodayCostFull => "Hoy: ${:.2} • {} tokens",
            LocaleKey::Last30DaysCostFull => "Últimos 30 días: ${:.2} • {} tokens",

            // Provider detail - Settings section
            LocaleKey::ProviderSettingsTitle => "Configuración",
            LocaleKey::ProviderAccountsTitle => "Cuentas",
            LocaleKey::ProviderOptionsTitle => "Opciones",
            LocaleKey::MenuBarMetric => "Métrica de barra de menú",
            LocaleKey::MenuBarMetricHelper => {
                "Elegir qué ventana controla el porcentaje de la barra de menú."
            }
            LocaleKey::UsageSource => "Fuente de uso",
            LocaleKey::ProviderNoCodexAccountsDetected => "Aún no se detectaron cuentas de Codex.",
            LocaleKey::ProviderCodexAutoImportHelp => {
                "Importa automáticamente cookies del navegador para extras del panel."
            }
            LocaleKey::ProviderCodexHistoryHelp => {
                "Almacena el historial local de uso de Codex (8 semanas) para personalizar predicciones de ritmo."
            }
            LocaleKey::ProviderOpenAiCookies => "Cookies de OpenAI",
            LocaleKey::ProviderHistoricalTracking => "Seguimiento histórico",
            LocaleKey::ProviderOpenAiWebExtras => "Extras web de OpenAI",
            LocaleKey::ProviderOpenAiWebExtrasHelp => {
                "Mostrar desglose de uso, historial de créditos y revisión de código vía chatgpt.com."
            }
            LocaleKey::ProviderCodexCreditsUnavailable => {
                "Créditos no disponibles; mantenga Codex en ejecución para actualizar."
            }
            LocaleKey::ProviderCodexLastFetchFailedTitle => "Última consulta de Codex fallida:",
            LocaleKey::ProviderCodexNotRunningHelp => {
                "Codex no está en ejecución. Intente ejecutar un comando de Codex primero."
            }
            LocaleKey::ProviderCookieSource => "Fuente de cookies",
            LocaleKey::CookieSourceManual => "Manual",
            LocaleKey::ProviderRegion => "Región",
            LocaleKey::ProviderClaudeCookies => "Cookies de Claude",
            LocaleKey::ProviderClaudeCookiesHelp => {
                "Se prefieren cookies/sessionKey del navegador porque coinciden con la página de uso de configuración de Claude."
            }
            LocaleKey::ProviderClaudeAvoidKeychainPrompts => "Evitar avisos de llavero",
            LocaleKey::ProviderClaudeAvoidKeychainPromptsHelp => {
                "Usar /usr/bin/security para leer credenciales de Claude y evitar avisos de llavero de CodexBar."
            }
            LocaleKey::ProviderCursorCookieSourceHelp => {
                "Importa automáticamente cookies del navegador o sesiones almacenadas."
            }
            LocaleKey::ProviderCursorCreditsHelp => {
                "Uso bajo demanda más allá de los límites del plan incluido."
            }
            LocaleKey::AutoFallbackHelp => {
                "Auto recurre a la siguiente fuente si la preferida falla."
            }
            LocaleKey::ProviderSourceOauthWeb => "OAuth + Web",
            LocaleKey::Automatic => "Automático",
            LocaleKey::Average => "Promedio",
            LocaleKey::ExtraUsage => "Uso extra",
            LocaleKey::OAuth => "OAuth",
            LocaleKey::Api => "API",
            LocaleKey::Web => "Web",

            // General tab sections
            LocaleKey::PrivacyTitle => "Privacidad",
            LocaleKey::HidePersonalInfo => "Ocultar información personal",
            LocaleKey::HidePersonalInfoHelper => {
                "Ocultar correos y nombres de cuenta (útil para transmisiones)"
            }
            LocaleKey::UpdatesTitle => "Actualizaciones",
            LocaleKey::UpdateChannelChoice => "Canal de actualización",
            LocaleKey::UpdateChannelChoiceHelper => {
                "Elegir entre versiones estables y versiones beta de prueba"
            }
            LocaleKey::AutoDownloadUpdates => "Buscar actualizaciones automáticamente",
            LocaleKey::AutoDownloadUpdatesHelper => {
                "Descargar actualizaciones del instalador en segundo plano cuando se encuentre una nueva versión"
            }
            LocaleKey::InstallUpdatesOnQuit => "Instalar actualizaciones al salir",
            LocaleKey::InstallUpdatesOnQuitHelper => {
                "Iniciar automáticamente un instalador listo al salir de CodexBar"
            }

            // Keyboard shortcuts
            LocaleKey::KeyboardShortcutsTitle => "Atajos de teclado",
            LocaleKey::GlobalShortcutLabel => "Atajo global",
            LocaleKey::GlobalShortcutHelper => {
                "Presione este atajo para abrir CodexBar desde cualquier lugar"
            }
            LocaleKey::ShortcutFormatHint => {
                "Formato: Ctrl+Shift+Tecla, Alt+Ctrl+Tecla, etc. Se requiere reiniciar para aplicar cambios."
            }
            LocaleKey::Saved => "Guardado (reiniciar para aplicar)",
            LocaleKey::InvalidFormat => "Formato de atajo no válido",
            LocaleKey::ShortcutHintPlaceholder => "p. ej., Ctrl+Shift+U",

            // Display/Preferences helpers
            LocaleKey::SelectProvider => "Seleccionar un proveedor",

            // Refresh interval labels
            LocaleKey::RefreshInterval30Sec => "30 seg",
            LocaleKey::RefreshInterval1Min => "1 min",
            LocaleKey::RefreshInterval5Min => "5 min",
            LocaleKey::RefreshInterval10Min => "10 min",

            // Cookies tab
            LocaleKey::BrowserCookiesTitle => "Cookies del navegador",
            LocaleKey::CookieImport => "Importación de cookies",
            LocaleKey::Provider => "Proveedor",
            LocaleKey::SelectPlaceholder => "Seleccionar...",
            LocaleKey::AutoRefreshInterval => "Intervalo de actualización automática",

            // About tab
            LocaleKey::AboutDescription => {
                "Una adaptación para Windows de la versión original de macOS."
            }
            LocaleKey::AboutDescriptionLine2 => {
                "Rastree el uso de proveedores de IA en su bandeja del sistema."
            }
            LocaleKey::ViewOnGitHub => "→ Ver en GitHub",
            LocaleKey::SubmitIssue => "→ Reportar un problema",
            LocaleKey::MaintainedBy => "Mantenido por colaboradores de CodexBar",
            LocaleKey::CommitLabel => "Commit",
            LocaleKey::BuildDateLabel => "Compilado",

            // Shared form controls
            LocaleKey::Save => "Guardar",
            LocaleKey::Cancel => "Cancelar",
            LocaleKey::Label => "Etiqueta",
            LocaleKey::Token => "Token",
            LocaleKey::AddAccount => "Agregar cuenta",
            LocaleKey::AccountAdded => "Cuenta agregada",
            LocaleKey::AccountRemoved => "Cuenta eliminada",
            LocaleKey::AccountSwitched => "Cuenta cambiada",
            LocaleKey::AccountLabelHint => "p. ej., Cuenta de trabajo, Personal...",
            LocaleKey::EnterApiKeyFor => "Ingresar clave API para {}",
            LocaleKey::PasteApiKeyHere => "Pegue su clave API aquí...",
            LocaleKey::ApiKeySaved => "Clave API guardada para {}",
            LocaleKey::ApiKeyRemoved => "Clave API eliminada para {}",
            LocaleKey::EnvironmentVariable => "Variable de entorno",
            LocaleKey::CookieSavedForProvider => "Cookies guardadas para {}",
            LocaleKey::CookieRemovedForProvider => "Cookies eliminadas para {}",

            // Usage helper functions
            LocaleKey::ShowUsedPercent => "{:.0}% usado",
            LocaleKey::ShowRemainingPercent => "{:.0}% restante",

            // Tauri desktop shell — Settings section headings
            LocaleKey::TabTokenAccounts => "Tokens",
            LocaleKey::SectionRefresh => "Automatización",
            LocaleKey::SectionNotifications => "Notificaciones",
            LocaleKey::SectionUsageThresholds => "Umbrales de uso",
            LocaleKey::SectionKeyboard => "Teclado",
            LocaleKey::SectionUsageRendering => "Representación de uso",
            LocaleKey::SectionTime => "Tiempo",
            LocaleKey::SectionLanguage => "Idioma",
            LocaleKey::SectionCredentialsSecurity => "Credenciales y seguridad",
            LocaleKey::SectionDebug => "Depuración",
            LocaleKey::SectionApiKeys => "Claves API",
            LocaleKey::SectionSavedCookies => "Cookies guardadas",
            LocaleKey::SectionImportFromBrowser => "Importar del navegador",
            LocaleKey::SectionAddCookieManually => "Agregar cookie manualmente",
            LocaleKey::SectionTokenAccounts => "Cuentas de token",
            LocaleKey::SectionSavedAccounts => "Cuentas guardadas",
            LocaleKey::SectionAddAccount => "Agregar cuenta",

            // Tauri desktop shell — General tab fields
            LocaleKey::RefreshIntervalLabel => "Intervalo de actualización",
            LocaleKey::RefreshIntervalHelper => {
                "Segundos entre actualizaciones automáticas de proveedores (0 = manual)."
            }
            LocaleKey::SoundVolumeHelper => "Volumen para sonidos de alerta de umbral (0–100).",
            LocaleKey::HighUsageWarningHelper => {
                "Mostrar una advertencia cuando el uso exceda este porcentaje."
            }
            LocaleKey::CriticalUsageWarningHelper => {
                "Mostrar una alerta crítica cuando el uso exceda este porcentaje."
            }
            LocaleKey::GlobalShortcutFieldLabel => "Atajo global",
            LocaleKey::GlobalShortcutToggleHelper => {
                "Combinación de teclas para alternar el panel de bandeja."
            }
            LocaleKey::ShortcutRecordButton => "Grabar",
            LocaleKey::ShortcutRecordingLabel => "Grabando…",
            LocaleKey::ShortcutRecordingHint => {
                "Presione modificadores + una tecla. Esc cancela, Retroceso borra."
            }
            LocaleKey::ShortcutClearButton => "Limpiar",
            LocaleKey::ShortcutEmptyPlaceholder => "No configurado",
            LocaleKey::NotificationTestSound => "Probar sonido",
            LocaleKey::NotificationTestSoundPlaying => "Reproduciendo…",

            // Tauri desktop shell — Display tab fields
            LocaleKey::TrayIconModeLabel => "Modo de ícono de bandeja",
            LocaleKey::TrayIconModeHelper => {
                "Ícono único combinado o un ícono por cada proveedor habilitado."
            }
            LocaleKey::TrayIconModeSingle => "Único",
            LocaleKey::TrayIconModePerProvider => "Por proveedor",
            LocaleKey::ShowProviderIcons => "Mostrar íconos de proveedores",
            LocaleKey::ShowProviderIconsHelper => {
                "Mostrar íconos de proveedores en el selector de bandeja."
            }
            LocaleKey::PreferHighestUsage => "Preferir uso más alto",
            LocaleKey::PreferHighestUsageHelper => {
                "Mostrar el proveedor más cercano a su límite en la pantalla de bandeja combinada."
            }
            LocaleKey::ShowPercentInTray => "Mostrar porcentaje en bandeja",
            LocaleKey::ShowPercentInTrayHelper => {
                "Reemplazar barra de uso con marca de proveedor + texto de porcentaje."
            }
            LocaleKey::DisplayModeLabel => "Modo de visualización",
            LocaleKey::DisplayModeHelper => {
                "Nivel de detalle mostrado en la etiqueta de barra de menú."
            }
            LocaleKey::DisplayModeDetailed => "Detallado",
            LocaleKey::DisplayModeCompact => "Compacto",
            LocaleKey::DisplayModeMinimal => "Mínimo",
            LocaleKey::WindowScaleLabel => "Escala de ventana",
            LocaleKey::WindowScaleHelper => {
                "Escala el contenido del panel emergente. La ventana sigue pudiendo redimensionarse libremente."
            }
            LocaleKey::WindowScaleAriaLabel => "Escala de ventana",
            LocaleKey::WindowMinimize => "Minimizar",
            LocaleKey::WindowMaximize => "Maximizar",
            LocaleKey::WindowRestore => "Restaurar",
            LocaleKey::WindowClose => "Cerrar",
            LocaleKey::ShowAsUsedLabel => "Mostrar como usado",
            LocaleKey::ShowAsUsedHelper => {
                "Mostrar barras de uso como consumido en lugar de restante."
            }
            LocaleKey::ShowAllTokenAccountsLabel => "Mostrar todas las cuentas de token",
            LocaleKey::ShowAllTokenAccountsHelper => {
                "Listar todas las cuentas de token en los menús de proveedores en lugar de colapsarlas."
            }
            LocaleKey::EnableAnimationsLabel => "Habilitar animaciones",
            LocaleKey::EnableAnimationsHelper => {
                "Transiciones suaves y barras de progreso animadas."
            }
            // Tauri desktop shell — Advanced tab fields
            LocaleKey::UpdateChannelStableOption => "Estable",
            LocaleKey::UpdateChannelBetaOption => "Beta",
            LocaleKey::AvoidKeychainPromptsLabel => "Evitar avisos de llavero (Claude)",
            LocaleKey::AvoidKeychainPromptsHelper => {
                "Omitir lecturas de credenciales de llavero para Claude para evitar diálogos de permiso del SO."
            }
            LocaleKey::DisableAllKeychainLabel => "Deshabilitar todo acceso al llavero",
            LocaleKey::DisableAllKeychainHelper => {
                "Desactivar lecturas de credenciales/llavero para todos los proveedores. También habilita la opción de Claude anterior."
            }

            // Tauri desktop shell — Theme (Phase 12)
            LocaleKey::SectionTheme => "Apariencia",
            LocaleKey::ThemeLabel => "Tema",
            LocaleKey::ThemeHelper => {
                "Auto sigue el esquema de color de su sistema. Claro y oscuro lo anulan."
            }
            LocaleKey::ThemeAutoOption => "Auto (sistema)",
            LocaleKey::ThemeLightOption => "Claro",
            LocaleKey::ThemeDarkOption => "Oscuro",

            // Tauri desktop shell — settings status / common
            LocaleKey::SettingsStatusSaving => "Guardando…",
            LocaleKey::ApiKeysTabHint => {
                "Configurar claves API para proveedores que usan autenticación basada en tokens. Las claves se almacenan localmente y nunca se transmiten."
            }

            // Tauri desktop shell — tray / popout
            LocaleKey::FetchingProviderData => "Obteniendo datos del proveedor…",
            LocaleKey::NoProvidersConfigured => "No hay proveedores configurados.",
            LocaleKey::EnableProvidersHint => {
                "Habilite proveedores en Configuración para ver datos de uso."
            }
            LocaleKey::OpenSettingsButton => "Abrir configuración",
            LocaleKey::TooltipRefresh => "Actualizar",
            LocaleKey::TooltipSettings => "Configuración",
            LocaleKey::TooltipPopOut => "Abrir panel",
            LocaleKey::TooltipBackToTray => "Volver a bandeja",
            LocaleKey::TrayCardErrorBadge => "Error",
            LocaleKey::SummaryProvidersLabel => "proveedores",
            LocaleKey::SummaryRefreshing => "actualizando…",
            LocaleKey::SummaryFailed => "fallido",
            LocaleKey::SummaryWithErrors => "con errores",

            // Tauri desktop shell — provider detail
            LocaleKey::DetailBackButton => "Volver",
            LocaleKey::DetailWindowPrimary => "Principal",
            LocaleKey::DetailWindowSecondary => "Secundario",
            LocaleKey::DetailWindowModelSpecific => "Específico del modelo",
            LocaleKey::DetailWindowTertiary => "Terciario",
            LocaleKey::DetailWindowMinutesSuffix => "ventana de m",
            LocaleKey::DetailWindowExhausted => "Agotado",
            LocaleKey::DetailPaceTitle => "Ritmo",
            LocaleKey::DetailPaceOnTrack => "En ritmo",
            LocaleKey::DetailPaceSlightlyAhead => "Ligeramente adelantado",
            LocaleKey::DetailPaceAhead => "Adelantado",
            LocaleKey::DetailPaceFarAhead => "Muy adelantado",
            LocaleKey::DetailPaceSlightlyBehind => "Ligeramente atrasado",
            LocaleKey::DetailPaceBehind => "Atrasado",
            LocaleKey::DetailPaceFarBehind => "Muy atrasado",
            LocaleKey::DetailPaceRunsOutIn => "Se agota en",
            LocaleKey::DetailPaceWillLastToReset => "Durará hasta el reinicio",
            LocaleKey::DetailCostTitle => "Costo",
            LocaleKey::DetailCostUsed => "Usado",
            LocaleKey::DetailCostLimit => "Límite",
            LocaleKey::DetailCostRemaining => "Restante",
            LocaleKey::DetailCostResets => "Reinicia",
            LocaleKey::DetailChartCost => "Costo (30 días)",
            LocaleKey::DetailChartCredits => "Créditos usados (30 días)",
            LocaleKey::DetailChartUsageBreakdown => "Uso por servicio (30 días)",
            LocaleKey::DetailChartEmpty => "Sin datos de gráfico aún.",
            LocaleKey::DetailUpdatedPrefix => "Actualizado",

            // Tauri desktop shell — update banner
            LocaleKey::BannerCheckingForUpdates => "Buscando actualizaciones…",
            LocaleKey::BannerUpdateAvailablePrefix => "Actualización",
            LocaleKey::BannerDownloadButton => "Descargar",
            LocaleKey::BannerViewRelease => "Ver versión",
            LocaleKey::BannerDismiss => "Descartar",
            LocaleKey::BannerDownloadingPrefix => "Descargando actualización",
            LocaleKey::BannerReadyToInstallSuffix => "lista para instalar",
            LocaleKey::BannerInstallRestart => "Instalar y reiniciar",
            LocaleKey::BannerUpdateFailedPrefix => "Actualización fallida",
            LocaleKey::BannerRetry => "Reintentar",

            // Tauri desktop shell — providers sidebar (Phase 6a)
            LocaleKey::ProviderSidebarSearch => "Buscar",
            LocaleKey::ProviderSidebarClearSearch => "Limpiar búsqueda de proveedores",
            LocaleKey::ProviderSidebarNoMatches => "Sin proveedores coincidentes",
            LocaleKey::ProviderSidebarReorderHint => "Arrastrar para reordenar",
            LocaleKey::ProviderSidebarMoveUp => "Subir",
            LocaleKey::ProviderSidebarMoveDown => "Bajar",
            LocaleKey::ProviderStatusOk => "Actualizado",
            LocaleKey::ProviderStatusStale => "Desactualizado",
            LocaleKey::ProviderStatusError => "Error",
            LocaleKey::ProviderStatusLoading => "Cargando",
            LocaleKey::ProviderStatusDisabled => "Deshabilitado",
            LocaleKey::ProviderDetailPlaceholder => "Panel de detalle llegando en Fase 6b",
            LocaleKey::ProviderIssueNeedsSignIn => "Requiere inicio de sesión",
            LocaleKey::ProviderIssueFetchNeedsAttention => {
                "La consulta del proveedor necesita atención"
            }
            LocaleKey::ProviderIssueCopy => "Copiar",
            LocaleKey::ProviderIssueUnsupportedSourceModePrefix => {
                "Modo de origen no soportado para este proveedor"
            }
            LocaleKey::CredentialStorageTitle => "Almacenamiento de credenciales",
            LocaleKey::CredentialRevokeStored => "Revocar credenciales almacenadas",
            LocaleKey::CredentialApiKeys => "Claves API",
            LocaleKey::CredentialManualCookies => "Cookies manuales",
            LocaleKey::CredentialTokenAccounts => "Cuentas de token",
            LocaleKey::CredentialProtectedPrefix => "Protegido",
            LocaleKey::CredentialStatusNotCreated => "No creada",
            LocaleKey::CredentialStatusPlaintext => "Texto plano",
            LocaleKey::CredentialStatusUnavailable => "No disponible",
            LocaleKey::CredentialStatusUnreadable => "No legible",
            LocaleKey::BrowserCookiesSectionTitle => "Cookies del navegador",
            LocaleKey::BrowserCookieNoneSaved => "Sin cookies guardadas.",
            LocaleKey::BrowserCookieSavedBadge => "Guardada",
            LocaleKey::BrowserCookieRemove => "Eliminar",
            LocaleKey::BrowserCookieImportSuccess => "Cookies importadas correctamente.",
            LocaleKey::BrowserCookieImportFromBrowser => "Importar desde navegador",
            LocaleKey::BrowserCookieProfileSingular => "perfil",
            LocaleKey::BrowserCookieProfilePlural => "perfiles",
            LocaleKey::BrowserCookiePlaceholderDefault => "Pegar valor de cabecera de cookie...",
            LocaleKey::BrowserCookiePlaceholderOllama => {
                "Pegar la cabecera Cookie completa o solo el valor __Secure-session..."
            }
            LocaleKey::BrowserCookiePlaceholderCurl => {
                "Pegar la cabecera Cookie o la solicitud cURL completa del navegador..."
            }
            LocaleKey::BrowserCookieSave => "Guardar cookie",

            // Phase 6d — credential detection
            LocaleKey::CredentialsSectionTitle => "Credenciales",
            LocaleKey::CredsStatusAuthenticated => "Autenticado",
            LocaleKey::CredsStatusNotSignedIn => "Sin iniciar sesión",
            LocaleKey::CredsStatusDetected => "Detectado",
            LocaleKey::CredsStatusNotDetected => "No detectado",
            LocaleKey::CredsStatusAvailable => "Disponible",
            LocaleKey::CredsStatusUnavailable => "No disponible",
            LocaleKey::CredsOpenFolderAction => "Abrir carpeta de credenciales",
            LocaleKey::CredsRefreshDetectionAction => "Actualizar detección",
            LocaleKey::CredsSavePathAction => "Guardar ruta",
            LocaleKey::CredsBrowseAction => "Explorar…",
            LocaleKey::CredsGeminiCliLabel => "Gemini CLI",
            LocaleKey::CredsGeminiCliHelperPrefix => "Usa credenciales OAuth de",
            LocaleKey::CredsGeminiCliSetupAction => "Configurar Gemini CLI",
            LocaleKey::CredsGeminiCliSetupHelp => {
                "Instale Gemini CLI y ejecute `gemini auth login` para iniciar sesión."
            }
            LocaleKey::CredsVertexAiLabel => "Google Cloud",
            LocaleKey::CredsVertexAiHelperPrefix => "Usa credenciales de Google Cloud de",
            LocaleKey::CredsVertexAiSetupAction => "Configurar autenticación de Google Cloud",
            LocaleKey::CredsVertexAiSetupHelp => {
                "Ejecute `gcloud auth application-default login` para crear credenciales."
            }
            LocaleKey::CredsJetBrainsLabel => "JetBrains IDE",
            LocaleKey::CredsJetBrainsHelperDetectedPrefix => {
                "Usando configuración de IDE detectada en"
            }
            LocaleKey::CredsJetBrainsHelperCustomPrefix => "Usando ruta base de IDE personalizada",
            LocaleKey::CredsJetBrainsHelperMissing => {
                "Instale un IDE de JetBrains con AI Assistant habilitado, luego actualice CodexBar."
            }
            LocaleKey::CredsJetBrainsCustomPathLabel => "Ruta personalizada",
            LocaleKey::CredsJetBrainsCustomPathPlaceholder => "%APPDATA%/JetBrains/IntelliJIdea...",
            LocaleKey::CredsJetBrainsSelectLabel => "Seleccione el IDE de JetBrains a monitorear.",
            LocaleKey::CredsJetBrainsAutoDetectOption => "Detectar automáticamente",
            LocaleKey::CredsKiroLabel => "Kiro CLI",
            LocaleKey::CredsKiroHelperAvailablePrefix => "Detectado en",
            LocaleKey::CredsKiroHelperMissing => {
                "kiro-cli: no se encontró en PATH ni en ubicaciones de instalación conocidas."
            }
            LocaleKey::CredsOpenAiHistoryHelp => {
                "Habilitar seguimiento histórico para ver el uso a lo largo del tiempo."
            }

            // Tauri desktop shell — Token accounts (Phase 6e, review)
            LocaleKey::TokenAccountActive => "Activo",
            LocaleKey::TokenAccountSetActive => "Establecer activo",
            LocaleKey::TokenAccountRemove => "Eliminar",
            LocaleKey::TokenAccountAddButton => "Agregar cuenta",
            LocaleKey::TokenAccountGithubLoginButton => "Iniciar sesión con GitHub",
            LocaleKey::TokenAccountEmpty => "No hay cuentas guardadas para este proveedor.",
            LocaleKey::TokenAccountLabelPlaceholder => "Etiqueta (p. ej. Trabajo, Personal)…",
            LocaleKey::TokenAccountProviderLabel => "Proveedor",
            LocaleKey::TokenAccountProviderPlaceholder => "Seleccionar proveedor…",
            LocaleKey::TokenAccountAddedPrefix => "Agregada",
            LocaleKey::TokenAccountUsedPrefix => "Usada",
            LocaleKey::TokenAccountTabHint => {
                "Administrar múltiples tokens de sesión o tokens API por proveedor. La cuenta activa se usa para todas las consultas. Solo los proveedores que requieren tokens manuales aparecen aquí."
            }
            LocaleKey::TokenAccountNoSupported => {
                "Actualmente ningún proveedor admite cuentas de token."
            }
            LocaleKey::TokenAccountInlineSummary => "Cuentas de token",

            // Phase 9 - Tray / pop-out pace badges + countdowns
            LocaleKey::TrayPaceBadgeSlow => "Lento",
            LocaleKey::TrayPaceBadgeSteady => "Constante",
            LocaleKey::TrayPaceBadgeRacing => "Acelerado",
            LocaleKey::TrayPaceBadgeBurning => "Crítico",
            LocaleKey::TrayResetsInLabel => "Reinicia en {}",
            LocaleKey::TrayResetsDueNow => "Reiniciando…",
        }
    }
}
