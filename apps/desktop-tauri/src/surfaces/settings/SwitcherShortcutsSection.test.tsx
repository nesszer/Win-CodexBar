import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

vi.mock("../../hooks/useLocale", () => import("../../test/mocks/locale"));

import SwitcherShortcutsSection from "./SwitcherShortcutsSection";
import type { SettingsSnapshot } from "../../types/bridge";

function renderSection(
  switcherShortcuts: Record<string, string> = {},
  saving = false,
) {
  const set = vi.fn();
  render(
    <SwitcherShortcutsSection
      settings={{ switcherShortcuts } as unknown as SettingsSnapshot}
      set={set as never}
      saving={saving}
    />,
  );
  return set;
}

/** Start recording on row `index` (previous = 0, next = 1, select1 = 2, ...). */
function startRecording(index: number) {
  fireEvent.click(
    screen.getAllByRole("button", { name: /: ShortcutRecordButton$/ })[index],
  );
}

async function press(init: KeyboardEventInit) {
  await act(async () => {
    window.dispatchEvent(new KeyboardEvent("keydown", { bubbles: true, ...init }));
  });
}

describe("SwitcherShortcutsSection", () => {
  it("shows one row per action with the resolved shortcuts", () => {
    renderSection({ next: "none", select2: "ctrl+alt+2" });

    expect(screen.getAllByRole("button", { name: /: ShortcutRecordButton$/ })).toHaveLength(11);
    expect(screen.getByText("SwitcherShortcutPrevious")).toBeInTheDocument();
    expect(screen.getByText("SwitcherShortcutNext")).toBeInTheDocument();
    expect(screen.getByText("SwitcherShortcutSelect 9")).toBeInTheDocument();
    expect(screen.getByText("left")).toBeInTheDocument();
    expect(screen.getByText("ctrl+alt+2")).toBeInTheDocument();
    expect(screen.getByText("SwitcherShortcutNone")).toBeInTheDocument();
  });

  it("gives each capture control a row-specific accessible name", () => {
    renderSection();

    expect(
      screen.getByRole("button", {
        name: "SwitcherShortcutNext: ShortcutRecordButton",
      }),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", {
        name: "SwitcherShortcutNext: ShortcutClearButton",
      }),
    ).toBeEnabled();
    expect(
      screen.getByRole("status", { name: "SwitcherShortcutNext: right" }),
    ).toBeInTheDocument();
  });

  it("saves a recorded key as an override", async () => {
    const set = renderSection();
    startRecording(1);
    await press({ key: "ArrowRight", code: "ArrowRight", shiftKey: true });

    expect(set).toHaveBeenCalledWith({ switcherShortcuts: { next: "shift+right" } });
  });

  it("keeps existing overrides when adding another", async () => {
    const set = renderSection({ next: "shift+right" });
    startRecording(2);
    await press({ key: "a", code: "KeyA", altKey: true });

    expect(set).toHaveBeenCalledWith({
      switcherShortcuts: { next: "shift+right", select1: "alt+a" },
    });
  });

  it("disables an action with Backspace", async () => {
    const set = renderSection();
    startRecording(0);
    await press({ key: "Backspace", code: "Backspace" });

    expect(set).toHaveBeenCalledWith({ switcherShortcuts: { previous: "none" } });
  });

  it("rejects a duplicate key with an inline error and saves nothing", async () => {
    const set = renderSection();
    startRecording(1);
    await press({ key: "ArrowLeft", code: "ArrowLeft" });

    expect(set).not.toHaveBeenCalled();
    expect(screen.getByRole("alert")).toHaveTextContent(
      "SwitcherShortcutNext: SwitcherShortcutErrorDuplicate",
    );
  });

  it("rejects reserved keys and clears the error after a valid change", async () => {
    const set = renderSection();
    startRecording(2);
    await press({ key: "r", code: "KeyR", ctrlKey: true });

    expect(set).not.toHaveBeenCalled();
    expect(screen.getByRole("alert")).toHaveTextContent("SwitcherShortcutErrorReserved");

    startRecording(2);
    await press({ key: "a", code: "KeyA", altKey: true });
    expect(set).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("resets to the defaults with an empty override map", () => {
    const set = renderSection({ next: "none" });
    fireEvent.click(screen.getByRole("button", { name: "SwitcherShortcutReset" }));

    expect(set).toHaveBeenCalledWith({ switcherShortcuts: {} });
  });

  it("disables editing while a save is in flight", () => {
    renderSection({}, true);

    expect(screen.getByRole("button", { name: "SwitcherShortcutReset" })).toBeDisabled();
    for (const button of screen.getAllByRole("button", { name: /: ShortcutRecordButton$/ })) {
      expect(button).toBeDisabled();
    }
  });
});
