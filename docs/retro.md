# Retrospective log

Incidents and the rules that came out of them. Entries are append-only: an entry is amended only to record that its rule changed, never deleted.

---

## 2026-08-07 — A verification agent destroyed a track's uncommitted work

**Severity:** high (recovered). Roughly 5.5KB of reviewed, passing work briefly lost.

### What happened

During Milestone 1, `edge/database` had completed the contracts 0.2.3 follow-up — order line modifier persistence and the hardened `ItemRemoved` payload. The work was in the working tree, unstaged, awaiting its verification gate.

The orchestrator dispatched a verifier and, among other checks, asked it to prove the event-type drift check actually constrained the crate's literals: *"try renaming a literal locally to confirm it fails, then revert."*

The verifier renamed a literal in `edge/database/src/repo.rs`, confirmed the check failed as intended, and reverted with `git checkout -- edge/database/src/repo.rs`. That command does not undo one edit — it restores the file to its committed state, discarding **every** unstaged change in it. The builder's entire file was destroyed. `lib.rs`, `model.rs` and `migrations.rs` survived and still referenced APIs that no longer existed, so the crate stopped compiling.

### Root cause

**The orchestrator's fault, not the verifier's.** A read-only gate was handed an instruction that required mutating a tracked file, in a tree holding another agent's uncommitted work, with no guidance to use a scratch copy or to stage first. The verifier followed the brief it was given, using the obvious command for the job.

Two conditions had to hold simultaneously, and both were the orchestrator's to prevent:
1. A destructive instruction in a read-only agent's brief.
2. A completed track left unstaged while another agent ran against the same tree.

### What went right

The verifier reported the incident immediately and completely: it marked its own verdict `FAIL (evaluator-caused data loss)`, named the exact command, listed which files survived and which APIs were left dangling, warned against committing the inconsistent tree, and stopped rather than attempting recovery that might compound the damage. That report is what made recovery quick. An agent that had quietly tried to reconstruct the file, or omitted the incident, would have turned a recoverable loss into a silent corruption.

### Recovery

Reconstructed from the builder agent's JSONL transcript: one base `Write` plus nine subsequent `Edit` calls replayed in order, all nine matching cleanly, 42,731 bytes. The extractor wrote to disk and printed only metadata, so no file content entered the orchestrator's context. Then re-validated: build clean, 35/35 tests, clippy clean under `-D warnings`, drift check green, and both downstream consumers still building. Committed as `1903165`.

### Rules adopted

1. **Verifiers must not mutate the working tree.** No state-changing git command, no editing tracked files — not even with intent to revert. Mutation-based spot-checks use a scratch copy outside the repository; if a check cannot be run without touching the tree, it goes unrun and the limitation goes in the verdict. — `.claude/agents/verifier.md`
2. **Commit before the next gate.** A track's completed work is never left unstaged while another agent runs. Unstaged work is unrecoverable; staged or committed work is not. — `.claude/commands/milestone.md`

### Notes for future incidents

- Agent transcripts under the session temp directory are a genuine recovery source for lost file content. Extract them with a script that writes to disk and prints only metadata — never read a transcript into context directly.
- "Revert it afterwards" in an agent brief is a smell. If a brief asks an agent to break something, it must also say precisely where the broken copy lives.

---

## 2026-08-10 — A backlog item was closed by satisfying its wording, not its purpose

**Severity:** low (caught at the gate, nothing shipped). But the failure mode is structural, not incidental.

### What happened

`docs/backlog-m2.md` carried a Milestone 1 item titled **POS cart persistence**. Its text read, in order: the cart lives in browser memory until Send, so a crash mid-order loses it; `add_order_item`/`remove_order_item` return `UNSUPPORTED_DB_OPERATION` at the Tauri layer even though the `edge/database` API exists; **wire them through**; this is about in-progress work on a system whose premise is that the shop floor never loses work.

Milestone 2's POS track was handed that item. It wired both commands through to the real `edge/database` calls, tested them at the Tauri layer, and reported the item closed. That was accurate: the imperative sentence in the entry — "wire them through" — was fully satisfied.

But **no frontend screen calls either command.** The cart still round-trips through one atomic `create_order` at Send. So after the change, a crash mid-order still loses the cart — the exact condition the entry existed to eliminate.

### Root cause

**The entry was written as a task, and the task was smaller than the problem.** Its first and last sentences named the real requirement (survive a crash); its middle sentence named a step toward it (wire the commands). A builder reading it for the actionable instruction found the step, did it correctly, and stopped. Nothing in the entry said the step was insufficient on its own.

Contributing: the orchestrator's dispatch repeated the entry's framing rather than its purpose, so the narrowing was inherited rather than caught at brief-writing time.

This is not a builder error. The builder disclosed precisely what it had and had not done — "no UI screen calls them yet" — in its own report, unprompted. That disclosure is the only reason the gap was caught before the item was marked done.

### What went right

The builder volunteered the limitation instead of reporting a clean close. The verifier, asked to judge the disclosure rather than record it, tested the actual condition and answered plainly: *crash mid-order still loses the cart — item genuinely NOT closed.* An honest builder report plus a verifier briefed to judge intent, not wording, caught something a passing test suite could not.

### Rules adopted

1. **A backlog entry states the condition that makes it closed, not the step someone guessed at.** Where an entry names both, the condition governs. Entries that only name a step get rewritten when they are picked up, before dispatch. — `docs/backlog-m2.md`
2. **Acceptance criteria are observable failures, not implemented APIs.** Milestone 2's acceptance gains: *crash mid-order → the cart survives.* An API that could prevent the loss does not count; the loss not happening counts. — `CLAUDE.md`
3. **A dispatch brief carries the purpose, not just the task text.** When an orchestrator forwards a backlog item, it forwards why the item exists, so a builder can tell when the literal instruction falls short. — `.claude/commands/milestone.md`

### Note

The item is reopened in `docs/backlog-m2.md` with the distinction written into it, so the next reader judges it against the crash rather than the API surface.

---

## 2026-08-11 — Two milestones of components that passed, and wiring that did not exist

**Severity:** high (no data lost; two milestones' acceptance evidence weaker than reported).

### What happened

Milestone 2 was reported with five of six acceptance criteria met and a `m2-complete` tag. Then a request for the item-1 runbook — the plain question *"what do I type to run this on two machines?"* — turned up three facts in about ten minutes:

1. **`edge/device` is a library with no binary.** The only caller of `server::start` in the entire repository is `tests/integration/kds-lan-bridge/src/main.rs`, a test harness. `apps/pos/src-tauri` does not depend on the crate at all.
2. **The only launcher binds loopback on an ephemeral port** (`127.0.0.1:0`), so nothing could reach it from another machine even by accident.
3. **Nothing connects the edge database to the broadcast hub.** `grep -rn "notify_kot_upserted\|Hub" apps/ edge/` outside `edge/device/src` returns nothing. A cashier pressing send-to-kitchen could never have reached a KDS screen.

Every component test passed. The cross-language socket test passed, and it was a genuine socket — it just started the server itself, because it was the only thing that could.

**This is the second milestone with the same shape.** Milestone 1 shipped seven bounded contexts, all tested, none mounted: `main.go` served `/health` and nothing else, so no context was reachable over HTTP by any caller. That was found the same way — by trying to use the thing rather than by any test.

### Root cause

**Acceptance was verified against harnesses, not against what ships.** Each track's Definition of Done was scoped to its own directory, and every track satisfied it. Nobody owned the seams *between* directories, so the seams were never built, and no gate was positioned to notice: a verifier checks the track in front of it, and every track was individually correct.

The harness made it worse rather than better. A test that constructs its own server proves the protocol and hides the absence of a launcher. It is genuinely valuable — it caught two real interop breaks — but it answers "do these two pieces agree?" and was read as answering "does this work?"

Contributing: the orchestrator relayed both gaps from the builder's own report ("not wired into `apps/pos/src-tauri`") into the milestone report as remaining work, rather than recognising that a deliverable with no caller is not delivered.

### Rules adopted

1. **An acceptance run exercises shipped binaries end to end. Harnesses do not count as evidence for it.** If the only thing that starts a component is a test, the component is not wired, whatever its tests say. — `CLAUDE.md`
2. **Every new component names its caller before its track closes.** A builder reporting "not wired into X yet" is reporting an unfinished deliverable, not a follow-up, unless X is explicitly out of the milestone. The orchestrator treats that sentence as a gate failure. — `.claude/commands/milestone.md`
3. **The runbook is written during the milestone, not after it.** "What do I type to see this work?" is the cheapest possible integration test, and both times it was asked, it found in minutes what a full milestone of green suites had not. — `docs/DEV_SETUP.md`

### Note

The `m2-complete` annotation already recorded item 1 as outstanding and the build as not shippable, so the tag did not overclaim. But it described the gap as *unmeasured latency*, when in fact the path being measured did not exist. Recording a criterion as unmet is not the same as knowing why.
