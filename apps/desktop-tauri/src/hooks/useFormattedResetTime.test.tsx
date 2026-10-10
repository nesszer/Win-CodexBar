import { act, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { LocaleProvider } from "../i18n/LocaleProvider";
import { buildBundle } from "../test/localeHarness";
import {
  normalizeResetDescription,
  useFormattedResetTime,
  type ResetTimeFormatMode,
} from "./useFormattedResetTime";
import * as tauri from "../lib/tauri";
import type { LocaleKey } from "../i18n/keys";

vi.mock("../lib/tauri", () => ({
  getLocaleStrings: vi.fn(),
  setUiLanguage: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => import("../test/mocks/event"));

function Probe({
  resetsAt,
  fallback,
  relative,
  mode,
}: {
  resetsAt: string | null;
  fallback: string | null;
  relative: boolean;
  mode?: ResetTimeFormatMode;
}) {
  const text = useFormattedResetTime(resetsAt, fallback, relative, mode);
  return <span data-testid="reset">{text ?? "null"}</span>;
}

async function mountWithLocale(ui: React.ReactNode) {
  (tauri.getLocaleStrings as ReturnType<typeof vi.fn>).mockResolvedValue(
    buildBundle({
      MetricResetsIn: "Resets in",
      ResetsInHoursMinutes: "Resets in {}h {}m",
      ResetsInMinutes: "Resets in {}m",
      ResetsInDaysHours: "Resets in {}d {}h",
      ResetsInHoursOnly: "Resets in {}h",
      ResetsInDaysOnly: "Resets in {}d",
      ResetsAtLabel: "Resets {}",
      ResetsAtTime: "Resets at {}",
      TrayResetsInLabel: "Resets in {}",
      DetailCostResets: "Resets",
      TrayResetsDueNow: "Resetting",
      NextExpiresInHoursMinutes: "Next expires in {}h {}m",
      NextExpiresInMinutes: "Next expires in {}m",
      NextExpiresInDaysHours: "Next expires in {}d {}h",
      NextExpiresDueNow: "Expires now",
    }),
  );
  const rendered = render(<LocaleProvider>{ui}</LocaleProvider>);
  await act(async () => {});
  return rendered;
}

function translator(table: Partial<Record<LocaleKey, string>>) {
  return (key: LocaleKey) => table[key] ?? key;
}

const english = translator({
  DetailCostResets: "Resets",
  ResetsAtLabel: "Resets {}",
  ResetsAtTime: "Resets at {}",
  TrayResetsInLabel: "Resets in {}",
  ResetsInMinutes: "Resets in {}m",
  ResetsInHoursMinutes: "Resets in {}h {}m",
  ResetsInHoursOnly: "Resets in {}h",
  ResetsInDaysOnly: "Resets in {}d",
  ResetsInDaysHours: "Resets in {}d {}h",
});

const russian = translator({
  DetailCostResets: "Сброс",
  ResetsAtLabel: "Сброс {}",
  ResetsAtTime: "Сброс в {}",
  TrayResetsInLabel: "Сброс через {}",
  ResetsInMinutes: "Сброс через {} мин",
  ResetsInHoursMinutes: "Сброс через {} ч {} мин",
  ResetsInHoursOnly: "Сброс через {} ч",
  ResetsInDaysOnly: "Сброс через {} д",
  ResetsInDaysHours: "Сброс через {} д {} ч",
});

describe("useFormattedResetTime", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2024-06-01T00:00:00Z"));
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it.each([
    ["Reset", "Resets"],
    ["Resets", "Resets"],
    ["Reset Jul 10 at 2:59am (Europe/Prague)", "Resets Jul 10 at 2:59am (Europe/Prague)"],
    ["Reset in 11m", "Resets in 11m"],
    ["Resets in 11m", "Resets in 11m"],
    ["Reset at 23:30 (UTC)", "Resets at 23:30 (UTC)"],
    ["  rEsEt In 2h 5m \n", "Resets in 2h 5m"],
    ["Reset demain à 23:30", "Resets demain à 23:30"],
    ["at 23:30 (UTC)", "Resets at 23:30 (UTC)"],
    ["Resetting soon", "Resets Resetting soon"],
    ["   \n\t", null],
  ] as const)("normalizes reset description %j", (description, expected) => {
    expect(normalizeResetDescription(description, english)).toBe(expected);
  });

  it.each([
    ["Resets", "Сброс"],
    ["Resets in 2h 10m", "Сброс через 2 ч 10 мин"],
    ["Resets in 12 hours", "Сброс через 12 ч"],
    ["Resets in 5 days", "Сброс через 5 д"],
    ["Resets in 1 minute", "Сброс через 1 мин"],
    ["Resets in 30 seconds", "Сброс через 1 мин"],
    ["Resets in 3d 4h", "Сброс через 3 д 4 ч"],
    ["Resets at 23:30 (UTC)", "Сброс в 23:30 (UTC)"],
    ["Resets Apr 3, 2pm", "Сброс Apr 3, 2pm"],
    ["Resets in a while", "Сброс через a while"],
  ] as const)("localizes reset description %j", (description, expected) => {
    expect(normalizeResetDescription(description, russian)).toBe(expected);
  });

  it("returns a complete localized countdown in relative mode", async () => {
    const target = new Date("2024-06-01T03:42:00Z").toISOString();
    await mountWithLocale(
      <Probe resetsAt={target} fallback="later" relative={true} />,
    );
    expect(screen.getByTestId("reset")).toHaveTextContent("Resets in 3h 42m");
  });

  it("omits zero hours for sub-hour resets", async () => {
    const target = new Date("2024-06-01T00:40:00Z").toISOString();
    await mountWithLocale(
      <Probe resetsAt={target} fallback="later" relative={true} />,
    );
    expect(screen.getByTestId("reset")).toHaveTextContent("Resets in 40m");
  });

  it("normalizes a fallback reset description in relative mode", async () => {
    await mountWithLocale(
      <Probe resetsAt={null} fallback="Reset in 3h" relative={true} />,
    );
    expect(screen.getByTestId("reset")).toHaveTextContent("Resets in 3h");
  });

  it("gives a parsed reset timestamp precedence over fallback wording", async () => {
    const target = new Date("2024-06-01T03:42:00Z").toISOString();
    await mountWithLocale(
      <Probe resetsAt={target} fallback="Reset in 99h" relative={true} />,
    );
    expect(screen.getByTestId("reset")).toHaveTextContent("Resets in 3h 42m");
  });

  it("returns an absolute local time without the reset label", async () => {
    const target = new Date("2024-06-01T03:42:00Z").toISOString();
    await mountWithLocale(
      <Probe resetsAt={target} fallback="later" relative={false} />,
    );
    expect(screen.getByTestId("reset")).not.toHaveTextContent("Resets in");
  });

  it('uses Next expires wording when mode is "expires"', async () => {
    const target = new Date("2024-06-01T03:42:00Z").toISOString();
    await mountWithLocale(
      <Probe resetsAt={target} fallback="later" relative={true} mode="expires" />,
    );
    expect(screen.getByTestId("reset")).toHaveTextContent("Next expires in 3h 42m");
  });
});
