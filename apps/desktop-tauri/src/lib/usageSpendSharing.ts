import type { UsageSpendRow, UsageSpendSummary } from "../types/bridge";

interface CivilDate {
  year: number;
  month: number;
  day: number;
}

/**
 * Keep Overview sharing aligned with the rows that Overview itself displays.
 *
 * The backend (`usage_spend.rs`) always emits `includedInOverview`, so
 * absent is not a state the backend produces; treat it defensively as
 * included so a payload gap can never silently drop rows from the shared
 * snapshot that Overview displays.
 */
export function filterUsageSpendSummaryForOverview(summary: UsageSpendSummary): UsageSpendSummary {
  return {
    ...summary,
    rows: summary.rows.filter((row) => row.includedInOverview !== false),
  };
}

function parseCivilDate(value: string): CivilDate | null {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(value);
  if (!match) return null;
  const [, yearText, monthText, dayText] = match;
  const year = Number(yearText);
  const month = Number(monthText);
  const day = Number(dayText);
  if (year < 1 || month < 1 || month > 12 || day < 1 || day > 31) return null;

  const instant = new Date(0);
  instant.setUTCFullYear(year, month - 1, day);
  instant.setUTCHours(12, 0, 0, 0);
  if (
    instant.getUTCFullYear() !== year ||
    instant.getUTCMonth() !== month - 1 ||
    instant.getUTCDate() !== day
  ) {
    return null;
  }
  return { year, month, day };
}

function civilDateInstant(date: CivilDate): Date {
  const instant = new Date(0);
  instant.setUTCFullYear(date.year, date.month - 1, date.day);
  instant.setUTCHours(12, 0, 0, 0);
  return instant;
}

function partsInTimeZone(instant: Date, timeZone: string): CivilDate {
  const parts = new Intl.DateTimeFormat("en-US", {
    timeZone,
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
  }).formatToParts(instant);
  const valueFor = (type: string) => parts.find((part) => part.type === type)?.value;
  const year = Number(valueFor("year"));
  const month = Number(valueFor("month"));
  const day = Number(valueFor("day"));
  if (![year, month, day].every(Number.isInteger)) {
    throw new RangeError("Timezone did not provide a complete date");
  }
  return { year, month, day };
}

/** Formats a civil reporting day without allowing the timezone offset to roll it into another day. */
export function formatUsageSpendReportingDay(
  reportingDay: string,
  dashboardTimezone: string,
  locale = "en-US",
): string {
  const civilDate = parseCivilDate(reportingDay);
  if (!civilDate) return reportingDay;

  try {
    const target = civilDateInstant(civilDate);
    // Validate the configured zone, but keep the reporting day as the civil
    // date supplied by the dashboard. Applying the zone offset to the instant
    // can move October 1 (and other boundary dates) into a different calendar
    // day in zones with a non-hour offset or a DST transition.
    partsInTimeZone(target, dashboardTimezone);
    return new Intl.DateTimeFormat(locale, {
      timeZone: "UTC",
      year: "numeric",
      month: "short",
      day: "numeric",
    }).format(target);
  } catch {
    return reportingDay;
  }
}

export function usageSpendSubscriptionCaption(count: number): string {
  return count === 1 ? "1 subscription" : `${count} subscriptions`;
}

export function usageSpendShareFooter(summary: UsageSpendSummary): string {
  return `Data through ${formatUsageSpendReportingDay(summary.reportingDay, summary.dashboardTimezone)} · ${usageSpendSubscriptionCaption(summary.rows.length)}`;
}

const currencyFormatters = new Map<string, Intl.NumberFormat>();

/** Canonical USD-style formatter for spend tables and share renders. */
export function formatUsd(value: number | null | undefined, currency: string): string {
  if (value == null || !Number.isFinite(value)) return "—";
  const code = currency || "USD";
  try {
    let formatter = currencyFormatters.get(code);
    if (!formatter) {
      formatter = new Intl.NumberFormat(undefined, {
        style: "currency",
        currency: code,
        maximumFractionDigits: 2,
      });
      currencyFormatters.set(code, formatter);
    }
    return formatter.format(value);
  } catch {
    return `$${value.toFixed(2)}`;
  }
}

/** Canonical "cost · tokens" cell for spend tables and share renders. */
export function formatSpendMetric(
  cost: number | null | undefined,
  tokens: number | null | undefined,
  currency: string,
  tokenLabel: string,
): string {
  const parts: string[] = [];
  if (cost != null && Number.isFinite(cost)) parts.push(formatUsd(cost, currency));
  if (tokens != null && Number.isFinite(tokens)) {
    parts.push(`${Math.max(0, tokens).toLocaleString()} ${tokenLabel}`);
  }
  return parts.length > 0 ? parts.join(" · ") : "—";
}

export interface UsageSpendSharePresentation {
  formatMetric?: (cost: number | null | undefined, tokens: number | null | undefined, sourceCurrency: string, tokenLabel: string) => string;
  displayCurrency?: (sourceCurrency: string) => string;
}

/**
 * Render the sanitized share-card PNG.
 *
 * Redaction invariant (upstream #2112): the rendered image must never contain
 * account emails or any provider-identity secrets. Only
 * `displayName / metrics / currency / source` cells are drawn, and the footer
 * states the guarantee. Keep it that way — do not add account fields to the
 * drawn cells or the footer.
 */
export function renderUsageSpendSharePng(
  summary: UsageSpendSummary,
  title: string,
  presentation: UsageSpendSharePresentation = {},
): string {
  const rows = summary.rows;
  const pad = 24;
  const rowH = 28;
  const headerH = 48;
  const colW = [160, 100, 100, 80, 160];
  const width = pad * 2 + colW.reduce((a, b) => a + b, 0);
  const height = pad * 2 + headerH + Math.max(1, rows.length) * rowH + 52;
  const canvas = document.createElement("canvas");
  canvas.width = width * 2;
  canvas.height = height * 2;
  const ctx = canvas.getContext("2d");
  if (!ctx) return "";
  ctx.scale(2, 2);

  ctx.fillStyle = "#0f1419";
  ctx.fillRect(0, 0, width, height);
  ctx.strokeStyle = "#243044";
  ctx.lineWidth = 1;
  ctx.strokeRect(0.5, 0.5, width - 1, height - 1);

  ctx.fillStyle = "#e7ecf3";
  ctx.font = "600 16px system-ui,Segoe UI,sans-serif";
  ctx.fillText(title, pad, pad + 18);

  ctx.fillStyle = "#8b9bb4";
  ctx.font = "12px system-ui,Segoe UI,sans-serif";
  ctx.fillText("Win-CodexBar · local estimates · no account emails", pad, pad + 36);

  const headers = ["Provider", "7 days", "30 days", "Currency", "Source"];
  let x = pad;
  const y0 = pad + headerH;
  ctx.fillStyle = "#9fb0c8";
  ctx.font = "600 12px system-ui,Segoe UI,sans-serif";
  headers.forEach((header, index) => {
    ctx.fillText(header, x, y0);
    x += colW[index];
  });

  ctx.strokeStyle = "#243044";
  ctx.beginPath();
  ctx.moveTo(pad, y0 + 8);
  ctx.lineTo(width - pad, y0 + 8);
  ctx.stroke();

  ctx.font = "13px system-ui,Segoe UI,sans-serif";
  if (rows.length === 0) {
    ctx.fillStyle = "#8b9bb4";
    ctx.fillText("No spend data yet.", pad, y0 + rowH);
  } else {
    rows.forEach((row, index) => {
      const y = y0 + (index + 1) * rowH;
      const cells = [
        row.displayName,
        presentation.formatMetric?.(row.sevenDay, row.sevenDayTokens, row.currency || "USD", "tokens")
          ?? formatSpendMetric(row.sevenDay, row.sevenDayTokens, row.currency, "tokens"),
        presentation.formatMetric?.(row.thirtyDay, row.thirtyDayTokens, row.currency || "USD", "tokens")
          ?? formatSpendMetric(row.thirtyDay, row.thirtyDayTokens, row.currency, "tokens"),
        presentation.displayCurrency?.(row.currency || "USD") ?? (row.currency || "USD"),
        row.source,
      ];
      let cellX = pad;
      cells.forEach((cell, cellIndex) => {
        ctx.fillStyle = cellIndex === 0 ? "#e7ecf3" : "#c5d0e0";
        const text = String(cell);
        const max = colW[cellIndex] - 8;
        let draw = text;
        if (ctx.measureText(draw).width > max) {
          while (draw.length > 1 && ctx.measureText(`${draw}…`).width > max) {
            draw = draw.slice(0, -1);
          }
          draw = `${draw}…`;
        }
        ctx.fillText(draw, cellX, y);
        cellX += colW[cellIndex];
      });
    });
  }

  ctx.fillStyle = "#8b9bb4";
  ctx.font = "12px system-ui,Segoe UI,sans-serif";
  ctx.fillText(usageSpendShareFooter(summary), pad, height - pad);
  return canvas.toDataURL("image/png");
}

export function downloadPng(dataUrl: string, filename: string): void {
  const anchor = document.createElement("a");
  anchor.href = dataUrl;
  anchor.download = filename;
  anchor.rel = "noopener";
  document.body.appendChild(anchor);
  anchor.click();
  anchor.remove();
}

/**
 * One share funnel for every surface: render → validate → download, mapping
 * all failure modes (no summary, renderer failure, exception) onto one error
 * message. Returns the localized error string or `null` on success.
 */
export function shareUsageSpendPng(
  summary: UsageSpendSummary | null,
  title: string,
  filename: string,
  presentation: UsageSpendSharePresentation = {},
): string | null {
  if (!summary) return "UsageSpendShareEmpty";
  try {
    const dataUrl = renderUsageSpendSharePng(summary, title, presentation);
    if (!dataUrl) return "UsageSpendShareFailed";
    downloadPng(dataUrl, filename);
    return null;
  } catch {
    return "UsageSpendShareFailed";
  }
}
