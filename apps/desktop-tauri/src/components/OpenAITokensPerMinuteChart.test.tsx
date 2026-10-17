import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { LocaleKey } from "../i18n/keys";
import { formatTokens } from "../lib/openaiThroughput";
import type {
  ModelRateLimitSnapshot,
  ProviderThroughputSnapshot,
  ThroughputModelSnapshot,
} from "../types/bridge";
import { OpenAITokensPerMinuteChart } from "./OpenAITokensPerMinuteChart";

const START = 1_788_220_800;
const t = (key: LocaleKey) => key as string;

function model(name: string, overrides: Partial<ThroughputModelSnapshot> = {}): ThroughputModelSnapshot {
  return {
    name,
    latestTpm: 0,
    peakTpm: 0,
    totalTokens: 0,
    peakLimitCountedTokens: 0,
    limitCountedTokensByMinute: [],
    ...overrides,
  };
}

/** Three reported minutes (peak 4,000 at START + 60), then two pending minutes. */
function throughput(overrides: Partial<ProviderThroughputSnapshot> = {}): ProviderThroughputSnapshot {
  return {
    minutes: [
      { start: START, totalTokens: 1_000, requests: 1, isPending: false },
      { start: START + 60, totalTokens: 4_000, requests: 2, isPending: false },
      { start: START + 120, totalTokens: 2_000, requests: 1, isPending: false },
      { start: START + 180, totalTokens: 0, requests: 0, isPending: true },
      { start: START + 240, totalTokens: 0, requests: 0, isPending: true },
    ],
    frontier: START + 120,
    latestTokensPerMinute: 2_000,
    peakTokensPerMinute: 4_000,
    peakAt: START + 60,
    fiveMinuteAverageTokensPerMinute: 2_333,
    models: [],
    windowMinutes: 60,
    fetchedAt: START + 300,
    projectId: null,
    ...overrides,
  };
}

describe("OpenAITokensPerMinuteChart", () => {
  it("draws reported minutes as solid bars and pending minutes as outlines", () => {
    const { container } = render(<OpenAITokensPerMinuteChart throughput={throughput()} t={t} />);
    const bars = container.querySelectorAll("rect.openai-tpm__bar");
    expect(bars).toHaveLength(5);
    expect(container.querySelectorAll("rect.openai-tpm__bar--pending")).toHaveLength(2);
    expect(container.querySelector(".openai-tpm__legend")).not.toBeNull();
  });

  it("never reports pending minutes as the peak", () => {
    const { container } = render(<OpenAITokensPerMinuteChart throughput={throughput()} t={t} />);
    const peakCell = Array.from(container.querySelectorAll(".openai-tpm__stats > div")).find(
      (cell) => cell.querySelector("dt")?.textContent === "OpenAiTpmPeak",
    );
    expect(peakCell?.querySelector("dd")?.textContent).toContain(formatTokens(4_000));
  });

  it("shows the empty state when no minute has reported traffic", () => {
    const pendingOnly = throughput({
      minutes: [{ start: START, totalTokens: 0, requests: 0, isPending: true }],
      peakTokensPerMinute: 0,
      latestTokensPerMinute: null,
    });
    const { container } = render(<OpenAITokensPerMinuteChart throughput={pendingOnly} t={t} />);
    expect(container.querySelector("svg")).toBeNull();
    expect(container.querySelector(".openai-tpm__empty")?.textContent).toBe("OpenAiTpmEmpty");
  });

  it("shows the limit line only when a manual limit matches a model in the window", () => {
    const busy = throughput({
      models: [model("gpt-5", { peakLimitCountedTokens: 4_000 })],
    });
    const limits: ModelRateLimitSnapshot[] = [{ model: "gpt-5", tokensPerMinute: 8_000 }];
    const withLimit = render(
      <OpenAITokensPerMinuteChart throughput={busy} t={t} limits={limits} />,
    );
    expect(withLimit.container.querySelector(".openai-tpm__limit")).not.toBeNull();
    withLimit.unmount();

    const unmatched = render(
      <OpenAITokensPerMinuteChart
        throughput={busy}
        t={t}
        limits={[{ model: "gpt-4o", tokensPerMinute: 8_000 }]}
      />,
    );
    expect(unmatched.container.querySelector(".openai-tpm__limit")).toBeNull();
  });

  it("lists per-model rows only when showModels is set", () => {
    const busy = throughput({ models: [model("gpt-5", { latestTpm: 900, peakTpm: 4_000 })] });
    const card = render(<OpenAITokensPerMinuteChart throughput={busy} t={t} />);
    expect(card.container.querySelector(".openai-tpm__models")).toBeNull();
    card.unmount();

    const live = render(<OpenAITokensPerMinuteChart throughput={busy} t={t} showModels />);
    expect(live.container.querySelector(".openai-tpm__model-name")?.textContent).toBe("gpt-5");
  });
});
