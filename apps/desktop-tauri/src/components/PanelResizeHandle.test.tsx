import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("../hooks/useLocale", () => import("../test/mocks/locale"));

import PanelResizeHandle from "./PanelResizeHandle";

/** jsdom has no PointerEvent, so fireEvent.pointer* drops screenX and button; React only reads the fields. */
function pointer(target: Element, type: string, init: { screenX?: number; button?: number } = {}) {
  const event = new MouseEvent(type, { bubbles: true, button: 0, ...init });
  Object.defineProperty(event, "pointerId", { value: 1 });
  fireEvent(target, event);
}

function renderHandle(side: "left" | "right", width = 310, zoom = 1) {
  const onResizeStart = vi.fn();
  const onResize = vi.fn();
  const onResizeEnd = vi.fn();
  render(
    <PanelResizeHandle
      side={side}
      width={width}
      zoom={zoom}
      onResizeStart={onResizeStart}
      onResize={onResize}
      onResizeEnd={onResizeEnd}
    />,
  );
  const handle = screen.getByRole("separator", { name: "PanelResizeHandle" });
  // jsdom has no pointer capture.
  handle.setPointerCapture = vi.fn();
  return { handle, onResizeStart, onResize, onResizeEnd };
}

describe("PanelResizeHandle", () => {
  it("exposes its range and width to assistive tech", () => {
    const { handle } = renderHandle("left", 400);
    expect(handle).toHaveAttribute("aria-valuenow", "400");
    expect(handle).toHaveAttribute("aria-valuemin", "280");
    expect(handle).toHaveAttribute("aria-valuemax", "560");
    expect(handle).toHaveAttribute("aria-orientation", "vertical");
  });

  it("widens when the left edge is dragged left, from the pointer's screen position", () => {
    const { handle, onResizeStart, onResize, onResizeEnd } = renderHandle("left");
    pointer(handle, "pointerdown", { screenX: 500, button: 0 });
    expect(onResizeStart).toHaveBeenCalledTimes(1);
    pointer(handle, "pointermove", { screenX: 440 });
    expect(onResize).toHaveBeenLastCalledWith(370);
    pointer(handle, "pointermove", { screenX: 400 });
    expect(onResize).toHaveBeenLastCalledWith(410);
    pointer(handle, "pointerup");
    expect(onResizeEnd).toHaveBeenCalledWith(410);
  });

  it("widens when the right edge is dragged right and divides the drag by the Panel scale", () => {
    const { handle, onResize, onResizeEnd } = renderHandle("right", 310, 1.5);
    pointer(handle, "pointerdown", { screenX: 100, button: 0 });
    pointer(handle, "pointermove", { screenX: 175 });
    // 75 screen px at 150 % is 50 width px.
    expect(onResize).toHaveBeenLastCalledWith(360);
    pointer(handle, "pointerup");
    expect(onResizeEnd).toHaveBeenCalledWith(360);
  });

  it("clamps to the allowed range", () => {
    const { handle, onResize } = renderHandle("right");
    pointer(handle, "pointerdown", { screenX: 0, button: 0 });
    pointer(handle, "pointermove", { screenX: 5000 });
    expect(onResize).toHaveBeenLastCalledWith(560);
    pointer(handle, "pointermove", { screenX: -5000 });
    expect(onResize).toHaveBeenLastCalledWith(280);
  });

  it("ignores a move with no drag and other mouse buttons", () => {
    const { handle, onResizeStart, onResize } = renderHandle("left");
    pointer(handle, "pointermove", { screenX: 10 });
    pointer(handle, "pointerdown", { screenX: 10, button: 2 });
    expect(onResizeStart).not.toHaveBeenCalled();
    expect(onResize).not.toHaveBeenCalled();
  });

  it("steps the width with the arrow keys, outward by the edge's own direction", () => {
    const left = renderHandle("left", 310);
    fireEvent.keyDown(left.handle, { key: "ArrowLeft" });
    expect(left.onResizeEnd).toHaveBeenLastCalledWith(320);
    fireEvent.keyDown(left.handle, { key: "ArrowRight" });
    expect(left.onResizeEnd).toHaveBeenLastCalledWith(300);
  });

  it("steps the right edge the other way and stops at the limits", () => {
    document.body.innerHTML = "";
    const right = renderHandle("right", 555);
    fireEvent.keyDown(right.handle, { key: "ArrowRight" });
    expect(right.onResizeEnd).toHaveBeenLastCalledWith(560);
    right.onResizeEnd.mockClear();
    document.body.innerHTML = "";
    const atMax = renderHandle("right", 560);
    fireEvent.keyDown(atMax.handle, { key: "ArrowRight" });
    expect(atMax.onResizeEnd).not.toHaveBeenCalled();
  });

  it("restores the default width on Home or a double-click", () => {
    const { handle, onResize, onResizeEnd } = renderHandle("left", 450);
    fireEvent.doubleClick(handle);
    expect(onResize).toHaveBeenLastCalledWith(310);
    expect(onResizeEnd).toHaveBeenLastCalledWith(310);
    onResizeEnd.mockClear();
    fireEvent.keyDown(handle, { key: "Home" });
    expect(onResizeEnd).toHaveBeenLastCalledWith(310);
  });

  it("does nothing on a double-click at the default width", () => {
    const { handle, onResizeEnd } = renderHandle("left", 310);
    fireEvent.doubleClick(handle);
    expect(onResizeEnd).not.toHaveBeenCalled();
  });
});
