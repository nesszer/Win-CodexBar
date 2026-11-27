import { describe, expect, it } from "vitest";
import { headerAccount } from "./MenuCard";

describe("headerAccount", () => {
  it("prefers the provider email over the token-account label", () => {
    expect(
      headerAccount({ accountEmail: "parity.user@example.com", accountLabel: "Work" }, false),
    ).toBe("parity.user@example.com");
  });

  it("falls back to the active token-account label", () => {
    expect(headerAccount({ accountEmail: null, accountLabel: "parity-github" }, false)).toBe(
      "parity-github",
    );
    expect(headerAccount({ accountEmail: "  ", accountLabel: " parity-github " }, false)).toBe(
      "parity-github",
    );
  });

  it("shows no email and no label under Hide Personal Info", () => {
    expect(headerAccount({ accountEmail: "parity.user@example.com", accountLabel: null }, true)).toBeNull();
    expect(
      headerAccount({ accountEmail: null, accountLabel: "parity-github" }, true),
    ).toBeNull();
    expect(
      headerAccount({ accountEmail: null, accountLabel: "parity.user@example.com" }, true),
    ).toBeNull();
  });

  it("shows nothing without an email or label", () => {
    expect(headerAccount({ accountEmail: null, accountLabel: null }, false)).toBeNull();
    expect(headerAccount({ accountEmail: null, accountLabel: "" }, true)).toBeNull();
  });
});
