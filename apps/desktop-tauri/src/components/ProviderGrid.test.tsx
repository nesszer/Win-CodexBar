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
});
