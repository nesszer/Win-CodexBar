import { render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import ProviderGrid from "./ProviderGrid";
import { providerPlaceholder } from "../lib/trayProviders";

vi.mock("../hooks/useLocale", () => import("../test/mocks/locale"));

const providers = [
  providerPlaceholder("codex", "Codex"),
  providerPlaceholder("claude", "Claude"),
];

function grid(selectedProviderId: string | null, variant?: "classic" | "modern") {
  return (
    <ProviderGrid
      providers={providers}
      selectedProviderId={selectedProviderId}
      showAsUsed={false}
      variant={variant}
      onSelect={() => {}}
    />
  );
}

afterEach(() => {
  delete (Element.prototype as Partial<Element>).scrollIntoView;
});

describe("ProviderGrid", () => {
  it("scrolls the active item into view when the selection changes", () => {
    const scrollIntoView = vi.fn();
    Element.prototype.scrollIntoView = scrollIntoView;
    const { rerender } = render(grid(null));
    scrollIntoView.mockClear();
    rerender(grid("claude"));
    expect(scrollIntoView).toHaveBeenCalledTimes(1);
    expect(scrollIntoView).toHaveBeenCalledWith({ block: "nearest", inline: "nearest" });
    expect(scrollIntoView.mock.contexts[0]).toBe(
      document.querySelector(".provider-grid__item--active"),
    );
  });

  it("lays out labelled tabs in one row up to eight items, wrapping up to eighteen", () => {
    const { container, rerender } = render(grid(null));
    const element = () => container.querySelector(".provider-grid");
    expect(element()).toHaveClass("provider-grid--tabs");
    expect(element()).toHaveAttribute("data-tab-layout", "row");

    const nine = Array.from({ length: 8 }, (_, i) => providerPlaceholder(`p${i}`, `P${i}`));
    rerender(
      <ProviderGrid providers={nine} selectedProviderId={null} showAsUsed={false} onSelect={() => {}} />,
    );
    expect(element()).toHaveAttribute("data-tab-layout", "wrap");

    const many = Array.from({ length: 18 }, (_, i) => providerPlaceholder(`p${i}`, `P${i}`));
    rerender(
      <ProviderGrid providers={many} selectedProviderId={null} showAsUsed={false} onSelect={() => {}} />,
    );
    expect(element()).not.toHaveClass("provider-grid--tabs");
  });

  it("classic keeps the compact icon grid and the glyph for All", () => {
    const { container } = render(grid(null, "classic"));
    const element = container.querySelector(".provider-grid");
    expect(element).not.toHaveClass("provider-grid--tabs");
    expect(element).not.toHaveAttribute("data-tab-layout");
    expect(container.querySelector(".provider-grid__icon-overview")?.textContent).toBe("⊞");
  });
});
