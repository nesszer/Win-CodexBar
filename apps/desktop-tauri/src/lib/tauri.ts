import { invoke } from "@tauri-apps/api/core";
import type {
  ApiKeyInfoBridge,
  ApiKeyProviderInfoBridge,
  AppInfoBridge,
  BootstrapState,
  CurrentSurfaceState,
  ProofCommand,
  ProofStatePayload,
  CookieInfoBridge,
  DetectedBrowserBridge,
  Language,
  LocaleStrings,
  ProviderCatalogEntry,
  ProviderChartData,
  ProviderDetail,
  ProviderSummary,
  ProviderUsageSnapshot,
  ProviderTokenAccountsBridge,
  TokenAccountSupportBridge,
  SettingsSnapshot,
  SettingsUpdate,
  SurfaceMode,
  SurfaceTargetForMode,
  VisibleSurfaceMode,
  UpdateStatePayload,
  CookieSourceOption,
  RegionOption,
  SafeDiagnostics,
  CredentialStorageStatus,
  WorkAreaRect,
} from "../types/bridge";

export function getBootstrapState(): Promise<BootstrapState> {
  return invoke<BootstrapState>("get_bootstrap_state");
}

export function getProviderCatalog(): Promise<ProviderCatalogEntry[]> {
  return invoke<ProviderCatalogEntry[]>("get_provider_catalog");
}

export function reorderProviders(ids: string[]): Promise<ProviderSummary[]> {
  return invoke<ProviderSummary[]>("reorder_providers", { ids });
}

export function getSettingsSnapshot(): Promise<SettingsSnapshot> {
  return invoke<SettingsSnapshot>("get_settings_snapshot");
}

export function updateSettings(
  patch: SettingsUpdate,
): Promise<SettingsSnapshot> {
  return invoke<SettingsSnapshot>("update_settings", { patch });
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

export function openSettingsWindow(tab: string): Promise<void> {
  return invoke<void>("open_settings_window", { tab });
}

export function closeSettingsWindow(): Promise<void> {
  return invoke<void>("close_settings_window");
}

export function getCurrentSurfaceMode(): Promise<SurfaceMode> {
  return invoke<SurfaceMode>("get_current_surface_mode");
}

export function getCurrentSurfaceState(): Promise<CurrentSurfaceState> {
  return invoke<CurrentSurfaceState>("get_current_surface_state");
}

export function getProofState(): Promise<ProofStatePayload> {
  return invoke<ProofStatePayload>("get_proof_state");
}

export function runProofCommand(command: ProofCommand): Promise<ProofStatePayload> {
  return invoke<ProofStatePayload>("run_proof_command", { command });
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

export function getSafeDiagnostics(): Promise<SafeDiagnostics> {
  return invoke<SafeDiagnostics>("get_safe_diagnostics");
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

export function listDetectedBrowsers(): Promise<DetectedBrowserBridge[]> {
  return invoke<DetectedBrowserBridge[]>("list_detected_browsers");
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

// ── Token account bridge ─────────────────────────────────────────────

export function getTokenAccountProviders(): Promise<TokenAccountSupportBridge[]> {
  return invoke<TokenAccountSupportBridge[]>("get_token_account_providers");
}

export function getTokenAccounts(
  providerId: string,
): Promise<ProviderTokenAccountsBridge> {
  return invoke<ProviderTokenAccountsBridge>("get_token_accounts", { providerId });
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

export function playNotificationSound(): Promise<void> {
  return invoke<void>("play_notification_sound");
}

export function reanchorTrayPanel(): Promise<void> {
  return invoke<void>("reanchor_tray_panel");
}

export function revealTrayPanelWindow(): Promise<void> {
  return invoke<void>("reveal_tray_panel_window");
}

export function quitApp(): Promise<void> {
  return invoke<void>("quit_app");
}
