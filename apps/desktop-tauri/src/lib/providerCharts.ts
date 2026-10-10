const PROVIDER_CHART_DATA_IDS = new Set(["claude", "codex", "muse", "openai", "pi"]);

export function providerSupportsChartData(providerId: string): boolean {
  return PROVIDER_CHART_DATA_IDS.has(providerId.toLowerCase());
}

/** Providers whose snapshot carries the per-day `openAiApiUsage` history. */
const DAILY_API_USAGE_IDS = new Set(["openaiapi", "groq"]);

export function providerShowsDailyApiUsage(providerId: string): boolean {
  return DAILY_API_USAGE_IDS.has(providerId.toLowerCase());
}
