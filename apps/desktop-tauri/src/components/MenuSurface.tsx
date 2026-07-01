import type { ReactNode } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
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
      {variant === "popout" && <PopoutTitleBar />}
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

/**
 * Draggable title bar for the PopOut window mode. This app runs borderless
 * (no native caption), so the window can only be moved via a frontend drag
 * region — mirroring the detached Settings window. Minimize/close map to the
 * native window; close routes through Rust's CloseRequested handler, which
 * hides the window back to the tray instead of quitting.
 */
function PopoutTitleBar() {
  return (
    <div className="popout-titlebar" data-tauri-drag-region>
      <span className="popout-titlebar__title" data-tauri-drag-region>
        CodexBar
      </span>
      <div className="popout-titlebar__controls">
        <button
          type="button"
          className="popout-titlebar__control popout-titlebar__control--minimize"
          onClick={() => void getCurrentWindow().minimize()}
          aria-label="Minimize"
          title="Minimize"
        />
        <button
          type="button"
          className="popout-titlebar__control popout-titlebar__control--maximize"
          onClick={() => void getCurrentWindow().toggleMaximize()}
          aria-label="Maximize"
          title="Maximize"
        >
          <svg aria-hidden viewBox="0 0 16 16" focusable="false">
            <rect x="4.5" y="4.5" width="7" height="7" />
          </svg>
        </button>
        <button
          type="button"
          className="popout-titlebar__control popout-titlebar__control--close"
          onClick={() => void getCurrentWindow().close()}
          aria-label="Close"
          title="Close"
        >
          <svg aria-hidden viewBox="0 0 16 16" focusable="false">
            <path d="M4.5 4.5l7 7M11.5 4.5l-7 7" />
          </svg>
        </button>
      </div>
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
