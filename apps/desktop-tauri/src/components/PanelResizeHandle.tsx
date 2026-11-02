import { useRef, useState, type KeyboardEvent, type PointerEvent } from "react";
import { useLocale } from "../hooks/useLocale";
import {
  TRAY_PANEL_WIDTH_DEFAULT,
  TRAY_PANEL_WIDTH_MAX,
  TRAY_PANEL_WIDTH_MIN,
  clampTrayPanelWidth,
} from "../lib/trayScale";

/** Arrow keys change the width by this many px, a quarter of a typical drag. */
const KEY_STEP = 10;

interface PanelResizeHandleProps {
  /** The panel edge this handle sits on; dragging it outward widens the panel. */
  side: "left" | "right";
  /** Current panel width in px at 100 % Panel scale. */
  width: number;
  /** Panel scale as a factor (1 is 100 %): screen px per width px. */
  zoom: number;
  onResizeStart: () => void;
  /** Live width while dragging, already clamped. */
  onResize: (width: number) => void;
  /** The drag or key press settled on this width. */
  onResizeEnd: (width: number) => void;
}

/**
 * A thin strip on a vertical edge of the tray panel that resizes its width by
 * dragging (screen coordinates, so the panel moving under the pointer while it
 * re-anchors does not skew the drag). It is also a keyboard separator: the arrow
 * keys step the width and Home or a double-click restores the default.
 */
export default function PanelResizeHandle({
  side,
  width,
  zoom,
  onResizeStart,
  onResize,
  onResizeEnd,
}: PanelResizeHandleProps) {
  const { t } = useLocale();
  const drag = useRef<{ x: number; width: number; latest: number } | null>(null);
  const [active, setActive] = useState(false);
  const outward = side === "left" ? -1 : 1;

  const finish = () => {
    const current = drag.current;
    drag.current = null;
    setActive(false);
    if (current) onResizeEnd(current.latest);
  };

  const onPointerDown = (event: PointerEvent<HTMLDivElement>) => {
    if (event.button !== 0) return;
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = { x: event.screenX, width, latest: width };
    setActive(true);
    onResizeStart();
  };

  const onPointerMove = (event: PointerEvent<HTMLDivElement>) => {
    const current = drag.current;
    if (!current) return;
    const delta = (event.screenX - current.x) * outward;
    const next = clampTrayPanelWidth(current.width + delta / zoom);
    if (next === current.latest) return;
    current.latest = next;
    onResize(next);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    let next: number | null = null;
    if (event.key === "ArrowLeft") next = width + KEY_STEP * -outward;
    else if (event.key === "ArrowRight") next = width + KEY_STEP * outward;
    else if (event.key === "Home") next = TRAY_PANEL_WIDTH_DEFAULT;
    if (next === null) return;
    event.preventDefault();
    const clamped = clampTrayPanelWidth(next);
    if (clamped === width) return;
    onResize(clamped);
    onResizeEnd(clamped);
  };

  return (
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label={t("PanelResizeHandle")}
      aria-valuenow={width}
      aria-valuemin={TRAY_PANEL_WIDTH_MIN}
      aria-valuemax={TRAY_PANEL_WIDTH_MAX}
      tabIndex={0}
      title={t("PanelResizeHandle")}
      className={`panel-resize-handle panel-resize-handle--${side}${active ? " panel-resize-handle--active" : ""}`}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={finish}
      onPointerCancel={finish}
      onKeyDown={onKeyDown}
      onDoubleClick={() => {
        if (width === TRAY_PANEL_WIDTH_DEFAULT) return;
        onResize(TRAY_PANEL_WIDTH_DEFAULT);
        onResizeEnd(TRAY_PANEL_WIDTH_DEFAULT);
      }}
    />
  );
}
