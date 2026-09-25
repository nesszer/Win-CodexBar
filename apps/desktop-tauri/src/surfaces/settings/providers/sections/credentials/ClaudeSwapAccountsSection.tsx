import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type {
  ClaudeSwapAccount,
  ClaudeSwapAccountsState,
  ClaudeSwapHistoricalUsage,
  ClaudeSwapSpendWindow,
  Language,
} from "../../../../../types/bridge";
import type { LocaleKey } from "../../../../../i18n/keys";
import {
  claudeSwapAccountsList,
  claudeSwapAccountReauthenticate,
  claudeSwapAccountSwitch,
  getSettingsSnapshot,
  updateSettings,
} from "../../../../../lib/tauri";
import {
  localClaudeReconciliationOutcome,
  useClaudeReconciliation,
} from "../../../../../hooks/useClaudeReconciliation";

interface Props {
  t: (key: LocaleKey) => string;
  language?: Language;
}

const EMPTY_STATE: ClaudeSwapAccountsState = {
  enabled: false,
  executableConfigured: false,
  accounts: [],
  error: null,
};

function usageLabel(
  t: (key: LocaleKey) => string,
  key: LocaleKey,
  window: { usedPercent: number } | null,
): string | null {
  if (!window) return null;
  return `${t(key)} ${Math.round(window.usedPercent)}%`;
}

function spendLabel(
  t: (key: LocaleKey) => string,
  spend: ClaudeSwapSpendWindow | null,
  locale: string,
): string | null {
  if (!spend) return null;
  const formatter = new Intl.NumberFormat(locale, {
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  });
  const currency = spend.currencyCode ? ` ${spend.currencyCode}` : "";
  return `${t("ClaudeSwapSpend")} ${formatter.format(spend.used)} / ${formatter.format(spend.limit)}${currency} (${Math.round(spend.usedPercent)}%)`;
}

function historicalLabel(
  t: (key: LocaleKey) => string,
  history: ClaudeSwapHistoricalUsage | null,
  locale: string,
): string | null {
  if (!history) return null;
  const windows = [
    usageLabel(t, "ProviderSession", history.fiveHour),
    usageLabel(t, "ProviderWeekly", history.sevenDay),
    ...history.scoped.map((window) => `${window.name} ${Math.round(window.usedPercent)}%`),
    spendLabel(t, history.spend, locale),
  ].filter(Boolean);
  const captured = new Intl.DateTimeFormat(locale, {
    dateStyle: "short",
    timeStyle: "short",
  }).format(new Date(history.fetchedAt));
  return `${t("ClaudeSwapHistoricalUsage")}: ${windows.join(" · ")} (${t("ClaudeSwapHistoricalCapturedAt")} ${captured})`;
}

function languageLocale(language: Language): string {
  return {
    english: "en-US",
    chinese: "zh-CN",
    chinesetraditional: "zh-TW",
    japanese: "ja-JP",
    korean: "ko-KR",
    spanish: "es-MX",
    portuguesebrazil: "pt-BR",
    russian: "ru-RU",
    turkish: "tr-TR",
  }[language];
}

/**
 * External Claude subscription accounts read from the claude-swap (`cswap`)
 * executable (issue #477, port of upstream claude-swap Phase 1-2).
 *
 * This list is deliberately separate from the built-in saved Claude Code
 * accounts above: it is display + explicit activation only, is hidden while the
 * integration is disabled or unconfigured, and CodexBar never reads or stores
 * cswap credentials.
 */
export function ClaudeSwapAccountsSection({ t, language = "english" }: Props) {
  const [enabled, setEnabled] = useState(false);
  const [executablePath, setExecutablePath] = useState("");
  const [pathDraft, setPathDraft] = useState("");
  const [state, setState] = useState<ClaudeSwapAccountsState>(EMPTY_STATE);
  const locale = languageLocale(language);
  const [busy, setBusy] = useState(false);
  const [operation, setOperation] = useState<{ generation: number; success: LocaleKey } | null>(null);
  const { snapshot, accept, reconciling } = useClaudeReconciliation();
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const mounted = useRef(false);

  const reload = useCallback(async () => {
    const next = await claudeSwapAccountsList();
    if (mounted.current) {
      setState(next);
      setEnabled(next.enabled);
    }
  }, []);

  useEffect(() => {
    mounted.current = true;
    void getSettingsSnapshot()
      .then((settings) => {
        if (!mounted.current) return;
        setEnabled(settings.claudeSwapEnabled ?? false);
        const path = settings.claudeSwapExecutablePath ?? "";
        setExecutablePath(path);
        setPathDraft(path);
      })
      .catch((e) => {
        if (mounted.current) setError(String(e));
      });
    const load = () => {
      void reload().catch((e) => {
        if (mounted.current) setError(String(e));
      });
    };
    load();
    const unlisten = listen("claude-accounts-updated", load);
    return () => {
      mounted.current = false;
      void unlisten.then((fn) => fn()).catch(() => {});
    };
  }, [reload]);

  useEffect(() => {
    const outcome = localClaudeReconciliationOutcome(snapshot, operation?.generation ?? null);
    if (!outcome) return;
    if (outcome.status === "failed") {
      setMessage(null);
      setError(outcome.detail);
    } else if (operation) {
      setMessage(t(operation.success));
      setError(null);
    }
  }, [operation, snapshot, t]);

  const runSettings = async (
    patch: { claudeSwapEnabled?: boolean; claudeSwapExecutablePath?: string },
  ) => {
    setBusy(true);
    setError(null);
    setMessage(null);
    try {
      await updateSettings(patch);
      // Persist the accepted path in local state so a later blur with the same
      // value is a no-op, while a failed save leaves the draft untouched and
      // remains retryable.
      if (mounted.current && patch.claudeSwapExecutablePath !== undefined) {
        setExecutablePath(patch.claudeSwapExecutablePath);
      }
      await reload();
    } catch (e) {
      if (mounted.current) setError(String(e));
    } finally {
      if (mounted.current) setBusy(false);
    }
  };

  const savePath = async () => {
    const next = pathDraft.trim();
    if (next === executablePath.trim()) return;
    await runSettings({ claudeSwapExecutablePath: next });
  };

  const runAccountAction = async (account: ClaudeSwapAccount) => {
    setBusy(true);
    setError(null);
    setMessage(null);
    try {
      const success = account.action === "reauthenticate"
        ? "ClaudeSwapReauthenticated"
        : "ClaudeSwapSwitched";
      let result;
      if (account.action === "reauthenticate") {
        result = await claudeSwapAccountReauthenticate(account.slot);
      } else if (account.action === "switch") {
        result = await claudeSwapAccountSwitch(account.slot);
      } else {
        throw new Error("This claude-swap account is not actionable.");
      }
      setOperation({ generation: result.generation, success });
      accept(result);
    } catch (e) {
      if (mounted.current) setError(String(e));
    } finally {
      if (mounted.current) setBusy(false);
    }
  };

  const status = !enabled
    ? t("ClaudeSwapStatusDisabled")
    : !state.executableConfigured
      ? t("ClaudeSwapStatusNoExecutable")
      : t("ClaudeSwapStatusReady");

  return (
    <section className="provider-detail-section claude-swap-accounts">
      <h4>{t("ClaudeSwapTitle")}</h4>
      <p className="settings-section__hint">{t("ClaudeSwapHint")}</p>
      <label className="provider-detail-toggle">
        <input
          type="checkbox"
          checked={enabled}
          disabled={busy || reconciling}
          onChange={(e) => void runSettings({ claudeSwapEnabled: e.target.checked })}
        />
        <span>
          <span className="provider-detail-toggle__label">{t("ClaudeSwapEnable")}</span>
          <span className="provider-detail-toggle__helper">{t("ClaudeSwapEnableHelp")}</span>
        </span>
      </label>
      <label className="provider-detail-field">
        <span className="provider-detail-field__label">{t("ClaudeSwapExecutablePath")}</span>
        <input
          type="text"
          className="provider-detail-field__input"
          value={pathDraft}
          disabled={busy || reconciling}
          placeholder={t("ClaudeSwapExecutablePathPlaceholder")}
          onChange={(e) => setPathDraft(e.target.value)}
          onBlur={() => void savePath()}
          onKeyDown={(e) => {
            if (e.key === "Enter") void savePath();
          }}
        />
      </label>
      {enabled && <p className="provider-detail-helper">{status}</p>}
      {reconciling && <p className="provider-detail-helper" role="status">{t("ClaudeSwapReconciling")}</p>}
      {error && (
        <div className="provider-detail-error" role="alert">
          {error}
        </div>
      )}
      {state.error && (
        <div className="provider-detail-error" role="alert">
          {state.error}
        </div>
      )}
      {message && (
        <div className="provider-detail-note" role="status">
          {message}
        </div>
      )}
      {enabled && state.executableConfigured && state.accounts.length === 0 && !state.error && (
        <p>{t("ClaudeSwapEmpty")}</p>
      )}
      <ul className="credential-list">
        {state.accounts.map((account) => {
          const session = usageLabel(t, "ProviderSession", account.fiveHour);
          const weekly = usageLabel(t, "ProviderWeekly", account.sevenDay);
          const scoped = account.scoped
            .map((window) => `${window.name} ${Math.round(window.usedPercent)}%`)
            .join(" \u00b7 ");
          const spend = spendLabel(t, account.spend, locale);
          const historical = historicalLabel(t, account.historicalUsage, locale);
          return (
            <li className="credential-card" key={account.id}>
              <div className="credential-card__header">
                <div className="credential-card__info">
                  <strong>{account.label}</strong>
                  <span className="credential-card__meta">
                    {[session, weekly, scoped || null, spend].filter(Boolean).join(" \u00b7 ") ||
                      (!account.error ? t("ClaudeSwapUsageUnavailable") : "")}
                  </span>
                  {historical && <span className="credential-card__meta">{historical}</span>}
                  {account.isActive && (
                    <span className="credential-card__badge credential-card__badge--set">
                      {t("TokenAccountActive")}
                    </span>
                  )}
                  {account.isDisabled && (
                    <span className="credential-card__meta">{t("ClaudeSwapDisabled")}</span>
                  )}
                  {account.error && (
                    <span className="credential-card__meta">{account.error}</span>
                  )}
                </div>
                <div className="credential-card__actions">
                  {account.action && (
                    <button
                      type="button"
                      className="credential-btn credential-btn--primary"
                      disabled={busy || reconciling}
                      onClick={() => void runAccountAction(account)}
                    >
                      {t(
                        account.action === "reauthenticate"
                          ? "ClaudeSwapReauthenticateButton"
                          : "ClaudeSwapSwitchButton",
                      )}
                    </button>
                  )}
                </div>
              </div>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
