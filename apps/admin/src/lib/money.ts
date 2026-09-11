/**
 * Money is integer paise end to end (CLAUDE.md). These functions are the
 * ONLY place this app converts between the two representations, and they
 * exist so that no component is ever tempted to multiply by 100 inline.
 *
 * Nothing here does floating-point arithmetic on money. `parseRupeesToPaise`
 * splits on the decimal point and works in integers, because `12.55 * 100` is
 * 1254.9999999999998 in IEEE 754 and `Math.round` on it is a coin flip that
 * happens to land right most of the time — which is the worst kind of bug,
 * since it survives every test written from the same intuition.
 *
 * `formatPaiseAsRupees` matches `apps/pos/src/domain/money.ts` byte for byte
 * in behaviour (₹ prefix, integer div/mod by 100, no float division) — two
 * apps, one product, one money format. `formatPaiseAsPlainDecimal` is the
 * unprefixed sibling POS also carries: it is what an *editable* price input
 * is seeded with, because a ₹ prefix in that string would break the
 * plain-decimal parse `parseRupeesToPaise` does on save.
 */

export function formatPaiseAsRupees(paise: number): string {
  const negative = paise < 0;
  const abs = Math.abs(paise);
  const rupees = Math.trunc(abs / 100);
  const remainder = abs % 100;
  return `${negative ? "-" : ""}₹${rupees}.${String(remainder).padStart(2, "0")}`;
}

export function formatPaiseAsPlainDecimal(paise: number): string {
  const negative = paise < 0;
  const abs = Math.abs(paise);
  const rupees = Math.trunc(abs / 100);
  const remainder = abs % 100;
  return `${negative ? "-" : ""}${rupees}.${String(remainder).padStart(2, "0")}`;
}

/**
 * Returns null on anything that is not a well-formed amount, rather than a
 * plausible number. A silent 0 or NaN on a price field is a wrong bill.
 */
export function parseRupeesToPaise(input: string): number | null {
  const trimmed = input.trim();
  if (!/^\d+(\.\d{1,2})?$/.test(trimmed)) return null;

  const [whole, fraction = ""] = trimmed.split(".");
  const paiseFraction = fraction.padEnd(2, "0");
  return Number(whole) * 100 + Number(paiseFraction);
}
