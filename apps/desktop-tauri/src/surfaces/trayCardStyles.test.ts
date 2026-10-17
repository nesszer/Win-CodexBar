import { describe, expect, it } from "vitest";
import { loadStyles, loadStylesheet, ruleBlock } from "../test/styles";

// jsdom never applies CSS, so these pin the card anatomy rules (spec 3.1-3.8, 5)
// in the stylesheet text.
const tray = loadStylesheet("surfaces/tray-card.css");
const settings = loadStylesheet("surfaces/settings/settings-layout.css");
const base = loadStyles();
const main = loadStylesheet("main.tsx");

describe("tray card stylesheet", () => {
  it("loads after every other stylesheet so it wins the tray cascade", () => {
    const imports = [...main.matchAll(/^import "(.+\.css)";$/gm)].map((match) => match[1]);
    expect(imports[imports.length - 1]).toBe("./surfaces/tray-card.css");
  });

  it("scopes every rule to the tray surface", () => {
    const withoutComments = tray.replace(/\/\*[\s\S]*?\*\//g, "");
    const selectors = [...withoutComments.matchAll(/(^|})\s*([^{}@]+)\{/g)].map((m) =>
      m[2].trim(),
    );
    expect(selectors.length).toBeGreaterThan(20);
    for (const group of selectors) {
      for (const selector of group.split(",")) {
        expect(selector.trim(), group).toMatch(/^\.menu-surface--tray\b/);
      }
    }
  });

  it("styles the header as name and email, then subtitle and plain plan text", () => {
    expect(ruleBlock(tray, ".menu-surface--tray .card-header__name")).toContain(
      "font: var(--mac-text-headline)",
    );
    const email = ruleBlock(tray, ".menu-surface--tray .card-header__email");
    expect(email).toContain("var(--mac-text-subheadline)");
    expect(email).toContain("color: var(--mac-text-2)");
    const plan = ruleBlock(tray, ".menu-surface--tray .card-header__plan");
    expect(plan).toContain("var(--mac-text-footnote)");
    expect(plan).not.toMatch(/border|background/);
    const error = ruleBlock(tray, ".menu-surface--tray .card-header__error");
    expect(error).toContain("color: var(--mac-error)");
    expect(error).toContain("-webkit-line-clamp: 4");
  });

  it("presses the copy button with a .18 fill and a .94 scale over 120ms", () => {
    const active = ruleBlock(tray, ".menu-surface--tray .menu-card__copy-btn:active");
    expect(active).toContain("scale(0.94)");
    expect(active).toContain("0.18");
    expect(ruleBlock(tray, ".menu-surface--tray .menu-card__copy-btn")).toContain("120ms");
  });

  it("puts 6px over and 16px under the header divider and uses the separator tone between blocks", () => {
    const divider = ruleBlock(tray, ".menu-surface--tray .menu-card__divider");
    expect(divider).toContain("background: var(--mac-divider)");
    expect(divider).toContain("margin: var(--mac-gap-header) 0 var(--mac-after-divider)");
    expect(ruleBlock(tray, ".menu-surface--tray .menu-card__separator")).toContain(
      "background: var(--mac-separator)",
    );
  });

  it("lays out key-value rows as caption label left and medium value right", () => {
    expect(ruleBlock(tray, ".menu-surface--tray .card-kv__title")).toContain(
      "text-transform: uppercase",
    );
    expect(ruleBlock(tray, ".menu-surface--tray .card-kv__row")).toContain(
      "justify-content: space-between",
    );
    expect(ruleBlock(tray, ".menu-surface--tray .card-kv__value")).toContain("font-weight: 500");
  });

  it("draws the inline dashboard as a two-column KPI grid over a 58px chart", () => {
    expect(ruleBlock(tray, ".menu-surface--tray .card-dash__kpis")).toContain(
      "grid-template-columns: minmax(118px, 1fr) minmax(100px, 1fr)",
    );
    expect(ruleBlock(tray, ".menu-surface--tray .card-dash__chart")).toContain(
      "height: var(--mac-chart-h)",
    );
    const bar = ruleBlock(tray, ".menu-surface--tray .card-dash__bar");
    expect(bar).toContain("background: var(--provider-accent)");
    expect(bar).toContain("var(--mac-chart-bar-r)");
  });

  it("never animates progress widths", () => {
    expect(tray).not.toMatch(/transition:[^;]*width/);
    expect(ruleBlock(tray, ".menu-surface--tray .card-extra__fill")).toContain(
      "background: var(--provider-accent)",
    );
  });

  it("restyles the switcher: accent selection, instant black .095 hover, 2px quota line", () => {
    const item = ruleBlock(tray, ".menu-surface--tray .provider-grid__item");
    expect(item).toContain("border-radius: 6px");
    expect(item).toContain("transition: none");
    expect(ruleBlock(tray, ".menu-surface--tray .provider-grid__item:hover")).toContain(
      "rgba(0, 0, 0, 0.095)",
    );
    const active = ruleBlock(tray, ".menu-surface--tray .provider-grid__item--active");
    expect(active).toContain("background: var(--mac-selection-bg)");
    expect(active).toContain("box-shadow: none");
    expect(ruleBlock(tray, ".menu-surface--tray .provider-grid__weekly-track")).toContain(
      "height: 2px",
    );
  });

  it("restyles the Overview spend summary as headline, 17px total and captions", () => {
    expect(ruleBlock(tray, ".menu-surface--tray .overview-spend")).toContain("padding: 10px 20px");
    const total = ruleBlock(tray, ".menu-surface--tray .overview-spend__total");
    expect(total).toContain("font-size: 17px");
    expect(total).toContain("font-weight: 700");
    expect(total).toContain("tabular-nums");
  });
});

describe("per-surface tint token", () => {
  it("is white on dark surfaces and black on light ones", () => {
    expect(base).toMatch(/:root\s*\{[^}]*--surface-tint-rgb: 255, 255, 255;/);
    expect(base).toMatch(/\[data-theme="light"\]\s*\{[^}]*--surface-tint-rgb: 0, 0, 0;/);
    expect(base).toMatch(/\.menu-surface--tray\s*\{[^}]*--surface-tint-rgb: 0, 0, 0;/);
  });

  it("replaces the tray-only dark-tint overrides", () => {
    for (const selector of [
      ".menu-surface--tray .menu-card__copy-btn:hover",
      ".menu-surface--tray .update-banner__action:hover",
      ".menu-surface--tray .credential-btn",
      ".menu-surface--tray .provider-grid__item--more .provider-grid__icon-overview",
    ]) {
      expect(base).not.toContain(`${selector} {`);
    }
    expect(ruleBlock(base, ".menu-card__copy-btn:hover")).toContain("var(--surface-tint-rgb)");
    expect(ruleBlock(base, ".update-banner__action:hover")).toContain("var(--surface-tint-rgb)");
    expect(ruleBlock(base, ".credential-btn:hover:not(:disabled)")).toContain(
      "var(--surface-tint-rgb)",
    );
  });
});

describe("menu card hover highlight opt-out", () => {
  const offRow =
    ".menu-surface--hover-highlight-off .menu-surface__footer-row--refresh:hover:not(:disabled):not(:focus-visible)";

  it("drops the pointer highlight from the Refresh row", () => {
    expect(ruleBlock(base, offRow)).toContain("color: var(--mac-text)");
    expect(ruleBlock(base, `${offRow}::before`)).toContain("background: transparent");
    expect(ruleBlock(base, `${offRow} .menu-surface__footer-shortcut`)).toContain(
      "color: var(--mac-text-3)",
    );
  });

  it("keeps keyboard selection and the native-style rows highlighted", () => {
    expect(base).not.toMatch(/hover-highlight-off[^{]*focus-visible\s*[,{]/);
    expect(base).not.toMatch(/hover-highlight-off \.menu-surface__footer-row[:\s{]/);
    expect(ruleBlock(base, ".menu-surface__footer-row:focus-visible::before")).toContain(
      "background: var(--mac-selection-bg)",
    );
  });
});

describe("Settings usage details", () => {
  it("colors the pace bars from the pace tokens and keeps budget values off pills", () => {
    expect(
      ruleBlock(settings, '.provider-usage-details__pace-fill[data-pace="burning"]'),
    ).toContain("var(--pace-burning-fg)");
    expect(ruleBlock(settings, ".provider-usage-details__pace-fill--expected")).toContain(
      "var(--pace-track-expected-fg)",
    );
    expect(ruleBlock(settings, ".provider-usage-details__budget-value")).toContain(
      "border-radius: 4px",
    );
  });
});
