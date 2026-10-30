import { emit } from "@tauri-apps/api/event";
import { version as appVersion } from "../../package.json";
import type {
  BootstrapState,
  CurrentSurfaceState,
  HttpServerConfig,
  HttpServerSnapshot,
  SessionFocusResult,
  SettingsSnapshot,
  SettingsUpdate,
  TokenAccountBridge,
} from "../types/bridge";
import {
  parseSettingsTabRequest,
  providersTabRequest,
} from "../surfaces/settings/settingsTabs";
import type { ProofScenario } from "./fixtures/scenarios";
import {
  codexSessionDetails,
  codexSessionToolActivity,
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
  "remove_vercel_access_token",
  "revoke_provider_credentials",
  "save_hooks_config",
  "set_active_token_account",
  "set_api_key",
  "set_jetbrains_ide_path",
  "set_manual_cookie",
  "set_openrouter_management_api_key",
  "set_provider_auto_resume_after_quota_reset",
  "set_provider_azure_api_version",
  "set_provider_gateway_url",
  "set_provider_optional_details",
  "set_provider_region",
  "set_provider_usage_source",
  "set_vercel_access_token",
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
  /** What `get_system_theme` reports (the Windows app theme). Dark when omitted. */
  systemTheme?: "light" | "dark";
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
  let claudeTokenAccounts: TokenAccountBridge[] | null =
    scenario.claudeTokenAccounts?.map((account) => ({ ...account })) ?? null;
  // Cookie-source picks persist for the session, as the real bridge persists
  // them to settings; `get_provider_detail` then reports the chosen source.
  const cookieSources = new Map<string, string>();
  const workspaceIds = new Map<string, string>(Object.entries(scenario.workspaceIds ?? {}));
  const warned = new Set<string>();
  let httpServer: HttpServerSnapshot = {
    config: {
      enabled: false,
      host: "127.0.0.1",
      port: 8080,
      token: "",
      refreshIntervalSecs: 60,
      requestTimeoutSecs: 30,
      identity: "redacted",
      allowPlainHttp: false,
    },
    status: { state: "stopped" },
  };
  // Mirrors only the token rule of the shell's validation; enough for scenarios.
  const applyHttpServer = (config: HttpServerConfig): HttpServerSnapshot => {
    if (config.enabled && config.token.trim() === "") {
      return { ...httpServer, status: { state: "invalid", code: "invalidToken" } };
    }
    const host = config.host.trim() === "localhost" ? "127.0.0.1" : config.host.trim();
    httpServer = {
      config,
      status: config.enabled
        ? { state: "running", url: `http://${host}:${config.port}/` }
        : { state: "stopped" },
    };
    return httpServer;
  };

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

  const tokenAccountSupport = (providerId: string) => ({
    providerId,
    displayName: providerId,
    title: "Token accounts",
    subtitle: "",
    placeholder: "",
  });
  const tokenAccounts = (providerId: string) => {
    const accounts = providerId === "claude" ? (claudeTokenAccounts ?? []) : [];
    return {
      providerId,
      support: tokenAccountSupport(providerId),
      accounts,
      activeIndex: Math.max(0, accounts.findIndex((account) => account.isActive)),
    };
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
    get_locale_strings: () => englishLocaleStrings(current().localeOverrides),
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
    get_http_server_state: () => httpServer,
    apply_http_server_config: (args) =>
      applyHttpServer(argsRecord(args.config) as unknown as HttpServerConfig),
    get_system_accent_color: () => "#4c8bf5",
    get_system_theme: () => options.systemTheme ?? "dark",
    // A fixed 1080p desktop minus the taskbar: headless browsers report a
    // small `screen`, which would cap the tray panel's auto-fit height.
    get_work_area_rect: () => ({ x: 0, y: 0, width: 1920, height: 1032 }),
    tray_visibility_status: () => ({ support: "supported", state: "promoted" }),
    get_credential_storage_status: () => ({
      manualCookies: "Windows Credential Manager",
      apiKeys: "Windows Credential Manager",
      tokenAccounts: "Windows Credential Manager",
      accessTokens: "missing",
    }),
    get_provider_detail: (args) => {
      const detail = providerDetail(current(), stringArg(args, "providerId"));
      const cookieSource = cookieSources.get(detail.id);
      return cookieSource === undefined ? detail : { ...detail, cookieSource };
    },
    set_provider_cookie_source: (args) => {
      cookieSources.set(stringArg(args, "providerId"), stringArg(args, "source"));
      return null;
    },
    get_provider_chart_data: (args) =>
      providerChartData(current(), stringArg(args, "providerId"), now),
    get_provider_local_usage_summary: (args) =>
      providerChartData(current(), stringArg(args, "providerId"), now).localUsage,
    get_usage_spend_summary: () => usageSpendSummary(current(), now),
    get_spend_contract: (args) =>
      spendContract(stringArg(args, "providerId"), settings.spendDashboardPeriod ?? "rolling:30"),
    // The real bridge offers no cookie-source options for providers that
    // never read browser cookies, which hides the picker.
    get_provider_cookie_source_options: (args) => {
      const providerId = stringArg(args, "providerId");
      const entry = current().catalog.find((candidate) => candidate.id === providerId);
      return entry?.cookieDomain
        ? [
            { value: "auto", label: "Automatic" },
            { value: "manual", label: "Manual" },
            { value: "off", label: "Off" },
          ]
        : [];
    },
    get_provider_region_options: () => [],
    get_provider_workspace_id: (args) => workspaceIds.get(stringArg(args, "providerId")) ?? null,
    // The shell rejects with a plain string, which the UI shows as-is.
    set_provider_workspace_id: (args) => {
      const providerId = stringArg(args, "providerId");
      const error = scenario.workspaceIdSaveErrors?.[providerId];
      if (error !== undefined) throw error;
      workspaceIds.set(providerId, stringArg(args, "workspaceId"));
      return null;
    },
    get_provider_gateway_url: () => "",
    get_provider_azure_api_version: () => null,
    get_api_keys: () => [],
    get_api_key_providers: () => [],
    has_openrouter_management_api_key: () => false,
    has_vercel_access_token: () => false,
    get_manual_cookies: () => [],
    list_detected_browsers: (args) =>
      scenario.detectedBrowsers ?? [
        { browserType: "edge", displayName: "Microsoft Edge", profileCount: 1 },
        { browserType: "chrome", displayName: "Google Chrome", profileCount: 2 },
        ...(stringArg(args, "providerId") === "claude"
          ? [{ browserType: "claude-desktop", displayName: "Claude desktop app", profileCount: 1 }]
          : []),
      ],
    get_token_account_providers: () =>
      claudeTokenAccounts ? [tokenAccountSupport("claude")] : [],
    get_token_accounts: (args) => tokenAccounts(stringArg(args, "providerId")),
    // Cache-only read: serves the scenario's per-account usage for Claude token rows.
    get_provider_account_usage: (args) => {
      const providerId = stringArg(args, "providerId");
      return {
        providerId,
        accounts: providerId === "claude" ? (scenario.claudeTokenAccountUsage ?? {}) : {},
      };
    },
    // Cache-only read: the pool accounts the scenario's claude-swap/tcr source lists,
    // with the usage the shell would have cached. Empty when the integration is off.
    get_claude_swap_overview_accounts: () => {
      const swap = scenario.claudeSwapAccounts;
      const source = settings.claudeSwapSource ?? "claude-swap";
      if (!settings.claudeSwapEnabled || !swap || (swap.source ?? "claude-swap") !== source) {
        return { accounts: [], usage: {} };
      }
      return {
        accounts: swap.accounts.map((account) => ({
          id: account.id,
          slot: account.slot,
          label: account.label,
        })),
        usage: scenario.claudeSwapOverviewUsage ?? {},
      };
    },
    set_token_account_claude_config_dir: (args) => {
      const accountId = stringArg(args, "accountId");
      const directory = typeof args.directory === "string" ? args.directory : null;
      claudeTokenAccounts = (claudeTokenAccounts ?? []).map((account) =>
        account.id === accountId
          ? {
              ...account,
              claudeConfigDirectory: directory,
              claudeConfigDirectoryAvailable: directory !== null,
            }
          : account,
      );
      return tokenAccounts("claude");
    },
    // Mirrors the shell's rules for the fixture: a blank label keeps the
    // saved one, and the credential is not modelled (the bridge never returns it).
    update_token_account: (args) => {
      const accountId = stringArg(args, "accountId");
      const label = stringArg(args, "label").trim();
      claudeTokenAccounts = (claudeTokenAccounts ?? []).map((account) =>
        account.id === accountId && label !== "" ? { ...account, label } : account,
      );
      return tokenAccounts(stringArg(args, "providerId"));
    },
    move_token_account: (args) => {
      const accountId = stringArg(args, "accountId");
      const index = typeof args.index === "number" ? args.index : -1;
      const accounts = claudeTokenAccounts ?? [];
      const source = accounts.findIndex((account) => account.id === accountId);
      if (source >= 0 && index >= 0 && index < accounts.length) {
        // isActive travels with the account, so the active selection is unchanged.
        const next = [...accounts];
        const [moved] = next.splice(source, 1);
        next.splice(index, 0, moved);
        claudeTokenAccounts = next;
      }
      return tokenAccounts(stringArg(args, "providerId"));
    },
    get_codex_accounts_state: () => scenario.codexAccounts,
    codex_accounts_list: () => scenario.codexAccounts.accounts,
    codex_account_snapshots: () => scenario.codexAccounts.snapshots,
    codex_account_fetch: (args) => scenario.codexAccounts.snapshots[stringArg(args, "id")] ?? null,
    get_claude_accounts_state: () => scenario.claudeAccounts,
    claude_reconciliation_state: () => null,
    claude_swap_accounts_list: () => {
      const swap = scenario.claudeSwapAccounts;
      if (!settings.claudeSwapEnabled || !swap) {
        return { enabled: false, executableConfigured: false, accounts: [], error: null };
      }
      // The fixture's accounts belong to one source; another source lists none.
      const source = settings.claudeSwapSource ?? "claude-swap";
      return source === (swap.source ?? "claude-swap") ? swap : { ...swap, source, accounts: [] };
    },
    grok_accounts_list: () => [],
    get_hooks_config: proofHooksConfig,
    list_agent_sessions: () => scenario.agentSessions ?? { status: "disabled" },
    focus_agent_session: (): SessionFocusResult => ({ status: "focused" }),
    get_gemini_cli_signed_in: () => ({ signedIn: true, credentialsPath: null }),
    get_vertexai_status: () => ({ hasCredentials: false, credentialsPath: null }),
    list_jetbrains_detected_ides: () => [],
    get_kiro_status: () => ({ available: false, hint: null }),
    get_codex_workspaces_snapshot: () => null,
    get_codex_session_tool_activity: (args) =>
      codexSessionToolActivity(current(), stringArg(args, "sessionId")),
    // The shell rejects with a bare error code string.
    get_codex_session_details: (args) => codexSessionDetails(current(), stringArg(args, "sessionId")),
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
