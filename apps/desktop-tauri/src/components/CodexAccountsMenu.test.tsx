import { act, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type {
  CodexAccount,
  CodexAccountsStateBridge,
  CodexAccountUsageSnapshot,
} from "../types/bridge";
import { buildBundle } from "../test/localeHarness";
import { LocaleProvider } from "../i18n/LocaleProvider";

const tauriMocks = vi.hoisted(() => ({
  getCodexAccountsState: vi.fn(),
  codexAccountSwitch: vi.fn(),
  codexAccountAdd: vi.fn(),
  codexAccountReauthenticate: vi.fn(),
  refreshProviders: vi.fn(),
  getLocaleStrings: vi.fn(),
}));

const eventMocks = vi.hoisted(() => ({
  listen: vi.fn((_event: string, _callback: () => void) => Promise.resolve(() => {})),
}));

vi.mock("../lib/tauri", () => tauriMocks);
vi.mock("@tauri-apps/api/event", () => eventMocks);

import CodexAccountsMenu from "./CodexAccountsMenu";

function account(id: string, extra: Partial<CodexAccount> = {}): CodexAccount {
  return {
    id,
    nickname: null,
    emailHint: `user-${id}@example.com`,
    authSubject: null,
    providerAccountId: null,
    codexHomePath: `C:/fake/${id}`,
    source: "managedByApp",
    createdAt: "2024-01-01T00:00:00Z",
    updatedAt: "2024-01-01T00:00:00Z",
    lastAuthenticatedAt: null,
    ...extra,
  };
}

function snapshot(
  usedPercent: number,
  resetAt: string | null = null,
): CodexAccountUsageSnapshot {
  return {
    email: "user@example.com",
    providerAccountId: null,
    plan: "free",
    allowed: true,
    limitReached: false,
    primaryWindow: { usedPercent, resetAt, limitWindowSeconds: 18_000 },
    secondaryWindow: null,
    credits: null,
    updatedAt: "2024-01-01T00:00:00Z",
  };
}

// Wrap the component so the `t` from useLocale is a stable identity that just
// returns the key (the component uses `t(key)` for locale strings and a badge
// label; returning the key is enough to assert rendering).
function renderMenu(
  hideEmail: boolean,
  state: CodexAccountsStateBridge,
  resetTimeRelative = true,
  showAsUsed = false,
) {
  tauriMocks.getCodexAccountsState.mockResolvedValue(state);
  tauriMocks.getLocaleStrings.mockResolvedValue(buildBundle({}));
  return render(
    <LocaleProvider>
      <CodexAccountsMenu
        showAsUsed={showAsUsed}
        hideEmail={hideEmail}
        resetTimeRelative={resetTimeRelative}
      />
    </LocaleProvider>,
  );
}

describe("CodexAccountsMenu", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("offers login actions even for a single account", async () => {
    renderMenu(false, {
      accounts: [account("1", { source: "ambient" })],
      accountOrdinals: { "1": 1 }, snapshots: {},
      needsAuthentication: { "1": true },
    });
    expect(await screen.findByRole("button", { name: "CodexAccountsAddButton" })).toBeDefined();
    const refresh = await screen.findByRole("button", { name: "CodexAccountsReauthenticateButton: user-1@example.com" });
    await waitFor(() => expect((refresh as HTMLButtonElement).disabled).toBe(false));
    await act(async () => { refresh.click(); });
    expect(tauriMocks.codexAccountReauthenticate).toHaveBeenCalledOnce();
  });

  it("expands the account list by default and places add account below the rows", async () => {
    const { container } = renderMenu(false, {
      accounts: [account("one"), account("two")],
      accountOrdinals: { one: 1, two: 2 }, snapshots: {},
    });
    const row = await screen.findByText("user-two@example.com");
    expect(container.querySelector("details")?.open).toBe(true);
    const add = screen.getByRole("button", { name: "CodexAccountsAddButton" });
    expect(row.compareDocumentPosition(add) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("offers account sign-in with no saved accounts and reports a login failure", async () => {
    renderMenu(false, { accounts: [], accountOrdinals: {}, snapshots: {} });
    const add = await screen.findByRole("button", { name: "CodexAccountsAddButton" });
    await waitFor(() => expect((add as HTMLButtonElement).disabled).toBe(false));
    tauriMocks.codexAccountAdd.mockRejectedValueOnce(new Error("Login cancelled"));
    await act(async () => { add.click(); });
    expect(tauriMocks.codexAccountAdd).toHaveBeenCalledOnce();
    expect(await screen.findByRole("alert")).toHaveTextContent("Login cancelled");
  });

  it("reauthenticates a single saved account without switching it and reports cancellation", async () => {
    renderMenu(false, { accounts: [account("expired")], accountOrdinals: { expired: 1 }, snapshots: {}, needsAuthentication: { expired: true } });
    const refresh = await screen.findByRole("button", {
      name: "CodexAccountsReauthenticateButton: user-expired@example.com",
    });
    await waitFor(() => expect(refresh).toBeEnabled());
    tauriMocks.codexAccountReauthenticate.mockRejectedValueOnce(new Error("Account setup cancelled."));
    await act(async () => { refresh.click(); });
    expect(tauriMocks.codexAccountReauthenticate).toHaveBeenCalledWith("expired");
    expect(tauriMocks.codexAccountSwitch).not.toHaveBeenCalled();
    expect(await screen.findByRole("alert")).toHaveTextContent("Account setup cancelled.");
    expect(refresh).toBeEnabled();
  });

  it("shows both quota windows for each account without expanding a disclosure", async () => {
    const both = { ...snapshot(30), secondaryWindow: { usedPercent: 65, resetAt: null, limitWindowSeconds: 604800 } };
    const { container } = renderMenu(false, {
      accounts: [account("1", { source: "ambient" }), account("2")],
      accountOrdinals: { "1": 1, "2": 2 }, snapshots: { "1": both, "2": snapshot(70) },
    });
    await screen.findByText("user-1@example.com");
    expect(container.querySelector("details")?.open).toBe(true);
    expect(screen.getByText("35% PanelLeftSuffix")).toBeDefined();
    expect(screen.getByText("7d")).toBeDefined();
    expect(screen.getByText("30% PanelLeftSuffix")).toBeDefined();
    expect(screen.queryByRole("button", { name: /CodexAccountsReauthenticateButton/ })).toBeNull();
  });

  it("keeps login actions disabled while account updates arrive during sign-in", async () => {
    let finish!: () => void;
    tauriMocks.codexAccountAdd.mockImplementationOnce(() => new Promise<void>((resolve) => { finish = resolve; }));
    renderMenu(false, { accounts: [], accountOrdinals: {}, snapshots: {} });
    const add = await screen.findByRole("button", { name: "CodexAccountsAddButton" }) as HTMLButtonElement;
    await waitFor(() => expect(add.disabled).toBe(false));
    await act(async () => { add.click(); });
    const callback = eventMocks.listen.mock.calls.find(([event]) => event === "codex-accounts-updated")?.[1] as (() => void) | undefined;
    await act(async () => { callback?.(); });
    expect(add.disabled).toBe(true);
    await act(async () => { finish(); });
    await waitFor(() => expect(add.disabled).toBe(false));
  });

  it("lists multiple accounts with usage bars and marks the ambient one active", async () => {
    const { container } = renderMenu(false, {
      accounts: [
        account("1", { source: "ambient" }),
        account("2"),
      ],
      accountOrdinals: { "1": 1, "2": 2 },
      snapshots: { "1": snapshot(30), "2": snapshot(70) },
    });
    await screen.findByText("user-1@example.com");
    expect(screen.getByText("user-2@example.com")).toBeDefined();

    const rows = container.querySelectorAll(".codex-menu-accounts__row");
    expect(rows.length).toBe(2);
    // Ambient row is marked active; its switch is disabled.
    expect(
      rows[0].className.includes("codex-menu-accounts__row--active"),
    ).toBe(true);
    expect(
      (rows[0].querySelector(".codex-menu-accounts__switch") as HTMLButtonElement)
        .disabled,
    ).toBe(true);

    // Usage bar widths map to the snapshot percentages.
    const fills = container.querySelectorAll(".codex-menu-accounts__bar-fill");
    expect((fills[0] as HTMLElement).style.width).toBe("70%");
    expect((fills[1] as HTMLElement).style.width).toBe("30%");
  });

  it("renders a usage bar from a weekly-only snapshot (primaryWindow: null)", async () => {
    const weeklyOnly: CodexAccountUsageSnapshot = {
      email: "weekly@example.com",
      providerAccountId: null,
      plan: "pro",
      allowed: true,
      limitReached: false,
      primaryWindow: null,
      secondaryWindow: {
        usedPercent: 42,
        resetAt: null,
        limitWindowSeconds: 604800,
      },
      credits: null,
      updatedAt: "2024-01-01T00:00:00Z",
    };
    const { container } = renderMenu(false, {
      accounts: [account("1", { source: "ambient" }), account("2")],
      accountOrdinals: { "1": 1, "2": 2 },
      snapshots: { "1": weeklyOnly },
    });
    await screen.findByText("user-1@example.com");

    const fills = container.querySelectorAll(
      ".codex-menu-accounts__bar-fill",
    );
    expect(fills.length).toBe(1);
    expect((fills[0] as HTMLElement).style.width).toBe("58%");
  });

  it("shows the five-hour usage and local reset time for each account", async () => {
    const resetAt = "2030-01-02T03:04:00Z";
    const expectedReset = new Intl.DateTimeFormat(undefined, {
      month: "short",
      day: "numeric",
      hour: "numeric",
      minute: "2-digit",
    }).format(new Date(resetAt));

    renderMenu(
      false,
      {
        accounts: [account("1", { source: "ambient" }), account("2")],
        accountOrdinals: { "1": 1, "2": 2 },
        snapshots: { "1": snapshot(30, resetAt), "2": snapshot(70, resetAt) },
      },
      false,
    );

    await screen.findByText("user-1@example.com");
    expect(screen.getAllByText("5h")).toHaveLength(2);
    expect(screen.getByText("70% PanelLeftSuffix")).toBeDefined();
    expect(screen.getAllByText(`MetricResetsIn ${expectedReset}`)).toHaveLength(2);
  });

  it("shows remaining capacity alongside used capacity when the used setting is enabled", async () => {
    renderMenu(false, {
      accounts: [account("1", { source: "ambient" }), account("2")],
      accountOrdinals: { "1": 1, "2": 2 }, snapshots: { "1": snapshot(30) },
    }, true, true);
    await screen.findByText("70% PanelLeftSuffix");
    expect(screen.getByText("30% PanelUsedSuffix")).toBeDefined();
    expect(screen.getByText("CodexAccountsUsageUnavailable")).toBeDefined();
  });

  it("distinguishes exhausted and full quotas from unknown usage", async () => {
    const { container } = renderMenu(false, {
      accounts: [account("1"), account("2"), account("3")],
      accountOrdinals: { "1": 1, "2": 2, "3": 3 },
      snapshots: { "1": snapshot(100), "2": snapshot(0), "3": snapshot(Number.NaN) },
    });
    await screen.findByText("0% PanelLeftSuffix");
    expect(screen.getByText("100% PanelLeftSuffix")).toBeDefined();
    expect(screen.getByText("CodexAccountsUsageUnavailable")).toBeDefined();
    const fills = container.querySelectorAll(".codex-menu-accounts__bar-fill");
    expect(fills).toHaveLength(2);
    expect((fills[0] as HTMLElement).style.width).toBe("0%");
    expect((fills[1] as HTMLElement).style.width).toBe("100%");
  });

  it("switches an account and kicks a provider refresh", async () => {
    renderMenu(false, {
      accounts: [account("1", { source: "ambient" }), account("2")],
      accountOrdinals: { "1": 1, "2": 2 },
      snapshots: {},
    });
    await screen.findByText("user-1@example.com");

    tauriMocks.codexAccountSwitch.mockResolvedValue({});
    tauriMocks.getCodexAccountsState.mockResolvedValue({
      accounts: [account("1", { source: "ambient" }), account("2")],
      accountOrdinals: { "1": 1, "2": 2 },
      snapshots: {},
    });
    const switchButtons = screen.getAllByText("CodexAccountsSwitchButton");
    const activeSwitch = switchButtons.find((b) => !(b as HTMLButtonElement).disabled);
    expect(activeSwitch).toBeDefined();
    await act(async () => {
      activeSwitch!.click();
    });
    expect(tauriMocks.codexAccountSwitch).toHaveBeenCalledWith("2");
    expect(tauriMocks.refreshProviders).toHaveBeenCalledTimes(1);
  });
  it("uses an opaque ordinal for every account while hideEmail is on", async () => {
    const { container: hidden } = renderMenu(true, {
      accounts: [account("1", { source: "ambient" }), account("2")],
      accountOrdinals: { "1": 1, "2": 2 },
      snapshots: {},
    });
    await waitFor(() => {
      expect(
        hidden.querySelectorAll(".codex-menu-accounts__email").length,
      ).toBe(2);
    });
    // Upstream 0.60.5 #3702 (141ecf642): every account is redacted, not only
    // ambient ones — managed accounts get the generic label too.
    const ambientLabel = hidden.querySelectorAll(
      ".codex-menu-accounts__email",
    )[0] as HTMLElement;
    expect(ambientLabel.getAttribute("title")).toBe("Account 1");
    expect(ambientLabel.firstChild?.textContent).toBe("Account 1");
    expect(ambientLabel.firstChild?.textContent).not.toContain("@");
    expect(ambientLabel.firstChild?.textContent).not.toContain("example.com");

    const managedLabel = hidden.querySelectorAll(
      ".codex-menu-accounts__email",
    )[1] as HTMLElement;
    expect(managedLabel.getAttribute("title")).toBe("Account 2");
    expect(managedLabel.textContent).toBe("Account 2");
    expect(managedLabel.textContent).not.toContain("user-2@example.com");
    const switches = hidden.querySelectorAll(
      ".codex-menu-accounts__switch",
    ) as NodeListOf<HTMLButtonElement>;
    expect(switches[0].disabled).toBe(true);
    expect(switches[1].disabled).toBe(false);

    const { container: visible } = renderMenu(false, {
      accounts: [account("1", { source: "ambient" }), account("2")],
      accountOrdinals: { "1": 1, "2": 2 },
      snapshots: {},
    });
    await waitFor(() => {
      expect(
        visible.querySelectorAll(".codex-menu-accounts__email").length,
      ).toBe(2);
    });
    const rawEmail = visible.querySelectorAll(
      ".codex-menu-accounts__email",
    )[1] as HTMLElement;
    expect(rawEmail.getAttribute("title")).toBe("user-2@example.com");
  });

  it("uses canonical opaque ordinals for every account supplied by the bridge", async () => {
    const first = account("uuid-b", {
      emailHint: "alice@example.com",
      nickname: "team@example.com",
      source: "ambient",
    });
    const second = account("uuid-a", {
      emailHint: "bob@example.com",
      nickname: "Private workspace",
    });

    const { container } = renderMenu(true, {
      accounts: [first, second],
      accountOrdinals: { "uuid-b": 2, "uuid-a": 1 },
      snapshots: {},
    });
    // Upstream 0.60.5 #3702 (141ecf642): every account is redacted, not only
    // ambient ones — managed accounts get the generic label too.
    await screen.findByText("Account 2");
    await screen.findByText("Account 1");
    const labels = container.querySelectorAll(".codex-menu-accounts__email");
    expect(labels[0].firstChild?.textContent).toBe("Account 2");
    expect(labels[1].textContent).toBe("Account 1");
    expect(labels[0].firstChild?.textContent).not.toContain("@");
    expect(labels[1].textContent).not.toContain("bob@example.com");
    expect(labels[1].textContent).not.toContain("Private workspace");
  });
});

