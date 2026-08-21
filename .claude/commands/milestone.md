---
description: Execute one Holler milestone autonomously with parallel builders and verification gates
---

Execute milestone $ARGUMENTS end to end. You are the orchestrator: you plan, serialize shared changes, dispatch builders, gate with the verifier, and merge. You do not implement module code yourself.

## 0. Update the milestone block FIRST — before reading anything else

Your first act, before planning and before any read of the spec files, is to
make CLAUDE.md say which milestone this is:

1. Write the bare number to `.claude/current-milestone`.
2. Rewrite CLAUDE.md's `## Current milestone:` block — heading, the
   `<!-- MILESTONE-MARKER: n -->` comment, scope, track graph, acceptance
   criteria and the **EXCLUDES list** — for THIS milestone.
3. Move the previous milestone into the "Completed milestones" note, honestly:
   met, code-complete-but-not-accepted, or blocked.
4. Run `node scripts/check-milestone-marker.mjs`. It must pass before you
   dispatch anything.

**Why this is step zero rather than a tidy-up.** CLAUDE.md said
"MILESTONE 2 — Kitchen" for the entire Milestone 3 build. Builder agents load
CLAUDE.md as primary context, so every M3 builder read M2's scope and M2's
EXCLUDES as the authoritative statement of what it could touch — a list that
bars aggregator KOTs and the waiter app and says nothing about billing. It went
unnoticed for a whole milestone, because nothing failed when it went false.
Do not dispatch a builder into a stale block.

## 1. Plan
- Read CLAUDE.md, HOLLER_MASTER_PROMPT.md §81 for this milestone's deliverables and EXCLUDES list, and the relevant docs/spec/ files.
- Produce a task graph: tasks, owning agent type (go-builder / pos-builder / rust-edge-builder), spec file(s) per task, owned directories per task, and dependency order (independent vs sequential).
- Print the task graph as your first output.

## 2. Serialized shared work (orchestrator only)
- If the milestone needs contract additions or shared migrations: propose them, show me the diff, and WAIT for my approval before applying. Record semantic changes in an ADR. This is the only step that may pause for input.
- Apply approved shared changes and commit before dispatching any builder.

## 3. Dispatch builders
- **A brief carries the PURPOSE, not just the task text.** When forwarding a backlog item, forward why it exists, so a builder can tell when the literal instruction falls short of the condition. A Milestone 2 item was closed by satisfying its wording while its purpose went unmet, because the dispatch inherited the entry's framing (docs/retro.md, 2026-08-10).
- Max 3 concurrent subagents (hardware limit).
- Each dispatch includes: task description, assigned spec file path(s), owned directory list, the milestone EXCLUDES list verbatim, and the instruction to report in its defined format.
- Independent tasks run in parallel; dependent tasks wait for their dependency's PASS.

## 4. Verification gate
- When a builder reports done, dispatch verifier with the module name and claimed changed paths.
- On FAIL: send the verifier's verdict back to the SAME builder for one retry. On second FAIL: stop that track and surface it to me with both reports.
- A builder's own success claim is never sufficient — only verifier PASS gates a merge.
- **"Not wired into X yet" is an unfinished deliverable, not a follow-up** — unless X is explicitly out of the milestone. Treat that sentence in a builder report as a gate failure. Two milestones shipped components whose only caller was a test; both were found by a human trying to use the product, not by any suite (docs/retro.md, 2026-08-11).
- **Every test figure must state its runtime environment.** "27 tests passed" concealed that all 27 ran under Node against a browser app. Require "27, vitest/jsdom (Node)" and "1, Playwright/headless Chromium" from builders and verifiers alike.
- **Ask a verifier to JUDGE disclosures, not just record them.** Several of this milestone's most valuable findings came from a verifier being told to rule on a builder's self-declared limitation rather than log it.
- **Falsify before trusting green.** A new test or harness must be shown to fail against the defect it targets — in a scratch copy outside the repo, never by mutating the working tree. Where a track adds its own guard, the verifier falsifies it independently, targeting a different property than the builder did.
- **Commit before the next gate.** Never leave one track's completed work unstaged while another agent runs. Either commit it after its PASS, or `git add` it so it is recoverable from the index. A whole track was once destroyed because it existed only as unstaged changes when another agent touched the tree (see docs/retro.md, 2026-08-07). Unstaged work is unrecoverable; staged or committed work is not.

## 5. Merge and integrate
- Merge PASSed worktrees one at a time. After each merge, run `make test` once; on integration failure, identify the conflicting pair and dispatch a fix to the responsible builder.
- Commit merged work with conventional messages: feat(<context>): <summary>.

## 6. Report (only after all tracks complete or stop)
Use the §95 format exactly:
### Implemented
### Verified   (only commands actually executed)
### Performance
### Remaining
### Next

Do not re-explain the vision. Do not ask permission between steps except at step 2's contract approval.
