import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import type { ClaudeSwapAccountsState } from "../../../../../types/bridge";
import type { LocaleKey } from "../../../../../i18n/keys";
import {
  claudeSwapAccountsList,
  claudeSwapAccountSwitch,
  getSettingsSnapshot,
  updateSettings,
} from "../../../../../lib/tauri";

interface Props {
  t: (key: LocaleKey) => string;
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

/**
 * External Claude subscription accounts read from the claude-swap (`cswap`)
 * executable (issue #477, port of upstream claude-swap Phase 1–2).
 *
 * This list is deliberately separate from the built-in saved Claude Code
 * accounts above: it is display + explicit activation only, is hidden while the
 * integration is disabled or unconfigured, and CodexBar never reads or stores
 * cswap credentials.
 */
export function ClaudeSwapAccountsSection({ t }: Props) {
  const [enabled, setEnabled] = useState(false);
  const [executablePath, setExecutablePath] = useState("");
  const [pathDraft, setPathDraft] = useState("");
  const [state, setState] = useState<ClaudeSwapAccountsState>(EMPTY_STATE);
  const [busy, setBusy] = useState(false);
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

  const runSettings = async (
    patch: { claudeSwapEnabled?: boolean; claudeSwapExecutablePath?: string },
  ) => {
    setBusy(true);
    setError(null);
    setMessage(null);
    try {
      await updateSettings(patch);
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

  const switchAccount = async (slot: number) => {
    setBusy(true);
    setError(null);
    setMessage(null);
    try {
      await claudeSwapAccountSwitch(slot);
      await reload();
      if (mounted.current) setMessage(t("ClaudeSwapSwitched"));
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
          disabled={busy}
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
          disabled={busy}
          placeholder={t("ClaudeSwapExecutablePathPlaceholder")}
          onChange={(e) => setPathDraft(e.target.value)}
          onBlur={() => void savePath()}
          onKeyDown={(e) => {
            if (e.key === "Enter") void savePath();
          }}
        />
      </label>
      {enabled && <p className="provider-detail-helper">{status}</p>}
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
            .join(" · ");
          return (
            <li className="credential-card" key={account.id}>
              <div className="credential-card__header">
                <div className="credential-card__info">
                  <strong>{account.label}</strong>
                  <span className="credential-card__meta">
                    {[session, weekly, scoped || null].filter(Boolean).join(" · ")}
                  </span>
                  {account.isActive && (
                    <span className="credential-card__badge credential-card__badge--set">
                      {t("TokenAccountActive")}
                    </span>
                  )}
                  {!account.isActive && account.error && (
                    <span className="credential-card__meta">{account.error}</span>
                  )}
                </div>
                <div className="credential-card__actions">
                  {!account.isActive && account.canActivate && (
                    <button
                      type="button"
                      className="credential-btn credential-btn--primary"
                      disabled={busy}
                      onClick={() => void switchAccount(account.slot)}
                    >
                      {t("ClaudeSwapSwitchButton")}
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
