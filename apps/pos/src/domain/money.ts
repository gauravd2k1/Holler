// Money is always integer paise (CLAUDE.md §Money). Every function here
// operates on integers only — no float division is ever used to render
// rupees, because 1/100 is not exactly representable in binary floating
// point and repeated float math on money silently drifts.

const PAISE_PER_RUPEE = 100;

/** Throws if `paise` is not a safe integer — callers must never pass a float. */
function assertIntegerPaise(paise: number): void {
  if (!Number.isInteger(paise)) {
    throw new Error(`money value must be an integer number of paise, got ${paise}`);
  }
}

/**
 * Formats integer paise as an Indian Rupee string, e.g. 12550 -> "₹125.50",
 * 0 -> "₹0.00", 5 -> "₹0.05", -12550 -> "-₹125.50". Uses only integer
 * arithmetic (div/mod by 100) and string concatenation — never a float
 * division of paise by 100.
 */
export function formatPaiseAsRupees(paise: number): string {
  assertIntegerPaise(paise);
  const negative = paise < 0;
  const abs = Math.abs(paise);
  const rupees = Math.trunc(abs / PAISE_PER_RUPEE);
  const remainderPaise = abs % PAISE_PER_RUPEE;
  const paiseStr = remainderPaise.toString().padStart(2, "0");
  const sign = negative ? "-" : "";
  return `${sign}₹${rupees}.${paiseStr}`;
}

/** Sums an array of integer paise amounts using integer addition only. */
export function sumPaise(amounts: readonly number[]): number {
  return amounts.reduce((total, amount) => {
    assertIntegerPaise(amount);
    return total + amount;
  }, 0);
}

/** unit_price_paise * quantity, both integers -> integer line total. */
export function lineTotalPaise(unitPricePaise: number, quantity: number): number {
  assertIntegerPaise(unitPricePaise);
  if (!Number.isInteger(quantity) || quantity <= 0) {
    throw new Error(`quantity must be a positive integer, got ${quantity}`);
  }
  return unitPricePaise * quantity;
}
