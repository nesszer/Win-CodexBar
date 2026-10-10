import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor, act } from "@testing-library/react";
import { buildBundle } from "../test/localeHarness";

vi.mock("../lib/tauri", () => ({
  getLocaleStrings: vi.fn(),
  setUiLanguage: vi.fn(),
}));

vi.mock("@tauri-apps/api/event", () => import("../test/mocks/event"));

import { LocaleProvider } from "../i18n/LocaleProvider";
import { useLocale } from "./useLocale";
import * as tauri from "../lib/tauri";

function Probe() {
  // NOTE: the Phase-13 spec mentions `LocaleKey.AppName`, but the shipped
  // enum has no such variant; TabGeneral is the closest always-present key
  // and exercises the same bundle-resolve path.
  const { t } = useLocale();
  return <span data-testid="app-name">{t("TabGeneral")}</span>;
}

describe("useLocale", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("returns a string from the backend locale bundle once it resolves", async () => {
    (tauri.getLocaleStrings as ReturnType<typeof vi.fn>).mockResolvedValue(
      buildBundle({ TabGeneral: "General" }),
    );

    render(
      <LocaleProvider>
        <Probe />
      </LocaleProvider>,
    );

    await waitFor(() => {
      expect(screen.getByTestId("app-name")).toHaveTextContent("General");
    });
  });

  it("falls back to the raw key when the backend omits it", async () => {
    (tauri.getLocaleStrings as ReturnType<typeof vi.fn>).mockResolvedValue({
      language: "english",
      entries: {}, // intentionally empty — probe should see the key name
    });

    render(
      <LocaleProvider>
        <Probe />
      </LocaleProvider>,
    );

    await waitFor(() => {
      expect(screen.getByTestId("app-name")).toHaveTextContent("TabGeneral");
    });
  });

  it("suspends rendering until the bundle is loaded", async () => {
    let resolveIt: (v: unknown) => void = () => {};
    (tauri.getLocaleStrings as ReturnType<typeof vi.fn>).mockImplementation(
      () =>
        new Promise((resolve) => {
          resolveIt = resolve;
        }),
    );

    const { container } = render(
      <LocaleProvider>
        <Probe />
      </LocaleProvider>,
    );
    expect(container).toBeEmptyDOMElement();

    await act(async () => {
      resolveIt(buildBundle({ TabGeneral: "General" }));
    });

    await waitFor(() => {
      expect(screen.getByTestId("app-name")).toHaveTextContent("General");
    });
  });
});
