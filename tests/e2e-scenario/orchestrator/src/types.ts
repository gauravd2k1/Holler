export type InvariantId =
  | "1_state_machine"
  | "2_kot_conservation"
  | "3_kds_fidelity"
  | "4_no_station_explicit"
  | "5_money"
  | "6_durability"
  | "7_outbox"
  | "8_status_echo";

export const ALL_INVARIANTS: InvariantId[] = [
  "1_state_machine",
  "2_kot_conservation",
  "3_kds_fidelity",
  "4_no_station_explicit",
  "5_money",
  "6_durability",
  "7_outbox",
  "8_status_echo",
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
  crashed: boolean;
  fatalError?: string;
}
