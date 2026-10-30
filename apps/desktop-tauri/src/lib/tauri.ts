import { invoke } from "@tauri-apps/api/core";
import type {
  HooksConfigSave,
  HooksConfigSnapshot,
  HttpServerConfig,
  HttpServerSnapshot,
  ClaudeAccount,
  ClaudeReconciliationSnapshot,
  GrokAccount,
  GrokAccountUsage,
  ClaudeSwapAccountsState,
  ApiKeyInfoBridge,
  ApiKeyProviderInfoBridge,
  AppInfoBridge,
  BootstrapState,
  CurrentSurfaceState,
  CookieInfoBridge,
  DetectedBrowserBridge,
  Language,
  LocaleStrings,
  NotificationSoundEvent,
  ProviderCatalogEntry,
  ProviderStatusEntry,
  ProviderChartData,
  ProviderDetail,
  ProviderLocalUsageSummary,
  ProviderSummary,
  ProviderUsageSnapshot,
  ProviderTokenAccountsBridge,
  ProviderAccountUsageBridge,
  ClaudeSwapOverviewAccountsBridge,
  TokenAccountSupportBridge,
  SettingsSnapshot,
  SettingsUpdate,
  SurfaceMode,
  SurfaceTargetForMode,
  VisibleSurfaceMode,
  UpdateStatePayload,
  CookieSourceOption,
  RegionOption,
  CredentialStorageStatus,
  WorkAreaRect,
  AgentSession,
  AgentSessionDiscoveryResult,
  SessionFocusResult,
  TrayVisibilityStatusDto,
  UsageSpendSummary,
  CodexSessionDetailCheckpoint,
  CodexSessionDetails,
  CodexSessionToolActivity,
  CodexAccount,
  CodexAccountUsageSnapshot,
  CodexAccountsStateBridge,
  CodexSwitchResult,
  DeepSeekPricingStatus,
  CurrencyRatesSnapshot,
  OrgRateLimitsSnapshot,
  ProviderThroughputSnapshot,
} from "../types/bridge";

export const claudeAccountsList = () => invoke<ClaudeAccount[]>("get_claude_accounts_state");
export const claudeReconciliationState = () =>
  invoke<ClaudeReconciliationSnapshot | null>("claude_reconciliation_state");
export const claudeAccountAdd = () => invoke<void>("claude_account_add");
export const claudeAccountReauthenticate = (id: string) => invoke<void>("claude_account_add", { id });
export const claudeAccountCancelLogin = () => invoke<void>("claude_account_cancel_login");
export const claudeAccountSaveCurrent = () => invoke<void>("claude_account_save_current");
export const claudeAccountRemove = (id: string) => invoke<void>("claude_account_remove", { id });
export const claudeAccountSwitch = (id: string) =>
  invoke<ClaudeReconciliationSnapshot>("claude_account_switch", { id });
export const grokAccountsList = () => invoke<GrokAccount[]>("grok_accounts_list");
export const grokAccountAdd = () => invoke<void>("grok_account_add");
export const grokAccountReauthenticate = (id: string) => invoke<void>("grok_account_reauthenticate", { id });
export const grokAccountCancelLogin = () => invoke<void>("grok_account_cancel_login");
export const grokAccountSaveCurrent = () => invoke<void>("grok_account_save_current");
export const grokAccountRemove = (id: string) => invoke<void>("grok_account_remove", { id });
export const grokAccountSwitch = (id: string) => invoke<void>("grok_account_switch", { id });
export const grokAccountFetch = (id: string) =>
  invoke<GrokAccountUsage>("grok_account_fetch", { id });
export const claudeSwapAccountsList = () =>
  invoke<ClaudeSwapAccountsState>("claude_swap_accounts_list");
export const claudeSwapAccountSwitch = (slot: number) =>
  invoke<ClaudeReconciliationSnapshot>("claude_swap_account_switch", { slot });
export const claudeSwapAccountReauthenticate = (slot: number) =>
  invoke<ClaudeReconciliationSnapshot>("claude_swap_account_reauthenticate", { slot });

export function getBootstrapState(): Promise<BootstrapState> {
  return invoke<BootstrapState>("get_bootstrap_state");
}

export function getProviderCatalog(): Promise<ProviderCatalogEntry[]> {
  return invoke<ProviderCatalogEntry[]>("get_provider_catalog");
}

export function getProviderStatuses(): Promise<ProviderStatusEntry[]> {
  return invoke<ProviderStatusEntry[]>("get_provider_statuses");
}

export function reorderProviders(ids: string[]): Promise<ProviderSummary[]> {
  return invoke<ProviderSummary[]>("reorder_providers", { ids });
}

/** Settings sidebar reorder: `ids` is the full listed order; hidden providers keep their slots. */
export function reorderVisibleProviders(ids: string[]): Promise<ProviderSummary[]> {
  return invoke<ProviderSummary[]>("reorder_visible_providers", { ids });
}

/** Restore the built-in order for the providers the Settings sidebar lists. */
export function restoreDefaultProviderOrder(): Promise<ProviderSummary[]> {
  return invoke<ProviderSummary[]>("restore_default_provider_order");
}

export function getSettingsSnapshot(): Promise<SettingsSnapshot> {
  return invoke<SettingsSnapshot>("get_settings_snapshot");
}

export function getCurrencyRates(preferredCurrencyCode: string): Promise<CurrencyRatesSnapshot> {
  return invoke<CurrencyRatesSnapshot>("get_currency_rates", { preferredCurrencyCode });
}

export function getStayAwakeStatus(): Promise<boolean> {
  return invoke<boolean>("get_stay_awake_status");
}

export function getHttpServerState(): Promise<HttpServerSnapshot> {
  return invoke<HttpServerSnapshot>("get_http_server_state");
}

export function applyHttpServerConfig(config: HttpServerConfig): Promise<HttpServerSnapshot> {
  return invoke<HttpServerSnapshot>("apply_http_server_config", { config });
}

export function getHooksConfig(): Promise<HooksConfigSnapshot> {
  return invoke<HooksConfigSnapshot>("get_hooks_config");
}

export function saveHooksConfig(request: HooksConfigSave): Promise<HooksConfigSnapshot> {
  return invoke<HooksConfigSnapshot>("save_hooks_config", { request });
}

export function updateSettings(
  patch: SettingsUpdate,
): Promise<SettingsSnapshot> {
  return invoke<SettingsSnapshot>("update_settings", { patch });
}

export function getTrayVisibilityStatus(): Promise<TrayVisibilityStatusDto> {
  return invoke<TrayVisibilityStatusDto>("tray_visibility_status");
}

export function listAgentSessions(): Promise<AgentSessionDiscoveryResult> {
  return invoke<AgentSessionDiscoveryResult>("list_agent_sessions");
}

export function focusAgentSession(
  session: AgentSession,
): Promise<SessionFocusResult> {
  return invoke<SessionFocusResult>("focus_agent_session", { session });
}

export function setSurfaceMode<M extends VisibleSurfaceMode>(
  mode: M,
  target: SurfaceTargetForMode<M>,
): Promise<SurfaceMode> {
  return invoke<SurfaceMode>("set_surface_mode", { mode, target });
}

export function dismissTrayPanel(): Promise<void> {
  return invoke<void>("dismiss_tray_panel");
}

/** Suppress flyout blur-dismiss while a resize/drag gesture is in flight. */
export function beginFlyoutGesture(): Promise<void> {
  return invoke<void>("begin_flyout_gesture");
}
export function endFlyoutGesture(): Promise<void> {
  return invoke<void>("end_flyout_gesture");
}

export function openSettingsWindow(tab: string): Promise<void> {
  return invoke<void>("open_settings_window", { tab });
}

/** Open (or focus) the detached flyout ("Pop Out Dashboard") window. */
export function openFlyoutWindow(): Promise<void> {
  return invoke<void>("open_flyout_window");
}

export function closeSettingsWindow(): Promise<void> {
  return invoke<void>("close_settings_window");
}

export function getCurrentSurfaceState(): Promise<CurrentSurfaceState> {
  return invoke<CurrentSurfaceState>("get_current_surface_state");
}

export function refreshProviders(): Promise<void> {
  return invoke<void>("refresh_providers");
}

export function refreshProvidersIfStale(): Promise<void> {
  return invoke<void>("refresh_providers_if_stale");
}

export function getCachedProviders(): Promise<ProviderUsageSnapshot[]> {
  return invoke<ProviderUsageSnapshot[]>("get_cached_providers");
}

export function getDeepSeekPricingStatus(): Promise<DeepSeekPricingStatus | null> {
  return invoke<DeepSeekPricingStatus | null>("get_deepseek_pricing_status");
}

export function getWorkAreaRect(): Promise<WorkAreaRect> {
  return invoke<WorkAreaRect>("get_work_area_rect");
}

export function getCredentialStorageStatus(): Promise<CredentialStorageStatus> {
  return invoke<CredentialStorageStatus>("get_credential_storage_status");
}

export function getUpdateState(): Promise<UpdateStatePayload> {
  return invoke<UpdateStatePayload>("get_update_state");
}

export function checkForUpdates(): Promise<UpdateStatePayload> {
  return invoke<UpdateStatePayload>("check_for_updates");
}

export function downloadUpdate(): Promise<UpdateStatePayload> {
  return invoke<UpdateStatePayload>("download_update");
}

export function applyUpdate(): Promise<void> {
  return invoke<void>("apply_update");
}

export function dismissUpdate(): Promise<UpdateStatePayload> {
  return invoke<UpdateStatePayload>("dismiss_update");
}

export function openReleasePage(): Promise<void> {
  return invoke<void>("open_release_page");
}

export function openExternalUrl(url: string): Promise<void> {
  return invoke<void>("open_external_url", { url });
}

// ── Credential store bridge ──────────────────────────────────────────

export function getApiKeys(): Promise<ApiKeyInfoBridge[]> {
  return invoke<ApiKeyInfoBridge[]>("get_api_keys");
}

export function getApiKeyProviders(): Promise<ApiKeyProviderInfoBridge[]> {
  return invoke<ApiKeyProviderInfoBridge[]>("get_api_key_providers");
}

export function setApiKey(
  providerId: string,
  apiKey: string,
  label?: string,
): Promise<ApiKeyInfoBridge[]> {
  return invoke<ApiKeyInfoBridge[]>("set_api_key", {
    providerId,
    apiKey,
    label: label ?? null,
  });
}

export function removeApiKey(providerId: string): Promise<ApiKeyInfoBridge[]> {
  return invoke<ApiKeyInfoBridge[]>("remove_api_key", { providerId });
}

export function hasOpenRouterManagementApiKey(): Promise<boolean> {
  return invoke<boolean>("has_openrouter_management_api_key");
}

export function setOpenRouterManagementApiKey(apiKey: string): Promise<void> {
  return invoke<void>("set_openrouter_management_api_key", { apiKey });
}

export function removeOpenRouterManagementApiKey(): Promise<void> {
  return invoke<void>("remove_openrouter_management_api_key");
}

export function hasVercelAccessToken(): Promise<boolean> {
  return invoke<boolean>("has_vercel_access_token");
}

export function setVercelAccessToken(accessToken: string): Promise<void> {
  return invoke<void>("set_vercel_access_token", { accessToken });
}

export function removeVercelAccessToken(): Promise<void> {
  return invoke<void>("remove_vercel_access_token");
}

export function getManualCookies(): Promise<CookieInfoBridge[]> {
  return invoke<CookieInfoBridge[]>("get_manual_cookies");
}

export function setManualCookie(
  providerId: string,
  cookieHeader: string,
): Promise<CookieInfoBridge[]> {
  return invoke<CookieInfoBridge[]>("set_manual_cookie", {
    providerId,
    cookieHeader,
  });
}

export function removeManualCookie(
  providerId: string,
): Promise<CookieInfoBridge[]> {
  return invoke<CookieInfoBridge[]>("remove_manual_cookie", { providerId });
}

/** With `providerId`, also lists a desktop app store only that provider may use. */
export function listDetectedBrowsers(
  providerId?: string,
): Promise<DetectedBrowserBridge[]> {
  return invoke<DetectedBrowserBridge[]>("list_detected_browsers", {
    providerId: providerId ?? null,
  });
}

export function importBrowserCookies(
  providerId: string,
  browserType: string,
): Promise<CookieInfoBridge[]> {
  return invoke<CookieInfoBridge[]>("import_browser_cookies", {
    providerId,
    browserType,
  });
}

export function getAppInfo(): Promise<AppInfoBridge> {
  return invoke<AppInfoBridge>("get_app_info");
}

export function getProviderChartData(
  providerId: string,
  accountEmail?: string,
): Promise<ProviderChartData> {
  return invoke<ProviderChartData>("get_provider_chart_data", { providerId, accountEmail });
}

export function getProviderLocalUsageSummary(
  providerId: string,
): Promise<ProviderLocalUsageSummary | null> {
  return invoke<ProviderLocalUsageSummary | null>("get_provider_local_usage_summary", { providerId });
}

/** `period` is a raw reporting period; omitted means the saved `costReportingPeriod`. */
export function getUsageSpendSummary(options?: { period?: string; forceRefresh?: boolean }): Promise<UsageSpendSummary> {
  return invoke<UsageSpendSummary>("get_usage_spend_summary", {
    period: options?.period ?? null,
    forceRefresh: options?.forceRefresh ?? null,
  });
}

/** Native tool operations of one Codex conversation, read on demand. */
export function getCodexSessionToolActivity(
  sessionId: string,
  historyDays?: number,
): Promise<CodexSessionToolActivity> {
  return invoke<CodexSessionToolActivity>("get_codex_session_tool_activity", {
    sessionId,
    historyDays: historyDays ?? null,
  });
}

/**
 * Request-level usage of one Codex conversation, read on demand. Rejects with a
 * `CodexSessionDetailError` code; `sourceChanged` means the logs moved past
 * the summary in `checkpoint`.
 */
export function getCodexSessionDetails(
  sessionId: string,
  checkpoint: CodexSessionDetailCheckpoint,
  historyDays?: number,
): Promise<CodexSessionDetails> {
  return invoke<CodexSessionDetails>("get_codex_session_details", {
    sessionId,
    historyDays: historyDays ?? null,
    checkpoint,
  });
}

export function writeUsageSpendExport(path: string, payload: string): Promise<void> {
  return invoke<void>("write_usage_spend_export", { path, payload });
}

/** Write portable preferences to `path`; resolves with how many were exported. */
export function exportPreferences(path: string): Promise<number> {
  return invoke<number>("export_preferences", { path });
}

/** Apply the preferences file at `path`; the shell validates before saving anything. */
export function importPreferences(path: string): Promise<SettingsSnapshot> {
  return invoke<SettingsSnapshot>("import_preferences", { path });
}

// ── Token account bridge ─────────────────────────────────────────────

export function getTokenAccountProviders(): Promise<TokenAccountSupportBridge[]> {
  return invoke<TokenAccountSupportBridge[]>("get_token_account_providers");
}

export function getTokenAccounts(
  providerId: string,
): Promise<ProviderTokenAccountsBridge> {
  return invoke<ProviderTokenAccountsBridge>("get_token_accounts", { providerId });
}

/** Last successful usage per saved token account. Reads the cache only; never fetches. */
export function getProviderAccountUsage(providerId: string): Promise<ProviderAccountUsageBridge> {
  return invoke<ProviderAccountUsageBridge>("get_provider_account_usage", { providerId });
}

/** claude-swap/tcr pool accounts and their last usage. Reads the cache only; never fetches. */
export function getClaudeSwapOverviewAccounts(): Promise<ClaudeSwapOverviewAccountsBridge> {
  return invoke<ClaudeSwapOverviewAccountsBridge>("get_claude_swap_overview_accounts");
}

export function addTokenAccount(
  providerId: string,
  label: string,
  token: string,
): Promise<ProviderTokenAccountsBridge> {
  return invoke<ProviderTokenAccountsBridge>("add_token_account", {
    providerId,
    label,
    token,
  });
}

/**
 * Edit a saved account in place. A blank `label` keeps the saved label; a
 * `null` or blank `token` keeps the saved credential, which is never returned.
 */
export function updateTokenAccount(
  providerId: string,
  accountId: string,
  label: string,
  token: string | null,
): Promise<ProviderTokenAccountsBridge> {
  return invoke<ProviderTokenAccountsBridge>("update_token_account", {
    providerId,
    accountId,
    label,
    token,
  });
}

/** Move a saved account to `index` in the list; the active account does not change. */
export function moveTokenAccount(
  providerId: string,
  accountId: string,
  index: number,
): Promise<ProviderTokenAccountsBridge> {
  return invoke<ProviderTokenAccountsBridge>("move_token_account", {
    providerId,
    accountId,
    index,
  });
}

export function removeTokenAccount(
  providerId: string,
  accountId: string,
): Promise<ProviderTokenAccountsBridge> {
  return invoke<ProviderTokenAccountsBridge>("remove_token_account", {
    providerId,
    accountId,
  });
}

export function setActiveTokenAccount(
  providerId: string,
  accountId: string,
): Promise<ProviderTokenAccountsBridge> {
  return invoke<ProviderTokenAccountsBridge>("set_active_token_account", {
    providerId,
    accountId,
  });
}

export function setTokenAccountClaudeConfigDir(
  providerId: string,
  accountId: string,
  directory: string | null,
): Promise<ProviderTokenAccountsBridge> {
  return invoke<ProviderTokenAccountsBridge>("set_token_account_claude_config_dir", {
    providerId,
    accountId,
    directory,
  });
}
// ── Phase 5 — i18n ────────────────────────────────────────────────────

export function getLocaleStrings(
  language?: Language | null,
): Promise<LocaleStrings> {
  return invoke<LocaleStrings>("get_locale_strings", {
    language: language ?? null,
  });
}

export function setUiLanguage(language: Language): Promise<void> {
  return invoke<void>("set_ui_language", { language });
}

// ── Phase 6b — provider detail pane ──────────────────────────────────

export function getProviderDetail(providerId: string): Promise<ProviderDetail> {
  return invoke<ProviderDetail>("get_provider_detail", { providerId });
}

export function openProviderDashboard(providerId: string): Promise<void> {
  return invoke<void>("open_provider_dashboard", { providerId });
}

export function openProviderStatusPage(providerId: string): Promise<void> {
  return invoke<void>("open_provider_status_page", { providerId });
}

export function triggerProviderLogin(providerId: string): Promise<void> {
  return invoke<void>("trigger_provider_login", { providerId });
}

export function revokeProviderCredentials(providerId: string): Promise<void> {
  return invoke<void>("revoke_provider_credentials", { providerId });
}

// ── Phase 6c — cookie source & region pickers ────────────────────────

export function getProviderCookieSourceOptions(
  providerId: string,
): Promise<CookieSourceOption[]> {
  return invoke<CookieSourceOption[]>("get_provider_cookie_source_options", {
    providerId,
  });
}

export function getProviderRegionOptions(providerId: string): Promise<RegionOption[]> {
  return invoke<RegionOption[]>("get_provider_region_options", { providerId });
}

export function setProviderUsageSource(providerId: string, source: string): Promise<void> {
  return invoke<void>("set_provider_usage_source", { providerId, source });
}

export function setProviderOptionalDetails(
  providerId: string,
  enabled: boolean,
): Promise<void> {
  return invoke<void>("set_provider_optional_details", { providerId, enabled });
}

/** On-demand Tokens/min window for OpenAI API Live Usage; 15, 60 or 360 minutes. */
export function getOpenAiLiveUsage(windowMinutes: 15 | 60 | 360): Promise<ProviderThroughputSnapshot> {
  return invoke<ProviderThroughputSnapshot>("get_openai_live_usage", { windowMinutes });
}

/** Organization project caps; the caller skips this when a project scope is saved. */
export function getOpenAiOrgRateLimits(): Promise<OrgRateLimitsSnapshot> {
  return invoke<OrgRateLimitsSnapshot>("get_openai_org_rate_limits");
}

export function setProviderAutoResumeAfterQuotaReset(
  providerId: string,
  enabled: boolean,
): Promise<void> {
  return invoke<void>("set_provider_auto_resume_after_quota_reset", {
    providerId,
    enabled,
  });
}

export function setProviderCookieSource(providerId: string, source: string): Promise<void> {
  return invoke<void>("set_provider_cookie_source", { providerId, source });
}

export function setProviderRegion(providerId: string, region: string): Promise<void> {
  return invoke<void>("set_provider_region", { providerId, region });
}

export function getProviderWorkspaceId(providerId: string): Promise<string | null> {
  return invoke<string | null>("get_provider_workspace_id", { providerId });
}

export function setProviderWorkspaceId(
  providerId: string,
  workspaceId: string,
): Promise<void> {
  return invoke<void>("set_provider_workspace_id", { providerId, workspaceId });
}

export function setProviderGatewayUrl(
  providerId: string,
  gatewayUrl: string,
): Promise<void> {
  return invoke<void>("set_provider_gateway_url", { providerId, gatewayUrl });
}

export function getProviderGatewayUrl(providerId: string): Promise<string> {
  return invoke<string>("get_provider_gateway_url", { providerId });
}

export function getProviderAzureApiVersion(providerId: string): Promise<string | null> {
  return invoke<string | null>("get_provider_azure_api_version", { providerId });
}

export function setProviderAzureApiVersion(
  providerId: string,
  apiVersion: string,
): Promise<void> {
  return invoke<void>("set_provider_azure_api_version", { providerId, apiVersion });
}

// ── Phase 6d — credential detection ──────────────────────────────────

export function openPath(path: string): Promise<void> {
  return invoke<void>("open_path", { path });
}

export function getGeminiCliSignedIn(): Promise<
  import("../types/bridge").GeminiCliStatus
> {
  return invoke("get_gemini_cli_signed_in");
}

export function getVertexAiStatus(): Promise<
  import("../types/bridge").VertexAiStatus
> {
  return invoke("get_vertexai_status");
}

export function listJetbrainsDetectedIdes(): Promise<
  import("../types/bridge").JetbrainsIde[]
> {
  return invoke("list_jetbrains_detected_ides");
}

export function setJetbrainsIdePath(path: string): Promise<void> {
  return invoke<void>("set_jetbrains_ide_path", { path });
}

export function getKiroStatus(): Promise<
  import("../types/bridge").KiroStatus
> {
  return invoke("get_kiro_status");
}

// ── Phase 7 — global shortcut capture + notification preview ──────────

export function registerGlobalShortcut(accelerator: string): Promise<void> {
  return invoke<void>("register_global_shortcut", { accelerator });
}

export function unregisterGlobalShortcut(): Promise<void> {
  return invoke<void>("unregister_global_shortcut");
}

export function playNotificationSound(event: NotificationSoundEvent): Promise<void> {
  return invoke<void>("play_notification_sound", { event });
}

export function reanchorTrayPanel(): Promise<void> {
  return invoke<void>("reanchor_tray_panel");
}

export function revealTrayPanelWindow(): Promise<void> {
  return invoke<void>("reveal_tray_panel_window");
}

export function getSystemAccentColor(): Promise<string | null> {
  return invoke<string | null>("get_system_accent_color");
}

/** The Windows app-theme setting; the native windows are pinned Dark, so the webview cannot read it. */
export function getSystemTheme(): Promise<"light" | "dark"> {
  return invoke<"light" | "dark">("get_system_theme");
}

export function quitApp(): Promise<void> {
  return invoke<void>("quit_app");
}

// ── Codex multi-account (ADR 0003) ───────────────────────────────────

export function codexAccountAdd(): Promise<CodexAccount> {
  return invoke<CodexAccount>("codex_account_add");
}

export function codexAccountReauthenticate(id?: string): Promise<CodexAccount> {
  return invoke<CodexAccount>("codex_account_reauthenticate", id ? { id } : undefined);
}

export function codexAccountRemove(id: string): Promise<void> {
  return invoke<void>("codex_account_remove", { id });
}

export function codexAccountSwitch(id: string): Promise<CodexSwitchResult> {
  return invoke<CodexSwitchResult>("codex_account_switch", { id });
}

export function codexAccountFetch(
  id: string,
): Promise<CodexAccountUsageSnapshot> {
  return invoke<CodexAccountUsageSnapshot>("codex_account_fetch", { id });
}

export function codexAccountRestartDesktop(
  switchId: string,
): Promise<void> {
  return invoke<void>("codex_account_restart_desktop", {
    switchId,
  });
}

export function getCodexAccountsState(): Promise<CodexAccountsStateBridge> {
  return invoke<CodexAccountsStateBridge>("get_codex_accounts_state");
}

export function getSafeDiagnostics(): Promise<string> {
  return invoke<string>("get_safe_diagnostics");
}
