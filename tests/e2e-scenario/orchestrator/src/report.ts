import fs from "node:fs";
import path from "node:path";
import { ALL_INVARIANTS, REQUIRED_SHAPES } from "./types";
import type { ScenarioResult } from "./types";

function percentile(sorted: number[], p: number): number | null {
  if (sorted.length === 0) return null;
  const idx = Math.min(sorted.length - 1, Math.floor((p / 100) * sorted.length));
  return sorted[idx];
}

export function writeReport(
  outPath: string,
  seedBase: number,
  results: ScenarioResult[],
): void {
  const lines: string[] = [];
  lines.push("# e2e-scenario-harness run report");
  lines.push("");
  lines.push(`Generated: ${new Date().toISOString()}`);
  lines.push(`Base seed: \`${seedBase}\` (reproduce a specific scenario with \`--seed ${seedBase}\`; each scenario's own seed is its \`seed\` field below and is independently reproducible via the same base + index).`);
  lines.push(`Scenario count: ${results.length}`);
  lines.push("");

  lines.push("## Pass/fail per invariant");
  lines.push("");
  lines.push("| Invariant | Checked | Passed | Failed | Unchecked |");
  lines.push("|---|---|---|---|---|");
  for (const inv of ALL_INVARIANTS) {
    let checked = 0, passed = 0, failed = 0;
    for (const r of results) {
      const o = r.invariants[inv];
      if (o.checked) {
        checked += 1;
        if (o.passed) passed += 1; else failed += 1;
      }
    }
    lines.push(`| ${inv} | ${checked} | ${passed} | ${failed} | ${results.length - checked} |`);
  }
  lines.push("");

  // Shapes come FIRST, above the invariant table, because they qualify it:
  // an invariant row reading 54/54 passed means nothing if the shape it
  // covers never occurred. A zero in this table invalidates the table above
  // it, and the CI job fails on one.
  lines.push("## Data shapes actually produced (green-on-absent-data guard)");
  lines.push("");
  lines.push("| Shape | Occurrences | Scenarios |");
  lines.push("|---|---|---|");
  for (const shape of REQUIRED_SHAPES) {
    const total = results.reduce((a, r) => a + (r.shapes[shape] ?? 0), 0);
    const scenarios = results.filter((r) => (r.shapes[shape] ?? 0) > 0).length;
    const flag = total === 0 ? " **ZERO — invariant is green on absent data**" : "";
    lines.push(`| ${shape} | ${total}${flag} | ${scenarios} |`);
  }
  lines.push("");

  const fatal = results.filter((r) => r.fatalError);
  lines.push(`## Fatal errors (harness/process-level, not invariant failures): ${fatal.length}`);
  lines.push("");
  for (const r of fatal) {
    lines.push(`### ${r.name} (seed ${r.seed})`);
    lines.push("```");
    lines.push(r.fatalError ?? "");
    lines.push("```");
  }
  lines.push("");

  lines.push("## Every scenario with at least one failed invariant (full replayable action sequence)");
  lines.push("");
  const failing = results.filter((r) => ALL_INVARIANTS.some((i) => r.invariants[i].checked && !r.invariants[i].passed));
  if (failing.length === 0) {
    lines.push("None.");
  }
  for (const r of failing) {
    lines.push(`### ${r.name} (seed ${r.seed})`);
    lines.push("");
    lines.push("Broken invariants:");
    for (const inv of ALL_INVARIANTS) {
      const o = r.invariants[inv];
      if (o.checked && !o.passed) lines.push(`- **${inv}**: ${o.detail ?? "(no detail)"}`);
    }
    lines.push("");
    lines.push("Action sequence:");
    lines.push("```json");
    lines.push(JSON.stringify(r.actions, null, 2));
    lines.push("```");
    lines.push("");
  }

  lines.push("## Findings (coverage gaps / product defects — not fixed by this track)");
  lines.push("");
  const uniqueFindings = new Set<string>();
  for (const r of results) for (const f of r.findings) uniqueFindings.add(f);
  if (uniqueFindings.size === 0) {
    lines.push("None.");
  }
  for (const f of uniqueFindings) lines.push(`- ${f}`);
  lines.push("");

  lines.push("## Latency distribution (invariant 3: KOT reaches a subscribed KDS client)");
  lines.push("");
  const l3 = results.flatMap((r) => r.latencySamples.filter((s) => s.invariant === "3_kds_fidelity").map((s) => s.ms)).sort((a, b) => a - b);
  lines.push(`Samples: ${l3.length}`);
  lines.push(`P50: ${percentile(l3, 50) ?? "n/a"}ms, P95: ${percentile(l3, 95) ?? "n/a"}ms, max: ${l3.length ? l3[l3.length - 1] : "n/a"}ms`);
  lines.push("");

  lines.push("## Latency distribution (invariant 8: KDS status change echoed POS-side)");
  lines.push("");
  const l8 = results.flatMap((r) => r.latencySamples.filter((s) => s.invariant === "8_status_echo").map((s) => s.ms)).sort((a, b) => a - b);
  lines.push(`Samples: ${l8.length}`);
  lines.push(`P50: ${percentile(l8, 50) ?? "n/a"}ms, P95: ${percentile(l8, 95) ?? "n/a"}ms, max: ${l8.length ? l8[l8.length - 1] : "n/a"}ms`);
  lines.push("");

  lines.push("## Crash-simulation scenarios");
  lines.push("");
  const crashed = results.filter((r) => r.crashed);
  lines.push(`${crashed.length} scenario(s) included a crash+recover step.`);
  lines.push("");

  fs.mkdirSync(path.dirname(outPath), { recursive: true });
  fs.writeFileSync(outPath, lines.join("\n"));
}
