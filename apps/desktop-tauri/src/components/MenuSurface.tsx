import type { CSSProperties, ReactNode } from "react";
import { useLocale } from "../hooks/useLocale";

export type MenuRowId =
  | "switchAccount"
  | "dashboard"
  | "statusPage"
  | "refresh"
  | "settings"
  | "about"
  | "quit";

export interface MenuFooterRow {
  id: MenuRowId;
  label: string;
  shortcut?: string;
  onClick: () => void;
}

function lineIcon(...paths: ReactNode[]) {
  return (
    <svg
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      strokeWidth={1.2}
      strokeLinecap="round"
      strokeLinejoin="round"
    >
      {paths}
    </svg>
  );
}

/** 16px line icons after the SF Symbols the macOS menu uses
 *  (key, chart.xyaxis.line, waveform.path.ecg, gearshape, info.circle,
 *  xmark.rectangle). Refresh has no icon on the Mac. */
const ROW_ICONS: Record<MenuRowId, ReactNode> = {
  switchAccount: lineIcon(
    <circle key="bow" cx="8" cy="4.6" r="3" />,
    <path key="blade" d="M8 7.6V14.5M8 11.4H9.8M8 13.2H9.4" />,
  ),
  dashboard: lineIcon(
    <path key="axes" d="M2.5 2.5V13.5H13.5" />,
    <path key="line" d="M4.5 10.5L7 7.5L9 9L12.5 4.5" />,
  ),
  statusPage: lineIcon(<path key="ecg" d="M1.5 8.5H4L5.5 4.5L8 12L10 6.5L11.2 8.5H14.5" />),
  refresh: null,
  settings: lineIcon(
    <path
      key="gear"
      d="M6.9 1.8H9.1L9.5 3.5L10.9 4.3L12.6 3.8L13.7 5.7L12.5 7V9L13.7 10.3L12.6 12.2L10.9 11.7L9.5 12.5L9.1 14.2H6.9L6.5 12.5L5.1 11.7L3.4 12.2L2.3 10.3L3.5 9V7L2.3 5.7L3.4 3.8L5.1 4.3L6.5 3.5Z"
    />,
    <circle key="hub" cx="8" cy="8" r="2" />,
  ),
  about: lineIcon(
    <circle key="ring" cx="8" cy="8" r="6.5" />,
    <path key="stem" d="M8 7.3V11.5" />,
    <circle key="dot" cx="8" cy="5" r="0.4" fill="currentColor" />,
  ),
  quit: lineIcon(
    <rect key="frame" x="1.5" y="3" width="13" height="10" rx="2" />,
    <path key="cross" d="M6 6L10 10M10 6L6 10" />,
  ),
};

interface MenuSurfaceProps {
  summary?: ReactNode;
  banner?: ReactNode;
  /** Footer rows in groups. A separator is drawn before each non-empty
   *  group, like the separators between the macOS menu sections. */
  footerGroups?: MenuFooterRow[][];
  /** Inline style applied to the root `menu-surface` element (e.g. CSS
   *  `zoom` for the tray flyout). */
  style?: CSSProperties;
  children: ReactNode;
}

/**
 * Flush, compact container for the tray panel (`TrayPanel`), the only
 * dashboard layout. It renders in the tray-panel flyout window.
 *
 * Mirrors the upstream macOS `MenuContent`: a 310pt menu panel holding a
 * stack of full provider cards (`MenuCard`), one per enabled provider,
 * followed by the menu rows.
 */
export default function MenuSurface({
  summary,
  banner,
  footerGroups = [],
  style,
  children,
}: MenuSurfaceProps) {
  const { t } = useLocale();
  const groups = footerGroups.filter((group) => group.length > 0);
  return (
    <div className="menu-surface menu-surface--tray" style={style}>
      {banner}
      {summary}
      <div className="menu-surface__body">{children}</div>
      {groups.length > 0 && (
        <nav className="menu-surface__footer" aria-label={t("PanelMenu")}>
          {groups.map((group) => [
            <div key={`sep-${group[0].id}`} className="menu-surface__footer-sep" />,
            ...group.map((row) => {
              const icon = ROW_ICONS[row.id];
              return (
                <button
                  key={row.id}
                  type="button"
                  className={`menu-surface__footer-row${row.id === "refresh" ? " menu-surface__footer-row--refresh" : ""}`}
                  aria-keyshortcuts={row.shortcut?.replace("Ctrl+", "Control+")}
                  onClick={row.onClick}
                >
                  {icon && (
                    <span className="menu-surface__footer-icon" aria-hidden>
                      {icon}
                    </span>
                  )}
                  <span>{row.label}</span>
                  {row.shortcut && (
                    <span className="menu-surface__footer-shortcut" aria-hidden>
                      {row.shortcut}
                    </span>
                  )}
                </button>
              );
            }),
          ])}
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
