import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, act, fireEvent } from "@testing-library/react";
import { buildBundle } from "../test/localeHarness";

vi.mock("../lib/tauri", () => ({
  getLocaleStrings: vi.fn(),
  setUiLanguage: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => import("../test/mocks/event"));

import { LocaleProvider } from "../i18n/LocaleProvider";
import { ShortcutCapture } from "./ShortcutCapture";
import * as tauri from "../lib/tauri";

async function mount(
  props: Partial<React.ComponentProps<typeof ShortcutCapture>> = {},
) {
  (tauri.getLocaleStrings as ReturnType<typeof vi.fn>).mockResolvedValue(
    buildBundle({
      ShortcutRecordButton: "Record",
      ShortcutRecordingLabel: "Recording",
      ShortcutRecordingHint: "press keys",
      ShortcutClearButton: "Clear",
      ShortcutEmptyPlaceholder: "none",
    }),
  );
  const onCommit = vi.fn();
  const onClear = vi.fn();
  const rendered = render(
    <LocaleProvider>
      <ShortcutCapture
        value=""
        onCommit={onCommit}
        onClear={onClear}
        {...props}
      />
    </LocaleProvider>,
  );
  await act(async () => {});
  return { ...rendered, onCommit, onClear };
}

describe("ShortcutCapture", () => {
  beforeEach(() => vi.clearAllMocks());

  it("renders the empty placeholder and record/clear buttons", async () => {
    await mount();
    expect(screen.getByText("none")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Record" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Clear" })).toBeInTheDocument();
  });

  it("captures a modifier+key combo while recording and emits onCommit", async () => {
    const { onCommit } = await mount();
    fireEvent.click(screen.getByRole("button", { name: "Record" }));
    // Recording state — hint text is visible now.
    expect(screen.getByText("press keys")).toBeInTheDocument();

    // Fire a Ctrl+Shift+K combo at the window (the component installs a
    // capture-phase listener on window).
    await act(async () => {
      window.dispatchEvent(
        new KeyboardEvent("keydown", {
          key: "K",
          code: "KeyK",
          ctrlKey: true,
          shiftKey: true,
          bubbles: true,
        }),
      );
    });

    expect(onCommit).toHaveBeenCalledTimes(1);
    expect(onCommit).toHaveBeenCalledWith("Ctrl+Shift+K");
  });

  it("uses a custom composer, recording hint and empty label", async () => {
    const compose = (event: KeyboardEvent) =>
      event.key === "ArrowLeft" ? "left" : null;
    const { onCommit } = await mount({
      compose,
      recordingHint: "custom hint",
      emptyLabel: "disabled",
    });
    expect(screen.getByText("disabled")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Record" }));
    expect(screen.getByText("custom hint")).toBeInTheDocument();

    // A bare key is not a valid accelerator, but the custom composer accepts it.
    await act(async () => {
      window.dispatchEvent(
        new KeyboardEvent("keydown", { key: "ArrowLeft", code: "ArrowLeft", bubbles: true }),
      );
    });
    expect(onCommit).toHaveBeenCalledWith("left");
  });

  it("cancels on Escape without committing", async () => {
    const { onCommit } = await mount();
    fireEvent.click(screen.getByRole("button", { name: "Record" }));
    await act(async () => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    });
    expect(onCommit).not.toHaveBeenCalled();
  });
});
