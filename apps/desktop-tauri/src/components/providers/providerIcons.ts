// Ported from rust/src/native_ui/provider_icons.rs and
// rust/src/native_ui/theme.rs::{provider_color, provider_icon}.
// Keep in sync with the Rust registries when new providers are added.

import { contrastRatio, mixWithWhite } from "../../lib/color";
import abacus from "./icons/ProviderIcon-abacus.svg?raw";
import abliteration from "./icons/ProviderIcon-abliteration.svg?raw";
import aerostack from "./icons/ProviderIcon-aerostack.svg?raw";
import alibaba from "./icons/ProviderIcon-alibaba.svg?raw";
import amp from "./icons/ProviderIcon-amp.svg?raw";
import antigravity from "./icons/ProviderIcon-antigravity.svg?raw";
import atlascloud from "./icons/ProviderIcon-atlascloud.svg?raw";
import augment from "./icons/ProviderIcon-augment.svg?raw";
import bedrock from "./icons/ProviderIcon-bedrock.svg?raw";
import chatbox from "./icons/ProviderIcon-chatbox.svg?raw";
import claude from "./icons/ProviderIcon-claude.svg?raw";
import claudeplatform from "./icons/ProviderIcon-claudeplatform.svg?raw";
import traecn from "./icons/ProviderIcon-traecn.svg?raw";
import codebuff from "./icons/ProviderIcon-codebuff.svg?raw";
import coderabbit from "./icons/ProviderIcon-coderabbit.svg?raw";
import codex from "./icons/ProviderIcon-codex.svg?raw";
import commandcode from "./icons/ProviderIcon-commandcode.svg?raw";
import copilot from "./icons/ProviderIcon-copilot.svg?raw";
import cosmic from "./icons/ProviderIcon-cosmic.svg?raw";
import crossmodel from "./icons/ProviderIcon-crossmodel.svg?raw";
import cursor from "./icons/ProviderIcon-cursor.svg?raw";
import deepgram from "./icons/ProviderIcon-deepgram.svg?raw";
import deepinfra from "./icons/ProviderIcon-deepinfra.svg?raw";
import devin from "./icons/ProviderIcon-devin.svg?raw";
import devpass from "./icons/ProviderIcon-devpass.svg?raw";
import fireworks from "./icons/ProviderIcon-fireworks.svg?raw";
import aiand from "./icons/ProviderIcon-aiand.svg?raw";
import aixy from "./icons/ProviderIcon-aixy.svg?raw";
import clinepass from "./icons/ProviderIcon-clinepass.svg?raw";
import longcat from "./icons/ProviderIcon-longcat.svg?raw";
import neuralwatt from "./icons/ProviderIcon-neuralwatt.svg?raw";
import zoommate from "./icons/ProviderIcon-zoommate.svg?raw";
import zenmux from "./icons/ProviderIcon-zenmux.svg?raw";
import deepseek from "./icons/ProviderIcon-deepseek.svg?raw";
import doubao from "./icons/ProviderIcon-doubao.svg?raw";
import elevenlabs from "./icons/ProviderIcon-elevenlabs.svg?raw";
import exa from "./icons/ProviderIcon-exa.svg?raw";
import factory from "./icons/ProviderIcon-factory.svg?raw";
import fal from "./icons/ProviderIcon-fal.svg?raw";
import flora from "./icons/ProviderIcon-flora.svg?raw";
import futureagi from "./icons/ProviderIcon-futureagi.svg?raw";
import gemini from "./icons/ProviderIcon-gemini.svg?raw";
import grok from "./icons/ProviderIcon-grok.svg?raw";
import groq from "./icons/ProviderIcon-groq.svg?raw";
import huggingface from "./icons/ProviderIcon-huggingface.svg?raw";
import ibmbob from "./icons/ProviderIcon-ibmbob.svg?raw";
import jetbrains from "./icons/ProviderIcon-jetbrains.svg?raw";
import kilo from "./icons/ProviderIcon-kilo.svg?raw";
import kimi from "./icons/ProviderIcon-kimi.svg?raw";
import langdock from "./icons/ProviderIcon-langdock.svg?raw";
import linkup from "./icons/ProviderIcon-linkup.svg?raw";
import kiro from "./icons/ProviderIcon-kiro.svg?raw";
import lithosai from "./icons/ProviderIcon-lithosai.svg?raw";
import llmman from "./icons/ProviderIcon-llmman.svg?raw";
import llmproxy from "./icons/ProviderIcon-llmproxy.svg?raw";
import manus from "./icons/ProviderIcon-manus.svg?raw";
import meta from "./icons/ProviderIcon-meta.svg?raw";
import mimo from "./icons/ProviderIcon-mimo.svg?raw";
import minimax from "./icons/ProviderIcon-minimax.svg?raw";
import mistral from "./icons/ProviderIcon-mistral.svg?raw";
import munito from "./icons/ProviderIcon-munito.svg?raw";
import muse from "./icons/ProviderIcon-muse.svg?raw";
import museai from "./icons/ProviderIcon-museai.svg?raw";
import notion from "./icons/ProviderIcon-notion.svg?raw";
import nous from "./icons/ProviderIcon-nous.svg?raw";
import workbuddy from "./icons/ProviderIcon-workbuddy.svg?raw";
import xai from "./icons/ProviderIcon-xai.svg?raw";
import xapi from "./icons/ProviderIcon-xapi.svg?raw";
import xkiro from "./icons/ProviderIcon-xkiro.svg?raw";
import ollama from "./icons/ProviderIcon-ollama.svg?raw";
import opencode from "./icons/ProviderIcon-opencode.svg?raw";
import opencodego from "./icons/ProviderIcon-opencodego.svg?raw";
import openrouter from "./icons/ProviderIcon-openrouter.svg?raw";
import parallel from "./icons/ProviderIcon-parallel.svg?raw";
import perplexity from "./icons/ProviderIcon-perplexity.svg?raw";
import qoder from "./icons/ProviderIcon-qoder.svg?raw";
import raycast from "./icons/ProviderIcon-raycast.svg?raw";
import replicate from "./icons/ProviderIcon-replicate.svg?raw";
import sailresearch from "./icons/ProviderIcon-sailresearch.svg?raw";
import sakana from "./icons/ProviderIcon-sakana.svg?raw";
import sofya from "./icons/ProviderIcon-sofya.svg?raw";
import soniox from "./icons/ProviderIcon-soniox.svg?raw";
import stepfun from "./icons/ProviderIcon-stepfun.svg?raw";
import sub2api from "./icons/ProviderIcon-sub2api.svg?raw";
import synthetic from "./icons/ProviderIcon-synthetic.svg?raw";
import tavily from "./icons/ProviderIcon-tavily.svg?raw";
import clawrouter from "./icons/ProviderIcon-clawrouter.svg?raw";
import t3chat from "./icons/ProviderIcon-t3chat.svg?raw";
import tinyapi from "./icons/ProviderIcon-tinyapi.svg?raw";
import venice from "./icons/ProviderIcon-venice.svg?raw";
import vercel from "./icons/ProviderIcon-vercel.svg?raw";
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
  abacus: tint(abacus),
  abliteration: tint(abliteration),
  aerostack: tint(aerostack),
  alibaba: tint(alibaba),
  amp: tint(amp),
  antigravity: tint(antigravity),
  atlascloud: tint(atlascloud),
  augment: tint(augment),
  bedrock: tint(bedrock),
  claude: tint(claude),
  claudeplatform: tint(claudeplatform),
  traecn: tint(traecn),
  chatbox: tint(chatbox),
  codebuff: tint(codebuff),
  coderabbit: tint(coderabbit),
  codex: tint(codex),
  commandcode: tint(commandcode),
  copilot: tint(copilot),
  cosmic: tint(cosmic),
  crossmodel: tint(crossmodel),
  cursor: tint(cursor),
  deepgram: tint(deepgram),
  deepinfra: tint(deepinfra),
  devin: tint(devin),
  devpass: tint(devpass),
  fireworks: tint(fireworks),
  aiand: tint(aiand),
  aixy: tint(aixy),
  clinepass: tint(clinepass),
  longcat: tint(longcat),
  neuralwatt: tint(neuralwatt),
  zoommate: tint(zoommate),
  zenmux: tint(zenmux),
  deepseek: tint(deepseek),
  doubao: tint(doubao),
  elevenlabs: tint(elevenlabs),
  exa: tint(exa),
  factory: tint(factory),
  fal: tint(fal),
  flora: tint(flora),
  futureagi: tint(futureagi),
  gemini: tint(gemini),
  grok: tint(grok),
  groq: tint(groq),
  huggingface: tint(huggingface),
  ibmbob: tint(ibmbob),
  jetbrains: tint(jetbrains),
  kilo: tint(kilo),
  kimi: tint(kimi),
  langdock: tint(langdock),
  linkup: tint(linkup),
  kiro: tint(kiro),
  lithosai: tint(lithosai),
  llmman: tint(llmman),
  llmproxy: tint(llmproxy),
  manus: tint(manus),
  meta: tint(meta),
  mimo: tint(mimo),
  minimax: tint(minimax),
  notion: tint(notion),
  nous: tint(nous),
  xai: tint(xai),
  xapi: tint(xapi),
  xkiro: tint(xkiro),
  mistral: tint(mistral),
  munito: tint(munito),
  muse: tint(muse),
  museai: tint(museai),
  workbuddy: tint(workbuddy),
  ollama: tint(ollama),
  opencode: tint(opencode),
  opencodego: tint(opencodego),
  openrouter: tint(openrouter),
  parallel: tint(parallel),
  perplexity: tint(perplexity),
  qoder: tint(qoder),
  raycast: tint(raycast),
  replicate: tint(replicate),
  sailresearch: tint(sailresearch),
  sakana: tint(sakana),
  sofya: tint(sofya),
  soniox: tint(soniox),
  stepfun: tint(stepfun),
  sub2api: tint(sub2api),
  synthetic: tint(synthetic),
  tavily: tint(tavily),
  clawrouter: tint(clawrouter),
  t3chat: tint(t3chat),
  tinyapi: tint(tinyapi),
  venice: tint(venice),
  vercel: tint(vercel),
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
  alibabatokenplan: { id: "alibabatokenplan", brandColor: "#ff6a00", fallbackLetter: "阿", svgPath: RAW.alibaba },
  amp:         { id: "amp",         brandColor: "#f34e3f", fallbackLetter: "⚡", svgPath: RAW.amp },
  antigravity: { id: "antigravity", brandColor: "#60ba7e", fallbackLetter: "◉", svgPath: RAW.antigravity },
  aerostack:   { id: "aerostack",   brandColor: "#6366F1", fallbackLetter: "◆", svgPath: RAW.aerostack },
  augment:     { id: "augment",     brandColor: "#1aa049", fallbackLetter: "A", svgPath: RAW.augment },
  claude:      { id: "claude",      brandColor: "#cc7c5e", fallbackLetter: "◈", svgPath: RAW.claude },
  claudeplatform: { id: "claudeplatform", brandColor: "#cc7c5e", fallbackLetter: "◈", svgPath: RAW.claudeplatform },
  traecn:       { id: "traecn",       brandColor: "#4D7CFE", fallbackLetter: "T", svgPath: RAW.traecn },
  chatbox:     { id: "chatbox",     brandColor: "#228BE6", fallbackLetter: "C", svgPath: RAW.chatbox },
  abliteration: { id: "abliteration", brandColor: "#1B1B1B", fallbackLetter: "A", svgPath: RAW.abliteration },
  pi:          { id: "pi",          brandColor: "#4d9abf", fallbackLetter: "P" },
  codebuff:    { id: "codebuff",    brandColor: "#00ff95", fallbackLetter: "B", svgPath: RAW.codebuff },
  coderabbit:  { id: "coderabbit",  brandColor: "#ff5c35", fallbackLetter: "C", svgPath: RAW.coderabbit },
  codex:       { id: "codex",       brandColor: "#49a3b0", fallbackLetter: "◆", svgPath: RAW.codex },
  copilot:     { id: "copilot",     brandColor: "#a855f7", fallbackLetter: "⬡", svgPath: RAW.copilot },
  cosmic:      { id: "cosmic",      brandColor: "#6366F1", fallbackLetter: "◆", svgPath: RAW.cosmic },
  cursor:      { id: "cursor",      brandColor: "#f54e00", fallbackLetter: "▸", svgPath: RAW.cursor },
  deepgram:    { id: "deepgram",    brandColor: "#6467f2", fallbackLetter: "D", svgPath: RAW.deepgram },
  deepinfra:   { id: "deepinfra",   brandColor: "#2a3275", fallbackLetter: "D", svgPath: RAW.deepinfra },
  devpass:     { id: "devpass",     brandColor: "#2563eb", fallbackLetter: "D", svgPath: RAW.devpass },
  fireworks:   { id: "fireworks",   brandColor: "#f25b1c", fallbackLetter: "F", svgPath: RAW.fireworks },
  aiand:       { id: "aiand",       brandColor: "#e25c2b", fallbackLetter: "&", svgPath: RAW.aiand },
  clinepass:   { id: "clinepass",   brandColor: "#5487c8", fallbackLetter: "C", svgPath: RAW.clinepass },
  longcat:     { id: "longcat",     brandColor: "#29e154", fallbackLetter: "L", svgPath: RAW.longcat },
  neuralwatt:  { id: "neuralwatt",  brandColor: "#d55934", fallbackLetter: "N", svgPath: RAW.neuralwatt },
  zoommate:    { id: "zoommate",    brandColor: "#0B5CFF", fallbackLetter: "Z", svgPath: RAW.zoommate },
  zenmux:      { id: "zenmux",      brandColor: "#6c5ce7", fallbackLetter: "Z", svgPath: RAW.zenmux },
  deepseek:    { id: "deepseek",    brandColor: "#4d6bfe", fallbackLetter: "D", svgPath: RAW.deepseek },
  elevenlabs:  { id: "elevenlabs",  brandColor: "#111827", fallbackLetter: "E", svgPath: RAW.elevenlabs },
  factory:     { id: "factory",     brandColor: "#ff6b35", fallbackLetter: "◎", svgPath: RAW.factory },
  fal:         { id: "fal",         brandColor: "#7C3AED", fallbackLetter: "F", svgPath: RAW.fal },
  flora:       { id: "flora",       brandColor: "#111111", fallbackLetter: "F", svgPath: RAW.flora },
  futureagi:   { id: "futureagi",   brandColor: "#7857FC", fallbackLetter: "F", svgPath: RAW.futureagi },
  gemini:      { id: "gemini",      brandColor: "#ab87ea", fallbackLetter: "✦", svgPath: RAW.gemini },
  grok:        { id: "grok",        brandColor: "#10a37f", fallbackLetter: "G", svgPath: RAW.grok },
  groq:        { id: "groq",        brandColor: "#f56844", fallbackLetter: "G", svgPath: RAW.groq },
  bifrost:     { id: "bifrost",     brandColor: "#33c09e", fallbackLetter: "B" },
  aixy:        { id: "aixy",        brandColor: "#123650", fallbackLetter: "A", svgPath: RAW.aixy },
  gitkraken:   { id: "gitkraken",   brandColor: "#179287", fallbackLetter: "G" },
  huggingface: { id: "huggingface", brandColor: "#ffd21e", fallbackLetter: "H", svgPath: RAW.huggingface },
  hyper:       { id: "hyper",       brandColor: "#ff60ff", fallbackLetter: "H" },
  helmcode:    { id: "helmcode",    brandColor: "#4934e1", fallbackLetter: "H" },
  v0:          { id: "v0",          brandColor: "#111111", fallbackLetter: "V" },
  typesafe:    { id: "typesafe",    brandColor: "#111111", fallbackLetter: "T" },
  jetbrains:   { id: "jetbrains",   brandColor: "#ff3399", fallbackLetter: "J", svgPath: RAW.jetbrains },
  kilo:        { id: "kilo",        brandColor: "#f27027", fallbackLetter: "K", svgPath: RAW.kilo },
  bedrock:     { id: "bedrock",     brandColor: "#01a88d", fallbackLetter: "B", svgPath: RAW.bedrock },
  kimi:        { id: "kimi",        brandColor: "#fe603c", fallbackLetter: "☽", svgPath: RAW.kimi },
  kimik2:      { id: "kimik2",      brandColor: "#4c00ff", fallbackLetter: "☽", svgPath: RAW.kimi },
  kiro:        { id: "kiro",        brandColor: "#9046ff", fallbackLetter: "K", svgPath: RAW.kiro },
  llmman:      { id: "llmman",      brandColor: "#6CC5B0", fallbackLetter: "L", svgPath: RAW.llmman },
  llmproxy:    { id: "llmproxy",    brandColor: "#24b47e", fallbackLetter: "L", svgPath: RAW.llmproxy },
  minimax:     { id: "minimax",     brandColor: "#fe603c", fallbackLetter: "M", svgPath: RAW.minimax },
  mistral:     { id: "mistral",     brandColor: "#ff5229", fallbackLetter: "M", svgPath: RAW.mistral },
  munito:      { id: "munito",      brandColor: "#702CF6", fallbackLetter: "M", svgPath: RAW.munito },
  muse:        { id: "muse",        brandColor: "#0668e1", fallbackLetter: "M", svgPath: RAW.muse },
  ollama:      { id: "ollama",      brandColor: "#888888", fallbackLetter: "○", svgPath: RAW.ollama },
  azureopenai: { id: "azureopenai", brandColor: "#0078d4", fallbackLetter: "A" },
  t3chat:      { id: "t3chat",      brandColor: "#f56647", fallbackLetter: "T", svgPath: RAW.t3chat },
  tinyapi:     { id: "tinyapi",     brandColor: "#f97316", fallbackLetter: "T", svgPath: RAW.tinyapi },
  opencode:    { id: "opencode",    brandColor: "#3b82f6", fallbackLetter: "○", svgPath: RAW.opencode },
  opencodego:  { id: "opencodego",  brandColor: "#3b82f6", fallbackLetter: "○", svgPath: RAW.opencodego },
  openrouter:  { id: "openrouter",  brandColor: "#6467f2", fallbackLetter: "R", svgPath: RAW.openrouter },
  parallel:    { id: "parallel",    brandColor: "#1D1C1A", fallbackLetter: "P", svgPath: RAW.parallel },
  perplexity:  { id: "perplexity",  brandColor: "#20b2aa", fallbackLetter: "P", svgPath: RAW.perplexity },
  vertexai:    { id: "vertexai",    brandColor: "#4285f4", fallbackLetter: "△", svgPath: RAW.vertexai },
  warp:        { id: "warp",        brandColor: "#938bb4", fallbackLetter: "W", svgPath: RAW.warp },
  windsurf:    { id: "windsurf",    brandColor: "#34e8bb", fallbackLetter: "W", svgPath: RAW.windsurf },
  wayfinder:   { id: "wayfinder",   brandColor: "#10a37f", fallbackLetter: "W" },
  zai:         { id: "zai",         brandColor: "#e85a6a", fallbackLetter: "Z", svgPath: RAW.zai },
  // Aliases / Rust-side normalizations without their own SVG.
  nanogpt:     { id: "nanogpt",     brandColor: "#687fa1", fallbackLetter: "N" },
  infini:      { id: "infini",      brandColor: "#687fa1", fallbackLetter: "I" },
  abacus:      { id: "abacus",      brandColor: "#814ee8", fallbackLetter: "A", svgPath: RAW.abacus },
  atlascloud:  { id: "atlascloud",  brandColor: "#5975F5", fallbackLetter: "A", svgPath: RAW.atlascloud },
  manus:       { id: "manus",       brandColor: "#34322d", fallbackLetter: "M", svgPath: RAW.manus },
  mimo:        { id: "mimo",        brandColor: "#ff6900", fallbackLetter: "M", svgPath: RAW.mimo },
  doubao:      { id: "doubao",      brandColor: "#3370ff", fallbackLetter: "D", svgPath: RAW.doubao },
  exa:         { id: "exa",         brandColor: "#0143D9", fallbackLetter: "E", svgPath: RAW.exa },
  commandcode: { id: "commandcode", brandColor: "#8c4edd", fallbackLetter: "C", svgPath: RAW.commandcode },
  crossmodel:  { id: "crossmodel",  brandColor: "#c084fc", fallbackLetter: "X", svgPath: RAW.crossmodel },
  qoder:       { id: "qoder",       brandColor: "#10b981", fallbackLetter: "Q", svgPath: RAW.qoder },
  raycast:     { id: "raycast",     brandColor: "#FF6363", fallbackLetter: "R", svgPath: RAW.raycast },
  replicate:   { id: "replicate",   brandColor: "#000000", fallbackLetter: "R", svgPath: RAW.replicate },
  sailresearch: { id: "sailresearch", brandColor: "#2C4681", fallbackLetter: "S", svgPath: RAW.sailresearch },
  codebuddy:   { id: "codebuddy",   brandColor: "#0052d9", fallbackLetter: "C" },
  sakana:      { id: "sakana",      brandColor: "#2975db", fallbackLetter: "S", svgPath: RAW.sakana },
  sofya:       { id: "sofya",       brandColor: "#b0b820", fallbackLetter: "S", svgPath: RAW.sofya },
  soniox:      { id: "soniox",      brandColor: "#253772", fallbackLetter: "S", svgPath: RAW.soniox },
  stepfun:     { id: "stepfun",     brandColor: "#2196f2", fallbackLetter: "S", svgPath: RAW.stepfun },
  sub2api:     { id: "sub2api",     brandColor: "#14b8a6", fallbackLetter: "S", svgPath: RAW.sub2api },
  venice:      { id: "venice",      brandColor: "#3c8fdd", fallbackLetter: "V", svgPath: RAW.venice },
  vercel:      { id: "vercel",      brandColor: "#737373", fallbackLetter: "V", svgPath: RAW.vercel },
  synthetic:   { id: "synthetic",   brandColor: "#141414", fallbackLetter: "S", svgPath: RAW.synthetic },
  tavily:      { id: "tavily",      brandColor: "#0F766E", fallbackLetter: "T", svgPath: RAW.tavily },
  clawrouter:  { id: "clawrouter",  brandColor: "#596EF6", fallbackLetter: "C", svgPath: RAW.clawrouter },
  ibmbob:      { id: "ibmbob",      brandColor: "#0E61FA", fallbackLetter: "B", svgPath: RAW.ibmbob },
  langdock:    { id: "langdock",    brandColor: "#5A4AE7", fallbackLetter: "L", svgPath: RAW.langdock },
  linkup:      { id: "linkup",      brandColor: "#202020", fallbackLetter: "L", svgPath: RAW.linkup },
  lithosai:    { id: "lithosai",    brandColor: "#6B7280", fallbackLetter: "L", svgPath: RAW.lithosai },
  museai:      { id: "museai",      brandColor: "#0668E1", fallbackLetter: "M", svgPath: RAW.museai },
  workbuddy:   { id: "workbuddy",   brandColor: "#0DC8A6", fallbackLetter: "W", svgPath: RAW.workbuddy },
  openaiapi:   { id: "openaiapi",   brandColor: "#0f826e", fallbackLetter: "O" },
  chutes:      { id: "chutes",      brandColor: "#3184ff", fallbackLetter: "C" },
  litellm:     { id: "litellm",     brandColor: "#4c89f0", fallbackLetter: "L" },
  poe:         { id: "poe",         brandColor: "#5d5cde", fallbackLetter: "P" },
  devin:       { id: "devin",       brandColor: "#317cff", fallbackLetter: "D", svgPath: RAW.devin },
  zed:         { id: "zed",         brandColor: "#084eff", fallbackLetter: "Z" },
  qwencloud:   { id: "qwencloud",   brandColor: "#615CED", fallbackLetter: "Q" },
  notion:      { id: "notion",      brandColor: "#337EA9", fallbackLetter: "N", svgPath: RAW.notion },
  nous:        { id: "nous",        brandColor: "#D6A55C", fallbackLetter: "N", svgPath: RAW.nous },
  xai:         { id: "xai",         brandColor: "#8e8e93", fallbackLetter: "X", svgPath: RAW.xai },
  xapi:        { id: "xapi",        brandColor: "#71767B", fallbackLetter: "X", svgPath: RAW.xapi },
  xkiro:       { id: "xkiro",       brandColor: "#52c99b", fallbackLetter: "X", svgPath: RAW.xkiro },
  meta:        { id: "meta",        brandColor: "#0467DF", fallbackLetter: "M", svgPath: RAW.meta },
};

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
  "chatbox ai": "chatbox",
  "chatbox-ai": "chatbox",
  "abliteration.ai": "abliteration",
  "abliteration ai": "abliteration",
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

/**
 * Providers whose card bars use the label color instead of the brand color,
 * as Mac `progressColorStyle: .label` (ElevenLabs and abliteration.ai, whose
 * brand colors are near-black). `--text-primary` is themed on every surface
 * (Settings and the menu); `--mac-text` exists only inside the menu popover.
 */
const LABEL_COLORED_BARS = new Set(["elevenlabs", "abliteration"]);
const LABEL_BAR_COLOR = "var(--text-primary, rgba(0, 0, 0, 0.85))";

/**
 * The card's `--provider-accent`: the user's override when set, otherwise the
 * label color for label-styled providers, otherwise the brand color.
 */
export function providerBarAccent(id: string, override?: string | null): string {
  if (override) return override;
  if (LABEL_COLORED_BARS.has(normalize(id))) return LABEL_BAR_COLOR;
  return getProviderIcon(id).brandColor;
}

/** Reference dark surface (upstream's palette audit uses the same #222222). */
const DARK_SURFACE = "#222222";
/** Below this a brand color is near-invisible on dark surfaces. */
const NEAR_INVISIBLE_ON_DARK = 1.5;
/** A lifted color reaches the WCAG 3:1 non-text minimum. */
const LIFTED_ON_DARK = 3;

/**
 * The tint for a near-black brand color on a dark surface (Settings in the
 * dark theme, the dark floating bar): the brand mixed toward white in 10%
 * steps until it reaches 3:1 on #222222. Returns null when the brand already
 * reads on dark, so every other provider keeps its exact color. Light
 * surfaces, the tray card above all, always use the brand color.
 */
export function brandColorOnDark(brandColor: string): string | null {
  const contrast = contrastRatio(brandColor, DARK_SURFACE);
  if (contrast === null || contrast >= NEAR_INVISIBLE_ON_DARK) return null;
  for (let step = 1; step <= 10; step += 1) {
    const lifted = mixWithWhite(brandColor, step / 10);
    if ((contrastRatio(lifted, DARK_SURFACE) ?? 0) >= LIFTED_ON_DARK) return lifted;
  }
  return null;
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
