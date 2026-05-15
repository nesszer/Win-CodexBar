import { useEffect, useState } from "react";
import type {
  ProviderCatalogEntry,
  ProviderUsageSnapshot,
  SettingsUpdate,
} from "../../../types/bridge";
import type { BootstrapState } from "../../../types/bridge";
import { useLocale } from "../../../hooks/useLocale";
import type { LocaleKey } from "../../../i18n/keys";
import {
  ProvidersSidebar,
  type ProviderSidebarRow,
  type ProviderSidebarStatus,
} from "../providers/ProvidersSidebar";
import { ProviderDetailPane } from "../providers/ProviderDetailPane";
import { getCachedProviders, reorderProviders } from "../../../lib/tauri";

interface ProvidersTabProps {
  settings: BootstrapState["settings"];
  providers: ProviderCatalogEntry[];
  set: (patch: SettingsUpdate) => void;
  saving: boolean;
}

export default function ProvidersTab({
  settings,
  providers,
  set,
  saving,
}: ProvidersTabProps) {
  const { t } = useLocale();
  const [selectedId, setSelectedId] = useState<string | null>(
    providers[0]?.id ?? null,
  );
  const [snapshots, setSnapshots] = useState<ProviderUsageSnapshot[]>([]);
  // Locally-owned catalog order so drag-reorder feels instant before the
  // backend `reorder_providers` round-trip settles.
  const [orderedProviders, setOrderedProviders] =
    useState<ProviderCatalogEntry[]>(providers);

  useEffect(() => {
    setOrderedProviders(providers);
  }, [providers]);

  useEffect(() => {
    void getCachedProviders().then(setSnapshots);
  }, []);

  const enabled = new Set(settings.enabledProviders);

  const toggle = (id: string, on: boolean) => {
    const next = new Set(enabled);
    if (on) next.add(id);
    else next.delete(id);
    set({ enabledProviders: [...next].sort() });
  };

  const snapshotMap = new Map(snapshots.map((s) => [s.providerId, s]));

  const rows: ProviderSidebarRow[] = orderedProviders.map((p) => {
    const isOn = enabled.has(p.id);
    const snap = snapshotMap.get(p.id) ?? null;
    return {
      id: p.id,
      displayName: p.displayName,
      enabled: isOn,
      status: deriveProviderStatus(isOn, snap),
      subtitlePrimary: providerSidebarSubtitle(p.id, isOn, snap, t),
      subtitleSecondary: providerSidebarMetric(snap),
    };
  });

  const handleReorder = (ids: string[]) => {
    const byId = new Map(orderedProviders.map((p) => [p.id, p]));
    const next = ids
      .map((id) => byId.get(id))
      .filter((p): p is ProviderCatalogEntry => Boolean(p));
    setOrderedProviders(next);
    void reorderProviders(ids).catch(() => {
      setOrderedProviders(providers);
    });
  };

  const selectedEntry =
    orderedProviders.find((p) => p.id === selectedId) ?? null;

  return (
    <div className="provider-split">
      <ProvidersSidebar
        providers={rows}
        selectedId={selectedId}
        onSelect={setSelectedId}
        onReorder={handleReorder}
        onToggleEnabled={toggle}
        disabled={saving}
      />
      <ProviderDetailPane
        providerId={selectedId}
        cookieDomain={selectedEntry?.cookieDomain ?? null}
        resetTimeRelative={settings.resetTimeRelative}
        providerMetrics={settings.providerMetrics}
        settingsDisabled={saving}
        onSettingsChange={set}
      />
    </div>
  );
}

// ── Provider sidebar subtitle helpers (port of
//    rust/src/native_ui/preferences.rs::provider_sidebar_subtitle). ─────

function deriveProviderStatus(
  isEnabled: boolean,
  snap: ProviderUsageSnapshot | null,
): ProviderSidebarStatus {
  if (!isEnabled) return "disabled";
  if (!snap) return "loading";
  if (snap.error) return "error";
  const updatedMs = new Date(snap.updatedAt).getTime();
  if (Number.isFinite(updatedMs)) {
    const ageMins = (Date.now() - updatedMs) / 60_000;
    if (ageMins > 10) return "stale";
  }
  return "ok";
}

/**
 * Minimal port of `provider_sidebar_source_hint`
 * (rust/src/native_ui/preferences.rs:3753). When we have a live snapshot the
 * backend-supplied `sourceLabel` wins; otherwise we fall back to the neutral
 * "Not detected" / "Disabled" copy.
 */
function providerSidebarSubtitle(
  providerId: string,
  isEnabled: boolean,
  snap: ProviderUsageSnapshot | null,
  t: (key: LocaleKey) => string,
): string {
  if (!isEnabled) {
    return `${t("ProviderDisabled")} — ${providerSourceHintShort(providerId, t)}`;
  }
  if (!snap) {
    return t("ProviderNotDetected");
  }
  const source = snap.sourceLabel || providerSourceHintShort(providerId, t);
  return source;
}

function providerSourceHintShort(
  providerId: string,
  t: (key: LocaleKey) => string,
): string {
  const id = providerId.toLowerCase();
  switch (id) {
    case "cursor":
    case "factory":
    case "droid":
    case "kimi":
    case "kimik2":
    case "augment":
    case "opencode":
    case "amp":
    case "ollama":
    case "alibaba":
    case "infini":
    case "manus":
    case "mimo":
    case "commandcode":
      return t("ProviderSourceWebShort");
    case "gemini":
    case "antigravity":
    case "jetbrains":
      return t("ProviderSourceCliShort");
    case "copilot":
      return t("ProviderSourceGithubApiShort");
    case "zai":
    case "vertexai":
    case "openrouter":
    case "bedrock":
    case "synthetic":
    case "nanogpt":
    case "warp":
    case "doubao":
    case "crof":
    case "stepfun":
    case "venice":
    case "openaiapi":
      return t("ProviderSourceApiShort");
    case "kiro":
      return t("ProviderSourceKiroEnvShort");
    case "claude":
    case "codex":
    case "minimax":
    default:
      return t("ProviderSourceAutoShort");
  }
}

function providerSidebarMetric(
  snap: ProviderUsageSnapshot | null,
): string | undefined {
  if (!snap) return undefined;
  const rate = snap.primary;
  if (!rate) return undefined;
  if (rate.isExhausted) return "100%";
  if (Number.isFinite(rate.usedPercent)) {
    return `${Math.round(Math.min(100, rate.usedPercent))}%`;
  }
  return undefined;
}
