// Ported from rust/src/native_ui/provider_icons.rs and
// rust/src/native_ui/theme.rs::{provider_color, provider_icon}.
// Keep in sync with the Rust registries when new providers are added.

import alibaba from "./icons/ProviderIcon-alibaba.svg?raw";
import amp from "./icons/ProviderIcon-amp.svg?raw";
import antigravity from "./icons/ProviderIcon-antigravity.svg?raw";
import augment from "./icons/ProviderIcon-augment.svg?raw";
import claude from "./icons/ProviderIcon-claude.svg?raw";
import codebuff from "./icons/ProviderIcon-codebuff.svg?raw";
import codex from "./icons/ProviderIcon-codex.svg?raw";
import copilot from "./icons/ProviderIcon-copilot.svg?raw";
import cursor from "./icons/ProviderIcon-cursor.svg?raw";
import deepseek from "./icons/ProviderIcon-deepseek.svg?raw";
import factory from "./icons/ProviderIcon-factory.svg?raw";
import gemini from "./icons/ProviderIcon-gemini.svg?raw";
import jetbrains from "./icons/ProviderIcon-jetbrains.svg?raw";
import kilo from "./icons/ProviderIcon-kilo.svg?raw";
import kimi from "./icons/ProviderIcon-kimi.svg?raw";
import kiro from "./icons/ProviderIcon-kiro.svg?raw";
import minimax from "./icons/ProviderIcon-minimax.svg?raw";
import mistral from "./icons/ProviderIcon-mistral.svg?raw";
import ollama from "./icons/ProviderIcon-ollama.svg?raw";
import opencode from "./icons/ProviderIcon-opencode.svg?raw";
import opencodego from "./icons/ProviderIcon-opencodego.svg?raw";
import openrouter from "./icons/ProviderIcon-openrouter.svg?raw";
import perplexity from "./icons/ProviderIcon-perplexity.svg?raw";
import synthetic from "./icons/ProviderIcon-synthetic.svg?raw";
import vertexai from "./icons/ProviderIcon-vertexai.svg?raw";
import warp from "./icons/ProviderIcon-warp.svg?raw";
import windsurf from "./icons/ProviderIcon-windsurf.svg?raw";
import zai from "./icons/ProviderIcon-zai.svg?raw";

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

const RAW: Record<string, string> = {
  alibaba: tint(alibaba),
  amp: tint(amp),
  antigravity: tint(antigravity),
  augment: tint(augment),
  claude: tint(claude),
  codebuff: tint(codebuff),
  codex: tint(codex),
  copilot: tint(copilot),
  cursor: tint(cursor),
  deepseek: tint(deepseek),
  factory: tint(factory),
  gemini: tint(gemini),
  jetbrains: tint(jetbrains),
  kilo: tint(kilo),
  kimi: tint(kimi),
  kiro: tint(kiro),
  minimax: tint(minimax),
  mistral: tint(mistral),
  ollama: tint(ollama),
  opencode: tint(opencode),
  opencodego: tint(opencodego),
  openrouter: tint(openrouter),
  perplexity: tint(perplexity),
  synthetic: tint(synthetic),
  vertexai: tint(vertexai),
  warp: tint(warp),
  windsurf: tint(windsurf),
  zai: tint(zai),
};

/**
 * Registry of provider icons. Matches the entries in
 * `rust/src/native_ui/provider_icons.rs` and pulls brand colors / fallback
 * letters from `rust/src/native_ui/theme.rs::{provider_color, provider_icon}`.
 */
export const PROVIDER_ICON_REGISTRY: Record<string, ProviderIcon> = {
  alibaba:     { id: "alibaba",     brandColor: "#ff6a00", fallbackLetter: "阿", svgPath: RAW.alibaba },
  amp:         { id: "amp",         brandColor: "#dc2626", fallbackLetter: "⚡", svgPath: RAW.amp },
  antigravity: { id: "antigravity", brandColor: "#60ba7e", fallbackLetter: "◉", svgPath: RAW.antigravity },
  augment:     { id: "augment",     brandColor: "#6366f1", fallbackLetter: "A", svgPath: RAW.augment },
  claude:      { id: "claude",      brandColor: "#cc7c5e", fallbackLetter: "◈", svgPath: RAW.claude },
  codebuff:    { id: "codebuff",    brandColor: "#44ff00", fallbackLetter: "B", svgPath: RAW.codebuff },
  codex:       { id: "codex",       brandColor: "#49a3b0", fallbackLetter: "◆", svgPath: RAW.codex },
  copilot:     { id: "copilot",     brandColor: "#a855f7", fallbackLetter: "⬡", svgPath: RAW.copilot },
  cursor:      { id: "cursor",      brandColor: "#00bfa5", fallbackLetter: "▸", svgPath: RAW.cursor },
  deepseek:    { id: "deepseek",    brandColor: "#527df0", fallbackLetter: "D", svgPath: RAW.deepseek },
  factory:     { id: "factory",     brandColor: "#ff6b35", fallbackLetter: "◎", svgPath: RAW.factory },
  gemini:      { id: "gemini",      brandColor: "#ab87ea", fallbackLetter: "✦", svgPath: RAW.gemini },
  jetbrains:   { id: "jetbrains",   brandColor: "#ff3399", fallbackLetter: "J", svgPath: RAW.jetbrains },
  kilo:        { id: "kilo",        brandColor: "#5d87ff", fallbackLetter: "K", svgPath: RAW.kilo },
  bedrock:     { id: "bedrock",     brandColor: "#ff9900", fallbackLetter: "B" },
  kimi:        { id: "kimi",        brandColor: "#fe603c", fallbackLetter: "☽", svgPath: RAW.kimi },
  kimik2:      { id: "kimik2",      brandColor: "#4c00ff", fallbackLetter: "☽", svgPath: RAW.kimi },
  kiro:        { id: "kiro",        brandColor: "#ff9900", fallbackLetter: "K", svgPath: RAW.kiro },
  minimax:     { id: "minimax",     brandColor: "#fe603c", fallbackLetter: "M", svgPath: RAW.minimax },
  mistral:     { id: "mistral",     brandColor: "#ff500f", fallbackLetter: "M", svgPath: RAW.mistral },
  ollama:      { id: "ollama",      brandColor: "#8b95b0", fallbackLetter: "○", svgPath: RAW.ollama },
  opencode:    { id: "opencode",    brandColor: "#3b82f6", fallbackLetter: "○", svgPath: RAW.opencode },
  opencodego:  { id: "opencodego",  brandColor: "#3b82f6", fallbackLetter: "○", svgPath: RAW.opencodego },
  openrouter:  { id: "openrouter",  brandColor: "#6b7280", fallbackLetter: "R", svgPath: RAW.openrouter },
  perplexity:  { id: "perplexity",  brandColor: "#1fb8cd", fallbackLetter: "P", svgPath: RAW.perplexity },
  synthetic:   { id: "synthetic",   brandColor: "#141414", fallbackLetter: "◇", svgPath: RAW.synthetic },
  vertexai:    { id: "vertexai",    brandColor: "#4285f4", fallbackLetter: "△", svgPath: RAW.vertexai },
  warp:        { id: "warp",        brandColor: "#6366f1", fallbackLetter: "W", svgPath: RAW.warp },
  windsurf:    { id: "windsurf",    brandColor: "#22c55e", fallbackLetter: "W", svgPath: RAW.windsurf },
  zai:         { id: "zai",         brandColor: "#e85a6a", fallbackLetter: "Z", svgPath: RAW.zai },
  // Aliases / Rust-side normalizations without their own SVG.
  nanogpt:     { id: "nanogpt",     brandColor: "#687fa1", fallbackLetter: "N" },
  infini:      { id: "infini",      brandColor: "#687fa1", fallbackLetter: "I" },
  abacus:      { id: "abacus",      brandColor: "#7c3aed", fallbackLetter: "A" },
  manus:       { id: "manus",       brandColor: "#34322d", fallbackLetter: "M" },
  mimo:        { id: "mimo",        brandColor: "#ff6900", fallbackLetter: "M" },
  doubao:      { id: "doubao",      brandColor: "#2563eb", fallbackLetter: "D" },
  commandcode: { id: "commandcode", brandColor: "#44ff00", fallbackLetter: "C" },
  crof:        { id: "crof",        brandColor: "#7c3aed", fallbackLetter: "C" },
  stepfun:     { id: "stepfun",     brandColor: "#999999", fallbackLetter: "S" },
  venice:      { id: "venice",      brandColor: "#111827", fallbackLetter: "V" },
  openaiapi:   { id: "openaiapi",   brandColor: "#10a37f", fallbackLetter: "O" },
};

const ALIASES: Record<string, string> = {
  droid: "factory",
  "z.ai": "zai",
  "vertex ai": "vertexai",
  "jetbrains ai": "jetbrains",
  "kimi k2": "kimik2",
  tongyi: "alibaba",
  qwen: "alibaba",
  qianwen: "alibaba",
  "open router": "openrouter",
  "aws bedrock": "bedrock",
  "aws-bedrock": "bedrock",
  "mistral ai": "mistral",
  "warp terminal": "warp",
  "warp ai": "warp",
  manicode: "codebuff",
  "deep seek": "deepseek",
  "deep-seek": "deepseek",
  codeium: "windsurf",
  "xiaomi mimo": "mimo",
  xiaomimimo: "mimo",
  "command code": "commandcode",
  "command-code": "commandcode",
  "step fun": "stepfun",
  "step-fun": "stepfun",
  "openai api": "openaiapi",
  "openai-api": "openaiapi",
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
