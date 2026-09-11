/**
 * Renders a UTC timestamp as outlet-local (IST) for a human to read.
 *
 * Storage stays UTC end to end (CLAUDE.md) — this is a RENDER-ONLY
 * conversion, never sent anywhere. `Intl.DateTimeFormat` does the conversion
 * rather than a hand-rolled +5:30 offset, the same choice made in
 * `apps/pos/src/lib/datetime.ts`.
 *
 * Distinct from `invoice.business_date`'s UTC-calendar-day bucketing defect
 * (filed to M6, CLAUDE.md) — that is a stored value, not a rendering choice,
 * and this function does not touch it.
 */
const IST_DATETIME = new Intl.DateTimeFormat("en-IN", {
  timeZone: "Asia/Kolkata",
  day: "2-digit",
  month: "short",
  year: "numeric",
  hour: "2-digit",
  minute: "2-digit",
  hour12: true,
});

export function formatIST(isoTimestamp: string | null | undefined): string {
  if (isoTimestamp == null || isoTimestamp === "") return "—";
  const date = new Date(isoTimestamp);
  if (Number.isNaN(date.getTime())) return isoTimestamp;
  return `${IST_DATETIME.format(date)} IST`;
}
