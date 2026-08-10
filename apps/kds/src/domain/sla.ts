// SLA urgency bucketing (docs/spec/kitchen.md §KDS). Thresholds are
// configurable — never a magic number baked into a component. Colour is
// never the only signal: callers must also render `elapsedMinutes` and the
// KOT status text alongside whatever colour a bucket implies.

export interface SlaThresholds {
  /** Strictly below this many minutes: GREEN. */
  greenUnderMinutes: number;
  /** From `greenUnderMinutes` up to (and including) this many minutes: AMBER.
   * Above it: RED. */
  amberUnderOrEqualMinutes: number;
}

// docs/spec/kitchen.md example values: GREEN <8min, AMBER 8-12, RED >12.
export const DEFAULT_SLA_THRESHOLDS: SlaThresholds = {
  greenUnderMinutes: 8,
  amberUnderOrEqualMinutes: 12,
};

export type SlaBucket = "GREEN" | "AMBER" | "RED";

export function slaBucket(elapsedMinutes: number, thresholds: SlaThresholds): SlaBucket {
  if (elapsedMinutes < thresholds.greenUnderMinutes) return "GREEN";
  if (elapsedMinutes <= thresholds.amberUnderOrEqualMinutes) return "AMBER";
  return "RED";
}

/** Whole minutes elapsed since `createdAt`, floored, never negative (a clock
 * skew must not render a negative ticket age). */
export function elapsedMinutes(createdAt: string, now: Date): number {
  const created = new Date(createdAt).getTime();
  const diffMs = now.getTime() - created;
  return Math.max(0, Math.floor(diffMs / 60_000));
}
