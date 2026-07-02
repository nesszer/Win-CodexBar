import { useEffect, useState } from "react";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useLocale } from "../hooks/useLocale";

/**
 * Draggable title bar for the PopOut window mode. The app runs borderless (no
 * native caption), so the window is moved via this frontend drag region,
 * mirroring the detached Settings window. Controls map to the native window;
 * close routes through Rust's CloseRequested handler, which hides the window
 * back to the tray instead of quitting.
 *
 * This owns the desktop window-chrome concerns (Tauri window APIs, min/max/
 * close, drag region) so the shared `MenuSurface` content container stays a
 * pure presentation component with no window dependency.
 */
export default function PopOutTitleBar() {
  const { t } = useLocale();
  const [maximized, setMaximized] = useState(false);

  // Track the maximized state so the middle control (and the title-bar
  // double-click) toggle between Maximize and Restore, and so the button's
  // label/glyph announce the correct action.
  useEffect(() => {
    const win = getCurrentWindow();
    let active = true;
    let unlisten: (() => void) | undefined;
    const sync = () => {
      win
        .isMaximized()
        .then((value) => {
          if (active) setMaximized(value);
        })
        .catch(() => {});
    };
    sync();
    win
      .onResized(sync)
      .then((fn) => {
        if (active) {
          unlisten = fn;
        } else {
          fn();
        }
      })
      .catch(() => {});
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  const maximizeLabel = maximized ? t("WindowRestore") : t("WindowMaximize");

  return (
    <div
      className="popout-titlebar"
      data-tauri-drag-region
      onDoubleClick={(event) => {
        // Double-clicking the title bar toggles maximize/restore, like a native
        // caption — but not when the double-click lands on a window control.
        if (
          (event.target as HTMLElement).closest(".popout-titlebar__controls")
        ) {
          return;
        }
        void getCurrentWindow().toggleMaximize();
      }}
    >
      <span className="popout-titlebar__title" data-tauri-drag-region>
        CodexBar
      </span>
      <div className="popout-titlebar__controls">
        <button
          type="button"
          className="popout-titlebar__control popout-titlebar__control--minimize"
          onClick={() => void getCurrentWindow().minimize()}
          aria-label={t("WindowMinimize")}
          title={t("WindowMinimize")}
        />
        <button
          type="button"
          className="popout-titlebar__control popout-titlebar__control--maximize"
          onClick={() => void getCurrentWindow().toggleMaximize()}
          aria-label={maximizeLabel}
          title={maximizeLabel}
        >
          {maximized ? (
            <svg aria-hidden viewBox="0 0 16 16" focusable="false">
              <rect x="4.5" y="6" width="6" height="5.5" />
              <path d="M6.5 6V4.5H12V10h-1.5" />
            </svg>
          ) : (
            <svg aria-hidden viewBox="0 0 16 16" focusable="false">
              <rect x="4.5" y="4.5" width="7" height="7" />
            </svg>
          )}
        </button>
        <button
          type="button"
          className="popout-titlebar__control popout-titlebar__control--close"
          onClick={() => void getCurrentWindow().close()}
          aria-label={t("WindowClose")}
          title={t("WindowClose")}
        >
          <svg aria-hidden viewBox="0 0 16 16" focusable="false">
            <path d="M4.5 4.5l7 7M11.5 4.5l-7 7" />
          </svg>
        </button>
      </div>
    </div>
  );
}
