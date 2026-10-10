// Brand colors mirror the Rust `brand_color` table in rust/src/core/provider.rs;
// providerIcons.test.ts fails when the ids or colors drift.

/**
 * Replace hard-coded fills/strokes in the bundled brand SVGs with
 * `currentColor` so the icon picks up the brand color via CSS, making each
 * provider visually distinct in compact tray rows.
 */
function tint(raw: string): string {
  return raw
    .replace(/fill="white"/gi, 'fill="currentColor"')
    .replace(/fill="#fff"/gi, 'fill="currentColor"')
    .replace(/fill="#ffffff"/gi, 'fill="currentColor"')
    .replace(/stroke="white"/gi, 'stroke="currentColor"');
}

export interface ProviderIcon {
  /** CLI-style provider id (lowercase, normalized). */
  id: string;
  /** Brand hex color. */
  brandColor: string;
  /** Single-character fallback used when no SVG is available. */
  fallbackLetter: string;
  /** Raw SVG markup when the provider ships a brand asset. */
  svgPath?: string;
}

// Each icons/ProviderIcon-<name>.svg is RAW[<name>].
const RAW: Record<string, string> = Object.fromEntries(
  Object.entries(
    import.meta.glob<string>("./icons/ProviderIcon-*.svg", {
      query: "?raw",
      import: "default",
      eager: true,
    }),
  ).map(([path, svg]) => [path.slice("./icons/ProviderIcon-".length, -".svg".length), tint(svg)]),
);

/** Provider icons keyed by normalized provider id. */
const ICON_ENTRIES: Record<string, Omit<ProviderIcon, "id">> = {
  alibaba:     { brandColor: "#ff6a00", fallbackLetter: "阿", svgPath: RAW.alibaba },
  alibabatokenplan: { brandColor: "#ff6a00", fallbackLetter: "阿", svgPath: RAW.alibaba },
  amp:         { brandColor: "#f34e3f", fallbackLetter: "⚡", svgPath: RAW.amp },
  antigravity: { brandColor: "#60ba7e", fallbackLetter: "◉", svgPath: RAW.antigravity },
  augment:     { brandColor: "#1aa049", fallbackLetter: "A", svgPath: RAW.augment },
  claude:      { brandColor: "#cc7c5e", fallbackLetter: "◈", svgPath: RAW.claude },
  pi:          { brandColor: "#7c3aed", fallbackLetter: "P" },
  codebuff:    { brandColor: "#00ff95", fallbackLetter: "B", svgPath: RAW.codebuff },
  coderabbit:  { brandColor: "#ff5c35", fallbackLetter: "C", svgPath: RAW.coderabbit },
  codex:       { brandColor: "#49a3b0", fallbackLetter: "◆", svgPath: RAW.codex },
  copilot:     { brandColor: "#a855f7", fallbackLetter: "⬡", svgPath: RAW.copilot },
  cursor:      { brandColor: "#f54e00", fallbackLetter: "▸", svgPath: RAW.cursor },
  deepgram:    { brandColor: "#13ef93", fallbackLetter: "D", svgPath: RAW.deepgram },
  deepinfra:   { brandColor: "#2a3275", fallbackLetter: "D", svgPath: RAW.deepinfra },
  devpass:     { brandColor: "#2563eb", fallbackLetter: "D", svgPath: RAW.devpass },
  fireworks:   { brandColor: "#f25b1c", fallbackLetter: "F", svgPath: RAW.fireworks },
  aiand:       { brandColor: "#e25c2b", fallbackLetter: "&", svgPath: RAW.aiand },
  clinepass:   { brandColor: "#5487c8", fallbackLetter: "C", svgPath: RAW.clinepass },
  longcat:     { brandColor: "#29e154", fallbackLetter: "L", svgPath: RAW.longcat },
  neuralwatt:  { brandColor: "#d55934", fallbackLetter: "N", svgPath: RAW.neuralwatt },
  zoommate:    { brandColor: "#0B5CFF", fallbackLetter: "Z", svgPath: RAW.zoommate },
  zenmux:      { brandColor: "#6c5ce7", fallbackLetter: "Z", svgPath: RAW.zenmux },
  deepseek:    { brandColor: "#4d6bfe", fallbackLetter: "D", svgPath: RAW.deepseek },
  elevenlabs:  { brandColor: "#111827", fallbackLetter: "E", svgPath: RAW.elevenlabs },
  factory:     { brandColor: "#ff6b35", fallbackLetter: "◎", svgPath: RAW.factory },
  gemini:      { brandColor: "#ab87ea", fallbackLetter: "✦", svgPath: RAW.gemini },
  grok:        { brandColor: "#111827", fallbackLetter: "G", svgPath: RAW.grok },
  groq:        { brandColor: "#f55036", fallbackLetter: "G", svgPath: RAW.groq },
  bifrost:     { brandColor: "#33c09e", fallbackLetter: "B" },
  aixy:        { brandColor: "#123650", fallbackLetter: "A", svgPath: RAW.aixy },
  gitkraken:   { brandColor: "#179287", fallbackLetter: "G" },
  huggingface: { brandColor: "#ffd21e", fallbackLetter: "H", svgPath: RAW.huggingface },
  hyper:       { brandColor: "#ff60ff", fallbackLetter: "H" },
  helmcode:    { brandColor: "#4f46e5", fallbackLetter: "H" },
  v0:          { brandColor: "#111827", fallbackLetter: "V" },
  typesafe:    { brandColor: "#2563eb", fallbackLetter: "T" },
  jetbrains:   { brandColor: "#ff3399", fallbackLetter: "J", svgPath: RAW.jetbrains },
  kilo:        { brandColor: "#5d87ff", fallbackLetter: "K", svgPath: RAW.kilo },
  bedrock:     { brandColor: "#01a88d", fallbackLetter: "B", svgPath: RAW.bedrock },
  kimi:        { brandColor: "#fe603c", fallbackLetter: "☽", svgPath: RAW.kimi },
  kimik2:      { brandColor: "#4c00ff", fallbackLetter: "☽", svgPath: RAW.kimi },
  kiro:        { brandColor: "#9046ff", fallbackLetter: "K", svgPath: RAW.kiro },
  llmman:      { brandColor: "#6CC5B0", fallbackLetter: "L", svgPath: RAW.llmman },
  llmproxy:    { brandColor: "#4f46e5", fallbackLetter: "L", svgPath: RAW.llmproxy },
  minimax:     { brandColor: "#fe603c", fallbackLetter: "M", svgPath: RAW.minimax },
  mistral:     { brandColor: "#ff5229", fallbackLetter: "M", svgPath: RAW.mistral },
  muse:        { brandColor: "#0668e1", fallbackLetter: "M", svgPath: RAW.muse },
  ollama:      { brandColor: "#8b95b0", fallbackLetter: "○", svgPath: RAW.ollama },
  azureopenai: { brandColor: "#0078d4", fallbackLetter: "A" },
  t3chat:      { brandColor: "#8b5cf6", fallbackLetter: "T", svgPath: RAW.t3chat },
  opencode:    { brandColor: "#3b82f6", fallbackLetter: "○", svgPath: RAW.opencode },
  opencodego:  { brandColor: "#3b82f6", fallbackLetter: "○", svgPath: RAW.opencodego },
  openrouter:  { brandColor: "#6b7280", fallbackLetter: "R", svgPath: RAW.openrouter },
  perplexity:  { brandColor: "#1fb8cd", fallbackLetter: "P", svgPath: RAW.perplexity },
  vertexai:    { brandColor: "#4285f4", fallbackLetter: "△", svgPath: RAW.vertexai },
  warp:        { brandColor: "#6366f1", fallbackLetter: "W", svgPath: RAW.warp },
  windsurf:    { brandColor: "#22c55e", fallbackLetter: "W", svgPath: RAW.windsurf },
  wayfinder:   { brandColor: "#14b8a6", fallbackLetter: "W" },
  zai:         { brandColor: "#e85a6a", fallbackLetter: "Z", svgPath: RAW.zai },
  // Aliases / Rust-side normalizations without their own SVG.
  nanogpt:     { brandColor: "#687fa1", fallbackLetter: "N" },
  infini:      { brandColor: "#687fa1", fallbackLetter: "I" },
  abacus:      { brandColor: "#814ee8", fallbackLetter: "A", svgPath: RAW.abacus },
  atlascloud:  { brandColor: "#5975F5", fallbackLetter: "A", svgPath: RAW.atlascloud },
  manus:       { brandColor: "#34322d", fallbackLetter: "M", svgPath: RAW.manus },
  mimo:        { brandColor: "#ff6900", fallbackLetter: "M", svgPath: RAW.mimo },
  doubao:      { brandColor: "#2563eb", fallbackLetter: "D", svgPath: RAW.doubao },
  commandcode: { brandColor: "#8c4edd", fallbackLetter: "C", svgPath: RAW.commandcode },
  crossmodel:  { brandColor: "#c084fc", fallbackLetter: "X", svgPath: RAW.crossmodel },
  qoder:       { brandColor: "#2563eb", fallbackLetter: "Q", svgPath: RAW.qoder },
  raycast:     { brandColor: "#FF6363", fallbackLetter: "R", svgPath: RAW.raycast },
  replicate:   { brandColor: "#000000", fallbackLetter: "R", svgPath: RAW.replicate },
  codebuddy:   { brandColor: "#0052d9", fallbackLetter: "C" },
  sakana:      { brandColor: "#0ea5e9", fallbackLetter: "S", svgPath: RAW.sakana },
  stepfun:     { brandColor: "#999999", fallbackLetter: "S", svgPath: RAW.stepfun },
  sub2api:     { brandColor: "#14b8a6", fallbackLetter: "S", svgPath: RAW.sub2api },
  venice:      { brandColor: "#3c8fdd", fallbackLetter: "V", svgPath: RAW.venice },
  vercel:      { brandColor: "#737373", fallbackLetter: "V", svgPath: RAW.vercel },
  openaiapi:   { brandColor: "#10a37f", fallbackLetter: "O" },
  chutes:      { brandColor: "#ff5c35", fallbackLetter: "C" },
  litellm:     { brandColor: "#0ea5e9", fallbackLetter: "L" },
  poe:         { brandColor: "#5d5fef", fallbackLetter: "P" },
  devin:       { brandColor: "#317cff", fallbackLetter: "D" },
  zed:         { brandColor: "#084ccf", fallbackLetter: "Z" },
  qwencloud:   { brandColor: "#615CED", fallbackLetter: "Q" },
  notion:      { brandColor: "#337EA9", fallbackLetter: "N", svgPath: RAW.notion },
  nous:        { brandColor: "#D6A55C", fallbackLetter: "N", svgPath: RAW.nous },
  xai:         { brandColor: "#8e8e93", fallbackLetter: "X", svgPath: RAW.xai },
  xkiro:       { brandColor: "#52c99b", fallbackLetter: "X", svgPath: RAW.xkiro },
  meta:        { brandColor: "#0467DF", fallbackLetter: "M", svgPath: RAW.meta },
};

export const PROVIDER_ICON_REGISTRY: Record<string, ProviderIcon> = Object.fromEntries(
  Object.entries(ICON_ENTRIES).map(([id, entry]) => [id, { id, ...entry }]),
);

const ALIASES: Record<string, string> = {
  droid: "factory",
  "z.ai": "zai",
  "vertex ai": "vertexai",
  "jetbrains ai": "jetbrains",
  "kimi k2": "kimik2",
  tongyi: "alibaba",
  qwen: "qwencloud",
  "qwen cloud": "qwencloud",
  "qwen-cloud": "qwencloud",
  "notion ai": "notion",
  "notion-ai": "notion",
  notionai: "notion",
  qianwen: "alibaba",
  "alibaba token plan": "alibabatokenplan",
  "alibaba-token-plan": "alibabatokenplan",
  "alibaba-token": "alibabatokenplan",
  "bailian-token-plan": "alibabatokenplan",
  "open router": "openrouter",
  "aws bedrock": "bedrock",
  "aws-bedrock": "bedrock",
  "mistral ai": "mistral",
  "warp terminal": "warp",
  "warp ai": "warp",
  manicode: "codebuff",
  "deep seek": "deepseek",
  "deep-seek": "deepseek",
  "deep infra": "deepinfra",
    "deep-infra": "deepinfra",
    di: "deepinfra",
    "fireworks-ai": "fireworks",
    fw: "fireworks",
  "ai&": "aiand",
  "ai-and": "aiand",
  "ai and": "aiand",
  "zen-mux": "zenmux",
  "cline-pass": "clinepass",
  "long-cat": "longcat",
  lc: "longcat",
  "neural-watt": "neuralwatt",
  nw: "neuralwatt",
  "zoom-mate": "zoommate",
  "zoom mate": "zoommate",
  codeium: "windsurf",
  "xiaomi mimo": "mimo",
  xiaomimimo: "mimo",
  "command code": "commandcode",
  "command-code": "commandcode",
  "cross model": "crossmodel",
  "cross-model": "crossmodel",
  "sakana ai": "sakana",
  "sakana-ai": "sakana",
  "step fun": "stepfun",
  "step-fun": "stepfun",
  "sub-2-api": "sub2api",
  "sub 2 api": "sub2api",
  "openai api": "openaiapi",
  "openai-api": "openaiapi",
  "azure openai": "azureopenai",
  "azure-openai": "azureopenai",
  "t3 chat": "t3chat",
  "t3-chat": "t3chat",
  // xai is its own Management API provider (not an alias of consumer Grok).
  "x.ai": "xai",
  "x-ai": "xai",
  "x-kiro": "xkiro",
  supergrok: "grok",
  "super-grok": "grok",
  "eleven labs": "elevenlabs",
  "eleven-labs": "elevenlabs",
  "11labs": "elevenlabs",
  dg: "deepgram",
  groqcloud: "groq",
  "groq cloud": "groq",
  "groq-cloud": "groq",
  "llm proxy": "llmproxy",
  "llm-proxy": "llmproxy",
  "chutes ai": "chutes",
  "chutes-ai": "chutes",
  "lite llm": "litellm",
  "lite-llm": "litellm",
  "zed ai": "zed",
  "zed-ai": "zed",
  metaspark: "meta",
  "meta-spark": "meta",
  musespark: "meta",
  "muse-spark": "meta",
  "muse spark": "meta",
  "meta muse spark": "meta",
};

function normalize(id: string): string {
  const lower = id.toLowerCase();
  const aliased = ALIASES[lower];
  if (aliased) return aliased;
  return lower.replace(/[ \-]/g, "");
}

/** Return the registry entry for a provider id, falling back to a generic one. */
export function getProviderIcon(id: string): ProviderIcon {
  const key = normalize(id);
  return (
    PROVIDER_ICON_REGISTRY[key] ?? {
      id: key,
      brandColor: "#5d87ff",
      fallbackLetter: id.charAt(0).toUpperCase() || "●",
    }
  );
}
