import type { CSSProperties, ReactNode } from "react";
import { useLocale } from "../hooks/useLocale";

export interface MenuFooterRow {
  icon: string;
  label: string;
  shortcut?: string;
  onClick: () => void;
}

interface MenuSurfaceProps {
  summary?: ReactNode;
  banner?: ReactNode;
  /** Non-button content rendered in the footer nav BEFORE the mapped
   *  `footerRows` (e.g. the tray zoom control). Rendering it here — rather
   *  than as a `footerRows` entry — keeps it a plain `div`, not a `button`. */
  footerLead?: ReactNode;
  footerRows?: MenuFooterRow[];
  /** Inline style applied to the root `menu-surface` element (e.g. CSS
   *  `zoom` for the tray flyout). */
  style?: CSSProperties;
  children: ReactNode;
}

/**
 * Flush, compact container for the tray panel (`TrayPanel`), the only
 * dashboard layout. It renders in the tray-panel flyout window.
 *
 * Mirrors the upstream macOS `MenuContent`: a narrow VStack(spacing: 8)
 * inside an NSMenu-like popover (310pt wide, vertical 6 / horizontal 10
 * padding, no hero framing). The body holds a stack of full provider
 * cards (`MenuCard`) — one per enabled provider — exactly like upstream.
 */
export default function MenuSurface({
  summary,
  banner,
  footerLead,
  footerRows,
  style,
  children,
}: MenuSurfaceProps) {
  const { t } = useLocale();
  return (
    <div className="menu-surface menu-surface--tray" style={style}>
      {banner}
      {summary}
      <div className="menu-surface__body">{children}</div>
      {(footerLead || (footerRows && footerRows.length > 0)) && (
        <nav className="menu-surface__footer" aria-label={t("PanelMenu")}>
          {footerLead}
          {footerRows?.map((row) => (
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
