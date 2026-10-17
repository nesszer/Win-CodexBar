import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { TOKEN_ACTIVITY_MODE_STORAGE_KEY, TokenActivityGrid } from "./TokenActivityGrid";
import { dayKey, dayNumber, type TokenActivityPoint } from "../../../lib/tokenActivity";
import { formatSpendDay } from "../../../lib/usageSpendBreakdown";

const EN: Record<string, string> = {
  TokenActivityTitle: "Token activity",
  TokenActivityInLastYear: "in the last year",
  TokenActivityUnavailable: "Unavailable",
  TokenActivityCoverage: "Coverage",
  TokenActivityNoActivity: "No activity in the last 12 months",
  TokenActivityLess: "Less",
  TokenActivityMore: "More",
  TokenActivityEachColumnWeek: "Each column = 1 week",
  TokenActivityRunningTotal: "Running total",
  TokenActivityDaily: "Daily",
  TokenActivityWeekly: "Weekly",
  TokenActivityCumulative: "Cumulative",
  TokenActivityView: "View",
  TokenActivityEarlier: "Earlier activity",
  TokenActivityLater: "More recent activity",
};
const t = (key: string) => EN[key] ?? key;

// 2024-12-31 is a Tuesday; cell 0 is Sunday 2023-12-31, today is cell 366.
const TODAY = "2024-12-31";
const PITCH = 13; // gridFrame(0): the minimum 10px cell plus 3px gap.

function year(tokens: (key: string) => number | null, scanned: (key: string) => boolean = () => true) {
  const end = dayNumber(TODAY) ?? 0;
  return Array.from({ length: 365 }, (_, offset): TokenActivityPoint => {
    const day = dayKey(end - 364 + offset);
    return { day, totalTokens: tokens(day), isScanned: scanned(day) };
  });
}

function cellPoint(index: number) {
  const column = Math.floor(index / 7);
  const row = index % 7;
  return { clientX: column * PITCH + 5, clientY: row * PITCH + 5 };
}

function renderGrid(points: TokenActivityPoint[], selectedDay: string | null = null) {
  const onSelectDay = vi.fn();
  const view = render(<TokenActivityGrid points={points} selectedDay={selectedDay} onSelectDay={onSelectDay} t={t} />);
  return { ...view, onSelectDay };
}

function stubViewport(width: number) {
  const descriptor = Object.getOwnPropertyDescriptor(HTMLElement.prototype, "clientWidth");
  Object.defineProperty(HTMLElement.prototype, "clientWidth", { configurable: true, get: () => width });
  return () => {
    if (descriptor) Object.defineProperty(HTMLElement.prototype, "clientWidth", descriptor);
  };
}

beforeEach(() => {
  window.localStorage.clear();
});

describe("TokenActivityGrid", () => {
  it("draws 365 day cells with the yearly total and legend", () => {
    const { container } = renderGrid(year(() => 1000));
    expect(screen.getByRole("heading", { name: "Token activity" })).toBeInTheDocument();
    expect(screen.getByText("365K in the last year")).toBeInTheDocument();
    expect(container.querySelectorAll("[data-day]")).toHaveLength(365);
    expect(container.querySelector("[data-day='2023-12-31']")).toBeNull();
    expect(container.querySelector("[data-day='2024-12-31']")).not.toBeNull();
    expect(screen.getByText("Less")).toBeInTheDocument();
    expect(screen.getByText("More")).toBeInTheDocument();
    expect(container.querySelectorAll(".token-activity__legend [data-level]")).toHaveLength(5);
    expect(container.querySelector(".token-activity__cell--unavailable")).toBeNull();
    expect(screen.queryByText("Unavailable")).toBeNull();
  });

  it("marks unknown days as unavailable with coverage and a legend swatch", () => {
    const { container } = renderGrid(year((day) => (day === "2024-12-20" ? null : 10)));
    expect(container.querySelector(".token-activity__summary")).toHaveTextContent("3.6K · Coverage: 364 / 365");
    const gap = container.querySelector("[data-day='2024-12-20'] .token-activity__cell--unavailable");
    expect(gap).not.toBeNull();
    expect(gap?.querySelector("line")).not.toBeNull();
    expect(
      screen.getByTestId("token-activity-cells").querySelectorAll(".token-activity__cell--unavailable"),
    ).toHaveLength(1);
    expect(screen.getByText("Unavailable")).toBeInTheDocument();
    expect(container.querySelector(".token-activity__legend [data-level='unavailable']")).not.toBeNull();
  });

  it("shows the empty state for a quiet year", () => {
    const { container } = renderGrid(year(() => 0));
    expect(screen.getByText("No activity in the last 12 months")).toBeInTheDocument();
    expect(container.querySelector("[data-day]")).toBeNull();
  });

  it("shows a tooltip for the hovered day and clears it on leave", () => {
    const { container } = renderGrid(year((day) => (day === "2024-12-29" ? 2500 : 1)));
    const cells = screen.getByTestId("token-activity-cells");
    fireEvent.mouseMove(cells, cellPoint(364));
    const tooltip = screen.getByRole("tooltip");
    expect(tooltip).toHaveTextContent("2.5K");
    expect(tooltip).toHaveTextContent(formatSpendDay("2024-12-29"));
    expect(container.querySelector(".token-activity__highlight")).not.toBeNull();
    fireEvent.mouseLeave(cells);
    expect(screen.queryByRole("tooltip")).toBeNull();
  });

  it("toggles the selected day on click and ignores unavailable days", () => {
    const points = year((day) => (day === "2024-12-30" ? null : 5));
    const first = renderGrid(points);
    const cells = screen.getByTestId("token-activity-cells");
    fireEvent.click(cells, cellPoint(364));
    expect(first.onSelectDay).toHaveBeenLastCalledWith("2024-12-29");
    fireEvent.click(cells, cellPoint(365));
    expect(first.onSelectDay).toHaveBeenCalledTimes(1);
    first.unmount();

    const second = renderGrid(points, "2024-12-29");
    expect(second.container.querySelector("[data-day='2024-12-29']")).toHaveAttribute("data-selected", "true");
    fireEvent.click(screen.getByTestId("token-activity-cells"), cellPoint(364));
    expect(second.onSelectDay).toHaveBeenLastCalledWith(null);
  });

  it("moves through days with the arrow keys and selects with Enter", () => {
    const { onSelectDay } = renderGrid(year(() => 7));
    const grid = screen.getByTestId("token-activity-daily");
    const live = grid.querySelector("[aria-live]");
    fireEvent.keyDown(grid, { key: "ArrowLeft" });
    expect(live).toHaveTextContent(`${formatSpendDay("2024-12-24")}: 7`);
    fireEvent.keyDown(grid, { key: "ArrowUp" });
    expect(live).toHaveTextContent(`${formatSpendDay("2024-12-23")}: 7`);
    fireEvent.keyDown(grid, { key: "Enter" });
    expect(onSelectDay).toHaveBeenLastCalledWith("2024-12-23");
  });

  it("switches to weekly and cumulative views and remembers the choice", () => {
    const points = year((day) => (day === "2024-06-05" ? null : 1));
    const { container, unmount } = renderGrid(points);
    fireEvent.click(screen.getByRole("radio", { name: "Weekly" }));
    expect(screen.getByRole("radio", { name: "Weekly" })).toHaveAttribute("aria-checked", "true");
    expect(screen.getByText("Each column = 1 week")).toBeInTheDocument();
    expect(screen.getByText("Unavailable")).toBeInTheDocument();
    expect(container.querySelectorAll("[data-week]")).toHaveLength(53);
    expect(window.localStorage.getItem(TOKEN_ACTIVITY_MODE_STORAGE_KEY)).toBe("weekly");

    fireEvent.click(screen.getByRole("radio", { name: "Cumulative" }));
    expect(screen.getByText("Running total")).toBeInTheDocument();
    expect(screen.getByTestId("token-activity-weeks").getAttribute("aria-label")).toContain("Coverage: 364 / 365");
    unmount();

    renderGrid(points);
    expect(screen.getByRole("radio", { name: "Cumulative" })).toHaveAttribute("aria-checked", "true");
  });

  describe("scrolling", () => {
    let restore: (() => void) | null = null;
    afterEach(() => {
      restore?.();
      restore = null;
    });

    it("hides the scroll buttons when the whole year fits", () => {
      restore = stubViewport(800);
      renderGrid(year(() => 1));
      expect(screen.queryByTestId("token-activity-earlier")).toBeNull();
      expect(screen.queryByTestId("token-activity-later")).toBeNull();
    });

    it("opens on the newest weeks and pages back to earlier activity", () => {
      restore = stubViewport(400);
      renderGrid(year(() => 1));
      const earlier = screen.getByRole("button", { name: /Earlier activity/ });
      const later = screen.getByRole("button", { name: /More recent activity/ });
      expect(earlier).toBeEnabled();
      expect(later).toBeDisabled();
      fireEvent.click(earlier);
      expect(earlier).toBeDisabled();
      expect(later).toBeEnabled();
      fireEvent.click(later);
      expect(earlier).toBeEnabled();
    });
  });
});
