import { useCallback, useEffect, useRef, useState } from "react";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";
import {
  getWorkAreaRect,
  reanchorTrayPanel,
  revealTrayPanelWindow,
} from "../lib/tauri";
import {
  decideTrayHeight,
  EMPTY_AUTOFIT_STATE,
  recordAutoFitCommit,
  type TrayAutoFitState,
} from "../lib/traySizing";

/** The macOS 0.70 menu width (tray-card-spec.md section 1) at 100% Panel scale. */
const TRAY_BASE_WIDTH = 310;
const TRAY_MAX_MEASURE_HEIGHT = 920;
const TRAY_OVERVIEW_MIN_HEIGHT = 200;
const TRAY_DETAIL_MIN_HEIGHT = 420;
const TRAY_DENSE_OVERVIEW_HEIGHT = 776;

export interface TrayPanelLayoutOptions {
  canMeasure: boolean;
  denseOverview: boolean;
  detailMode: boolean;
  layoutKey: string;
  /** Panel scale as a factor: the CSS `zoom` TrayPanel puts on the surface.
   *  The window width and height follow it. Defaults to 1. */
  zoom?: number;
}

export interface TrayPanelLayout {
  layoutReady: boolean;
  requestLayout: () => void;
}

/**
 * Fits the tray flyout window to its content: a fixed width of 310 px times
 * the Panel scale, and a height measured from the rendered surface.
 */
export function useTrayPanelLayout({
  canMeasure,
  denseOverview,
  detailMode,
  layoutKey,
  zoom = 1,
}: TrayPanelLayoutOptions): TrayPanelLayout {
  const [layoutReady, setLayoutReady] = useState(false);
  const [layoutRevision, setLayoutRevision] = useState(0);
  const layoutReadyRef = useRef(false);
  const resizeRunRef = useRef(0);
  const layoutTimerRef = useRef<number | undefined>(undefined);
  // The window's actual physical size after our last resize. Win32 may snap
  // a requested size, and lib/traySizing compares candidates against it.
  const lastSizeRef = useRef<{ width: number; height: number } | null>(null);
  const programmaticInFlightRef = useRef(0);
  // Sizing decision state (committed frame, one-frame history, learned
  // oscillation pair). The #261 cycle detection lives in lib/traySizing.
  const sizingStateRef = useRef<TrayAutoFitState>(EMPTY_AUTOFIT_STATE);

  const applySize = useCallback(async (size: LogicalSize): Promise<void> => {
    const win = getCurrentWindow();
    try {
      await win.setSize(size);
      const actual = await win.innerSize();
      lastSizeRef.current = { width: actual.width, height: actual.height };
    } catch {
      /* ignore */
    }
  }, []);

  const requestLayout = useCallback(() => {
    if (layoutTimerRef.current !== undefined) {
      window.clearTimeout(layoutTimerRef.current);
    }
    layoutTimerRef.current = window.setTimeout(() => {
      setLayoutRevision((current) => current + 1);
    }, layoutReadyRef.current ? 100 : 16);
  }, []);

  useEffect(() => {
    requestLayout();
  }, [layoutKey, requestLayout]);

  useEffect(() => {
    const surface = document.querySelector<HTMLElement>(".menu-surface--tray");
    if (!surface || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => {
      // Measuring temporarily removes the surface/body constraints, which
      // resizes the observed elements. Do not feed that programmatic change
      // back into another pass or the capped flyout flashes between its
      // measured and committed layouts forever.
      if (
        layoutReadyRef.current &&
        programmaticInFlightRef.current > 0
      ) {
        return;
      }
      requestLayout();
    });
    // The surface fills the window, so content changes show up on the body
    // and the footer rather than on the surface itself.
    for (const part of [
      surface,
      surface.querySelector(".menu-surface__body"),
      surface.querySelector(".menu-surface__footer"),
    ]) {
      if (part) observer.observe(part);
    }
    return () => observer.disconnect();
  }, [requestLayout]);

  useEffect(() => {
    return () => {
      if (layoutTimerRef.current !== undefined) {
        window.clearTimeout(layoutTimerRef.current);
      }
    };
  }, []);

  useEffect(() => {
    if (!canMeasure) return;

    const minHeight = detailMode
      ? TRAY_DETAIL_MIN_HEIGHT
      : denseOverview
        ? TRAY_DENSE_OVERVIEW_HEIGHT
        : TRAY_OVERVIEW_MIN_HEIGHT;
    const width = Math.round(TRAY_BASE_WIDTH * zoom);

    const resize = async () => {
      const run = ++resizeRunRef.current;
      const surface = document.querySelector<HTMLElement>(".menu-surface--tray");
      if (!surface) return;
      const html = document.documentElement;
      const pageBody = document.body;
      const workArea = await getWorkAreaRect().catch(() => null);
      const maxHeight = Math.max(
        minHeight,
        Math.min(
          TRAY_MAX_MEASURE_HEIGHT,
          (workArea?.height ?? TRAY_MAX_MEASURE_HEIGHT) - 16,
        ),
      );

      const body = surface.querySelector<HTMLElement>(".menu-surface__body");
      const stack = surface.querySelector<HTMLElement>(".menu-stack");
      const footer = surface.querySelector<HTMLElement>(".menu-surface__footer");

      const previous = {
        htmlOverflow: html.style.overflow,
        bodyOverflow: pageBody.style.overflow,
        bodyMinHeight: pageBody.style.minHeight,
        surfaceMinHeight: surface.style.minHeight,
        surfaceHeight: surface.style.height,
        surfaceMaxHeight: surface.style.maxHeight,
        surfaceOverflow: surface.style.overflow,
        bodyInnerOverflow: body?.style.overflow,
        bodyFlex: body?.style.flex,
        stackOverflow: stack?.style.overflow,
      };
      let committedHeight = false;

      html.style.overflow = "visible";
      pageBody.style.overflow = "visible";
      pageBody.style.minHeight = "0";
      surface.style.minHeight = "0";
      surface.style.height = "auto";
      surface.style.maxHeight = "none";
      surface.style.overflow = "visible";
      if (body) {
        body.style.overflow = "visible";
        body.style.flex = "0 0 auto";
      }
      if (stack) {
        stack.style.overflow = "visible";
      }

      const revealPanel = async () => {
        if (run !== resizeRunRef.current) return;
        layoutReadyRef.current = true;
        setLayoutReady(true);
        await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
        if (run === resizeRunRef.current) {
          await Promise.resolve(revealTrayPanelWindow()).catch(() => {});
        }
      };

      // Every resize this pass causes, and any that arrive shortly after,
      // must not start another pass. The trailing delay absorbs late events.
      programmaticInFlightRef.current += 1;
      try {
        // Text wraps at the window width, so the width has to be in place
        // before the height is measured.
        const committed = sizingStateRef.current.committed;
        if (!layoutReadyRef.current || committed?.width !== width) {
          const height =
            layoutReadyRef.current && committed ? committed.height : minHeight;
          sizingStateRef.current = recordAutoFitCommit(
            sizingStateRef.current,
            width,
            height,
            window.devicePixelRatio,
          );
          await applySize(new LogicalSize(width, height));
        }

        await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));
        await new Promise<void>((resolve) => requestAnimationFrame(() => resolve()));

        if (run !== resizeRunRef.current) return;

        // WebView2 reports bounding rects in rendered px but scrollHeight in
        // the zoomed surface's local px (measured on Edge 154), so the local
        // value is scaled before the two are compared. The surface fills the
        // window and parks spare height above the footer; that gap is not
        // content. The extra 1 px absorbs DPI rounding of the window height.
        const surfaceRect = surface.getBoundingClientRect();
        const bodyRect = body?.getBoundingClientRect();
        const footerRect = footer?.getBoundingClientRect();
        const spare =
          bodyRect && footerRect
            ? Math.max(0, footerRect.top - bodyRect.bottom)
            : 0;
        const contentBottom = Math.max(
          surfaceRect.top + surface.scrollHeight * zoom,
          surfaceRect.bottom,
        );
        const height = Math.min(
          Math.max(Math.ceil(contentBottom - spare) + 1, minHeight),
          maxHeight,
        );

        // #261: two-state cycle detection on physical targets. Normal rule
        // commits ANY real change (even +5 physical px); only exact same-
        // frame equality is a no-op, and a bounded A→B→A pair (span ≤8
        // physical, the reporter's 7) locks onto its larger member. The DOM
        // constraint is set AFTER the decision from the RETAINED height, so
        // surface and window never diverge.
        const decision = decideTrayHeight(
          {
            measuredHeight: height,
            expectedWidth: width,
            minHeight,
            maxHeight,
            // WebView layout px ↔ Win32 physical px ratio; CSS zoom does not
            // affect it.
            scaleFactor: window.devicePixelRatio,
            zoom,
            lastAppliedPhysicalHeight: lastSizeRef.current?.height ?? null,
          },
          sizingStateRef.current,
        );
        sizingStateRef.current = decision.state;
        // A zoomed element's max-height renders at value × zoom.
        surface.style.maxHeight = `${decision.height / zoom}px`;
        committedHeight = true;

        if (decision.commit) {
          await applySize(new LogicalSize(width, decision.height));
          await Promise.resolve(reanchorTrayPanel()).catch(() => {});
        }

        await revealPanel();
      } catch (error) {
        console.warn("CodexBar tray panel resize failed", error);
        void revealPanel();
      } finally {
        if (!committedHeight) {
          surface.style.maxHeight = previous.surfaceMaxHeight;
        }
        surface.style.minHeight = previous.surfaceMinHeight;
        surface.style.height = previous.surfaceHeight;
        surface.style.overflow = previous.surfaceOverflow;
        html.style.overflow = previous.htmlOverflow;
        pageBody.style.overflow = previous.bodyOverflow;
        pageBody.style.minHeight = previous.bodyMinHeight;
        if (body) {
          body.style.overflow = previous.bodyInnerOverflow ?? "";
          body.style.flex = previous.bodyFlex ?? "";
        }
        if (stack) {
          stack.style.overflow = previous.stackOverflow ?? "";
        }
        window.setTimeout(() => {
          programmaticInFlightRef.current = Math.max(
            0,
            programmaticInFlightRef.current - 1,
          );
        }, 200);
      }
    };

    const timer = window.setTimeout(
      () => void resize(),
      layoutReadyRef.current ? 25 : 0,
    );

    return () => {
      window.clearTimeout(timer);
      resizeRunRef.current += 1;
    };
  }, [canMeasure, denseOverview, detailMode, layoutRevision, applySize, zoom]);

  return { layoutReady, requestLayout };
}
