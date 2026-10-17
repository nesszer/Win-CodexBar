import { cleanup, configure, render, screen } from "@testing-library/react";
import { clearMocks } from "@tauri-apps/api/mocks";
import { afterEach, describe, expect, it, vi } from "vitest";
import { parseProofParams } from "./params";
import type { ProofRouter } from "./router";

// Each case re-imports the whole App after vi.resetModules(); under a loaded
// full-suite run that cold import alone can exceed the default timeouts.
configure({ asyncUtilTimeout: 10_000 });

async function renderProof(search: string): Promise<ProofRouter> {
  window.history.replaceState({}, "", `/proof.html${search}`);
  // Fresh modules per render: surface and settings stores are module singletons.
  vi.resetModules();
  const { installProofMocks } = await import("./install");
  const router = installProofMocks(parseProofParams(search));
  const { default: App } = await import("../App");
  render(<App />);
  return router;
}

afterEach(async () => {
  cleanup();
  // Unmount unlistens resolve asynchronously and still need the mocked internals.
  await new Promise((resolve) => setTimeout(resolve, 0));
  clearMocks();
  window.history.replaceState({}, "", "/");
});

describe("proof build renders App over the mocked bridge", { timeout: 20_000 }, () => {
  it("shows the tray panel with the default providers in the main window", async () => {
    const router = await renderProof("?window=main&fixture=default");
    expect((await screen.findAllByText("Codex")).length).toBeGreaterThan(0);
    expect((await screen.findAllByText("Claude")).length).toBeGreaterThan(0);
    expect(screen.queryByText("Bootstrap failed")).not.toBeInTheDocument();
    expect(router.unhandledCommands()).toEqual([]);
  });

  it("renders the flyout for the errors scenario", async () => {
    await renderProof("?window=flyout&fixture=errors");
    expect((await screen.findAllByText("Cursor")).length).toBeGreaterThan(0);
  });

  it("renders Settings > General in the settings window", async () => {
    const router = await renderProof("?window=settings&tab=general&fixture=default");
    expect(await screen.findByText("Start at login")).toBeInTheDocument();
    expect(router.unhandledCommands()).toEqual([]);
  });

  it("renders Settings > Providers with the enabled providers listed", async () => {
    await renderProof("?window=settings&tab=providers&fixture=default");
    expect((await screen.findAllByText("Codex")).length).toBeGreaterThan(0);
    expect((await screen.findAllByText("Claude")).length).toBeGreaterThan(0);
  });

  it("renders the Codex pane for two accounts", async () => {
    await renderProof("?window=settings&tab=providers:codex&fixture=two-codex-accounts");
    expect((await screen.findAllByText(/work@example\.com/)).length).toBeGreaterThan(0);
    expect((await screen.findAllByText(/personal@example\.com/)).length).toBeGreaterThan(0);
  });

  it("renders the main window with no providers enabled", async () => {
    const router = await renderProof("?window=main&fixture=empty");
    await vi.waitFor(
      () => {
        expect(document.body.textContent).not.toBe("");
        expect(screen.queryByText("Loading shell contract…")).not.toBeInTheDocument();
      },
      { timeout: 10_000 },
    );
    expect(screen.queryByText("Codex")).not.toBeInTheDocument();
    expect(router.unhandledCommands()).toEqual([]);
  });
});
