import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import type { UsageBarModel } from "../lib/metricRowModel";
import { loadStyles, ruleBlock } from "../test/styles";
import UsageProgressBar from "./UsageProgressBar";

function bar(overrides: Partial<UsageBarModel> = {}): UsageBarModel {
  return {
    fillPercent: 63,
    valuePercent: 63,
    valueText: "63%",
    pacePercent: null,
    paceDeficit: false,
    markers: [],
    ...overrides,
  };
}

function attrs(container: HTMLElement, selector: string, name: string): Array<string | null> {
  return Array.from(container.querySelectorAll(selector), (node) => node.getAttribute(name));
}

function maskCutsOf(container: HTMLElement, group: Element | null) {
  const id = group?.getAttribute("mask")?.match(/^url\(#(.+)\)$/)?.[1];
  const mask = id ? container.querySelector(`mask[id="${id}"]`) : null;
  return Array.from(mask?.querySelectorAll("rect[fill='black']") ?? [], (rect) => ({
    x: rect.getAttribute("x"),
    width: rect.getAttribute("width"),
    opacity: rect.getAttribute("fill-opacity"),
  }));
}

describe("UsageProgressBar", () => {
  it("exposes the value and markers as an accessible progress bar", () => {
    render(
      <UsageProgressBar label="Session" bar={bar({ valueText: "63%. Quota warnings: 10%, 30%" })} />,
    );
    const progress = screen.getByRole("progressbar", { name: "Session" });
    expect(progress.getAttribute("aria-valuenow")).toBe("63");
    expect(progress.getAttribute("aria-valuemin")).toBe("0");
    expect(progress.getAttribute("aria-valuemax")).toBe("100");
    expect(progress.getAttribute("aria-valuetext")).toBe("63%. Quota warnings: 10%, 30%");
    expect(progress.getAttribute("height")).toBe("6");
  });

  it("draws a full rounded track and a fill as wide as the shown percent", () => {
    const { container } = render(<UsageProgressBar label="Session" bar={bar({ fillPercent: 37.5 })} />);
    expect(attrs(container, ".menu-metric__progress-track", "width")).toEqual(["100%"]);
    expect(attrs(container, ".menu-metric__progress-track", "rx")).toEqual(["3"]);
    expect(attrs(container, ".menu-metric__progress-fill", "width")).toEqual(["37.5%"]);
    expect(attrs(container, ".menu-metric__progress-fill", "rx")).toEqual(["3"]);
  });

  it("leaves an empty bar without a fill or masks", () => {
    const { container } = render(<UsageProgressBar label="Session" bar={bar({ fillPercent: 0 })} />);
    expect(container.querySelector(".menu-metric__progress-fill")).toBeNull();
    expect(container.querySelector("mask")).toBeNull();
    expect(container.querySelector("[mask]")).toBeNull();
  });

  it("punches 5px gaps for quota warnings and draws a 1px stripe in each", () => {
    const { container } = render(
      <UsageProgressBar
        label="Session"
        bar={bar({
          markers: [
            { percent: 10, kind: "warning" },
            { percent: 30, kind: "warning" },
          ],
        })}
      />,
    );
    expect(attrs(container, ".menu-metric__progress-warning", "x")).toEqual(["10%", "30%"]);
    expect(attrs(container, ".menu-metric__progress-warning", "width")).toEqual(["1", "1"]);
    expect(attrs(container, ".menu-metric__progress-warning", "transform")).toEqual([
      "translate(-0.5 0)",
      "translate(-0.5 0)",
    ]);
    const fillGroup = container.querySelector(".menu-metric__progress-fill")?.parentElement ?? null;
    expect(maskCutsOf(container, fillGroup)).toEqual([
      { x: "10%", width: "5", opacity: "0.9" },
      { x: "30%", width: "5", opacity: "0.9" },
    ]);
  });

  it("draws work day ticks in the lower half without punching the bar", () => {
    const { container } = render(
      <UsageProgressBar
        label="Weekly"
        bar={bar({
          markers: [
            { percent: 20, kind: "workday" },
            { percent: 40, kind: "workday" },
          ],
        })}
      />,
    );
    expect(attrs(container, ".menu-metric__progress-workday", "x")).toEqual(["20%", "40%"]);
    expect(attrs(container, ".menu-metric__progress-workday", "y")).toEqual(["3", "3"]);
    expect(attrs(container, ".menu-metric__progress-workday", "height")).toEqual(["3", "3"]);
    expect(container.querySelector("mask")).toBeNull();
  });

  it.each([
    [false, "false"],
    [true, "true"],
  ])("cuts a 6px gap for the pace stripe and marks paceDeficit %s as deficit %s", (paceDeficit, deficit) => {
    const { container } = render(
      <UsageProgressBar
        label="Weekly"
        bar={bar({ pacePercent: 40, paceDeficit, markers: [{ percent: 20, kind: "workday" }] })}
      />,
    );
    const stripe = container.querySelector(".menu-metric__progress-pace");
    expect(stripe?.getAttribute("x")).toBe("40%");
    expect(stripe?.getAttribute("width")).toBe("2");
    expect(stripe?.getAttribute("transform")).toBe("translate(-1 0)");
    expect(stripe?.getAttribute("data-deficit")).toBe(deficit);
    const tick = container.querySelector(".menu-metric__progress-workday");
    const pacedGroup = tick?.parentElement ?? null;
    expect(pacedGroup?.contains(container.querySelector(".menu-metric__progress-track"))).toBe(true);
    expect(pacedGroup?.contains(stripe ?? null)).toBe(false);
    expect(maskCutsOf(container, pacedGroup)).toEqual([{ x: "40%", width: "6", opacity: "0.9" }]);
  });

  it("colors the bar from the panel tokens and the pace stripe by direction", () => {
    const css = loadStyles();
    expect(ruleBlock(css, ".menu-metric__progress-track")).toContain("fill: var(--usage-bar-track)");
    expect(ruleBlock(css, ".menu-metric__progress-fill")).toContain(
      "fill: var(--provider-accent, var(--usage-bar-normal))",
    );
    expect(ruleBlock(css, ".menu-metric__progress-warning")).toContain(
      "fill: color-mix(in srgb, var(--text-primary) 68%, transparent)",
    );
    expect(ruleBlock(css, ".menu-metric__progress-workday")).toContain(
      "fill: color-mix(in srgb, var(--text-primary) 30%, transparent)",
    );
    expect(ruleBlock(css, ".menu-metric__progress-pace")).toContain("fill: var(--mac-pace-ok, #34c759)");
    expect(ruleBlock(css, '.menu-metric__progress-pace[data-deficit="true"]')).toContain(
      "fill: var(--mac-pace-bad, #ff3b30)",
    );
  });
});
