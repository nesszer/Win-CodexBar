import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { HookRulePayload, HooksConfigSave, HooksConfigSnapshot } from "../../../types/bridge";

const tauriMocks = vi.hoisted(() => ({
  getHooksConfig: vi.fn(),
  saveHooksConfig: vi.fn(),
}));

vi.mock("../../../lib/tauri", () => tauriMocks);
vi.mock("../../../hooks/useLocale", () => ({
  useLocale: () => ({ t: (key: string) => (key === "HooksRulesFooter" ? "saved in {}" : key) }),
}));

import HooksTab, { newHookRule, ruleEvent, thresholdFraction } from "./HooksTab";

const providers = [
  { id: "codex", displayName: "Codex", cookieDomain: null },
  { id: "claude", displayName: "Claude", cookieDomain: null },
];

function rule(overrides: Partial<HookRulePayload> = {}): HookRulePayload {
  return { ...newHookRule(), executable: "C:\\Tools\\notify.exe", ...overrides };
}

function snapshot(overrides: Partial<HooksConfigSnapshot> = {}): HooksConfigSnapshot {
  return {
    enabled: false,
    settingsEnabled: false,
    fileEnabled: false,
    rules: [],
    path: "C:\\Users\\proof\\AppData\\Roaming\\CodexBar\\hooks.json",
    error: null,
    maxRules: 32,
    maxArguments: 32,
    ...overrides,
  };
}

/** The backend echo: save returns what it stored, with both flags following `enabled`. */
function echoSaves(start: HooksConfigSnapshot) {
  let current = start;
  tauriMocks.saveHooksConfig.mockImplementation(async (request: HooksConfigSave) => {
    const on = request.enabled ?? null;
    current = {
      ...current,
      rules: request.rules,
      settingsEnabled: on ?? current.settingsEnabled,
      fileEnabled: on ?? current.fileEnabled,
      enabled: on ?? current.enabled,
    };
    return current;
  });
}

function lastSave(): HooksConfigSave {
  const calls = tauriMocks.saveHooksConfig.mock.calls;
  return calls[calls.length - 1][0] as HooksConfigSave;
}

async function renderTab(start: HooksConfigSnapshot) {
  tauriMocks.getHooksConfig.mockResolvedValue(start);
  echoSaves(start);
  const view = render(<HooksTab providers={providers} />);
  await screen.findByText("HooksTrustWarning");
  await waitFor(() => expect(tauriMocks.getHooksConfig).toHaveBeenCalled());
  await act(async () => {});
  return view;
}

describe("HooksTab helpers", () => {
  it("clamps a typed percentage to 1..100 and stores a fraction", () => {
    expect(thresholdFraction("85")).toBe(0.85);
    expect(thresholdFraction("0")).toBe(0.01);
    expect(thresholdFraction("250")).toBe(1);
    expect(thresholdFraction("")).toBeNull();
    expect(thresholdFraction("abc")).toBeNull();
  });

  it("shows the first listed event when a rule has no single event", () => {
    expect(ruleEvent(rule({ event: null, events: ["refresh_failed", "quota_low"] }))).toBe(
      "refresh_failed",
    );
    expect(ruleEvent(rule({ event: "quota_reset" }))).toBe("quota_reset");
  });

  it("adds a quota_reached rule with an empty executable, as the Mac does", () => {
    expect(newHookRule()).toEqual({
      enabled: true,
      event: "quota_reached",
      events: [],
      provider: null,
      threshold: null,
      executable: "",
      arguments: [],
      timeoutSecs: 10,
    });
  });
});

describe("HooksTab", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("shows the Enable switch, the Mac subtitle, the trust warning and the empty state", async () => {
    await renderTab(snapshot());
    expect(screen.getByText("HooksEnableLabel")).toBeInTheDocument();
    expect(screen.getByText("HooksEnableSubtitle")).toBeInTheDocument();
    expect(screen.getByText("HooksTrustWarning")).toBeInTheDocument();
    expect(screen.getByText("HooksEmpty")).toBeInTheDocument();
    expect(
      screen.getByText("saved in C:\\Users\\proof\\AppData\\Roaming\\CodexBar\\hooks.json"),
    ).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "HooksEnableLabel" })).not.toBeChecked();
    expect(screen.queryByText("HooksPartlyOn")).toBeNull();
  });

  it("turns both flags on with one save from the Enable switch", async () => {
    await renderTab(snapshot({ rules: [rule()] }));
    fireEvent.click(screen.getByRole("checkbox", { name: "HooksEnableLabel" }));
    await waitFor(() => expect(tauriMocks.saveHooksConfig).toHaveBeenCalledTimes(1));
    expect(lastSave()).toEqual({ enabled: true, rules: [rule()] });
    await waitFor(() =>
      expect(screen.getByRole("checkbox", { name: "HooksEnableLabel" })).toBeChecked(),
    );
  });

  it("reads as off with a caption when only one flag is on", async () => {
    await renderTab(snapshot({ settingsEnabled: true, fileEnabled: false, enabled: false }));
    expect(screen.getByRole("checkbox", { name: "HooksEnableLabel" })).not.toBeChecked();
    expect(screen.getByText("HooksPartlyOn")).toBeInTheDocument();
  });

  it("reads as off with a caption when only the file flag is on, and the switch turns both on", async () => {
    const start = snapshot({ settingsEnabled: false, fileEnabled: true, enabled: false });
    await renderTab(start);
    expect(screen.getByRole("checkbox", { name: "HooksEnableLabel" })).not.toBeChecked();
    expect(screen.getByText("HooksPartlyOn")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("checkbox", { name: "HooksEnableLabel" }));
    await waitFor(() => expect(tauriMocks.saveHooksConfig).toHaveBeenCalledTimes(1));
    expect(lastSave()).toEqual({ enabled: true, rules: [] });
    await waitFor(() => expect(screen.queryByText("HooksPartlyOn")).toBeNull());
    expect(screen.getByRole("checkbox", { name: "HooksEnableLabel" })).toBeChecked();
  });

  it("adds a rule, sets it up and saves each edit without touching the master flags", async () => {
    await renderTab(snapshot({ enabled: true, settingsEnabled: true, fileEnabled: true }));
    fireEvent.click(screen.getByRole("button", { name: "+ HooksAddRule" }));
    await waitFor(() => expect(screen.getAllByTestId("hook-rule")).toHaveLength(1));
    expect(lastSave()).toEqual({ enabled: null, rules: [newHookRule()] });

    const row = screen.getByTestId("hook-rule");
    fireEvent.change(within(row).getByRole("combobox", { name: "HooksEvent" }), {
      target: { value: "quota_low" },
    });
    await waitFor(() => expect(lastSave().rules[0].event).toBe("quota_low"));

    fireEvent.change(within(row).getByRole("combobox", { name: "HooksProvider" }), {
      target: { value: "codex" },
    });
    await waitFor(() => expect(lastSave().rules[0].provider).toBe("codex"));

    const threshold = await within(row).findByRole("textbox", { name: "HooksThreshold" });
    fireEvent.change(threshold, { target: { value: "85" } });
    fireEvent.blur(threshold);
    await waitFor(() => expect(lastSave().rules[0].threshold).toBe(0.85));

    const executable = within(row).getByRole("textbox", { name: "HooksExecutable" });
    expect(executable).toHaveAttribute("placeholder", "HooksExecutablePlaceholder");
    fireEvent.change(executable, { target: { value: "C:\\Tools\\my-command.exe" } });
    fireEvent.blur(executable);
    await waitFor(() => expect(lastSave().rules[0].executable).toBe("C:\\Tools\\my-command.exe"));

    fireEvent.click(within(row).getByRole("button", { name: "+ HooksAddArgument" }));
    await waitFor(() => expect(lastSave().rules[0].arguments).toEqual([""]));
    const argument = within(row).getByRole("textbox", { name: "HooksArgument" });
    fireEvent.change(argument, { target: { value: "--title" } });
    fireEvent.blur(argument);

    await waitFor(() =>
      expect(lastSave()).toEqual({
        enabled: null,
        rules: [
          {
            enabled: true,
            event: "quota_low",
            events: [],
            provider: "codex",
            threshold: 0.85,
            executable: "C:\\Tools\\my-command.exe",
            arguments: ["--title"],
            timeoutSecs: 10,
          },
        ],
      }),
    );
    for (const [request] of tauriMocks.saveHooksConfig.mock.calls) {
      expect((request as HooksConfigSave).enabled).toBeNull();
    }
  });

  it("shows the threshold field only for quota_low", async () => {
    await renderTab(
      snapshot({ rules: [rule({ event: "quota_low", threshold: 0.9 }), rule({ event: "quota_reset" })] }),
    );
    const [low, reset] = screen.getAllByTestId("hook-rule");
    expect(within(low).getByRole("textbox", { name: "HooksThreshold" })).toHaveValue("90");
    expect(within(reset).queryByRole("textbox", { name: "HooksThreshold" })).toBeNull();
  });

  it("lists the seven events in the Mac order and Any provider first", async () => {
    await renderTab(snapshot({ rules: [rule()] }));
    const row = screen.getByTestId("hook-rule");
    const events = within(row).getByRole("combobox", { name: "HooksEvent" });
    expect(Array.from(events.querySelectorAll("option"), (o) => o.value)).toEqual([
      "quota_low",
      "quota_reached",
      "quota_reset",
      "usage_updated",
      "provider_unavailable",
      "provider_recovered",
      "refresh_failed",
    ]);
    const provider = within(row).getByRole("combobox", { name: "HooksProvider" });
    expect(Array.from(provider.querySelectorAll("option"), (o) => o.textContent)).toEqual([
      "HooksAnyProvider",
      "Codex",
      "Claude",
    ]);
    fireEvent.change(provider, { target: { value: "claude" } });
    await waitFor(() => expect(lastSave().rules[0].provider).toBe("claude"));
    fireEvent.change(provider, { target: { value: "" } });
    await waitFor(() => expect(lastSave().rules[0].provider).toBeNull());
  });

  it("deletes a rule and an argument", async () => {
    await renderTab(
      snapshot({
        rules: [rule({ arguments: ["a", "b"] }), rule({ event: "quota_reset" })],
      }),
    );
    const [first] = screen.getAllByTestId("hook-rule");
    fireEvent.click(within(first).getAllByRole("button", { name: "HooksDeleteArgument" })[0]);
    await waitFor(() => expect(lastSave().rules[0].arguments).toEqual(["b"]));

    fireEvent.click(within(first).getByRole("button", { name: "HooksDeleteRule" }));
    await waitFor(() => expect(lastSave().rules).toEqual([rule({ event: "quota_reset" })]));
    await waitFor(() => expect(screen.getAllByTestId("hook-rule")).toHaveLength(1));
  });

  it("stops adding rules at 32 and arguments at 32", async () => {
    const full = Array.from({ length: 32 }, () =>
      rule({ arguments: Array.from({ length: 32 }, (_, i) => `a${i}`) }),
    );
    await renderTab(snapshot({ rules: full }));
    expect(screen.getAllByTestId("hook-rule")).toHaveLength(32);
    // Role queries over 32 × 32 argument rows are slow enough under a loaded
    // CI runner to hit the default 5 s timeout; text queries find the same
    // buttons, and the larger timeout covers the heavy render itself.
    expect(screen.getByText("+ HooksAddRule").closest("button")).toBeDisabled();
    const addArguments = screen.getAllByText("+ HooksAddArgument");
    expect(addArguments).toHaveLength(32);
    for (const add of addArguments) {
      expect(add.closest("button")).toBeDisabled();
    }
  }, 20_000);

  it("keeps an unreadable hooks.json read-only and shows why", async () => {
    await renderTab(snapshot({ error: "expected value at line 1 column 1" }));
    expect(
      screen.getByText("HooksLoadError".replace("{}", "expected value at line 1 column 1")),
    ).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "HooksEnableLabel" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "+ HooksAddRule" })).toBeDisabled();
  });

  it("shows a save error and reloads what is on disk", async () => {
    await renderTab(snapshot({ rules: [rule()] }));
    tauriMocks.saveHooksConfig.mockRejectedValueOnce("hooks support at most 32 rules (got 33)");
    fireEvent.click(screen.getByRole("button", { name: "+ HooksAddRule" }));
    expect(await screen.findByText("HooksSaveError")).toBeInTheDocument();
    await waitFor(() => expect(screen.getAllByTestId("hook-rule")).toHaveLength(1));
    expect(tauriMocks.getHooksConfig).toHaveBeenCalledTimes(2);
  });
});
