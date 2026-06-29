import { describe, it, expect } from "vitest";
import { ALL_LOCALE_KEYS } from "./keys";

describe("ALL_LOCALE_KEYS", () => {
  it("includes LanguageSpanishOption after LanguageJapaneseOption", () => {
    const jaIdx = ALL_LOCALE_KEYS.indexOf("LanguageJapaneseOption");
    expect(jaIdx).toBeGreaterThan(-1);

    const esIdx = ALL_LOCALE_KEYS.indexOf("LanguageSpanishOption");
    expect(esIdx).toBe(jaIdx + 1);
  });

  it("has exactly the expected number of language options", () => {
    const languageOptions = ALL_LOCALE_KEYS.filter((k) =>
      k.startsWith("Language"),
    );
    // EnglishOption, ChineseOption, JapaneseOption, SpanishOption
    expect(languageOptions).toHaveLength(4);
  });
});
