import { render } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import ProviderGrid from "./ProviderGrid";
import { providerPlaceholder } from "../lib/trayProviders";

vi.mock("../hooks/useLocale", () => import("../test/mocks/locale"));

const providers = [
  providerPlaceholder("codex", "Codex"),
  providerPlaceholder("claude", "Claude"),
];

function grid(selectedProviderId: string | null) {
  return (
    <ProviderGrid
      providers={providers}
      selectedProviderId={selectedProviderId}
      showAsUsed={false}
      onSelect={() => {}}
    />
  );
}

afterEach(() => {
  delete (Element.prototype as Partial<Element>).scrollIntoView;
});

function tabLayoutFor(count: number) {
  const many = Array.from({ length: count }, (_, i) => providerPlaceholder(`p${i}`, `P${i}`));
  const { container, unmount } = render(
    <ProviderGrid providers={many} selectedProviderId={null} showAsUsed={false} onSelect={() => {}} />,
  );
  const layout = container.querySelector(".provider-grid")?.getAttribute("data-tab-layout") ?? null;
  unmount();
  return layout;
}

describe("ProviderGrid", () => {
  it("wraps labelled tabs once a single row would be narrower than a wrapped row", () => {
    expect(tabLayoutFor(5)).toBe("row");
    expect(tabLayoutFor(6)).toBe("wrap");
    expect(tabLayoutFor(17)).toBe("wrap");
    expect(tabLayoutFor(18)).toBeNull();
  });

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
});
