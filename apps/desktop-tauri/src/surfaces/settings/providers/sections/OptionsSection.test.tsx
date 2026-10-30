import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { OptionsSection, type OptionSettings } from "./OptionsSection";

vi.mock("../../../../lib/tauri", () => ({
  setProviderAutoResumeAfterQuotaReset: vi.fn(),
  setProviderOptionalDetails: vi.fn(),
}));

const t = (key: string) => key;

const detail = (id: string, cookieSource: string | null = null) => ({
  id,
  cookieSource,
  autoResumeAfterQuotaReset: false,
  autoResumeSupported: false,
  optionalDetailsEnabled: false,
  optionalDetailsSupported: false,
});

function settings(extra: Partial<OptionSettings> = {}): OptionSettings {
  return {
    antigravityAdditionalProfileHomes: [],
    codexOpenaiWebExtras: undefined,
    codexHistoricalTracking: false,
    codexExternalOauthSourcesAllowed: false,
    codexLocalSessionCostLedgerEnabled: false,
    codexOpencodeLocalHistoryEnabled: false,
    codexOpenaiWebBatterySaverEnabled: false,
    claudeAvoidKeychainPrompts: false,
    claudeAllowReadingClaudeCodeCredentials: false,
    claudeSwapEnabled: false,
    claudeSwapExecutablePath: "",
    disableKeychainAccess: false,
    ...extra,
  } as OptionSettings;
}

function toggle(label: string) {
  return screen.getByRole("checkbox", { name: label });
}

describe("OptionsSection", () => {
  it("shows the Antigravity profile-folder list only for Antigravity", () => {
    const { unmount } = render(
      <OptionsSection
        detail={detail("antigravity")}
        settings={settings({ antigravityAdditionalProfileHomes: ["D:/gemini-work"] })}
        disabled={false}
        t={t}
        onSettingsChange={vi.fn()}
        onReload={vi.fn()}
      />,
    );
    expect(screen.getByText("AntigravityProfileHomesLabel")).toBeTruthy();
    expect(screen.getByText("D:/gemini-work")).toBeTruthy();
    unmount();

    render(
      <OptionsSection
        detail={detail("gemini")}
        settings={settings()}
        disabled={false}
        t={t}
        onSettingsChange={vi.fn()}
        onReload={vi.fn()}
      />,
    );
    expect(screen.queryByText("AntigravityProfileHomesLabel")).toBeNull();
  });

  it("lists the Codex options with web extras on by default and marks unused ones", () => {
    render(
      <OptionsSection
        detail={detail("codex")}
        settings={settings()}
        disabled={false}
        t={t}
        onSettingsChange={vi.fn()}
        onReload={vi.fn()}
      />,
    );
    const labels = Array.from(document.querySelectorAll(".settings-field__label")).map(
      (label) => label.textContent,
    );
    expect(labels).toEqual([
      "ProviderCodexLocalCostEstimates",
      "ProviderCodexOpenCodeLocalHistory",
      "ProviderCodexHistoricalTracking",
      "ProviderCodexOpenAiWebExtras",
      "ProviderCodexExternalOauth",
      "ProviderCodexBatterySaver",
    ]);
    expect(toggle("ProviderCodexOpenAiWebExtras")).toBeChecked();
    expect(screen.getByText(/ProviderCodexHistoryHelp ProviderOptionNotUsedOnWindows/)).toBeTruthy();
    expect(screen.queryByText(/ProviderCodexExternalOauthHelp ProviderOptionNotUsedOnWindows/)).toBeNull();
    expect(toggle("ProviderCodexOpenCodeLocalHistory")).not.toBeChecked();
    expect(screen.getByText("ProviderCodexOpenCodeLocalHistoryHelp")).toBeTruthy();
  });

  it("hides the battery saver while OpenAI web extras are off", () => {
    render(
      <OptionsSection
        detail={detail("codex")}
        settings={settings({ codexOpenaiWebExtras: false })}
        disabled={false}
        t={t}
        onSettingsChange={vi.fn()}
        onReload={vi.fn()}
      />,
    );
    expect(screen.queryByText("ProviderCodexBatterySaver")).toBeNull();
  });

  it("writes the Codex option that was switched", async () => {
    const onSettingsChange = vi.fn();
    render(
      <OptionsSection
        detail={detail("codex")}
        settings={settings()}
        disabled={false}
        t={t}
        onSettingsChange={onSettingsChange}
        onReload={vi.fn()}
      />,
    );
    await act(async () => fireEvent.click(toggle("ProviderCodexExternalOauth")));
    expect(onSettingsChange).toHaveBeenCalledWith({ codexExternalOauthSourcesAllowed: true });
    await act(async () => fireEvent.click(toggle("ProviderCodexOpenAiWebExtras")));
    expect(onSettingsChange).toHaveBeenLastCalledWith({ codexOpenaiWebExtras: false });
    await act(async () => fireEvent.click(toggle("ProviderCodexOpenCodeLocalHistory")));
    expect(onSettingsChange).toHaveBeenLastCalledWith({ codexOpencodeLocalHistoryEnabled: true });
  });

  it("lists the Claude options and locks Avoid keychain prompts under the Advanced switch", async () => {
    const onSettingsChange = vi.fn();
    const { rerender } = render(
      <OptionsSection
        detail={detail("claude")}
        settings={settings()}
        disabled={false}
        t={t}
        onSettingsChange={onSettingsChange}
        onReload={vi.fn()}
      />,
    );
    expect(toggle("ProviderClaudeAvoidKeychainPrompts")).toBeEnabled();
    await act(async () => fireEvent.click(toggle("ClaudeSwapEnable")));
    expect(onSettingsChange).toHaveBeenCalledWith({ claudeSwapEnabled: true });

    rerender(
      <OptionsSection
        detail={detail("claude")}
        settings={settings({ disableKeychainAccess: true })}
        disabled={false}
        t={t}
        onSettingsChange={onSettingsChange}
        onReload={vi.fn()}
      />,
    );
    expect(toggle("ProviderClaudeAvoidKeychainPrompts")).toBeDisabled();
    expect(toggle("ProviderClaudeAllowReadingClaudeCodeCredentials")).toBeEnabled();
  });

  it("shows the claude-swap path inline under its toggle only while the toggle is on", async () => {
    const onSettingsChange = vi.fn();
    const { rerender } = render(
      <OptionsSection
        detail={detail("claude")}
        settings={settings()}
        disabled={false}
        t={t}
        onSettingsChange={onSettingsChange}
        onReload={vi.fn()}
      />,
    );
    expect(screen.queryByRole("textbox", { name: "ClaudeSwapExecutablePath" })).toBeNull();

    rerender(
      <OptionsSection
        detail={detail("claude")}
        settings={settings({ claudeSwapEnabled: true, claudeSwapExecutablePath: "C:/tools/cswap.exe" })}
        disabled={false}
        t={t}
        onSettingsChange={onSettingsChange}
        onReload={vi.fn()}
      />,
    );
    const input = screen.getByRole("textbox", { name: "ClaudeSwapExecutablePath" });
    expect(input).toHaveValue("C:/tools/cswap.exe");
    expect(input).toHaveAttribute("placeholder", "ClaudeSwapExecutablePathPlaceholder");
    const toggleRow = toggle("ClaudeSwapEnable").closest(".settings-field")!;
    expect(toggleRow.nextElementSibling).toBe(input.closest(".settings-field"));

    await act(async () => {
      fireEvent.change(input, { target: { value: "D:/bin/cswap.exe" } });
      fireEvent.keyDown(input, { key: "Enter" });
    });
    expect(onSettingsChange).toHaveBeenLastCalledWith({ claudeSwapExecutablePath: "D:/bin/cswap.exe" });
  });

  it("shows the Source picker only while claude-swap is on and writes the chosen source", async () => {
    const onSettingsChange = vi.fn();
    const { rerender } = render(
      <OptionsSection
        detail={detail("claude")}
        settings={settings()}
        disabled={false}
        t={t}
        onSettingsChange={onSettingsChange}
        onReload={vi.fn()}
      />,
    );
    expect(screen.queryByRole("combobox", { name: "ClaudeSwapSource" })).toBeNull();

    rerender(
      <OptionsSection
        detail={detail("claude")}
        settings={settings({ claudeSwapEnabled: true })}
        disabled={false}
        t={t}
        onSettingsChange={onSettingsChange}
        onReload={vi.fn()}
      />,
    );
    const picker = screen.getByRole("combobox", { name: "ClaudeSwapSource" });
    expect(picker).toHaveValue("claude-swap");
    expect(Array.from((picker as HTMLSelectElement).options).map((o) => o.textContent)).toEqual([
      "claude-swap",
      "tcr",
    ]);
    expect(screen.getByText("ClaudeSwapHint")).toBeTruthy();
    await act(async () => fireEvent.change(picker, { target: { value: "tcr" } }));
    expect(onSettingsChange).toHaveBeenLastCalledWith({ claudeSwapSource: "tcr" });

    rerender(
      <OptionsSection
        detail={detail("claude")}
        settings={settings({ claudeSwapEnabled: true, claudeSwapSource: "tcr" })}
        disabled={false}
        t={t}
        onSettingsChange={onSettingsChange}
        onReload={vi.fn()}
      />,
    );
    expect(screen.getByRole("combobox", { name: "ClaudeSwapSource" })).toHaveValue("tcr");
    expect(screen.getByRole("textbox", { name: "ClaudeTcrExecutablePath" })).toBeTruthy();
    expect(screen.getByText("ClaudeTcrHint")).toBeTruthy();
    expect(screen.queryByText("ClaudeSwapHint")).toBeNull();
  });

  it("offers Claude desktop app cookies only while the cookie source is Automatic", async () => {
    const onSettingsChange = vi.fn();
    const { rerender } = render(
      <OptionsSection
        detail={detail("claude", "manual")}
        settings={settings()}
        disabled={false}
        t={t}
        onSettingsChange={onSettingsChange}
        onReload={vi.fn()}
      />,
    );
    expect(screen.queryByRole("checkbox", { name: "ProviderClaudeDesktopCookies" })).toBeNull();

    rerender(
      <OptionsSection
        detail={detail("claude", "auto")}
        settings={settings()}
        disabled={false}
        t={t}
        onSettingsChange={onSettingsChange}
        onReload={vi.fn()}
      />,
    );
    expect(toggle("ProviderClaudeDesktopCookies")).not.toBeChecked();
    expect(screen.getByText("ProviderClaudeDesktopCookiesHelp")).toBeTruthy();
    await act(async () => fireEvent.click(toggle("ProviderClaudeDesktopCookies")));
    expect(onSettingsChange).toHaveBeenLastCalledWith({ claudeDesktopCookiesEnabled: true });

    rerender(
      <OptionsSection
        detail={detail("claude", "auto")}
        settings={settings({ claudeDesktopCookiesEnabled: true })}
        disabled={false}
        t={t}
        onSettingsChange={onSettingsChange}
        onReload={vi.fn()}
      />,
    );
    expect(toggle("ProviderClaudeDesktopCookies")).toBeChecked();
  });

  it("renders an empty group for a provider without options", () => {
    const { container } = render(
      <OptionsSection
        detail={detail("gemini")}
        settings={settings()}
        disabled={false}
        t={t}
        onSettingsChange={vi.fn()}
        onReload={vi.fn()}
      />,
    );
    expect(container.querySelector(".settings-group__box")?.childElementCount).toBe(0);
  });
});
