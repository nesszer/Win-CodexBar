import { describe, expect, it } from "vitest";
import type { LocaleKey } from "../i18n/keys";
import { providerCostPeriodTitle } from "./providerLabels";

const translate = (key: LocaleKey) => `translated:${key}`;

describe("provider labels", () => {
  it("localizes Atlas Cloud balance period and generic period text", () => {
    expect(
      providerCostPeriodTitle("atlascloud", "Atlas Cloud balance", translate),
    ).toBe("translated:AtlasCloudBalance");
    expect(providerCostPeriodTitle("other", "This month", translate)).toBe(
      "translated:ProviderTextThisMonth",
    );
    expect(providerCostPeriodTitle("other", "Gemini Pro", translate)).toBe("Gemini Pro");
  });
});
