import type { CSSProperties } from "react";

const HEX_COLOR = /^#([0-9a-f]{2})([0-9a-f]{2})([0-9a-f]{2})$/i;

function srgbByteToLinear(byte: number): number {
  const c = byte / 255;
  return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
}

export function accentSelectionStyle(accent: string | null): CSSProperties {
  const match = accent ? HEX_COLOR.exec(accent) : null;
  if (!match) return {};
  const [r, g, b] = match.slice(1).map((hex) => srgbByteToLinear(parseInt(hex, 16)));
  const luminance = 0.2126 * r + 0.7152 * g + 0.0722 * b;
  const whiteContrast = 1.05 / (luminance + 0.05);
  return {
    "--mac-selection-bg": accent,
    "--mac-selection-text": whiteContrast >= 4.5 ? "#fff" : "#000",
  } as CSSProperties;
}
