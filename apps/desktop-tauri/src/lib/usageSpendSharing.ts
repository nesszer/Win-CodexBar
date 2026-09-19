interface CivilDate {
  year: number;
  month: number;
  day: number;
}

export interface UsageSpendShareSummary {
  rows: readonly unknown[];
  reportingDay: string;
  dashboardTimezone: string;
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

export function usageSpendShareFooter(summary: UsageSpendShareSummary): string {
  return `Data through ${formatUsageSpendReportingDay(summary.reportingDay, summary.dashboardTimezone)} · ${usageSpendSubscriptionCaption(summary.rows.length)}`;
}
