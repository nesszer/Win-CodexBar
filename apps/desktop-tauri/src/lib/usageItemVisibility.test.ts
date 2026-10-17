import { describe, expect, it } from "vitest";
import { fromFtl } from "../test/localeHarness";
import {
  CLAUDE_EXTRA_USAGE_ITEM_ID,
  CLAUDE_RESET_CREDITS_ITEM_ID,
  SUBSCRIPTION_ITEM_ID,
  claudePlanDateNote,
  projectClaudeSections,
} from "./usageItemVisibility";

const en = fromFtl("en-US.ftl");

function sections(hidden: string[]) {
  return {
    inventory: [
      { id: "reset-credits", title: "Limit Reset Credits", availableCount: 1, nextExpiresAt: null },
      { id: "other", title: "Other", availableCount: 3, nextExpiresAt: null },
    ],
    cost: {
      used: 0,
      limit: null,
      remaining: null,
      currencyCode: "USD",
      period: "Extra usage",
      resetsAt: null,
      formattedUsed: "$0.00",
      formattedLimit: null,
      balance: 5,
      formattedBalance: null,
    },
    subscription: {
      startsAt: null,
      expiresAt: null,
      renewsAt: "2026-12-01T00:00:00+00:00",
      renewsAtIsDateOnly: true,
    },
    sourceLabel: "web",
    hiddenUsageItemIds: hidden,
  };
}

describe("projectClaudeSections", () => {
  it("drops only the hidden Claude sections", () => {
    const all = projectClaudeSections("claude", sections([
      CLAUDE_RESET_CREDITS_ITEM_ID,
      CLAUDE_EXTRA_USAGE_ITEM_ID,
      SUBSCRIPTION_ITEM_ID,
    ]));
    expect(all.inventory.map((item) => item.id)).toEqual(["other"]);
    expect(all.cost).toBeNull();
    expect(all.subscription).toBeNull();

    const none = sections([]);
    expect(projectClaudeSections("claude", none)).toBe(none);
  });

  it("leaves other providers and Admin API spend alone", () => {
    const codex = sections([CLAUDE_RESET_CREDITS_ITEM_ID, CLAUDE_EXTRA_USAGE_ITEM_ID]);
    expect(projectClaudeSections("codex", codex)).toBe(codex);

    const admin = { ...sections([CLAUDE_EXTRA_USAGE_ITEM_ID]), sourceLabel: "admin-api" };
    expect(projectClaudeSections("claude", admin).cost).not.toBeNull();
  });
});

describe("claudePlanDateNote", () => {
  it("keeps a calendar-only date on its own day", () => {
    expect(claudePlanDateNote("claude", sections([]).subscription, en)).toBe(
      "Renews: Dec 1, 2026",
    );
  });

  it("prefers the renewal, else reports the expiry", () => {
    expect(
      claudePlanDateNote(
        "claude",
        {
          startsAt: null,
          expiresAt: "2026-11-20T00:00:00+00:00",
          renewsAt: null,
          expiresAtIsDateOnly: true,
        },
        en,
      ),
    ).toBe("Plan expires: Nov 20, 2026");
  });

  it("never invents a date", () => {
    expect(claudePlanDateNote("claude", null, en)).toBeNull();
    expect(
      claudePlanDateNote("claude", { startsAt: null, expiresAt: null, renewsAt: null }, en),
    ).toBeNull();
    expect(claudePlanDateNote("zed", sections([]).subscription, en)).toBeNull();
  });
});
