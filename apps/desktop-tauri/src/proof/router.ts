import { emit } from "@tauri-apps/api/event";
import { version as appVersion } from "../../package.json";
import type {
  BootstrapState,
  CurrentSurfaceState,
  SettingsSnapshot,
  SettingsUpdate,
} from "../types/bridge";
import {
  parseSettingsTabRequest,
  providersTabRequest,
} from "../surfaces/settings/settingsTabs";
import type { ProofScenario } from "./fixtures/scenarios";
import {
  proofAppInfo,
  proofCurrencyRates,
  proofHooksConfig,
  proofUpdateState,
  providerChartData,
  providerDetail,
  providerSummaries,
  spendContract,
  usageSpendSummary,
} from "./fixtures/bridgeData";
import { PROOF_LANGUAGES, englishLocaleStrings } from "./locale";
import type { ProofWindow } from "./params";

export const UNHANDLED_COMMAND_WARNING = "[proof-mock] unhandled command:";

type Handler = (args: Record<string, unknown>) => unknown;

/** Plugin namespaces whose commands are window chrome or OS dialogs: inert in a browser. */
const INERT_PLUGIN_PREFIXES = ["plugin:window|", "plugin:webview|", "plugin:dialog|", "plugin:event|"];

/**
 * User actions (sign-ins, credential writes, OS side effects) the browser
 * accepts as no-ops. They return null and are recorded in `calls()`, so a
 * scenario can assert that a click reached the backend. Give one a real
 * handler when a scenario needs its result.
 */
export const INERT_ACTIONS = [
  "add_token_account",
  "apply_update",
  "claude_account_add",
  "claude_account_cancel_login",
  "claude_account_remove",
  "claude_account_save_current",
  "claude_account_switch",
  "claude_swap_account_reauthenticate",
  "claude_swap_account_switch",
  "codex_account_add",
  "codex_account_reauthenticate",
  "codex_account_remove",
  "codex_account_restart_desktop",
  "codex_account_switch",
  "download_update",
  "export_preferences",
  "focus_agent_session",
  "get_openai_live_usage",
  "get_openai_org_rate_limits",
  "grok_account_add",
  "grok_account_cancel_login",
  "grok_account_fetch",
  "grok_account_reauthenticate",
  "grok_account_remove",
  "grok_account_save_current",
  "grok_account_switch",
  "import_browser_cookies",
  "import_preferences",
  "open_path",
  "play_notification_sound",
  "quit_app",
  "register_global_shortcut",
  "remove_api_key",
  "remove_manual_cookie",
  "remove_openrouter_management_api_key",
  "remove_token_account",
  "revoke_provider_credentials",
  "save_hooks_config",
  "set_active_token_account",
  "set_api_key",
  "set_jetbrains_ide_path",
  "set_manual_cookie",
  "set_openrouter_management_api_key",
  "set_provider_auto_resume_after_quota_reset",
  "set_provider_azure_api_version",
  "set_provider_cookie_source",
  "set_provider_gateway_url",
  "set_provider_optional_details",
  "set_provider_region",
  "set_provider_usage_source",
  "set_provider_workspace_id",
  "trigger_provider_login",
  "unregister_global_shortcut",
  "write_usage_spend_export",
] as const;

export interface ProofCall {
  cmd: string;
  args: Record<string, unknown>;
}

export interface ProofRouterOptions {
  window: ProofWindow;
  tab: string;
  now?: Date;
}

export interface ProofRouter {
  handle: (cmd: string, payload?: unknown) => unknown;
  /** Command names with a fixture-backed handler. */
  handledCommands: readonly string[];
  /** Commands that reached the fallback since this router was created. */
  unhandledCommands: () => string[];
  /** Every non-plugin command handled so far, in order. */
  calls: () => ProofCall[];
  settings: () => SettingsSnapshot;
}

function argsRecord(payload: unknown): Record<string, unknown> {
  return payload !== null && typeof payload === "object" && !Array.isArray(payload)
    ? (payload as Record<string, unknown>)
    : {};
}

function stringArg(args: Record<string, unknown>, key: string): string {
  const value = args[key];
  return typeof value === "string" ? value : "";
}

function emitSoon(event: string, payload: unknown): void {
  window.setTimeout(() => {
    void emit(event, payload).catch(() => {});
  }, 0);
}

function windowPluginResult(cmd: string): unknown {
  switch (cmd) {
    case "plugin:window|inner_size":
    case "plugin:window|outer_size":
      return { width: window.innerWidth, height: window.innerHeight };
    case "plugin:window|inner_position":
    case "plugin:window|outer_position":
      return { x: 0, y: 0 };
    case "plugin:window|scale_factor":
      return window.devicePixelRatio || 1;
    case "plugin:window|is_visible":
    case "plugin:window|is_focused":
      return true;
    default:
      return null;
  }
}

export function createProofRouter(
  scenario: ProofScenario,
  options: ProofRouterOptions,
): ProofRouter {
  const now = options.now ?? new Date();
  let settings: SettingsSnapshot = { ...scenario.settings };
  const warned = new Set<string>();

  const bootstrap = (): BootstrapState => ({
    contractVersion: "v1",
    providers: scenario.catalog,
    settings,
  });

  const surfaceState = (): CurrentSurfaceState => {
    if (options.window === "settings") {
      const request = parseSettingsTabRequest(options.tab);
      const tab = request?.providerId
        ? providersTabRequest(request.providerId)
        : (request?.tab ?? "general");
      return { mode: "settings", target: { kind: "settings", tab } };
    }
    return { mode: "trayPanel", target: { kind: "summary" } };
  };

  // Fixture builders read settings, so they get the patched copy.
  const current = (): ProofScenario => ({ ...scenario, settings });
  const enabledProviders = () =>
    scenario.providers.filter((snapshot) => settings.enabledProviders.includes(snapshot.providerId));

  const handlers: Record<string, Handler> = {
    get_bootstrap_state: bootstrap,
    get_settings_snapshot: () => settings,
    update_settings: (args) => {
      const patch = argsRecord(args.patch) as SettingsUpdate;
      settings = { ...settings, ...patch } as SettingsSnapshot;
      return settings;
    },
    get_provider_catalog: () => scenario.catalog,
    get_provider_statuses: () => [],
    reorder_providers: () => providerSummaries(current()),
    reorder_visible_providers: () => providerSummaries(current()),
    restore_default_provider_order: () => providerSummaries(current()),
    get_cached_providers: enabledProviders,
    refresh_providers: () => {
      const ids = enabledProviders().map((snapshot) => snapshot.providerId);
      emitSoon("refresh-started", { providerIds: ids });
      emitSoon("refresh-complete", {
        providerCount: ids.length,
        errorCount: enabledProviders().filter((snapshot) => snapshot.error !== null).length,
      });
      return null;
    },
    refresh_providers_if_stale: () => null,
    get_current_surface_state: surfaceState,
    set_surface_mode: (args) => args.mode ?? "trayPanel",
    get_locale_strings: () => englishLocaleStrings(),
    set_ui_language: () => null,
    get_available_languages: () => PROOF_LANGUAGES,
    get_currency_rates: (args) =>
      proofCurrencyRates(stringArg(args, "preferredCurrencyCode") || "AUTO"),
    get_update_state: proofUpdateState,
    check_for_updates: proofUpdateState,
    dismiss_update: proofUpdateState,
    get_app_info: () => proofAppInfo(appVersion),
    get_deepseek_pricing_status: () => null,
    get_stay_awake_status: () => false,
    get_system_accent_color: () => "#4c8bf5",
    // A fixed 1080p desktop minus the taskbar: headless browsers report a
    // small `screen`, which would cap the tray panel's auto-fit height.
    get_work_area_rect: () => ({ x: 0, y: 0, width: 1920, height: 1032 }),
    tray_visibility_status: () => ({ support: "supported", state: "promoted" }),
    get_credential_storage_status: () => ({
      manualCookies: "Windows Credential Manager",
      apiKeys: "Windows Credential Manager",
      tokenAccounts: "Windows Credential Manager",
    }),
    get_provider_detail: (args) => providerDetail(current(), stringArg(args, "providerId")),
    get_provider_chart_data: (args) =>
      providerChartData(current(), stringArg(args, "providerId"), now),
    get_provider_local_usage_summary: (args) =>
      providerChartData(current(), stringArg(args, "providerId"), now).localUsage,
    get_usage_spend_summary: () => usageSpendSummary(current(), now),
    get_spend_contract: (args) =>
      spendContract(stringArg(args, "providerId"), settings.spendDashboardPeriod ?? "rolling:30"),
    get_provider_cookie_source_options: () => [
      { value: "auto", label: "Automatic" },
      { value: "manual", label: "Manual" },
      { value: "off", label: "Off" },
    ],
    get_provider_region_options: () => [],
    get_provider_workspace_id: () => null,
    get_provider_gateway_url: () => "",
    get_provider_azure_api_version: () => null,
    get_api_keys: () => [],
    get_api_key_providers: () => [],
    has_openrouter_management_api_key: () => false,
    get_manual_cookies: () => [],
    list_detected_browsers: () => [
      { browserType: "edge", displayName: "Microsoft Edge", profileCount: 1 },
      { browserType: "chrome", displayName: "Google Chrome", profileCount: 2 },
    ],
    get_token_account_providers: () => [],
    get_token_accounts: (args) => ({
      providerId: stringArg(args, "providerId"),
      support: {
        providerId: stringArg(args, "providerId"),
        displayName: stringArg(args, "providerId"),
        title: "Token accounts",
        subtitle: "",
        placeholder: "",
      },
      accounts: [],
      activeIndex: 0,
    }),
    get_codex_accounts_state: () => scenario.codexAccounts,
    codex_accounts_list: () => scenario.codexAccounts.accounts,
    codex_account_snapshots: () => scenario.codexAccounts.snapshots,
    codex_account_fetch: (args) => scenario.codexAccounts.snapshots[stringArg(args, "id")] ?? null,
    get_claude_accounts_state: () => scenario.claudeAccounts,
    claude_reconciliation_state: () => null,
    claude_swap_accounts_list: () => ({
      enabled: false,
      executableConfigured: false,
      accounts: [],
      error: null,
    }),
    grok_accounts_list: () => [],
    get_hooks_config: proofHooksConfig,
    list_agent_sessions: () => ({ status: "disabled" }),
    get_gemini_cli_signed_in: () => ({ signedIn: true, credentialsPath: null }),
    get_vertexai_status: () => ({ hasCredentials: false, credentialsPath: null }),
    list_jetbrains_detected_ides: () => [],
    get_kiro_status: () => ({ available: false, hint: null }),
    get_codex_workspaces_snapshot: () => null,
    get_safe_diagnostics: () => "Proof build: mocked IPC, no diagnostics collected.",
    // Window and shell side effects are no-ops in a browser.
    dismiss_tray_panel: () => null,
    begin_flyout_gesture: () => null,
    end_flyout_gesture: () => null,
    open_settings_window: () => null,
    open_flyout_window: () => null,
    close_settings_window: () => null,
    reanchor_tray_panel: () => null,
    reveal_tray_panel_window: () => null,
    resize_float_bar: () => null,
    open_external_url: () => null,
    open_release_page: () => null,
    open_provider_dashboard: () => null,
    open_provider_status_page: () => null,
  };

  const inertActions = new Set<string>(INERT_ACTIONS);
  const calls: ProofCall[] = [];

  const handle = (cmd: string, payload?: unknown): unknown => {
    if (cmd.startsWith("plugin:window|")) return windowPluginResult(cmd);
    if (INERT_PLUGIN_PREFIXES.some((prefix) => cmd.startsWith(prefix))) return null;
    const args = argsRecord(payload);
    calls.push({ cmd, args });
    const handler = handlers[cmd];
    if (handler) return handler(args);
    if (inertActions.has(cmd)) return null;
    if (!warned.has(cmd)) {
      warned.add(cmd);
      console.warn(`${UNHANDLED_COMMAND_WARNING} ${cmd}`);
    }
    return null;
  };

  return {
    handle,
    handledCommands: Object.keys(handlers),
    unhandledCommands: () => [...warned],
    calls: () => [...calls],
    settings: () => settings,
  };
}
