---
description: Execute one Holler milestone autonomously with parallel builders and verification gates
---

Execute milestone $ARGUMENTS end to end. You are the orchestrator: you plan, serialize shared changes, dispatch builders, gate with the verifier, and merge. You do not implement module code yourself.

## 1. Plan
- Read CLAUDE.md, HOLLER_MASTER_PROMPT.md §81 for this milestone's deliverables and EXCLUDES list, and the relevant docs/spec/ files.
- Produce a task graph: tasks, owning agent type (go-builder / pos-builder / rust-edge-builder), spec file(s) per task, owned directories per task, and dependency order (independent vs sequential).
- Print the task graph as your first output.

## 2. Serialized shared work (orchestrator only)
- If the milestone needs contract additions or shared migrations: propose them, show me the diff, and WAIT for my approval before applying. Record semantic changes in an ADR. This is the only step that may pause for input.
- Apply approved shared changes and commit before dispatching any builder.

## 3. Dispatch builders
- Max 3 concurrent subagents (hardware limit).
- Each dispatch includes: task description, assigned spec file path(s), owned directory list, the milestone EXCLUDES list verbatim, and the instruction to report in its defined format.
- Independent tasks run in parallel; dependent tasks wait for their dependency's PASS.

## 4. Verification gate
- When a builder reports done, dispatch verifier with the module name and claimed changed paths.
- On FAIL: send the verifier's verdict back to the SAME builder for one retry. On second FAIL: stop that track and surface it to me with both reports.
- A builder's own success claim is never sufficient — only verifier PASS gates a merge.
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
