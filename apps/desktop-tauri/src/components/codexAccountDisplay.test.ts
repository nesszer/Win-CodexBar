import { describe, expect, it } from "vitest";
import type { CodexAccount } from "../types/bridge";
import {
  buildCodexAccountDisplayNames,
  buildCodexAccountSurfaceLabels,
} from "./codexAccountDisplay";

function account(id: string, providerAccountId: string): CodexAccount {
  return {
    id,
    nickname: null,
    emailHint: "same@example.com",
    authSubject: null,
    providerAccountId,
    codexHomePath: `C:/private/${providerAccountId}`,
    source: "managedByApp",
    createdAt: "2024-01-01T00:00:00Z",
    updatedAt: "2024-01-01T00:00:00Z",
    lastAuthenticatedAt: null,
  };
}

describe("Codex account display labels", () => {
  it("keeps same-email workspace labels opaque and stable across ordering", () => {
    const first = account(
      "11111111-1111-1111-1111-111111111111",
      "workspace-alpha",
    );
    const second = account(
      "22222222-2222-2222-2222-222222222222",
      "workspace-beta",
    );

    const labels = buildCodexAccountDisplayNames([first, second]);
    const reversed = buildCodexAccountDisplayNames([second, first]);

    expect(labels[first.id]).not.toBe(labels[second.id]);
    expect(labels[first.id]).toMatch(/^same@example\.com · [0-9a-f]{8}$/);
    expect(labels[second.id]).toMatch(/^same@example\.com · [0-9a-f]{8}$/);
    expect(labels[first.id]).not.toContain("workspace-alpha");
    expect(labels[second.id]).not.toContain("workspace-beta");
    expect(reversed[first.id]).toBe(labels[first.id]);
    expect(reversed[second.id]).toBe(labels[second.id]);
  });

  it("binds canonical labels to account ids without exposing fallback fields", () => {
    const first = account(
      "33333333-3333-3333-3333-333333333333",
      "internal-one",
    );
    const second = account(
      "44444444-4444-4444-4444-444444444444",
      "internal-two",
    );
    const canonical = {
      [first.id]: "same@example.com · 1111aaaa",
      [second.id]: "same@example.com · 2222bbbb",
    };

    const labels = buildCodexAccountDisplayNames(
      [second, first],
      canonical,
    );

    expect(labels[first.id]).toBe(canonical[first.id]);
    expect(labels[second.id]).toBe(canonical[second.id]);
    expect(Object.values(labels).join(" ")).not.toContain("internal-");
  });

  it("uses a generic privacy-safe fallback when identity fields are absent", () => {
    const missingIdentity = {
      ...account("55555555-5555-5555-5555-555555555555", "secret-workspace"),
      nickname: null,
      emailHint: null,
      authSubject: "auth0|secret-subject",
      codexHomePath: "C:/private/secret-workspace",
    };

    const [label] = Object.values(
      buildCodexAccountDisplayNames([missingIdentity]),
    );

    expect(label).toBe("Workspace");
    expect(label).not.toContain("secret-workspace");
    expect(label).not.toContain("secret-subject");
    expect(label).not.toContain("C:/private");
  });

  it("redacts only the ambient account while preserving managed labels", () => {
    const system = account("11111111-1111-1111-1111-111111111111", "system");
    const managed = account("22222222-2222-2222-2222-222222222222", "managed");
    const accounts = [
      { ...system, source: "ambient" as const, nickname: "Private System Name" },
      { ...managed, nickname: "Work" },
    ];
    const hidden = buildCodexAccountSurfaceLabels(
      accounts,
      {},
      { [system.id]: 1, [managed.id]: 2 },
      true,
      "Account",
    );
    expect(hidden[system.id]).toBe("Account 1");
    expect(hidden[system.id]).not.toContain("@");
    expect(hidden[system.id]).not.toContain("Private System Name");
    expect(hidden[managed.id]).toBe("same@example.com — Work");

    const reordered = buildCodexAccountSurfaceLabels(
      [accounts[1], accounts[0]],
      {},
      { [system.id]: 1, [managed.id]: 2 },
      true,
      "Account",
    );
    expect(reordered[system.id]).toBe("Account 1");

    const visible = buildCodexAccountSurfaceLabels(
      accounts,
      {},
      { [system.id]: 1, [managed.id]: 2 },
      false,
      "Account",
    );
    expect(visible[system.id]).toBe("same@example.com — Private System Name");
    expect(visible[managed.id]).toBe("same@example.com — Work");
  });
});
