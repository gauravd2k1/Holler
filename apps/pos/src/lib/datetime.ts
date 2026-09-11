/**
 * Renders a UTC timestamp as outlet-local (IST) for a human to read.
 *
 * Storage stays UTC end to end (CLAUDE.md) — this is a RENDER-ONLY
 * conversion, never written back anywhere. `Intl.DateTimeFormat` does the
 * conversion so no fixed +5:30 offset is hand-rolled here (DST does not
 * apply to IST, but hand-rolling an offset is exactly the kind of thing that
 * quietly breaks the next time this function is copied to a timezone that
 * has one).
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

/** Returns the input unchanged if it is not a parseable timestamp, rather
 * than a placeholder that could be mistaken for a real time. */
export function formatIST(isoTimestamp: string | null | undefined): string {
  if (isoTimestamp == null || isoTimestamp === "") return "—";
  const date = new Date(isoTimestamp);
  if (Number.isNaN(date.getTime())) return isoTimestamp;
  // en-IN's ICU data renders the day period lowercase ("01:49 am"), which on
  // a real screen at a metre reads as a typo rather than a time — observed
  // on the admin goods-receipts screen (T7 browser pass). Uppercased here
  // rather than via an Intl option, because `dayPeriod` casing is not a
  // configurable field on this formatter.
  return `${IST_DATETIME.format(date).replace(/\b(am|pm)\b/, (m) => m.toUpperCase())} IST`;
}
