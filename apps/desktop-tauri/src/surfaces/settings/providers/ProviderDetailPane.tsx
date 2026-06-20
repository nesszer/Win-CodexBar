import { useCallback, useEffect, useState } from "react";
import type {
  CookieSourceOption,
  CredentialStorageStatus,
  ProviderDetail,
  RegionOption,
  SettingsSnapshot,
  SettingsUpdate,
} from "../../../types/bridge";
import { useLocale } from "../../../hooks/useLocale";
import {
  getCredentialStorageStatus,
  getProviderCookieSourceOptions,
  getProviderDetail,
  getProviderRegionOptions,
  getTokenAccountProviders,
  openProviderDashboard,
  openProviderStatusPage,
  refreshProviders,
  revokeProviderCredentials,
  triggerProviderLogin,
} from "../../../lib/tauri";
import { listen } from "@tauri-apps/api/event";

import { IdentitySection } from "./sections/IdentitySection";
import { UsageSection } from "./sections/UsageSection";
import { PaceSection } from "./sections/PaceSection";
import { CostSection } from "./sections/CostSection";
import { QuickActionsSection } from "./sections/QuickActionsSection";
import { ChartsSection } from "./sections/charts/ChartsSection";
import { CookieSourceSection } from "./sections/CookieSourceSection";
import { RegionSection } from "./sections/RegionSection";
import { GeminiCliCreds } from "./sections/credentials/GeminiCliCreds";
import { VertexAiCreds } from "./sections/credentials/VertexAiCreds";
import { JetBrainsCreds } from "./sections/credentials/JetBrainsCreds";
import { KiroCreds } from "./sections/credentials/KiroCreds";
import { ClaudeCreds } from "./sections/credentials/ClaudeCreds";
import { OpenAiExtras } from "./sections/credentials/OpenAiExtras";
import { TokenAccountsPanel } from "../tokens/TokenAccountsPanel";
import { ApiKeySection } from "./ApiKeySection";
import { CookieSection } from "./CookieSection";
import { MenuBarMetricSection } from "./sections/MenuBarMetricSection";

interface Props {
  providerId: string | null;
  cookieDomain?: string | null;
  resetTimeRelative: boolean;
  providerMetrics: SettingsSnapshot["providerMetrics"];
  settingsDisabled: boolean;
  onSettingsChange: (patch: SettingsUpdate) => void;
}

/**
 * Orchestrates the Settings → Providers right-hand detail pane.
 *
 * Top-level port of
 * `rust/src/native_ui/preferences.rs::render_provider_detail_panel`
 * (lines 4301–6698). Only the header, usage bars, pace, cost and the
 * quick-action bar are implemented here. Cookie-source picker (6c),
 * credential detection UIs (6d), inline token accounts (6e) and charts
 * (6f) are wired in as sub-sections below.
 */
export function ProviderDetailPane({
  providerId,
  cookieDomain = null,
  resetTimeRelative,
  providerMetrics,
  settingsDisabled,
  onSettingsChange,
}: Props) {
  const { t } = useLocale();
  const [detail, setDetail] = useState<ProviderDetail | null>(null);
  const [cookieOptions, setCookieOptions] = useState<CookieSourceOption[]>([]);
  const [regionOptions, setRegionOptions] = useState<RegionOption[]>([]);
  const [credentialStatus, setCredentialStatus] =
    useState<CredentialStorageStatus | null>(null);
  const [credentialRevision, setCredentialRevision] = useState(0);
  const [tokenProviderIds, setTokenProviderIds] = useState<Set<string>>(
    () => new Set(),
  );
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // Load the set of providers that support token accounts once.
  useEffect(() => {
    let cancelled = false;
    void getTokenAccountProviders()
      .then((list) => {
        if (!cancelled) {
          setTokenProviderIds(new Set(list.map((p) => p.providerId)));
        }
      })
      .catch(() => {
        // Non-fatal: inline token section will simply not render.
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const load = useCallback(async (id: string, signal?: { stale: boolean }) => {
    setLoading(true);
    setError(null);
    try {
      const [next, cookieOpts, regionOpts, storageStatus] = await Promise.all([
        getProviderDetail(id),
        getProviderCookieSourceOptions(id),
        getProviderRegionOptions(id),
        getCredentialStorageStatus(),
      ]);
      if (signal?.stale) return;
      setDetail(next);
      setCookieOptions(cookieOpts);
      setRegionOptions(regionOpts);
      setCredentialStatus(storageStatus);
    } catch (e) {
      if (signal?.stale) return;
      setError(String(e));
      setDetail(null);
      setCookieOptions([]);
      setRegionOptions([]);
      setCredentialStatus(null);
    } finally {
      if (!signal?.stale) setLoading(false);
    }
  }, []);

  useEffect(() => {
    if (!providerId) {
      setDetail(null);
      setCookieOptions([]);
      setRegionOptions([]);
      return;
    }
    // Clear stale detail immediately so we don't render the old provider
    setDetail(null);
    setCookieOptions([]);
    setRegionOptions([]);
    setCredentialStatus(null);
    const signal = { stale: false };
    void load(providerId, signal);
    return () => { signal.stale = true; };
  }, [providerId, load]);

  // Live-refresh when a new snapshot lands for this provider.
  useEffect(() => {
    if (!providerId) return;
    const signal = { stale: false };
    const unlistenPromise = listen<{ providerId?: string }>(
      "provider-updated",
      (event) => {
        const pid = event.payload?.providerId;
        if (!pid || pid === providerId) {
          void load(providerId, signal);
        }
      },
    );
    return () => {
      signal.stale = true;
      void unlistenPromise.then((fn) => fn());
    };
  }, [providerId, load]);

  if (!providerId) {
    return (
      <div className="provider-detail">
        <div className="provider-detail-empty">
          {t("StateNoProviderSelected")}
        </div>
      </div>
    );
  }

  if (loading && !detail) {
    return (
      <div className="provider-detail">
        <div className="provider-detail-empty">
          {t("StateLoadingProviders")}
        </div>
      </div>
    );
  }

  if (error && !detail) {
    return (
      <div className="provider-detail">
        <div className="provider-detail-empty provider-detail-empty--error">
          {t("StateError")}: {error}
        </div>
      </div>
    );
  }

  if (!detail) return null;

  const subtitle = buildSubtitle(detail, t);

  const handleRefresh = async () => {
    setBusy(true);
    try {
      await refreshProviders();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const handleSwitchAccount = async () => {
    setBusy(true);
    try {
      await triggerProviderLogin(detail.id);
      setCredentialRevision((value) => value + 1);
      await refreshProviders();
      await load(detail.id);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const handleRevokeCredentials = async () => {
    setBusy(true);
    setError(null);
    try {
      await revokeProviderCredentials(detail.id);
      setCredentialRevision((value) => value + 1);
      await load(detail.id);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const handleOpenDashboard = () => {
    void openProviderDashboard(detail.id).catch((e) => setError(String(e)));
  };

  const handleOpenStatusPage = () => {
    void openProviderStatusPage(detail.id).catch((e) => setError(String(e)));
  };

  const handleCopyError = () => {
    if (detail.lastError && navigator.clipboard) {
      void navigator.clipboard.writeText(detail.lastError);
    }
  };

  const handleBuyCredits = () => {
    if (detail.buyCreditsUrl) {
      void openProviderDashboard(detail.id).catch((e) => setError(String(e)));
    }
  };

  return (
    <div className="provider-detail">
      <IdentitySection provider={detail} subtitle={subtitle} t={t} />

      {detail.lastError && (
        <ProviderIssueNotice
          detail={detail}
          message={detail.lastError}
          onCopy={handleCopyError}
        />
      )}

      <UsageSection
        provider={detail}
        resetTimeRelative={resetTimeRelative}
        t={t}
      />
      <MenuBarMetricSection
        provider={detail}
        providerMetrics={providerMetrics}
        disabled={settingsDisabled}
        t={t}
        onChange={onSettingsChange}
      />
      <PaceSection pace={detail.pace} t={t} />
      <CostSection cost={detail.cost} t={t} />

      {/* Per-provider sub-sections ported in Phases 6c–6f. */}
      <CookieSourceSection
        providerId={detail.id}
        currentValue={detail.cookieSource}
        options={cookieOptions}
        t={t}
        onChanged={() => void load(detail.id)}
      />
      <RegionSection
        providerId={detail.id}
        currentValue={detail.region}
        options={regionOptions}
        t={t}
        onChanged={() => void load(detail.id)}
      />
      <CredentialsDispatcher providerId={detail.id} t={t} />
      <CredentialStorageSection
        status={credentialStatus}
        busy={busy}
        onRevoke={handleRevokeCredentials}
      />
      {tokenProviderIds.has(detail.id) && (
        <TokenAccountsPanel
          key={`token-${detail.id}-${credentialRevision}`}
          providerId={detail.id}
          compact
        />
      )}
      <ApiKeySection
        key={`api-${detail.id}-${credentialRevision}`}
        providerId={detail.id}
      />
      <CookieSection
        key={`cookie-${detail.id}-${credentialRevision}`}
        providerId={detail.id}
        cookieDomain={cookieDomain}
      />
      <ChartsSection
        providerId={detail.id}
        accountEmail={detail.email}
        t={t}
      />

      <QuickActionsSection
        provider={detail}
        busy={busy}
        onRefresh={handleRefresh}
        onSwitchAccount={handleSwitchAccount}
        onOpenDashboard={handleOpenDashboard}
        onOpenStatusPage={handleOpenStatusPage}
        onCopyError={handleCopyError}
        onBuyCredits={handleBuyCredits}
        t={t}
      />
    </div>
  );
}

function ProviderIssueNotice({
  detail,
  message,
  onCopy,
}: {
  detail: ProviderDetail;
  message: string;
  onCopy: () => void;
}) {
  const cleaned = message.replace(/^last fetch failed:\s*/i, "").trim();
  const lower = cleaned.toLowerCase();
  const needsLogin =
    lower.includes("auth.json not found") ||
    lower.includes("not signed in") ||
    lower.includes("credentials not found") ||
    lower.includes("oauth credentials not found") ||
    lower.includes("run `") ||
    lower.includes("run codex") ||
    lower.includes("run claude");
  const title = needsLogin
    ? `${detail.displayName} needs sign-in`
    : "Provider fetch needs attention";

  return (
    <div className="provider-detail-error" role="status">
      <div className="provider-detail-error__header">
        <strong>{title}</strong>
        <button
          type="button"
          className="provider-detail-error__copy"
          onClick={onCopy}
        >
          Copy
        </button>
      </div>
      <p>{cleaned}</p>
    </div>
  );
}

function CredentialStorageSection({
  status,
  busy,
  onRevoke,
}: {
  status: CredentialStorageStatus | null;
  busy: boolean;
  onRevoke: () => void;
}) {
  if (!status) return null;

  return (
    <section className="provider-detail-section provider-detail-credential-storage">
      <div className="provider-detail-section__header">
        <h4>Credential Storage</h4>
        <button
          className="credential-btn credential-btn--danger"
          disabled={busy}
          onClick={onRevoke}
        >
          Revoke Stored Credentials
        </button>
      </div>
      <dl className="provider-detail-grid provider-detail-grid--storage">
        <dt>API keys</dt>
        <dd>{storageLabel(status.apiKeys)}</dd>
        <dt>Manual cookies</dt>
        <dd>{storageLabel(status.manualCookies)}</dd>
        <dt>Token accounts</dt>
        <dd>{storageLabel(status.tokenAccounts)}</dd>
      </dl>
    </section>
  );
}

function storageLabel(value: string): string {
  if (value.startsWith("protected:")) {
    return `Protected (${value.slice("protected:".length)})`;
  }
  switch (value) {
    case "missing":
      return "Not created";
    case "plaintext":
      return "Plaintext";
    case "unavailable":
      return "Unavailable";
    case "unreadable":
      return "Unreadable";
    default:
      return value;
  }
}

function buildSubtitle(
  detail: ProviderDetail,
  t: (k: Parameters<ReturnType<typeof useLocale>["t"]>[0]) => string,
): string {
  const parts: string[] = [];
  if (detail.sourceLabel) parts.push(detail.sourceLabel);
  if (detail.lastUpdated) {
    const ago = relativeAgo(detail.lastUpdated);
    if (ago) parts.push(`${t("DetailUpdatedPrefix")} ${ago}`);
  } else if (!detail.hasSnapshot) {
    parts.push(t("ProviderUsageNotFetchedYet"));
  }
  return parts.join(" · ");
}

function relativeAgo(iso: string): string | null {
  const t = new Date(iso).getTime();
  if (!Number.isFinite(t)) return null;
  const diff = Date.now() - t;
  const secs = Math.round(Math.abs(diff) / 1000);
  if (secs < 60) return `${secs}s`;
  const mins = Math.round(secs / 60);
  if (mins < 60) return `${mins}m`;
  const hrs = Math.round(mins / 60);
  if (hrs < 24) return `${hrs}h`;
  return `${Math.round(hrs / 24)}d`;
}

/**
 * Dispatch the appropriate Phase-6d credential component based on the
 * current provider. Providers without a bespoke credentials UI render
 * nothing. Mirrors the `provider_id == ProviderId::*` chain in
 * `rust/src/native_ui/preferences.rs::render_provider_detail_panel`.
 */
function CredentialsDispatcher({
  providerId,
  t,
}: {
  providerId: string;
  t: ReturnType<typeof useLocale>["t"];
}) {
  switch (providerId) {
    case "gemini":
      return <GeminiCliCreds providerId={providerId} t={t} />;
    case "vertexai":
      return <VertexAiCreds providerId={providerId} t={t} />;
    case "jetbrains":
      return <JetBrainsCreds t={t} />;
    case "kiro":
      return <KiroCreds t={t} />;
    case "claude":
      return <ClaudeCreds t={t} />;
    case "codex":
      return <OpenAiExtras providerId={providerId} t={t} />;
    case "openaiapi":
      return <OpenAiExtras providerId={providerId} t={t} />;
    case "litellm":
    case "devin":
    case "zed":
      return <OpenAiExtras providerId={providerId} t={t} />;
    default:
      return null;
  }
}
