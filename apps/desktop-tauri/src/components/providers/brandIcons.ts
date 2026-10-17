import antigravityPng from "../../assets/brand-icons/Brand-ProviderIcon-antigravity.png";
import bedrockSvg from "../../assets/brand-icons/Brand-ProviderIcon-bedrock.svg?raw";
import claudeSvg from "../../assets/brand-icons/Brand-ProviderIcon-claude.svg?raw";
import codexPng from "../../assets/brand-icons/Brand-ProviderIcon-codex.png";
import mistralSvg from "../../assets/brand-icons/Brand-ProviderIcon-mistral.svg?raw";
import museSvg from "../../assets/brand-icons/Brand-ProviderIcon-muse.svg?raw";
import vertexaiSvg from "../../assets/brand-icons/Brand-ProviderIcon-vertexai.svg?raw";
import { getProviderIcon } from "./providerIcons";

/**
 * Original brand artwork for Usage & Spend provider headings, chart legends and
 * inspectors (upstream CodexBar 0.73.0 `Brand-ProviderIcon-*`). Each file is
 * byte-identical to the upstream resource; its colors are not recolored.
 *
 * Provenance and SHA-256 pins: upstream `docs/provider-brand-icons.md`
 * (codex PNG 8e82b26c, antigravity PNG 193ba180, bedrock 6a0f3817,
 * claude 4cd39a38, mistral 13e29ba8, muse b49f20f0, vertexai 17922247).
 * The marks identify third-party products and remain their owners' property.
 *
 * Only these providers have curated artwork. Everything else keeps the
 * adaptive monochrome template from `providerIcons.ts`.
 */
export type BrandArtwork = { kind: "svg"; markup: string } | { kind: "image"; src: string };

const BRAND_ARTWORK = new Map<string, BrandArtwork>([
  ["antigravity", { kind: "image", src: antigravityPng }],
  ["bedrock", { kind: "svg", markup: bedrockSvg }],
  ["claude", { kind: "svg", markup: claudeSvg }],
  ["codex", { kind: "image", src: codexPng }],
  ["mistral", { kind: "svg", markup: mistralSvg }],
  ["muse", { kind: "svg", markup: museSvg }],
  ["vertexai", { kind: "svg", markup: vertexaiSvg }],
]);

/** Curated brand artwork for a provider id or alias, or null when none is bundled. */
export function brandArtworkFor(providerId: string): BrandArtwork | null {
  return BRAND_ARTWORK.get(getProviderIcon(providerId).id) ?? null;
}

const ICON_SCALES = new Map<string, { brand: number; monochrome: number }>([
  ["cursor", { brand: 1.25, monochrome: 1.25 }],
  ["codex", { brand: 1.17, monochrome: 1.24 }],
  ["antigravity", { brand: 1.38, monochrome: 1.14 }],
  ["bedrock", { brand: 0.84, monochrome: 1 }],
  ["muse", { brand: 1.08, monochrome: 1 }],
  ["vertexai", { brand: 1.08, monochrome: 1 }],
]);

/**
 * Upstream `SpendProviderIcon.artworkScale`: each bundled mark's transparent
 * padding is normalized inside the shared 20 point slot. Brand and monochrome
 * artwork have separate compensations; `isBrand` is false for template fallbacks.
 */
export function spendIconScale(providerId: string, isBrand: boolean): number {
  const scale = ICON_SCALES.get(getProviderIcon(providerId).id);
  if (!scale) return 1;
  return isBrand ? scale.brand : scale.monochrome;
}
