/**
 * Money is integer paise end to end (CLAUDE.md). This is the only place this
 * app converts to a display string — no component multiplies or divides a
 * money field itself.
 *
 * Formatting only. Never used for arithmetic: the cart total is a sum of
 * integer paise fields, done in `useCart`, and this function is applied only
 * at the point of render.
 */
export function formatPaise(paise: number): string {
  const negative = paise < 0;
  const abs = Math.abs(paise);
  const rupees = Math.floor(abs / 100);
  const remainder = abs % 100;
  return `${negative ? "-" : ""}₹${rupees}.${String(remainder).padStart(2, "0")}`;
}
