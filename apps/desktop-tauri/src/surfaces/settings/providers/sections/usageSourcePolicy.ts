import type { LocaleKey } from "../../../../i18n/keys";

export interface UsageSourceOption {
  value: string;
  label: string;
  labelKey?: LocaleKey;
  description: string;
  descriptionKey?: LocaleKey;
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
  nous: {
    options: [
      { value: "auto", label: "Auto", description: "Uses the Hermes Agent login or the configured Nous token." },
      { value: "oauth", label: "Hermes OAuth", description: "Uses the read-only Nous Portal token from Hermes Agent." },
    ],
  },
  hyper: {
    options: [
      { value: "auto", label: "Auto", description: "Tries the Charm Hyper browser session, then the configured API key." },
      { value: "web", label: "Browser session", description: "Uses the selected hyper.charm.land browser session only." },
      { value: "oauth", label: "API", description: "Uses the configured Charm Hyper API key only." },
    ],
  },
  groq: {
    options: [
      { value: "auto", label: "Auto", description: "Tries the console.groq.com browser session, then Enterprise Prometheus metrics with the API key." },
      { value: "web", label: "Browser session", description: "Uses the console.groq.com browser session or manual cookie header only." },
      { value: "oauth", label: "API", description: "Uses the configured API key for Enterprise Prometheus metrics only." },
    ],
  },
  zed: {
    options: [
      {
        value: "auto",
        label: "Auto",
        labelKey: "ProviderSourceAutoShort",
        description: "Uses the Zed editor credential; the browser session is used only when Browser session is selected.",
        descriptionKey: "ProviderZedUsageSourceAutoHelp",
      },
      {
        value: "oauth",
        label: "API",
        labelKey: "ProviderSourceApiShort",
        description: "Uses the Zed editor credential only.",
        descriptionKey: "ProviderZedUsageSourceApiHelp",
      },
      {
        value: "web",
        label: "Browser session",
        labelKey: "ProviderZedBrowserSession",
        description: "Reads token spend from the zed.dev browser session or manual cookie header only, with no editor-credential fallback.",
        descriptionKey: "ProviderZedUsageSourceWebHelp",
      },
    ],
  },
  gitkraken: {
    options: [
      { value: "auto", label: "Auto", description: "Uses the configured GitKraken access token." },
      { value: "oauth", label: "API", description: "Uses the configured GitKraken access token only." },
    ],
  },
  bifrost: {
    options: [
      { value: "auto", label: "Auto", description: "Uses the configured Bifrost gateway and virtual key." },
      { value: "oauth", label: "API", description: "Uses the configured Bifrost gateway and virtual key only." },
    ],
  },
  aixy: {
    options: [
      { value: "auto", label: "Auto", description: "Uses the configured Aixy API key." },
      { value: "oauth", label: "API", description: "Uses the configured Aixy API key only." },
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
