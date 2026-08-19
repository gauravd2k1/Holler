export type InvariantId =
  | "1_state_machine"
  | "2_kot_conservation"
  | "3_kds_fidelity"
  | "4_no_station_explicit"
  | "5_money"
  | "6_durability"
  | "7_outbox"
  | "8_status_echo"
  | "9_tax_reconciliation"
  | "10_payment_settlement"
  | "11_discount"
  | "12_split_conservation"
  | "13_invoice_print";

export const ALL_INVARIANTS: InvariantId[] = [
  "1_state_machine",
  "2_kot_conservation",
  "3_kds_fidelity",
  "4_no_station_explicit",
  "5_money",
  "6_durability",
  "7_outbox",
  "8_status_echo",
  "9_tax_reconciliation",
  "10_payment_settlement",
  "11_discount",
  "12_split_conservation",
  "13_invoice_print",
];

/** The data shapes a run must actually PRODUCE, not merely have code for.
 * Every one of these was previously "covered" by an invariant that passed on
 * every scenario without the shape ever occurring — a discount of zero, a
 * split of one part, a print path with no printer. A run that exercises none
 * of them is green on absent data, so `run.ts` fails the run when any count
 * here is zero rather than reporting a pass.
 *
 * Counted per scenario and summed across the run. */
export type ShapeId =
  | "discount_applied_nonzero"
  | "discount_refused_without_permission"
  | "discount_refused_without_reason"
  | "split_invoice_multi_part"
  | "invoice_print_job_enqueued"
  | "invoice_print_job_printed";

export const REQUIRED_SHAPES: ShapeId[] = [
  "discount_applied_nonzero",
  "discount_refused_without_permission",
  "discount_refused_without_reason",
  "split_invoice_multi_part",
  "invoice_print_job_enqueued",
  // Enqueued is not printed. This one only counts when the job reached
  // PRINTED, which means the ESC/POS render ran and the bytes went out over
  // a real transport — the difference between "a row exists" and "a bill
  // came out".
  "invoice_print_job_printed",
];

export interface InvariantOutcome {
  checked: boolean;
  passed: boolean;
  detail?: string;
}

export interface ActionLogEntry {
  seq: number;
  action: string;
  request?: unknown;
  ok: boolean;
  error?: { code: string; message: string };
  latencyMs?: number;
}

export interface ScenarioResult {
  name: string;
  seed: number;
  actions: ActionLogEntry[];
  invariants: Record<InvariantId, InvariantOutcome>;
  findings: string[];
  latencySamples: { invariant: "3_kds_fidelity" | "8_status_echo"; ms: number }[];
  /** How many times this scenario produced each shape. Absent keys are zero.
   * Recorded from OBSERVED results (a discount seen non-zero on a persisted
   * invoice line, a split group read back with >1 invoice, a print job read
   * back by id) — never from "we sent the request". */
  shapes: Partial<Record<ShapeId, number>>;
  crashed: boolean;
  fatalError?: string;
}
