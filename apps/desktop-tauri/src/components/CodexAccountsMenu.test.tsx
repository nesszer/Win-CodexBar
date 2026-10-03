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
  refreshProviders: vi.fn(),
  getLocaleStrings: vi.fn(),
}));

const eventMocks = vi.hoisted(() => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
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
) {
  tauriMocks.getCodexAccountsState.mockResolvedValue(state);
  tauriMocks.getLocaleStrings.mockResolvedValue(buildBundle({}));
  return render(
    <LocaleProvider>
      <CodexAccountsMenu
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

  it("renders nothing for a single-account setup (single-account fallback)", async () => {
    const { container } = renderMenu(false, {
      accounts: [account("1", { source: "ambient" })],
      accountOrdinals: { "1": 1 },
      snapshots: {},
    });
    await waitFor(() => {
      expect(
        container.querySelector(".codex-menu-accounts"),
      ).toBeNull();
    });
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
    expect((fills[0] as HTMLElement).style.width).toBe("30%");
    expect((fills[1] as HTMLElement).style.width).toBe("70%");
  });

  it("renders both five-hour and weekly usage bars when both windows exist", async () => {
    const both: CodexAccountUsageSnapshot = {
      email: "both@example.com",
      providerAccountId: null,
      plan: "plus",
      allowed: true,
      limitReached: false,
      primaryWindow: {
        usedPercent: 14,
        resetAt: "2030-01-02T03:04:00Z",
        limitWindowSeconds: 18_000,
      },
      secondaryWindow: {
        usedPercent: 93,
        resetAt: "2030-01-08T03:04:00Z",
        limitWindowSeconds: 604_800,
      },
      credits: null,
      updatedAt: "2024-01-01T00:00:00Z",
    };
    const { container } = renderMenu(false, {
      accounts: [account("1", { source: "ambient" }), account("2")],
      accountOrdinals: { "1": 1, "2": 2 },
      snapshots: { "1": both },
    }, false);
    await screen.findByText("user-1@example.com");
    expect(screen.getByText("14% PanelUsedSuffix")).toBeInTheDocument();
    expect(screen.getByText("93% PanelUsedSuffix")).toBeInTheDocument();
    const fills = container.querySelectorAll(".codex-menu-accounts__bar-fill");
    expect(fills.length).toBe(2);
    expect((fills[0] as HTMLElement).style.width).toBe("14%");
    expect((fills[1] as HTMLElement).style.width).toBe("93%");
    expect((fills[1] as HTMLElement).dataset.level).toBe("critical");
    expect(screen.getByText("5h")).toBeInTheDocument();
    expect(screen.getByText("7d")).toBeInTheDocument();
    for (const resetAt of [both.primaryWindow!.resetAt!, both.secondaryWindow!.resetAt!]) {
      const formatted = new Intl.DateTimeFormat(undefined, {
        month: "short", day: "numeric", hour: "numeric", minute: "2-digit",
      }).format(new Date(resetAt));
      expect(screen.getByText(`MetricResetsIn ${formatted}`)).toBeInTheDocument();
    }
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
    expect((fills[0] as HTMLElement).style.width).toBe("42%");
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
    expect(screen.getByText("30% PanelUsedSuffix")).toBeDefined();
    expect(screen.getAllByText(`MetricResetsIn ${expectedReset}`)).toHaveLength(2);
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
    expect(screen.getByText("CodexAccountsSwitchedHint")).toBeInTheDocument();
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

