import type { ReactNode } from "react";
import { useLocale } from "../hooks/useLocale";

export interface MenuSurfaceAction {
  icon: string;
  title: string;
  onClick: () => void;
}

export interface MenuFooterRow {
  icon: string;
  label: string;
  shortcut?: string;
  onClick: () => void;
}

interface MenuSurfaceProps {
  variant: "tray" | "popout";
  /** Optional window chrome (e.g. the PopOut title bar) rendered flush at the
   *  top. A slot keeps this shared content container free of window APIs. */
  titleBar?: ReactNode;
  onRefresh: () => void;
  isRefreshing: boolean;
  actions: MenuSurfaceAction[];
  summary?: ReactNode;
  banner?: ReactNode;
  footerRows?: MenuFooterRow[];
  children: ReactNode;
}

/**
 * Flush, compact container that both `TrayPanel` and `PopOutPanel` consume.
 *
 * Mirrors the upstream macOS `MenuContent`: a narrow VStack(spacing: 8)
 * inside an NSMenu-like popover (310pt wide, vertical 6 / horizontal 10
 * padding, no hero framing). The body holds a stack of full provider
 * cards (`MenuCard`) — one per enabled provider — exactly like upstream.
 */
export default function MenuSurface({
  variant,
  titleBar,
  onRefresh,
  isRefreshing,
  actions,
  summary,
  banner,
  footerRows,
  children,
}: MenuSurfaceProps) {
  return (
    <div className={`menu-surface menu-surface--${variant}`}>
      {titleBar}
      {banner}
      {summary}
      <div className="menu-surface__body">{children}</div>
      {footerRows && footerRows.length > 0 && (
        <nav className="menu-surface__footer" aria-label="Menu">
          {footerRows.map((row) => (
            <button
              key={row.label}
              type="button"
              className={`menu-surface__footer-row${row.icon ? "" : " menu-surface__footer-row--no-icon"}`}
              onClick={row.onClick}
            >
              {row.icon && (
                <span className="menu-surface__footer-icon" aria-hidden>
                  {row.icon}
                </span>
              )}
              <span>{row.label}</span>
              {row.shortcut && (
                <span className="menu-surface__footer-shortcut">{row.shortcut}</span>
              )}
            </button>
          ))}
        </nav>
      )}
    </div>
  );
}

interface MenuSummaryProps {
  total: number;
  errorCount: number;
  isRefreshing: boolean;
  lastRefresh: { providerCount: number; errorCount: number } | null;
}

export function MenuSummary({
  total,
  errorCount,
  isRefreshing,
  lastRefresh,
}: MenuSummaryProps) {
  const { t } = useLocale();
  const providersLabel = t("SummaryProvidersLabel");
  const providerLabel =
    total === 1 && providersLabel.toLocaleLowerCase("en-US") === "providers"
      ? "provider"
      : providersLabel;
  const parts: string[] = [`${total} ${providerLabel}`];
  if (isRefreshing) {
    parts.push(t("SummaryRefreshing"));
  } else if (lastRefresh && lastRefresh.errorCount > 0) {
    parts.push(`${lastRefresh.errorCount} ${t("SummaryFailed")}`);
  }
  if (!isRefreshing && errorCount > 0) {
    parts.push(`${errorCount} ${t("SummaryWithErrors")}`);
  }
  return <div className="menu-surface__summary">{parts.join(" · ")}</div>;
}

interface MenuEmptyProps {
  isLoading: boolean;
  onSettings: () => void;
}

export function MenuEmpty({ isLoading, onSettings }: MenuEmptyProps) {
  const { t } = useLocale();

  if (isLoading) {
    return (
      <div className="menu-surface__empty">
        <div className="menu-surface__spinner" />
        <p>{t("FetchingProviderData")}</p>
      </div>
    );
  }

  return (
    <div className="menu-surface__empty">
      <p>{t("NoProvidersConfigured")}</p>
      <p className="menu-surface__hint">{t("EnableProvidersHint")}</p>
      <button
        className="menu-surface__primary-btn"
        onClick={onSettings}
        type="button"
      >
        {t("OpenSettingsButton")}
      </button>
    </div>
  );
}
