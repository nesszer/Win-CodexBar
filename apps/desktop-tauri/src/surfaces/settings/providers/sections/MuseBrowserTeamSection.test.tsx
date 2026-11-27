import { act, fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  getProviderWorkspaceId,
  setProviderWorkspaceId,
} from "../../../../lib/tauri";
import type { ProviderDisplayDetailSnapshot } from "../../../../types/bridge";
import { MuseBrowserTeamSection } from "./MuseBrowserTeamSection";

vi.mock("../../../../lib/tauri", () => ({
  getProviderWorkspaceId: vi.fn(),
  setProviderWorkspaceId: vi.fn(),
}));

function row(id: string, title: string, value: string): ProviderDisplayDetailSnapshot {
  return { id, sectionTitle: null, title, value, secondaryValue: null, progress: null };
}

const DETAILS = [
  row("browser-teams-status", "Browser teams", "Choose a browser team ID in Muse Code settings"),
  row("browser-team-11", "Alpha", "11"),
  row("browser-team-22", "Beta", "22"),
];

function renderSection(details: ProviderDisplayDetailSnapshot[] | undefined = DETAILS) {
  const onChanged = vi.fn();
  render(
    <MuseBrowserTeamSection
      providerId="muse"
      details={details}
      disabled={false}
      t={(key) => key}
      onChanged={onChanged}
    />,
  );
  return onChanged;
}

describe("MuseBrowserTeamSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(getProviderWorkspaceId).mockResolvedValue(null);
    vi.mocked(setProviderWorkspaceId).mockResolvedValue(undefined);
  });

  it("lists only team rows and never preselects the first team", async () => {
    renderSection();
    const select = (await screen.findByRole("combobox")) as HTMLSelectElement;

    expect(select.value).toBe("");
    expect(Array.from(select.options).map((option) => option.value)).toEqual([
      "",
      "11",
      "22",
    ]);
    expect(select.options[2].textContent).toBe("Beta (22)");
    expect(screen.getByText("MuseBrowserTeamHelp")).toBeInTheDocument();
  });

  it("persists the chosen team as the provider workspace value", async () => {
    const onChanged = renderSection();
    const select = await screen.findByRole("combobox");
    await vi.waitFor(() => expect(select).toBeEnabled());

    await act(async () => {
      fireEvent.change(select, { target: { value: "22" } });
    });

    await vi.waitFor(() =>
      expect(setProviderWorkspaceId).toHaveBeenCalledWith("muse", "22"),
    );
    expect(onChanged).toHaveBeenCalledOnce();
  });

  it("keeps a saved team visible when the session no longer lists it", async () => {
    vi.mocked(getProviderWorkspaceId).mockResolvedValue("99");
    renderSection([]);

    const select = (await screen.findByRole("combobox")) as HTMLSelectElement;
    await vi.waitFor(() => expect(select.value).toBe("99"));
    expect(Array.from(select.options).map((option) => option.textContent)).toContain(
      "99 (MuseBrowserTeamUnavailable)",
    );
    expect(screen.getByText("MuseBrowserTeamNoTeams")).toBeInTheDocument();
  });

  it("waits for the saved team to load before allowing a change", async () => {
    let resolveWorkspaceId!: (value: string | null) => void;
    vi.mocked(getProviderWorkspaceId).mockReturnValue(
      new Promise<string | null>((resolve) => {
        resolveWorkspaceId = resolve;
      }),
    );
    renderSection();

    const select = (await screen.findByRole("combobox")) as HTMLSelectElement;
    expect(select).toBeDisabled();
    await act(async () => resolveWorkspaceId("22"));

    await vi.waitFor(() => expect(select).toBeEnabled());
    expect(select.value).toBe("22");
  });

  it("reports a failed save without changing the selection", async () => {
    vi.mocked(setProviderWorkspaceId).mockRejectedValue("Muse browser team ID is invalid");
    const onChanged = renderSection();
    const select = (await screen.findByRole("combobox")) as HTMLSelectElement;
    await vi.waitFor(() => expect(select).toBeEnabled());

    await act(async () => {
      fireEvent.change(select, { target: { value: "11" } });
    });

    expect(await screen.findByText("Muse browser team ID is invalid")).toBeInTheDocument();
    expect(onChanged).not.toHaveBeenCalled();
    expect(select.value).toBe("");
  });
});
