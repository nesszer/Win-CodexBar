import { readFileSync } from "node:fs";
import { expect } from "vitest";

// jsdom runs with `css: false`, so styles.css is never applied and computed
// styles are empty. Layout tests assert on the stylesheet text instead.
// import.meta.dirname (not .url) survives vitest's jsdom transform as the real
// on-disk directory.
let cached: string | undefined;

/**
 * The stylesheet text behind src/styles.css, read once per test file. Its
 * `@import` lines are inlined in order, so the result is the full cascade.
 */
export function loadStyles(): string {
  if (cached === undefined) {
    if (!import.meta.dirname) {
      throw new Error("import.meta.dirname unavailable to vitest runner");
    }
    const src = `${import.meta.dirname}/..`;
    cached = readFileSync(`${src}/styles.css`, "utf8").replace(
      /^@import "\.\/([^"]+)";\r?\n/gm,
      (_, file: string) => readFileSync(`${src}/${file}`, "utf8"),
    );
  }
  return cached;
}

/**
 * Declarations of the first rule whose selector is exactly `selector`. The
 * match is anchored to a line start so `.a` cannot match inside `.b .a`.
 */
export function ruleBlock(css: string, selector: string): string {
  const escaped = selector.replace(/[^\w-]/g, "\\$&");
  const match = css.match(
    new RegExp("(?:^|\\r?\\n)" + escaped + "\\s*\\{([^}]*)\\}"),
  );
  expect(match, `no rule for ${selector}`).not.toBeNull();
  return match![1];
}
