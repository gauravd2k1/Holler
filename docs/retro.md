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

---

## 2026-08-11 — The KDS crashed in every browser while every test passed

**Severity:** medium (caught on first real-device use; nothing shipped). The third instance of one pattern in as many days.

### What happened

With the LAN wiring finally in place, the KDS was opened in a real browser — on the laptop and on a phone — and crashed on mount:

```
Uncaught TypeError: Illegal invocation
    at ConnectionController.start (connectionController.ts:64)
```

The cause is one line in the constructor:

```ts
this.setIntervalFn = deps.setIntervalFn ?? setInterval;
```

`setInterval` in a browser is a method on the global object and checks its receiver. Storing the bare global on an instance field detaches it, so `this.setIntervalFn(...)` invoked it with the `ConnectionController` as `this`, and the browser rejected it. `clearInterval` had the identical defect, which would have fired on `stop()`.

**Node's timers are plain functions and do not care.** So the KDS unit suite passed, and so did the cross-language socket harness — which is a genuine WebSocket against a genuine compiled Rust server, and still ran the client under Node.

### Root cause

**Every automated check for this app runs in Node; the app runs in a browser.** `vitest` uses jsdom, whose timers are Node's. The `kds-lan` harness imports the real client modules into a Node process. Both are valuable — the harness caught two real interop breaks — but neither can observe a browser-only failure mode, and nothing in the pipeline could.

The existing tests were also structurally blind here: `makeController` never injects `setIntervalFn`, so the default path *was* exercised — but under `vi.useFakeTimers()`, which replaces the global with a plain function and erases the receiver check that constitutes the bug.

This is the same shape as the two entries above. Something passed, and the passing thing was not the thing that ships. Wiring, then binaries, now runtime.

### What went right

The fix is one `.bind(globalThis)` on each line, and it is now guarded by a test that installs a receiver-checking global to reproduce browser semantics under Node. That test was verified to fail — with the exact `TypeError: Illegal invocation` — before the fix was restored.

### Rules adopted

1. **A Node-based harness proves protocol, not browser runtime.** The `kds-lan` suite is a real WebSocket against a real compiled Rust server and it is worth having — it caught two genuine interop breaks. But it answers "do these two ends agree on the wire?" and cannot answer "does this app run?". Never let the first stand in for the second.
2. **Every UI app carries one real-browser smoke test in CI.** Headless Chromium, mount the app, assert it reaches its working state with no console errors. One test, permanently, per app — this class of failure is invisible to every other check we run. — `.github/workflows/ci.yml`
3. **A test report states the runtime it ran in.** "26 tests passed" concealed the fact that all 26 ran under Node against a browser app. A report that says *jsdom/Node* invites the question that was never asked. Builders and verifiers name the environment, not just the count. — `.claude/commands/milestone.md`
4. **Never store a bare global builtin on an object field.** `setTimeout`, `setInterval`, `fetch`, `WebSocket`, `crypto.*` and friends are receiver-bound in browsers. Bind at capture, or call them free. — `CLAUDE.md`
   **No linter enforces this.** `@typescript-eslint/unbound-method` was added for it and provably does not catch it: reverting the real bug and re-running ESLint produces zero errors, because `lib.dom.d.ts` declares these as global *function declarations*, not interface-bound methods, so there is no receiver for the rule to reason about. The rule is kept for genuine unbound class-method mistakes; the guard for *this* class of bug is rule 2's browser smoke test and nothing else. Recorded because the first attempt shipped config comments asserting the opposite, which is worse than no guard — it tells the next reader they are covered.
5. **When a test injects a fake for a builtin, the default path stays untested.** Fake-timer helpers replace the very semantics a default-path test would be checking. If a default matters, assert it against something that behaves like the real host.

---

## 2026-08-11 — "Passes the PowerShell parser" was not the same as "runs"

**Severity:** low (one broken dev script, caught immediately). Recorded because the mistake is the same one as the entry above, made while writing up the entry above.

### What happened

`run-dev.ps1` was edited to close DEV_SETUP gap 4. It was validated with:

```powershell
[System.Management.Automation.Language.Parser]::ParseFile(...)
```

which reported no errors, and the change was committed as verified. On the user's machine `powershell.exe -File` failed immediately:

```
The string is missing the terminator: ".
At apps\pos\run-dev.ps1:104 char:105
```

### Root cause

Two things compounded.

**The content.** The edits introduced em-dashes (U+2014). One landed inside a double-quoted string on line 94. Neither `.ps1` in this repo carries a UTF-8 BOM, so Windows PowerShell 5.1 reads them as Windows-1252, and the em-dash's three UTF-8 bytes `E2 80 94` decode to `â`, `€`, `”`. That third character is U+201D, which **PowerShell accepts as a double-quote delimiter** — so the string closed early, the rest of the file parsed as code, and the parser gave up 10 lines later. The reported line is where it failed, not where it broke.

**The check.** `Parser::ParseFile` decodes the file the way .NET chooses, not the way `powershell.exe -File` does. It read the same bytes as UTF-8, saw a valid em-dash inside a string, and passed. The validation and the failure disagreed because they were not reading the same file in the same way.

Worse: the BOM that would have prevented this appeared in the working tree, and I discarded it as stray debris without asking what had put it there. It was load-bearing.

### Root cause behind the root cause

This is the same error as the browser entry above, in a different language: **a check that resembles the real thing was accepted in place of the real thing.** There, Node timers stood in for browser timers. Here, a .NET parser API stood in for the interpreter that actually runs the script. Both passed. Neither ran what ships.

### Rules adopted

1. **Parser-validation is not execution-validation.** A syntax check that does not go through the real interpreter, with the real file-reading path, proves nothing about whether the thing runs. For a script, invoke it — with arguments that make it exit early and harmlessly if it has side effects (`-EnvFile <nonexistent>` here; an unknown parameter name for `dev-bootstrap.ps1`, which forces a binding failure *after* the whole file has parsed).
2. **Keep `.ps1` files pure ASCII.** Windows PowerShell 5.1 reads BOM-less files as the system ANSI codepage, and a mojibaked smart quote is a string delimiter. Both scripts are now ASCII-only, which makes the encoding question moot rather than answered.
3. **Do not discard an unexplained file change as debris.** A BOM appearing in a `.ps1` is a signal about how something else reads that file. Find out what wrote it before reverting it.

### Note

The fix was falsified before being accepted: restoring one em-dash into a scratch copy reproduced the user's error at the same line with the same message, and the ASCII version executed to its intended early exit. That is the standard this entry exists to enforce.

---

## 2026-08-14 — Three gates that could not fail, and a seam the orchestrator built

Nine tracks landed in one session (T7c, T13 retry, T9 + retry, T10, T11, T11b + retry, contracts 0.4.4, a CI fix). Eight passed their gate; one failed and was fixed. The tracks are not what this entry is about.

Three separate things this project had been counting as evidence turned out to be incapable of producing evidence. They were found on the same day, by different gates, and they are the same failure.

### 1. The backend suite reported green while skipping 29 tests

`HOLLER_TEST_DATABASE_URL` unset caused every Postgres-backed test to `t.Skip`. `go test ./...` exited 0 and printed `ok` for all 12 packages. Among the skipped: `TestBuildRouter_SyncConfigEndToEnd`, which **M2 acceptance item 4 names by hand** as a test that had never run.

M2 recorded "a skip is not a pass" and moved on. The mechanism that produces the skip was left in place, so it produced it again.

### 2. CI had institutionalised the same thing

`.github/workflows/ci.yml` ran `cd backend && go test ./...` on `ubuntu-latest` with **no Postgres service and no `HOLLER_TEST_DATABASE_URL`**. So the pipeline had never once executed a Postgres-backed test, and reported the backend green on every push. The CI story in `CLAUDE.md` — "lint, format, unit, integration" — was partly fictional for the backend, and had been since the job was written.

### 3. The e2e harness had not compiled for two features

`docs/RESUME.md` recorded "the e2e harness (54 scenarios, 0 invariant violations) were green at their last run and untouched since". The harness had not built since ADR-017 device enrollment and M3 Track B landed: `server::start` gained a required `DeviceTokenVerifier`, `MenuItem` gained `tax_profile_id`, `NewOrderItemRequest` gained `modifiers`. Nobody had run it, so nobody saw it fail to build — and "no invariant violations" is what a harness that cannot start reports.

The figure was repeated in status summaries all session, including by this orchestrator, before anyone tried to run it.

### Root cause behind the root cause

The existing entries in this file say it already: *a check that resembles the real thing was accepted in place of the real thing.* This is its terminal form. **A check that does not execute reports the same thing as a check that executes and finds nothing.** Zero failures is the output of a passing suite, a skipped suite, and a suite that cannot compile — and nothing in the output distinguishes them.

Every one of the three had a visible tell that was never read: a skip count, an absent service block, a build error nobody triggered. None was hidden. They were unlooked-at, because the summary line said what everyone wanted.

### 4. The orchestrator built the seam it kept warning about

T11b (scoped to `tests/`) and the T9 retry (scoped to `edge/database` + `apps/pos`) were dispatched in parallel, each told to stay in its directory. The directory partition was clean and both tracks honoured it. But the harness in `tests/` is a **consumer** of `apps/pos`, and the T9 retry added a parameter to `record_payment_impl`. T11b committed on top of that change while verifying against a worktree taken before it, so **its own commit did not build** — re-creating, within the same session, the exact bit-rot it had just been dispatched to fix.

Directory partitioning prevents *edit* conflicts. It does nothing about *interface* conflicts, and the orchestrator applied it as though it did — while telling every verifier that session that "the seam is where the defects are".

### Rules adopted

1. **A suite that cannot fail must be made to fail loudly, not documented.** An unset `HOLLER_TEST_DATABASE_URL` is now a `t.Fatal` via `backend/internal/platform/testdb`, with `HOLLER_SKIP_PG_TESTS=1` as a deliberate opt-in. Noting a hazard in a doc does not remove it; the two M2 acceptance failures were both preceded by the hazard being written down.
2. **Assert the skip count, not just the exit code.** The backend CI job now fails if any test skips. An exit code cannot distinguish "ran and passed" from "did not run", so something else must.
3. **Every green number carries the command that produced it and when.** "54 scenarios, 0 violations" survived because it was quoted without a date or a command. A count with no reproduction instruction is a rumour.
4. **Partition parallel tracks by interface, not only by directory.** Before dispatching concurrent tracks, ask which of them *calls* code another one *owns*. If one does, either serialize them or require the consumer to rebuild against the producer's final commit before its gate.
5. **A track's own verification must run against the tree it commits to.** Verifying on a scratch worktree taken before a sibling merge proves the code worked somewhere that no longer exists.
6. **An invariant nobody has watched fail is not a gate.** Invariant 10 (`payment_settlement`) reported `checked && passed` on 54 scenarios before anyone established it *could* go red. It was then falsified deliberately — 12/12 scenarios red, each naming itself with exact paise — and only then counted. Invariant 9 was falsified when written; 10 was not, and the difference was invisible in the report.

### Note

The pattern that caught all three was the same one this file already recommends: ask a verifier to falsify a property the builder did not target. The builder of the harness fix could not see that the harness had never built, because the builder ran it the way it had always been run. The gate ran it against `main`.

---

## 2026-08-15 — One contract field broke five consumers, and the migration that added it was inert

Contracts 0.4.5 added four things. Verifying them looked thorough and was not.

### What was verified, and what that proved

The migrations were checked by applying every `*.sql` file directly into a fresh SQLite database and a recreated Postgres schema, then **falsifying each new constraint**: `UPDATE`/`DELETE` on `payment` rejected by name while a reversal insert still succeeded, `cash_shift` close still permitted, `print_job` rejecting neither-set/both-set/duplicates. All of it passed. All of it was real.

None of it proved the edge would ever run those files.

`edge/database/src/migrations.rs` registered migrations only through `0008`. The three new files sat on disk, **unregistered and therefore inert**, through the whole release. The payment append-only triggers did not exist in any real edge database. Neither did `print_job.invoice_id` or `menu_item.hsn_sac`. It was found by the next track, which needed the column and discovered it was not there.

**The artifact was verified; the path was not.** This is the same error the 2026-08-14 entry describes — a check that resembles the real thing accepted in place of the real thing — committed one day after that entry was written, by the author of that entry.

### The consequence that made it worse

Because the constraint was inert, verifying it in a scratch database could not fail. The partial index

```sql
CREATE UNIQUE INDEX idx_print_job_kot_printer
  ON print_job(kot_id, printer_id) WHERE kot_id IS NOT NULL;
```

was necessary and correct: SQLite treats NULLs as distinct in a UNIQUE index, so once `kot_id` became nullable an unqualified index would have permitted unlimited `(NULL, printer)` rows and silently lost idempotency for invoice jobs. But SQLite also requires an `ON CONFLICT` **target to match a partial index's WHERE clause**, and `edge/printer/src/spool.rs` still said `ON CONFLICT(kot_id, printer_id)`. Six spool tests broke — the moment the migration was registered, not when it was written. The index was reasoned about carefully in isolation and its only consumer was never opened.

### Five consumers, one field

`menu_item.hsn_sac` is one nullable column. Adding it broke, in sequence:

| Consumer | How it broke | Caught by |
|---|---|---|
| `edge/database` migrations | file never registered | the next track needing the column |
| `edge/sync` | `MenuItem` struct literal | a build, after the fix above |
| `edge/device` | `MenuItem` struct literal | running the crate |
| `apps/pos/src-tauri` | three `MenuItem` literals | the final sweep |
| `edge/printer` | partial-index `ON CONFLICT` (from `print_job`, same release) | running the crate |

Plus `edge/sync` had *already* been broken since 0.4.2 by `tax_profile_id`, unnoticed for two releases.

Every consumer builds `MenuItem` by struct literal, so a field addition breaks all of them simultaneously. **That is the correct design** — the compile error is what should have caught `tax_profile_id` at 0.4.2, and `..Default::default()` would have silently absorbed it instead. The defect is not the literals. It is that nothing tells the author of a contract change which crates they have just broken, and nothing runs those crates.

### Root cause behind the root cause

Contract changes were being treated as edits to `packages/contracts/`. They are not. **A contract change is a multi-crate change**, and the contract directory is only the first file touched. The frozen-contract discipline (ADR-008) makes the *shape* review rigorous while leaving the *blast radius* entirely unexamined — the rubric asks about authority, uniqueness and credential material, and never asks "what calls this?"

### Rules adopted

1. **Verify the runner, not the file.** A migration is verified when a database built by the application's own migration path is inspected and shows the change. Applying SQL by hand proves the SQL parses. Query `sqlite_master` (or the equivalent) through the real code path.
2. **A contract change is not complete until every consumer builds and its tests run.** Enumerate consumers before writing the migration — `grep` the type name across the workspace — and list them in the ADR. The 0.4.5 addendum listed none, which is why five were found one at a time over several hours.
3. **Changing a constraint means opening its consumers.** A partial index, a CHECK, a NOT NULL: find the queries that write the table and read them. `ON CONFLICT` in particular must match a partial index's predicate, and no test in the *owning* crate will tell you.
4. **Extend the contract review rubric with blast radius.** Alongside authority and uniqueness, the self-review must answer: which crates construct or query this shape, and have they been built? A rubric that only examines the shape passes a change that breaks five callers.
5. **Fixture values are behaviour when a guard reads them.** `hsn_sac: None` in a fixture is not a placeholder once issuance rejects a NULL code — it turns every billing test into a rejection test. Give fixtures deliberate, plausible values.

### Note

The count assertion that was supposed to catch the unregistered migration was `assert_eq!(MIGRATIONS.len(), 11)` — a hand-maintained literal, which catches nothing that a person bumping it by hand would not also bump. It has been replaced with a symmetric comparison against the contracts directory that **panics with the path if the directory cannot be read**, rather than skipping. A check that silently passes when it cannot find its inputs is the same failure one level up, and this file already records three instances of it.
