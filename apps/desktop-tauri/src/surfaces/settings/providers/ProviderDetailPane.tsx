import { useCallback, useEffect, useReducer, useState } from "react";
import type {
  ProviderCatalogEntry,
  ProviderUsageSnapshot,
  SettingsSnapshot,
  SettingsUpdate,
} from "../../../types/bridge";
import { SettingsGroup } from "../../../components/FormControls";
import { formatRelativeUpdated } from "../../../lib/relativeTime";
import { nextEnabledProviders } from "../settingsTabs";
import { useLocale } from "../../../hooks/useLocale";
import { providerAllowsPace } from "../../../lib/providerPace";
import { providerBarAccent } from "../../../components/providers/providerIcons";
import {
  getCredentialStorageStatus,
  getProviderCookieSourceOptions,
  getProviderDetail,
  getProviderGatewayUrl,
  getProviderRegionOptions,
  getTokenAccountProviders,
  openProviderDashboard,
  openProviderStatusPage,
  refreshProviders,
  revokeProviderCredentials,
  setProviderGatewayUrl,
  triggerProviderLogin,
} from "../../../lib/tauri";
import { listen } from "@tauri-apps/api/event";

import {
  initialState,
  providerDetailPaneReducer,
} from "./providerDetailPaneState";
import { buildSubtitle } from "./providerDetailFormat";
import { IdentitySection } from "./sections/IdentitySection";
import { UsageSection } from "./sections/UsageSection";
import { UsageItemVisibilitySection } from "./sections/UsageItemVisibilitySection";
import { UsageDetailsSection } from "./sections/UsageDetailsSection";
import { CostSection } from "./sections/CostSection";
import { QuickActionsSection } from "./sections/QuickActionsSection";
import { ChartsSection } from "./sections/charts/ChartsSection";
import { CookieSourceSection } from "./sections/CookieSourceSection";
import { UsageSourceSection } from "./sections/UsageSourceSection";
import { shouldShowCookieSource } from "./sections/usageSourcePolicy";
import { RegionSection } from "./sections/RegionSection";
import { CodexAccountsSection } from "./sections/credentials/CodexAccountsSection";
import { ClaudeAccountsSection } from "./sections/credentials/ClaudeAccountsSection";
import { GrokAccountsSection } from "./sections/credentials/GrokAccountsSection";
import { TokenAccountsPanel } from "../tokens/TokenAccountsPanel";
import { ApiKeySection } from "./ApiKeySection";
import { CookieSection } from "./CookieSection";
import { MenuBarMetricSection } from "./sections/MenuBarMetricSection";
import { AccentColorSection } from "./sections/AccentColorSection";
import { ProviderIssueNotice } from "./sections/ProviderIssueNotice";
import { CredentialStorageSection } from "./sections/CredentialStorageSection";
import { CredentialsDispatcher } from "./sections/CredentialsDispatcher";
import {
  isGatewayProviderId,
  WayfinderGatewaySection,
} from "./sections/WayfinderGatewaySection";
import { AzureApiVersionSection } from "./sections/AzureApiVersionSection";
import { MuseBrowserTeamSection } from "./sections/MuseBrowserTeamSection";
import { QuotaWarningsSection } from "./sections/QuotaWarningsSection";
import { OptionsSection } from "./sections/OptionsSection";

interface Props {
  providerId: string | null;
  /** Full settings snapshot: header switch, Quota warnings and Options read from it. */
  settings: SettingsSnapshot;
  /** Provider catalog, for writing `enabledProviders` in catalog order. */
  catalog: readonly Pick<ProviderCatalogEntry, "id">[];
  /** Live tray snapshot for `providerId`; feeds the details moved out of the tray card. */
  liveSnapshot?: ProviderUsageSnapshot | null;
  cookieDomain?: string | null;
  resetTimeRelative: boolean;
  /** Show "N% used" next to remaining on per-account usage rows. */
  showAsUsed?: boolean;
  providerMetrics: SettingsSnapshot["providerMetrics"];
  copilotSeatCreditEntitlement: SettingsSnapshot["copilotSeatCreditEntitlement"];
  /** Per-provider accent color overrides (CLI name → hex color). */
  providerAccentColors: SettingsSnapshot["providerAccentColors"];
  wayfinderGatewayUrl: string;
  hidePersonalInfo: boolean;
  settingsDisabled: boolean;
  onSettingsChange: (patch: SettingsUpdate) => void;
}

/**
 * Settings > Providers detail pane, in the macOS order
 * (`PreferencesProviderDetailView`): header, Usage, Visible usage items,
 * Error, Menu bar metric, Connection, Token accounts (then the Windows-only
 * credential storage), Fields, Accounts, Accent color, Quota warnings and
 * Options. Groups whose sections render nothing are hidden by CSS.
 */
export function ProviderDetailPane({
  providerId,
  settings,
  catalog,
  liveSnapshot = null,
  cookieDomain = null,
  resetTimeRelative,
  showAsUsed = false,
  providerMetrics,
  copilotSeatCreditEntitlement,
  providerAccentColors,
  wayfinderGatewayUrl,
  hidePersonalInfo,
  settingsDisabled,
  onSettingsChange,
}: Props) {
  const { t, language } = useLocale();
  const [gatewayLoadedProviderId, setGatewayLoadedProviderId] =
    useState<string | null>(null);
  const [state, dispatch] = useReducer(
    providerDetailPaneReducer,
    { wayfinderGatewayUrl, providerId },
    (init) => initialState(init.wayfinderGatewayUrl, init.providerId),
  );

  if (
    providerId !== state.syncedProviderId ||
    wayfinderGatewayUrl !== state.syncedGatewayUrl
  ) {
    dispatch({ type: "SYNC_PROPS", providerId, wayfinderGatewayUrl });
  }

  const {
    detail,
    cookieOptions,
    regionOptions,
    credentialStatus,
    credentialRevision,
    tokenProviderIds,
    loading,
    error,
    busy,
    gatewayDraft,
    gatewayError,
  } = state;

  const setErr = (e: unknown) =>
    dispatch({ type: "SET_ERROR", error: String(e) });
  const setBusy = (value: boolean) =>
    dispatch({ type: "SET_BUSY", busy: value });

  const load = useCallback(async (id: string, signal?: { stale: boolean }) => {
    dispatch({ type: "LOAD_START" });
    try {
      const [next, cookieOpts, regionOpts, storageStatus] = await Promise.all([
        getProviderDetail(id),
        getProviderCookieSourceOptions(id),
        getProviderRegionOptions(id),
        getCredentialStorageStatus(),
      ]);
      if (signal?.stale) return;
      dispatch({
        type: "LOAD_SUCCESS",
        detail: next,
        cookieOptions: cookieOpts,
        regionOptions: regionOpts,
        credentialStatus: storageStatus,
      });
    } catch (e) {
      if (signal?.stale) return;
      dispatch({ type: "LOAD_ERROR", error: String(e) });
    } finally {
      // Keep stale guard — concurrent provider switches must not clear loading
      // for the in-flight replacement load.
      if (!signal?.stale) dispatch({ type: "LOAD_FINISH" });
    }
  }, []);

  const gatewayProviderId =
    providerId !== null && isGatewayProviderId(providerId) ? providerId : null;

  useEffect(() => {
    setGatewayLoadedProviderId(null);
    if (!gatewayProviderId) return;
    let cancelled = false;
    void getProviderGatewayUrl(gatewayProviderId).then((url) => {
      if (!cancelled) {
        dispatch({ type: "SET_GATEWAY_DRAFT", draft: url });
        setGatewayLoadedProviderId(gatewayProviderId);
      }
    }).catch((e) => {
      if (!cancelled) {
        dispatch({ type: "SAVE_GATEWAY_ERROR", error: String(e) });
        setGatewayLoadedProviderId(gatewayProviderId);
      }
    });
    return () => { cancelled = true; };
  }, [gatewayProviderId]);

  const saveGateway = async () => {
    dispatch({ type: "SAVE_GATEWAY_START" });
    try {
      if (!gatewayProviderId) return;
      await setProviderGatewayUrl(gatewayProviderId, gatewayDraft);
      await load(gatewayProviderId);
    } catch (e) {
      dispatch({ type: "SAVE_GATEWAY_ERROR", error: String(e) });
    } finally {
      setBusy(false);
    }
  };

  // Load the set of providers that support token accounts once.
  useEffect(() => {
    let cancelled = false;
    void getTokenAccountProviders()
      .then((list) => {
        if (!cancelled) {
          dispatch({
            type: "SET_TOKEN_PROVIDERS",
            ids: list.map((p) => p.providerId),
          });
        }
      })
      .catch(() => {
        // Non-fatal: inline token section will simply not render.
      });
    return () => {
      cancelled = true;
    };
  }, []);

  useEffect(() => {
    if (!providerId) {
      dispatch({ type: "LOAD_CLEAR", mode: "empty" });
      return;
    }
    // Clear stale detail immediately so we don't render the old provider
    dispatch({ type: "LOAD_CLEAR", mode: "switch" });
    const signal = { stale: false };
    void load(providerId, signal);
    return () => {
      signal.stale = true;
    };
  }, [providerId, load]);

  // Live-refresh when a new snapshot lands for this provider.
  useEffect(() => {
    if (!providerId) return;
    const signal = { stale: false };
    const unlistenPromise = listen<{ providerId?: string }>(
      "provider-updated",
      (event) => {
        const pid = event.payload?.providerId;
        if (!pid || pid === providerId) void load(providerId, signal);
      },
    );
    return () => {
      signal.stale = true;
      void unlistenPromise.then((fn) => fn());
    };
  }, [providerId, load]);

  // Visibility changes are persisted through the shared settings command and
  // broadcast to detached windows. Reload the selected provider so the raw
  // descriptor list and hidden IDs stay authoritative after that event.
  useEffect(() => {
    if (!providerId) return;
    const signal = { stale: false };
    const unlistenPromise = listen("settings-changed", () => {
      void load(providerId, signal);
    });
    return () => {
      signal.stale = true;
      void unlistenPromise.then((fn) => fn());
    };
  }, [providerId, load]);

  if (!providerId) {
    return emptyDetail(t("StateNoProviderSelected"));
  }
  if (loading && !detail) {
    return emptyDetail(t("StateLoadingProviders"));
  }
  if (error && !detail) {
    return emptyDetail(`${t("StateError")}: ${error}`, true);
  }
  if (!detail) return null;

  const subtitle = buildSubtitle(detail, t);
  const reload = () => void load(detail.id);
  const credKey = `${detail.id}-${credentialRevision}`;

  const handleRefresh = async () => {
    setBusy(true);
    try {
      await refreshProviders();
    } catch (e) {
      setErr(e);
    } finally {
      setBusy(false);
    }
  };

  const handleSwitchAccount = async () => {
    setBusy(true);
    try {
      await triggerProviderLogin(detail.id);
      dispatch({ type: "BUMP_CREDENTIAL_REVISION" });
      await refreshProviders();
      await load(detail.id);
    } catch (e) {
      setErr(e);
    } finally {
      setBusy(false);
    }
  };

  const handleRevokeCredentials = async () => {
    setBusy(true);
    dispatch({ type: "SET_ERROR", error: null });
    try {
      await revokeProviderCredentials(detail.id);
      dispatch({ type: "BUMP_CREDENTIAL_REVISION" });
      await load(detail.id);
    } catch (e) {
      setErr(e);
    } finally {
      setBusy(false);
    }
  };

  const handleOpenDashboard = () => {
    void openProviderDashboard(detail.id).catch(setErr);
  };
  const handleOpenStatusPage = () => {
    void openProviderStatusPage(detail.id).catch(setErr);
  };
  const handleBuyCredits = () => {
    if (detail.buyCreditsUrl) {
      void openProviderDashboard(detail.id).catch(setErr);
    }
  };

  const enabled = settings.enabledProviders.includes(detail.id);
  const handleToggleEnabled = (on: boolean) =>
    onSettingsChange({
      enabledProviders: nextEnabledProviders(catalog, settings.enabledProviders, detail.id, on),
    });
  // macOS shows the error group only for a recorded fetch error; every
  // snapshot carries an errorState, including "ready" after a good fetch.
  const hasFetchError = Boolean(detail.lastError?.trim());
  const updatedMs = detail.lastUpdated ? Date.parse(detail.lastUpdated) : Number.NaN;
  const updatedText = Number.isFinite(updatedMs)
    ? formatRelativeUpdated(updatedMs, t)
    : busy
      ? t("SectionRefreshing")
      : hasFetchError
        ? t("ProviderTextUnavailable")
        : t("ProviderNotFetchedYetTitle");

  return (
    <div className="provider-detail">
      <IdentitySection
        provider={detail}
        subtitle={subtitle}
        t={t}
        hidePersonalInfo={hidePersonalInfo}
        updatedText={updatedText}
        enabled={enabled}
        onToggleEnabled={handleToggleEnabled}
        onRefresh={() => void handleRefresh()}
        busy={busy}
      />

      <SettingsGroup
        className="provider-detail-group provider-usage-group"
        title={t("ProviderUsage")}
      >
        <UsageSection
          provider={detail}
          resetTimeRelative={resetTimeRelative}
          display={{
            showResetWhenExhausted: settings.showResetWhenExhausted,
            showPace:
              (settings.showPace ?? true) &&
              providerAllowsPace(detail.id, detail.sourceLabel),
            showAsUsed,
            usageThresholds: settings,
            quotaWarningMarkersVisible: settings.quotaWarningMarkersVisible ?? true,
            weeklyProgressWorkDays: settings.weeklyProgressWorkDays ?? null,
            workdayTickStyle: settings.workdayTickStyle ?? "subtle",
          }}
          accentColor={providerBarAccent(detail.id, providerAccentColors[detail.id])}
          t={t}
        />
        <CostSection cost={detail.cost} t={t} />
        <UsageDetailsSection
          provider={liveSnapshot?.providerId === detail.id ? liveSnapshot : null}
          paceReservePositive={settings.menuBarPaceReservePositive ?? false}
        />
        <ChartsSection
          providerId={detail.id}
          accountEmail={detail.email}
          accentColor={providerAccentColors[detail.id]}
          openAiApiUsage={detail.openAiApiUsage}
          t={t}
        />
        <QuickActionsSection
          provider={detail}
          busy={busy}
          onSwitchAccount={handleSwitchAccount}
          onOpenDashboard={handleOpenDashboard}
          onOpenStatusPage={handleOpenStatusPage}
          onBuyCredits={handleBuyCredits}
          t={t}
        />
      </SettingsGroup>

      <UsageItemVisibilitySection
        provider={detail}
        disabled={settingsDisabled}
        t={t}
        onChange={onSettingsChange}
        costSummaryHidden={settings.costSummaryDisplayStyle === "hidden"}
      />

      {hasFetchError && (
        <ProviderIssueNotice detail={detail} t={t} hidePersonalInfo={hidePersonalInfo} />
      )}

      <MenuBarMetricSection
        provider={detail}
        providerMetrics={providerMetrics}
        disabled={settingsDisabled}
        t={t}
        onChange={onSettingsChange}
      />

      <SettingsGroup
        className="provider-detail-group provider-connection-group"
        title={t("ProviderSectionConnection")}
      >
        <UsageSourceSection
          providerId={detail.id}
          currentValue={detail.usageSource}
          t={t}
          onChanged={reload}
        />
        {shouldShowCookieSource(detail.id, detail.usageSource) && (
          <CookieSourceSection
            providerId={detail.id}
            currentValue={detail.cookieSource}
            options={cookieOptions}
            manualCookieMissing={detail.manualCookieMissing}
            t={t}
            onChanged={reload}
          />
        )}
        {detail.id === "muse" && (
          <MuseBrowserTeamSection
            providerId={detail.id}
            details={detail.displayDetails}
            disabled={settingsDisabled}
            t={t}
            onChanged={reload}
          />
        )}
        <RegionSection
          providerId={detail.id}
          currentValue={detail.region}
          options={regionOptions}
          t={t}
          onChanged={reload}
        />
        {detail.id === "azureopenai" && (
          <AzureApiVersionSection
            providerId={detail.id}
            disabled={settingsDisabled}
            onChanged={reload}
          />
        )}
        {isGatewayProviderId(detail.id) &&
          gatewayLoadedProviderId === detail.id && (
          <WayfinderGatewaySection
            providerId={detail.id}
            draft={gatewayDraft}
            error={gatewayError}
            busy={busy}
            disabled={settingsDisabled}
            onDraftChange={(draft) =>
              dispatch({ type: "SET_GATEWAY_DRAFT", draft })
            }
            onSave={() => void saveGateway()}
            t={t}
          />
        )}
      </SettingsGroup>

      <SettingsGroup className="provider-detail-group provider-token-accounts-group">
        {tokenProviderIds.has(detail.id) && (
          <TokenAccountsPanel
            key={`token-${credKey}`}
            providerId={detail.id}
            compact
          />
        )}
      </SettingsGroup>
      <SettingsGroup className="provider-detail-group provider-credential-storage-group">
        <CredentialStorageSection
          status={credentialStatus}
          busy={busy}
          onRevoke={handleRevokeCredentials}
          t={t}
        />
      </SettingsGroup>

      <SettingsGroup className="provider-detail-group provider-fields-group">
        <CredentialsDispatcher providerId={detail.id} t={t} />
        <ApiKeySection key={`api-${credKey}`} providerId={detail.id} />
        <CookieSection
          key={`cookie-${credKey}`}
          providerId={detail.id}
          cookieDomain={cookieDomain}
          onChanged={reload}
        />
      </SettingsGroup>

      <SettingsGroup className="provider-detail-group provider-accounts-group">
        {detail.id === "codex" && (
          <CodexAccountsSection t={t} hidePersonalInfo={hidePersonalInfo} />
        )}
        {detail.id === "claude" && (
          <ClaudeAccountsSection
            t={t}
            language={language}
            hidePersonalInfo={hidePersonalInfo}
            showAsUsed={showAsUsed}
            resetTimeRelative={resetTimeRelative}
            claudeSwapEnabled={settings.claudeSwapEnabled ?? false}
            claudeSwapExecutablePath={settings.claudeSwapExecutablePath ?? ""}
          />
        )}
        {detail.id === "grok" && (
          <GrokAccountsSection
            t={t}
            hidePersonalInfo={hidePersonalInfo}
            resetTimeRelative={resetTimeRelative}
          />
        )}
      </SettingsGroup>

      <AccentColorSection
        providerId={detail.id}
        accentColor={providerAccentColors[detail.id] ?? null}
        disabled={settingsDisabled}
        t={t}
        onChange={onSettingsChange}
      />

      <QuotaWarningsSection
        providerId={detail.id}
        settings={settings}
        disabled={settingsDisabled}
        t={t}
        onChange={onSettingsChange}
      />

      <OptionsSection
        detail={detail}
        settings={settings}
        disabled={settingsDisabled}
        t={t}
        onSettingsChange={onSettingsChange}
        onReload={reload}
      />
    </div>
  );
}

function emptyDetail(message: string, isError = false) {
  return (
    <div className="provider-detail">
      <div
        className={
          isError
            ? "provider-detail-empty provider-detail-empty--error"
            : "provider-detail-empty"
        }
      >
        {message}
      </div>
    </div>
  );
}
