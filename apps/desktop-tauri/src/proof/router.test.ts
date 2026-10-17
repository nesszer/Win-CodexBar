import { afterEach, describe, expect, it, vi } from "vitest";
import type {
  BootstrapState,
  CurrentSurfaceState,
  LocaleStrings,
  ProviderDetail,
  ProviderUsageSnapshot,
  SettingsSnapshot,
} from "../types/bridge";
import { PROOF_SCENARIO_NAMES, buildProofScenario } from "./fixtures/scenarios";
import { parseProofParams } from "./params";
import { UNHANDLED_COMMAND_WARNING, createProofRouter } from "./router";

const NOW = new Date("2026-03-10T12:00:00Z");

function routerFor(name: (typeof PROOF_SCENARIO_NAMES)[number], window: "main" | "settings" = "main") {
  return createProofRouter(buildProofScenario(name, NOW), { window, tab: "providers", now: NOW });
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("proof router", () => {
  it.each(PROOF_SCENARIO_NAMES)("serves bootstrap, settings and providers for %s", (name) => {
    const scenario = buildProofScenario(name, NOW);
    const router = routerFor(name);

    const bootstrap = router.handle("get_bootstrap_state") as BootstrapState;
    expect(bootstrap.settings.enabledProviders).toEqual(scenario.settings.enabledProviders);
    expect(bootstrap.providers.length).toBeGreaterThan(0);

    const settings = router.handle("get_settings_snapshot") as SettingsSnapshot;
    expect(settings).toEqual(scenario.settings);

    const cached = router.handle("get_cached_providers") as ProviderUsageSnapshot[];
    expect(cached.map((snapshot) => snapshot.providerId)).toEqual(
      scenario.providers.map((snapshot) => snapshot.providerId),
    );

    const locale = router.handle("get_locale_strings", { language: null }) as LocaleStrings;
    expect(locale.entries.TabGeneral).toBe("General");

    const surface = router.handle("get_current_surface_state") as CurrentSurfaceState;
    expect(surface.mode).toBe("trayPanel");
  });

  it("keeps each provider's identity on its own snapshot", () => {
    const providers = buildProofScenario("default", NOW).providers;
    const emails = providers.map((snapshot) => snapshot.accountEmail).filter(Boolean);
    expect(new Set(emails).size).toBe(emails.length);
    for (const email of emails) expect(email).toMatch(/@example\.com$/);
  });

  it("covers the scenario shapes the screenshots rely on", () => {
    expect(buildProofScenario("default", NOW).providers.map((p) => p.providerId)).toEqual(
      expect.arrayContaining(["codex", "claude"]),
    );
    expect(buildProofScenario("two-codex-accounts", NOW).codexAccounts.accounts).toHaveLength(2);
    const errors = buildProofScenario("errors", NOW).providers;
    expect(errors.some((snapshot) => snapshot.error !== null)).toBe(true);
    expect(
      errors.some((snapshot) => NOW.getTime() - Date.parse(snapshot.updatedAt) > 10 * 60_000),
    ).toBe(true);
    expect(buildProofScenario("empty", NOW).settings.enabledProviders).toEqual([]);
  });

  it("answers provider detail without leaking another provider's identity", () => {
    const router = routerFor("default");
    const claude = router.handle("get_provider_detail", { providerId: "claude" }) as ProviderDetail;
    expect(claude.email).toBe("claude.user@example.com");
    const openrouter = router.handle("get_provider_detail", { providerId: "openrouter" }) as ProviderDetail;
    expect(openrouter.hasSnapshot).toBe(false);
    expect(openrouter.email).toBeNull();
  });

  it("applies settings patches to later reads", () => {
    const router = routerFor("default");
    router.handle("update_settings", { patch: { theme: "light" } });
    expect((router.handle("get_settings_snapshot") as SettingsSnapshot).theme).toBe("light");
  });

  it("keeps provider detail in step with patched enabled providers", () => {
    const router = routerFor("default");
    const enabled = () =>
      (router.handle("get_provider_detail", { providerId: "claude" }) as ProviderDetail).enabled;
    expect(enabled()).toBe(true);
    const remaining = buildProofScenario("default", NOW).settings.enabledProviders.filter(
      (id) => id !== "claude",
    );
    router.handle("update_settings", { patch: { enabledProviders: remaining } });
    expect(enabled()).toBe(false);
    expect(
      (router.handle("get_cached_providers") as ProviderUsageSnapshot[]).map((s) => s.providerId),
    ).not.toContain("claude");
  });

  it("reports the requested settings tab as the shell target", () => {
    const router = createProofRouter(buildProofScenario("default", NOW), {
      window: "settings",
      tab: "providers:claude",
      now: NOW,
    });
    expect(router.handle("get_current_surface_state")).toEqual({
      mode: "settings",
      target: { kind: "settings", tab: "providers:claude" },
    });
  });

  it("returns null and warns once for an unknown command", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const router = routerFor("default");
    expect(router.handle("definitely_not_a_command")).toBeNull();
    expect(router.handle("definitely_not_a_command")).toBeNull();
    expect(warn).toHaveBeenCalledTimes(1);
    expect(warn).toHaveBeenCalledWith(`${UNHANDLED_COMMAND_WARNING} definitely_not_a_command`);
    expect(router.unhandledCommands()).toEqual(["definitely_not_a_command"]);
  });

  it("answers window and dialog plugin commands without warning", () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const router = routerFor("default");
    expect(router.handle("plugin:window|set_size", {})).toBeNull();
    expect(router.handle("plugin:window|inner_size", {})).toEqual({
      width: window.innerWidth,
      height: window.innerHeight,
    });
    expect(router.handle("plugin:dialog|open", {})).toBeNull();
    expect(warn).not.toHaveBeenCalled();
  });
});

describe("proof URL params", () => {
  it("falls back to safe defaults", () => {
    expect(parseProofParams("?window=nope&fixture=nope&theme=nope")).toEqual({
      window: "main",
      tab: "general",
      fixture: "default",
      theme: null,
    });
  });

  it("reads window, tab, fixture and theme", () => {
    expect(parseProofParams("?window=settings&tab=menu&fixture=errors&theme=light")).toEqual({
      window: "settings",
      tab: "menu",
      fixture: "errors",
      theme: "light",
    });
  });
});
