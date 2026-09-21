export interface UsageSourceOption {
  value: string;
  label: string;
  description: string;
}

export interface UsageSourcePolicy {
  options: readonly UsageSourceOption[];
  hideCookieSourceValues?: readonly string[];
}

const POLICIES: Readonly<Record<string, UsageSourcePolicy>> = {
  grok: {
    options: [
      { value: "auto", label: "Auto", description: "Tries the local Grok login first, then browser cookies." },
      { value: "cli", label: "Grok CLI", description: "Uses the locally selected Grok login principal only." },
      { value: "oauth", label: "SuperGrok OAuth", description: "Uses the local SuperGrok OAuth principal only, without browser cookies." },
      { value: "web", label: "Browser cookies", description: "Uses the configured grok.com browser session only." },
    ],
  },
  alibabatokenplan: {
    options: [
      { value: "auto", label: "Auto", description: "Tries the signed-in Bailian CLI first, then browser cookies." },
      { value: "cli", label: "Bailian CLI", description: "Uses the locally signed-in Bailian CLI only." },
      { value: "web", label: "Browser cookies", description: "Uses the configured Model Studio / Bailian browser session only." },
    ],
    hideCookieSourceValues: ["cli"],
  },
  antigravity: {
    options: [
      {
        value: "auto",
        label: "Auto",
        description: "Auto skips agy reports without account identity for selected or injected Google accounts. Try Local API / agy CLI to use the local app or agy's signed-in account, which may differ.",
      },
      {
        value: "cli",
        label: "Local API / agy CLI",
        description: "Uses the local Antigravity app or agy's signed-in account, which may differ from the selected Google account.",
      },
    ],
  },
  muse: {
    options: [
      { value: "auto", label: "Auto", description: "Uses the local Muse Code device login." },
      { value: "oauth", label: "Muse Code login", description: "Uses the local Muse Code device-code login only." },
    ],
  },
  venice: {
    options: [
      {
        value: "auto",
        label: "Auto",
        description: "Uses the Venice API key or token account; browser sessions are used only when Web is selected.",
      },
      {
        value: "oauth",
        label: "API",
        description: "Uses the Venice API key or token account only.",
      },
      {
        value: "web",
        label: "Browser session",
        description: "Reads Venice subscription credits from the selected browser session or manual cookie header.",
      },
    ],
  },
};

export function usageSourcePolicy(providerId: string): UsageSourcePolicy | null {
  return POLICIES[providerId] ?? null;
}

export function shouldShowCookieSource(
  providerId: string,
  usageSource: string | null | undefined,
): boolean {
  const hiddenValues = usageSourcePolicy(providerId)?.hideCookieSourceValues;
  return !hiddenValues?.includes(usageSource ?? "auto");
}
