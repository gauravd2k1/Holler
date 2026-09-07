/**
 * Quantities are integer micro-units end to end — gram/litre/piece x 10^6,
 * with the scale in the field name (CLAUDE.md, contracts 0.5.0). This module is
 * the only place this app converts, for the same reason money.ts is.
 *
 * The binding range limit here is JavaScript's 2^53, not i64: a quantity that
 * fits in the database can still lose precision in a browser, so anything at or
 * beyond Number.MAX_SAFE_INTEGER is refused rather than silently rounded.
 */

const MICRO = 1_000_000;

export function formatMicro(micro: number): string {
  const whole = Math.floor(Math.abs(micro) / MICRO);
  const fraction = Math.abs(micro) % MICRO;
  const sign = micro < 0 ? "-" : "";
  if (fraction === 0) return `${sign}${whole}`;
  return `${sign}${whole}.${String(fraction).padStart(6, "0").replace(/0+$/, "")}`;
}

/**
 * Returns null on anything that is not a well-formed quantity. Integer maths
 * only: `1.5 * 1_000_000` is exact, but `0.07 * 1_000_000` is 70000.00000000001,
 * and a receipt is the worst place to discover that.
 */
export function parseToMicro(input: string): number | null {
  const trimmed = input.trim();
  if (!/^\d+(\.\d{1,6})?$/.test(trimmed)) return null;

  const [whole, fraction = ""] = trimmed.split(".");
  const micro = Number(whole) * MICRO + Number(fraction.padEnd(6, "0"));
  if (!Number.isSafeInteger(micro)) return null;
  return micro;
}
