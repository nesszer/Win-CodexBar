export function formatEta(seconds: number): string {
  const totalSeconds = Math.max(0, seconds);
  if (totalSeconds < 60 * 60) return `${Math.round(totalSeconds / 60)}m`;

  if (totalSeconds < 24 * 60 * 60) {
    return `${Math.round(totalSeconds / 60 / 60)}h`;
  }

  return `${Math.round(totalSeconds / 60 / 60 / 24)}d`;
}
