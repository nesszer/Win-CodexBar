import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const SRC = `${import.meta.dirname}/..`;

function source(relative: string): string {
  return readFileSync(`${SRC}/${relative}`, "utf8");
}

describe("production entry isolation", () => {
  it.each(["main.tsx", "App.tsx", "lib/tauri.ts"])("%s never imports the proof bridge", (file) => {
    const text = source(file);
    expect(text).not.toMatch(/from\s+["'][^"']*\/proof(\/|["'])/);
    expect(text).not.toMatch(/import\(\s*["'][^"']*\/proof(\/|["'])/);
    expect(text).not.toContain("@tauri-apps/api/mocks");
  });

  it("index.html still boots the production entry", () => {
    const html = readFileSync(`${SRC}/../index.html`, "utf8");
    expect(html).toContain('src="/src/main.tsx"');
    expect(html).not.toContain("proof");
  });
});
