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

/**
 * Parses a cashier-typed rupees string (e.g. "12.50", "-5", "0") into
 * integer paise via string manipulation only — never `Number(x) * 100`,
 * which is a float multiplication on a decimal that is frequently not
 * exactly representable (e.g. 0.1 + 0.2 problems). This is the one place a
 * decimal rupee amount typed at a keyboard is allowed to become paise; every
 * other function in this module already assumes paise going in.
 *
 * Returns `null` for anything that is not a valid amount with at most two
 * decimal places, rather than silently truncating or rounding a typo into
 * real money.
 */
export function parseRupeesToPaise(input: string): number | null {
  const trimmed = input.trim();
  const match = /^(-?)(\d+)(?:\.(\d{1,2}))?$/.exec(trimmed);
  if (!match) return null;
  const [, sign, rupeesPart, decimalPart = ""] = match;
  const paiseDecimal = decimalPart.padEnd(2, "0");
  const rupees = Number.parseInt(rupeesPart, 10);
  const paise = Number.parseInt(paiseDecimal, 10);
  const total = rupees * PAISE_PER_RUPEE + paise;
  return sign === "-" ? -total : total;
}
