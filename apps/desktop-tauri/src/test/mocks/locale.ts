/** vi.mock factory for hooks/useLocale: identity translations under English. */
export const useLocale = () => ({ t: (key: string) => key, language: "english" });
