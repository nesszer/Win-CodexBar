import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  AntigravityProfileHomesField,
  mergeProfileHomes,
} from "./AntigravityProfileHomesField";

const mocks = vi.hoisted(() => ({ open: vi.fn() }));

vi.mock("@tauri-apps/plugin-dialog", () => ({ open: mocks.open }));

const t = (key: string) => key;

beforeEach(() => {
  mocks.open.mockReset();
});

describe("mergeProfileHomes", () => {
  it("appends picked folders and drops blanks and case-insensitive repeats", () => {
    expect(
      mergeProfileHomes(
        ["D:\\Gemini-Work"],
        ["  d:/gemini-work  ", "", "E:\\Other", "e:\\other"],
      ),
    ).toEqual(["D:\\Gemini-Work", "E:\\Other"]);
  });
});

describe("AntigravityProfileHomesField", () => {
  it("lists saved folders and removes one on click", () => {
    const onSettingsChange = vi.fn();
    render(
      <AntigravityProfileHomesField
        value={["D:\\gemini-a", "D:\\gemini-b"]}
        disabled={false}
        t={t}
        onSettingsChange={onSettingsChange}
      />,
    );
    expect(screen.getByText("D:\\gemini-a")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: "AntigravityProfileHomesRemove D:\\gemini-a" }));
    expect(onSettingsChange).toHaveBeenCalledWith({
      antigravityAdditionalProfileHomes: ["D:\\gemini-b"],
    });
  });

  it("shows the empty hint when no folder is saved", () => {
    render(
      <AntigravityProfileHomesField
        value={[]}
        disabled={false}
        t={t}
        onSettingsChange={vi.fn()}
      />,
    );
    expect(screen.getByText("AntigravityProfileHomesEmpty")).toBeTruthy();
  });

  it("opens a multi-folder picker and saves the merged list", async () => {
    mocks.open.mockResolvedValue(["E:\\new-home", "d:\\GEMINI-A"]);
    const onSettingsChange = vi.fn();
    render(
      <AntigravityProfileHomesField
        value={["D:\\gemini-a"]}
        disabled={false}
        t={t}
        onSettingsChange={onSettingsChange}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "AntigravityProfileHomesAdd" }));
    await waitFor(() => expect(onSettingsChange).toHaveBeenCalledTimes(1));
    expect(mocks.open).toHaveBeenCalledWith({ directory: true, multiple: true });
    expect(onSettingsChange).toHaveBeenCalledWith({
      antigravityAdditionalProfileHomes: ["D:\\gemini-a", "E:\\new-home"],
    });
  });

  it("leaves the saved list alone when the picker is cancelled or adds nothing new", async () => {
    mocks.open.mockResolvedValueOnce(null);
    mocks.open.mockResolvedValueOnce(["D:\\GEMINI-A"]);
    const onSettingsChange = vi.fn();
    render(
      <AntigravityProfileHomesField
        value={["D:\\gemini-a"]}
        disabled={false}
        t={t}
        onSettingsChange={onSettingsChange}
      />,
    );
    const add = screen.getByRole("button", { name: "AntigravityProfileHomesAdd" });
    fireEvent.click(add);
    await waitFor(() => expect(mocks.open).toHaveBeenCalledTimes(1));
    fireEvent.click(add);
    await waitFor(() => expect(mocks.open).toHaveBeenCalledTimes(2));
    expect(onSettingsChange).not.toHaveBeenCalled();
  });

  it("reports a picker failure without saving", async () => {
    mocks.open.mockRejectedValue("dialog unavailable");
    const onSettingsChange = vi.fn();
    render(
      <AntigravityProfileHomesField
        value={[]}
        disabled={false}
        t={t}
        onSettingsChange={onSettingsChange}
      />,
    );
    fireEvent.click(screen.getByRole("button", { name: "AntigravityProfileHomesAdd" }));
    expect(await screen.findByRole("status")).toBeTruthy();
    expect(screen.getByRole("status").textContent).toContain(
      "AntigravityProfileHomesPickFailed",
    );
    expect(onSettingsChange).not.toHaveBeenCalled();
  });

  it("disables add and remove while settings are saving", () => {
    render(
      <AntigravityProfileHomesField
        value={["D:\\gemini-a"]}
        disabled={true}
        t={t}
        onSettingsChange={vi.fn()}
      />,
    );
    expect(
      (screen.getByRole("button", { name: "AntigravityProfileHomesAdd" }) as HTMLButtonElement)
        .disabled,
    ).toBe(true);
  });
});
