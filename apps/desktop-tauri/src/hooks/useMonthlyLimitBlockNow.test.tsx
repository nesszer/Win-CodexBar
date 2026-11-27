import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { MonthlyLimitBlockSnapshot } from "../types/bridge";
import { isMonthlyLimitBlockActive } from "../lib/monthlyLimitBlock";
import { useMonthlyLimitBlockNow } from "./useMonthlyLimitBlockNow";

const DAY_MS = 24 * 60 * 60 * 1000;

function Probe({ block }: { block: MonthlyLimitBlockSnapshot | null }) {
  const now = useMonthlyLimitBlockNow([block]);
  return (
    <span data-testid="state">{isMonthlyLimitBlockActive(block, now) ? "blocked" : "open"}</span>
  );
}

describe("useMonthlyLimitBlockNow", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-06-01T00:00:00Z"));
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("re-renders exactly when a weeks-away pool reset passes", async () => {
    render(<Probe block={{ resetsAt: "2026-06-21T00:00:00Z" }} />);
    expect(screen.getByTestId("state")).toHaveTextContent("blocked");

    await act(async () => {
      await vi.advanceTimersByTimeAsync(20 * DAY_MS - 1);
    });
    expect(screen.getByTestId("state")).toHaveTextContent("blocked");

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1);
    });
    expect(screen.getByTestId("state")).toHaveTextContent("open");
  });

  it("keeps a block without a known reset", async () => {
    render(<Probe block={{ resetsAt: null }} />);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(365 * DAY_MS);
    });
    expect(screen.getByTestId("state")).toHaveTextContent("blocked");
  });
});
