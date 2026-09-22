import { renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ClaudeReconciliationSnapshot } from "../types/bridge";

const mocks = vi.hoisted(() => ({
  claudeReconciliationState: vi.fn(),
  listen: vi.fn(),
}));
vi.mock("../lib/tauri", () => ({
  claudeReconciliationState: mocks.claudeReconciliationState,
}));
vi.mock("@tauri-apps/api/event", () => ({ listen: mocks.listen }));
import {
  localClaudeReconciliationOutcome,
  selectClaudeReconciliation,
  useClaudeReconciliation,
} from "./useClaudeReconciliation";

const snapshot = (
  generation: number,
  status: ClaudeReconciliationSnapshot["status"],
  detail: string = status,
): ClaudeReconciliationSnapshot => ({
  generation,
  status,
  detail,
  providerRefreshGeneration: null,
});

describe("selectClaudeReconciliation", () => {
  beforeEach(() => {
    vi.resetAllMocks();
    mocks.listen.mockResolvedValue(() => {});
    mocks.claudeReconciliationState.mockResolvedValue(null);
  });

  it("hydrates a newly mounted surface from the authoritative query", async () => {
    mocks.claudeReconciliationState.mockResolvedValue(snapshot(7, "pending"));
    const { result } = renderHook(() => useClaudeReconciliation());
    await waitFor(() => expect(result.current.snapshot).toEqual(snapshot(7, "pending")));
    expect(result.current.reconciling).toBe(true);
  });

  it("recovers a pending operation from the mount-time query", () => {
    expect(selectClaudeReconciliation(null, snapshot(4, "pending"))).toEqual(
      snapshot(4, "pending"),
    );
  });

  it("accepts the matching late failure", () => {
    expect(
      selectClaudeReconciliation(snapshot(4, "pending"), snapshot(4, "failed", "late")),
    ).toEqual(snapshot(4, "failed", "late"));
  });

  it("ignores stale and duplicate terminal states", () => {
    const current = snapshot(5, "succeeded");
    expect(selectClaudeReconciliation(current, snapshot(4, "failed"))).toBe(current);
    expect(selectClaudeReconciliation(current, snapshot(5, "failed"))).toBe(current);
  });

  it("lets a newer generation supersede an older terminal", () => {
    expect(
      selectClaudeReconciliation(snapshot(4, "failed"), snapshot(5, "pending")),
    ).toEqual(snapshot(5, "pending"));
  });
});

describe("localClaudeReconciliationOutcome", () => {
  it("ignores an old hydrated failure when no local operation owns it", () => {
    expect(
      localClaudeReconciliationOutcome(snapshot(7, "failed", "old failure"), null),
    ).toBeNull();
  });

  it("ignores a terminal failure from a different generation", () => {
    expect(
      localClaudeReconciliationOutcome(snapshot(8, "failed", "other failure"), 9),
    ).toBeNull();
  });

  it("returns a matching late failure", () => {
    const failure = snapshot(10, "failed", "late failure");
    expect(localClaudeReconciliationOutcome(failure, 10)).toBe(failure);
  });
});
