import { brandArtworkFor, spendIconScale } from "./brandIcons";
import { getProviderIcon } from "./providerIcons";
import "./SpendProviderIcon.css";

/** Upstream `ProviderBrandIcon.Style`: brand keeps original colors; monochrome is the adaptive template. */
export type SpendIconStyle = "brand" | "monochrome";

interface Props {
  providerId: string;
  /** Provider headings, chart legends and inspectors use brand; child rows stay monochrome. */
  style?: SpendIconStyle;
  /** Shared Usage & Spend slot, in CSS pixels (upstream 20 point slot). */
  size?: number;
}

/**
 * Upstream `SpendProviderIcon`: one 20 point slot for every Usage & Spend
 * provider icon. Curated brand artwork renders unmodified; otherwise the
 * monochrome provider mark (or its fallback letter) renders at 76% opacity.
 * The art may overflow the slot when a scale compensates transparent padding.
 */
export function SpendProviderIcon({ providerId, style = "brand", size = 20 }: Props) {
  const brand = style === "brand" ? brandArtworkFor(providerId) : null;
  const scale = spendIconScale(providerId, brand != null);
  const artSize = size * scale;
  const slot = {
    width: size,
    height: size,
  };
  const art = { width: artSize, height: artSize };

  let content;
  if (brand?.kind === "image") {
    content = <img className="spend-provider-icon__art" src={brand.src} alt="" draggable={false} style={art} />;
  } else if (brand?.kind === "svg") {
    content = (
      <span
        className="spend-provider-icon__art"
        style={art}
        // eslint-disable-next-line react/no-danger -- bundled brand SVGs, no user input.
        dangerouslySetInnerHTML={{ __html: brand.markup }}
      />
    );
  } else {
    const mark = getProviderIcon(providerId);
    content = mark.svgPath ? (
      <span
        className="spend-provider-icon__art"
        style={art}
        // eslint-disable-next-line react/no-danger -- bundled provider SVGs, no user input.
        dangerouslySetInnerHTML={{ __html: mark.svgPath }}
      />
    ) : (
      <span className="spend-provider-icon__letter">{mark.fallbackLetter}</span>
    );
  }

  return (
    <span
      className={`spend-provider-icon spend-provider-icon--${brand ? "brand" : "monochrome"}`}
      style={slot}
      aria-hidden="true"
    >
      {content}
    </span>
  );
}
