import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import ProviderAccountsMenu from "./ProviderAccountsMenu";

describe("ProviderAccountsMenu", () => {
  it("shows accounts and the add action by default, and preserves a collapse across updates", async () => {
    const onLayoutChange = vi.fn();
    const content = (count: number) => <ProviderAccountsMenu title="Accounts" count={count} onLayoutChange={onLayoutChange} actions={<button>Add account</button>}>
      <ul><li>Saved account</li></ul>
    </ProviderAccountsMenu>;
    const { container, rerender } = render(content(1));
    const details = container.querySelector("details")!;
    expect(details.open).toBe(true);
    const add = screen.getByRole("button", { name: "Add account" });
    expect(screen.getByText("Saved account").compareDocumentPosition(add) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    fireEvent.click(screen.getByText("Accounts"));
    await waitFor(() => expect(details.open).toBe(false));
    await waitFor(() => expect(onLayoutChange).toHaveBeenCalled());
    rerender(content(2));
    expect(details.open).toBe(false);
    fireEvent.click(screen.getByText("Accounts"));
    await waitFor(() => expect(details.open).toBe(true));
    expect(screen.getByRole("button", { name: "Add account" })).toBeEnabled();
  });
});
