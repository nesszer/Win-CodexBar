import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

const mocks = vi.hoisted(() => ({
  open: vi.fn(),
  save: vi.fn(),
  exportPreferences: vi.fn(),
  importPreferences: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: mocks.open,
  save: mocks.save,
}));
vi.mock("../../../lib/tauri", () => ({
  exportPreferences: mocks.exportPreferences,
  importPreferences: mocks.importPreferences,
}));
vi.mock("../../../hooks/useLocale", () => import("../../../test/mocks/locale"));

import PreferencesTransferSection from "./PreferencesTransferSection";

describe("PreferencesTransferSection", () => {
  beforeEach(() => {
    vi.resetAllMocks();
  });

  it("exports to the chosen path and reports success", async () => {
    mocks.save.mockResolvedValue("C:\\temp\\prefs.json");
    mocks.exportPreferences.mockResolvedValue(12);
    render(<PreferencesTransferSection />);

    fireEvent.click(screen.getByText("PreferencesExportButton"));

    await waitFor(() =>
      expect(mocks.exportPreferences).toHaveBeenCalledWith("C:\\temp\\prefs.json"),
    );
    expect(await screen.findByRole("status")).toHaveTextContent(
      "PreferencesExportSuccess",
    );
    expect(mocks.save).toHaveBeenCalledWith(
      expect.objectContaining({ defaultPath: "codexbar-preferences.json" }),
    );
  });

  it("does nothing when the save dialog is cancelled", async () => {
    mocks.save.mockResolvedValue(null);
    render(<PreferencesTransferSection />);

    fireEvent.click(screen.getByText("PreferencesExportButton"));

    await waitFor(() => expect(mocks.save).toHaveBeenCalled());
    expect(mocks.exportPreferences).not.toHaveBeenCalled();
    expect(screen.queryByRole("status")).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("imports the chosen file and reports success", async () => {
    mocks.open.mockResolvedValue("C:\\temp\\prefs.json");
    mocks.importPreferences.mockResolvedValue({});
    render(<PreferencesTransferSection />);

    fireEvent.click(screen.getByText("PreferencesImportButton"));

    await waitFor(() =>
      expect(mocks.importPreferences).toHaveBeenCalledWith("C:\\temp\\prefs.json"),
    );
    expect(await screen.findByRole("status")).toHaveTextContent(
      "PreferencesImportSuccess",
    );
    expect(mocks.open).toHaveBeenCalledWith(
      expect.objectContaining({ multiple: false }),
    );
  });

  it("does nothing when the open dialog is cancelled", async () => {
    mocks.open.mockResolvedValue(null);
    render(<PreferencesTransferSection />);

    fireEvent.click(screen.getByText("PreferencesImportButton"));

    await waitFor(() => expect(mocks.open).toHaveBeenCalled());
    expect(mocks.importPreferences).not.toHaveBeenCalled();
  });

  it("shows the shell's rejection and no success message", async () => {
    mocks.open.mockResolvedValue("C:\\temp\\bad.json");
    mocks.importPreferences.mockRejectedValue(
      "Invalid or non-portable preference: refresh_interval_secs",
    );
    render(<PreferencesTransferSection />);

    fireEvent.click(screen.getByText("PreferencesImportButton"));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Invalid or non-portable preference: refresh_interval_secs",
    );
    expect(screen.queryByRole("status")).toBeNull();
  });

  it("disables both buttons while a transfer is running", async () => {
    let finish: (value: string | null) => void = () => {};
    mocks.save.mockReturnValue(
      new Promise<string | null>((resolve) => {
        finish = resolve;
      }),
    );
    render(<PreferencesTransferSection />);

    fireEvent.click(screen.getByText("PreferencesExportButton"));

    await waitFor(() =>
      expect(screen.getByText("PreferencesImportButton")).toBeDisabled(),
    );
    expect(screen.getByText("PreferencesExportButton")).toBeDisabled();
    finish(null);
    await waitFor(() =>
      expect(screen.getByText("PreferencesImportButton")).toBeEnabled(),
    );
  });
});
