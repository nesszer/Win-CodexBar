import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { SpendProviderIcon } from "./SpendProviderIcon";

describe("SpendProviderIcon", () => {
  it("renders the brand PNG for Codex in provider headers", () => {
    const { container } = render(<SpendProviderIcon providerId="codex" />);
    const img = container.querySelector("img");
    expect(img).not.toBeNull();
    expect(img?.getAttribute("src")).toBeTruthy();
    expect(container.querySelector(".spend-provider-icon--brand")).not.toBeNull();
    expect(container.querySelector(".spend-provider-icon--monochrome")).toBeNull();
  });

  it("renders the brand SVG with its original fills, not the monochrome tint", () => {
    const { container } = render(<SpendProviderIcon providerId="claude" />);
    const root = container.querySelector(".spend-provider-icon--brand");
    expect(root).not.toBeNull();
    expect(root?.querySelector("svg")).not.toBeNull();
    expect(root?.innerHTML).not.toContain("currentColor");
  });

  it("renders the adaptive monochrome artwork when requested for child rows", () => {
    const { container } = render(<SpendProviderIcon providerId="claude" style="monochrome" />);
    const root = container.querySelector(".spend-provider-icon--monochrome");
    expect(root).not.toBeNull();
    expect(container.querySelector(".spend-provider-icon--brand")).toBeNull();
    expect(container.querySelector("img")).toBeNull();
    expect(root?.querySelector("svg")).not.toBeNull();
  });

  it("keeps the 20 point slot and applies the per-provider scale", () => {
    const { container } = render(<SpendProviderIcon providerId="antigravity" />);
    const slot = container.querySelector<HTMLElement>(".spend-provider-icon");
    expect(slot?.style.width).toBe("20px");
    expect(slot?.style.height).toBe("20px");
    const art = container.querySelector<HTMLElement>(".spend-provider-icon img");
    expect(art?.style.width).toBe(`${20 * 1.38}px`);
  });

  it("falls back to the provider letter when no artwork exists", () => {
    const { container } = render(<SpendProviderIcon providerId="openaiapi" />);
    expect(container.querySelector("img")).toBeNull();
    expect(container.querySelector(".spend-provider-icon--monochrome")?.textContent).toBe("O");
  });

  it("is hidden from assistive technology because the provider name is printed beside it", () => {
    const { container } = render(<SpendProviderIcon providerId="codex" />);
    expect(container.querySelector(".spend-provider-icon")?.getAttribute("aria-hidden")).toBe("true");
  });
});
