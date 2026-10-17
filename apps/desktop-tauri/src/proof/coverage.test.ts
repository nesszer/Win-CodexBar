import { readdirSync, readFileSync } from "node:fs";
import { describe, expect, it, vi } from "vitest";
import { buildProofScenario } from "./fixtures/scenarios";
import { INERT_ACTIONS, createProofRouter } from "./router";

const SRC = `${import.meta.dirname}/..`;
const INVOKE_CALL = /\binvoke\s*(?:<[^>]*>)?\(\s*["']([A-Za-z0-9_]+)["']/g;

function productionSources(dir: string): string[] {
  return readdirSync(dir, { withFileTypes: true }).flatMap((entry) => {
    const path = `${dir}/${entry.name}`;
    if (entry.isDirectory()) return entry.name === "proof" ? [] : productionSources(path);
    return /\.tsx?$/.test(entry.name) && !/\.(test|spec)\.tsx?$/.test(entry.name) ? [path] : [];
  });
}

function invokedCommands(): Map<string, string> {
  const commands = new Map<string, string>();
  for (const file of productionSources(SRC)) {
    for (const match of readFileSync(file, "utf8").matchAll(INVOKE_CALL)) {
      if (!commands.has(match[1])) commands.set(match[1], file.slice(SRC.length + 1));
    }
  }
  return commands;
}

describe("proof router coverage", () => {
  it("finds the frontend's invoke calls", () => {
    expect(invokedCommands().size).toBeGreaterThan(50);
  });

  // A new backend command must either get a fixture handler or be declared an
  // inert action, so the proof build never silently answers it with null.
  it("handles or explicitly declares every command the frontend invokes", () => {
    const known = new Set([...handledCommands(), ...INERT_ACTIONS]);
    const missing = [...invokedCommands()]
      .filter(([command]) => !known.has(command))
      .map(([command, file]) => `${command} (${file})`);
    expect(missing).toEqual([]);
  });

  it("declares no inert action that also has a handler or is no longer invoked", () => {
    const handled = new Set(handledCommands());
    const invoked = invokedCommands();
    expect(INERT_ACTIONS.filter((command) => handled.has(command))).toEqual([]);
    expect(INERT_ACTIONS.filter((command) => !invoked.has(command))).toEqual([]);
  });

  it("records inert actions without warning", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const router = createProofRouter(buildProofScenario("default", new Date()), {
      window: "main",
      tab: "general",
    });
    expect(router.handle("codex_account_switch", { id: "work" })).toBeNull();
    expect(router.calls()).toEqual([{ cmd: "codex_account_switch", args: { id: "work" } }]);
    expect(warn).not.toHaveBeenCalled();
    warn.mockRestore();
  });
});

function handledCommands(): readonly string[] {
  return createProofRouter(buildProofScenario("default", new Date()), {
    window: "main",
    tab: "general",
  }).handledCommands;
}
