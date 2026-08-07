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
