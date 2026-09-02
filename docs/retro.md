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

`docs/backlog.md` carried a Milestone 1 item titled **POS cart persistence**. Its text read, in order: the cart lives in browser memory until Send, so a crash mid-order loses it; `add_order_item`/`remove_order_item` return `UNSUPPORTED_DB_OPERATION` at the Tauri layer even though the `edge/database` API exists; **wire them through**; this is about in-progress work on a system whose premise is that the shop floor never loses work.

Milestone 2's POS track was handed that item. It wired both commands through to the real `edge/database` calls, tested them at the Tauri layer, and reported the item closed. That was accurate: the imperative sentence in the entry — "wire them through" — was fully satisfied.

But **no frontend screen calls either command.** The cart still round-trips through one atomic `create_order` at Send. So after the change, a crash mid-order still loses the cart — the exact condition the entry existed to eliminate.

### Root cause

**The entry was written as a task, and the task was smaller than the problem.** Its first and last sentences named the real requirement (survive a crash); its middle sentence named a step toward it (wire the commands). A builder reading it for the actionable instruction found the step, did it correctly, and stopped. Nothing in the entry said the step was insufficient on its own.

Contributing: the orchestrator's dispatch repeated the entry's framing rather than its purpose, so the narrowing was inherited rather than caught at brief-writing time.

This is not a builder error. The builder disclosed precisely what it had and had not done — "no UI screen calls them yet" — in its own report, unprompted. That disclosure is the only reason the gap was caught before the item was marked done.

### What went right

The builder volunteered the limitation instead of reporting a clean close. The verifier, asked to judge the disclosure rather than record it, tested the actual condition and answered plainly: *crash mid-order still loses the cart — item genuinely NOT closed.* An honest builder report plus a verifier briefed to judge intent, not wording, caught something a passing test suite could not.

### Rules adopted

1. **A backlog entry states the condition that makes it closed, not the step someone guessed at.** Where an entry names both, the condition governs. Entries that only name a step get rewritten when they are picked up, before dispatch. — `docs/backlog.md`
2. **Acceptance criteria are observable failures, not implemented APIs.** Milestone 2's acceptance gains: *crash mid-order → the cart survives.* An API that could prevent the loss does not count; the loss not happening counts. — `CLAUDE.md`
3. **A dispatch brief carries the purpose, not just the task text.** When an orchestrator forwards a backlog item, it forwards why the item exists, so a builder can tell when the literal instruction falls short. — `.claude/commands/milestone.md`

### Note

The item is reopened in `docs/backlog.md` with the distinction written into it, so the next reader judges it against the crash rather than the API surface.

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

## 2026-08-20 — A function named for outlet-local time, computing UTC, on a row that already held the timezone

Found during M4 contract planning, not by any test, and not by anyone using the
feature.

`apps/pos/src-tauri/src/commands/billing.rs`:

```rust
/// Outlet-local business day. Truncates the UTC invoice moment to its date
/// part rather than resolving `outlet.timezone` — the same known limitation
/// already disclosed on `edge/database/src/repo.rs`'s display-number reset
/// bucketing ...
fn business_date_from(instant_iso: &str) -> String {
    instant_iso.get(0..10).unwrap_or(instant_iso).to_string()
}
```

The name says outlet-local. The first doc line says outlet-local. The body takes
the first ten characters of a UTC instant.

### Why this is a defect and not a limitation

It was filed as a known limitation, disclosed honestly in two places, and
inherited from an older disclosure on the display-number reset. That framing was
wrong, and the disclosure is what made it comfortable.

In IST the UTC day rolls at **05:30 local**. Every Indian restaurant trading past
midnight — which is most of them — has been assigning invoice numbers and
day-end / cash-shift reconciliation to the wrong business day for the entire
window between local midnight and 05:30. That is not a rounding difference at a
boundary nobody reaches. It is the busiest tail of a dinner service, landing in
the wrong day's books, in shipped M3 code. The M3 milestone record claims
correctness it does not have.

`outlet.timezone` has existed since `packages/contracts/sqlite/0001_init.sql:13`,
defaulting to `Asia/Kolkata`. The data needed to compute this correctly was on
the row the whole time.

### The lesson

**A disclosure is not a severity assessment, and repeating one is not
re-assessing it.** The comment was accurate about the mechanism and silent about
the consequence, so each subsequent reader — including the one who wrote the
second disclosure by pointing at the first — inherited "known limitation" without
anyone recomputing what it cost. Nobody ever multiplied "UTC day" by "IST" by
"restaurants serve dinner late".

Two specific habits fall out:

1. **Name and doc-comment are claims, and a wrong one is worse than none.** A
   function called `business_date_from` with "Outlet-local business day" on the
   first line reads as correct at every call site. Had it been named
   `utc_date_prefix`, its callers would have looked wrong on sight.
2. **Quantify a limitation in the units of the business, once, at the moment you
   disclose it.** "Buckets by UTC day" is a mechanism. "Every bill between
   midnight and 05:30 IST books to the previous day" is a severity — and it would
   never have survived three disclosures unfixed.

### What changed

The corrected definition lands as a schema-level decision in contracts v0.5.0
(ADR-018 §9.2), not as an implementation fix in a later track: `business_date` is
a 0.5.0 column, `stock_balance_snapshot` keys on it, and a day-start time is
outlet config. Settling it after the column froze would have been backwards.

## 2026-08-20 — Naming the class: a claim that nothing verifies

Two instances surfaced within one week, and they are the same defect wearing
different clothes.

- **`business_date_from`** — named "business date", doc-commented "Outlet-local
  business day", computing the first ten characters of a UTC instant.
- **The migration symmetry check** — named
  `every_contract_sqlite_file_is_registered_and_vice_versa`, reading only the
  SQLite directory, and therefore structurally unable to notice that
  `stock_balance_snapshot` had no PostgreSQL counterpart. It had been green for
  every one of `invoice_sequence`'s versions without ever having looked.

### The class

**A NAME OR COMMENT THAT ASSERTS A PROPERTY NOTHING VERIFIES.**

It is worse than making no claim at all, and the reason is not aesthetic: **it
stops the next reader from checking.** An unnamed, undocumented behaviour gets
read. A function called `business_date_from` with "Outlet-local business day"
on the first line gets *trusted*, at every call site, by every reader, forever.
The claim consumes the scrutiny that would have found the defect.

That is why both of these survived multiple readings by people who were
specifically looking for problems. Nobody re-derived what the comment asserted,
because the comment had already answered the question.

A third instance appeared while writing the guard for the first two: the new
append-only lint attributed PostgreSQL trigger comments — which necessarily sit
*after* their table, since plpgsql needs the table to exist — to whichever table
happened to be defined next. It failed on `stock_deduction_gap`, a table with no
immutability claim at all. The guard was wrong in exactly the way the things it
guards were wrong, on its first run.

### What changes

**§66 now covers meta-tests and guards, not only feature tests.** Every lint,
symmetry check, ratchet and invariant is falsified before it is trusted: made to
fail on purpose, and observed failing for the stated reason. A guard nobody has
watched fail is not a guard — the rule already applied to invariants, and the
only reason it had not been applied to guards is that guards feel like
infrastructure rather than assertions. They are assertions.

Three guards landed with this milestone, each watched to fail first:
`every_single_store_migration_is_declared`,
`every_append_only_claim_has_a_trigger_behind_it`, and
`postgres_db_side_uuid_defaults_only_ever_decrease`. The second one found two
unenforced `APPEND-ONLY` comments (`audit_event`, `cash_movement`) and one
wording defect (`invoice`) on the run that made it pass, which is the argument
for the class being real rather than two coincidences.

### The cheap habit that would have caught all of them

When writing a name or a comment that asserts a property, ask: *what would fail
if this were false?* If the answer is "nothing", either write the check or
weaken the claim to what is actually true. "Truncates to the UTC date" would
have been correct, ugly, and impossible to misread.

## 2026-08-21 — Every Milestone 3 builder was handed Milestone 2's scope

Found during M4 planning, while updating CLAUDE.md for the new milestone.

`CLAUDE.md` line 100 read `## Current milestone: MILESTONE 2 — Kitchen` — with
M2's scope, M2's acceptance criteria and M2's EXCLUDES list beneath it —
throughout the whole of the Milestone 3 build.

Builder agents load CLAUDE.md as their primary context. So every M3 builder
received, as the authoritative statement of what it was allowed to touch, a
scope line about KOTs and station routing and an EXCLUDES list barring
aggregator KOTs, expo polish, label printers and the waiter app. Billing was
not mentioned in either. The tax engine, the invoice, split payments and the
cash shift were all built against a block describing a different milestone.

### Why nothing caught it

Nothing was checking. The block is prose in a file nobody diffs for meaning,
and its wrongness produced no failure: builders were also given their own task
briefs and spec files, which were correct, so the work landed correctly anyway.
The stale block was a loaded gun that happened not to go off — an M3 builder
that consulted EXCLUDES to decide whether something was in scope would have got
a wrong answer, and we would have no way of knowing whether one did.

This is the same class as the two entries above it: **a claim that nothing
verifies**. It is the third instance in a week, and the most consequential,
because this claim is the one every builder reads first.

### The fix is structural, not a careful edit

A careful edit fixes today and guarantees M5 repeats it, because the failure
mode is forgetting, and being more careful is not a mechanism.

- `.claude/current-milestone` holds the number, authoritatively.
- `scripts/check-milestone-marker.mjs` fails the build when CLAUDE.md's heading
  or its `MILESTONE-MARKER` comment disagrees with that file. Falsified before
  being trusted: set the marker to 5, watched it fail naming both values, set
  it back.
- `/milestone <n>` now updates the block as **step 0**, before it reads
  anything or dispatches anyone.

### The lesson, stated for the next reader

**Context handed to an agent is production input, and it decays like code.**
We version, review and drift-check the contracts an agent is given. The prose
that tells it what milestone it is in — what it may build and what it must not
touch — had no version, no review and no check, and it was wrong for months.
If a document is load-bearing for a machine, treat it like code: give it a
single source of truth and something that fails when the copies disagree.

## 2026-08-21 — Two agents owned one crate, and the commit was the least of it

T0b (seeding) and T2 (deduction fixes) were dispatched concurrently. Both owned
`edge/database`. Both edited `repo.rs`. By the time it surfaced — as a
`git status` showing one file with two tracks' work interleaved — every option
was bad: split by file and the tree does not compile; revert one and live
in-progress work is destroyed; commit together and the history stops
distinguishing them.

Committing together was correct. Manufacturing a clean history by rewriting an
agent's work to suit it would have been worse, and the commit message says
plainly what happened.

**But the commit was never the problem. The dispatch was.**

Disjoint directory ownership is already the rule, and it came out of the
worktree data-loss incident (2026-08-07). It is a **dispatch-time** decision:
the moment two briefs name the same directory, the outcome is determined, and
nothing later recovers it. What made it easy to miss here is that the two tasks
*sounded* disjoint — "seed some data" and "fix three defects" — while sharing a
repository module neither brief mentioned.

**The check is on the owned-directory list, not on the task description.** Two
briefs naming the same directory are a conflict however unrelated the work
reads. If the work genuinely must share a directory, serialise it: dispatch,
wait, dispatch again.


## 2026-08-23 — The gap detector was the outage

**Severity:** high (caught before the milestone closed, never ran at an outlet).

M4's T4 added `entry_seq` contiguity checking to the cloud's ledger ingest, so a
lost stream row would be visible instead of vanishing. It shipped rejecting any
`entry_seq` beyond the outlet's high-water mark.

That turns one lost row into a permanent, silent outage. Entry 5 is refused;
6, 7, 8 are refused behind it, forever; and because nothing downstream can tell
"quiet outlet" from "replay wedged since Tuesday", nobody finds out. The
mechanism added to make loss visible was the mechanism that hid it — and it
would have hidden far more than the single row that triggered it.

The fix: record the hole (`ledger_replay_gap`, with `resolved_at` so a hole
that later fills stops claiming a loss, and a UNIQUE span key so one hole stays
one row), accept the entry, keep going.

### The lesson, stated for the next reader

**A detector that blocks is not a detector.** When a check finds a violation it
has two jobs that look like one: make it visible, and decide what happens next.
Rejecting is the reflex — it is what validation does — but for a *stream*, a
rejection is not a rejection of one row, it is a rejection of every row behind
it. Ask which of "loud" and "blocking" the check actually needs, and note that
blocking is almost never the part anyone wanted.

**And check the mirror image.** The same defect exists at the other end of any
replayed stream: a sender that retries one permanently-rejected row forever
strands everything behind it just as completely. Both ends were bounded here —
the cloud records and continues, the edge gives up on the entry rather than the
stream — because fixing only the end you were looking at leaves the outage
intact and moves it one hop.

### Postscript, same day: the harness did it too

The test harness written to prove the fix used `Server::recv()` with no
deadline. A script one request longer than the requests that actually arrived
blocked the responder thread forever, `join()` with it, and the run had to be
killed at the outer timeout — twice, since the abandoned process then held a
lock on its own binary and the next build failed to link.

So: a replay mechanism that blocks forever waiting for something that never
arrives, fixed by someone who then built a test harness that blocks forever
waiting for something that never arrives. **Same shape, one layer up.** Third
application of "check the mirror image" this week, and the first one aimed at
the checking code rather than the code under check.

**A test that hangs is worse than a test that fails.** A failure names the
problem in under a second; a hang names nothing and costs the full timeout
every iteration, and it degrades the environment around it. Every receive in
`edge/sync/tests` now carries a deadline, so a wrong expectation surfaces as a
fast, legible failure. Removing the cause fixes one test; a deadline removes
the class.

## 2026-08-23 — An unnamed workaround hid its defect for a milestone

`repo::update_sync_cursor` treated `Option<&str>` as clear-on-`None`, so the
first failed sync attempt after a successful push wiped `last_pushed_outbox_id`
to NULL. `pull_and_apply_config` had a small piece of code that read the
existing value back out and passed it in unchanged — a workaround, correct,
undocumented, and never described as one.

Nothing broke, because outbox resumption keys off `published_at` and no reader
of that column exists. The column simply stopped meaning anything, and stayed
that way.

### The class

**An unnamed workaround hides the defect it works around, twice.**

First, it makes the defect *harmless* — and harmless defects are never fixed,
because nothing ever fails to prompt anyone. The workaround is what buys the
silence.

Second, the workaround is itself a trap. It reads as redundant code. A future
reader tidying away that pointless read-it-back silently reintroduces the bug,
and now there is no workaround and still no failure — just a column quietly
meaning something different from what its name says.

The rule: **when you route around a defect, name it in place.** A comment
saying *"this exists because X is broken"* costs one line, keeps the defect
visible so it can be fixed, and stops the next reader from deleting the only
thing holding the behaviour together. If it is not worth a line of explanation,
fix the defect instead — that is usually the cheaper of the two anyway, as it
was here (`COALESCE(?1, last_pushed_outbox_id)`, and the workaround deleted).

## 2026-08-23 — A red formatter withdrew every Rust test verdict for four pushes

**Severity:** high (no defect shipped; four pushes' worth of test results never existed).

M4 acceptance criterion 1 was recorded UNPROVEN. The natural reading is that
the test ran and failed. It had never run.

The `edge` CI job was `fmt → clippy → test`, three steps of one job. Steps in a
GitHub Actions job are fail-fast, so `cargo fmt --check` failing did not report
a formatting problem — it **withdrew the verdict from every Rust test behind
it**. Three of the four edge crates had simply never been formatted, so the job
had been stopping at step one across `6f72ba6..8a2819e`. The run showed one red
job, which is what it would have shown if the tests had failed, so nothing about
the display distinguished "style broke" from "the code is wrong" from "nothing
ran".

Clearing `fmt` was not enough: `edge/printer` also failed `clippy -D warnings`
(`manual_range_contains`, present since the file landed in `4a43c2c`). **Two
unrelated breakages, one hiding behind the other**, both invisible, and fixing
only the first would have moved the red one step right with the tests still
unrun.

### The lesson, stated for the next reader

**A fail-fast job makes every later step unobservable when an earlier one
breaks.** Independent checks queued in sequence are not "a job with several
checks" — they are a chain in which only the first failure is ever reported.
Style checks are the common offender because they are cheap, so they get put
first, so they are the ones standing in front of everything expensive.

Style (fmt, clippy, vet, lint, typecheck) now lives in `*-style` jobs beside
the test jobs, never in front of them. Everything still gates the merge:
making style non-blocking would have "fixed" the masking by letting the style
rot, and the goal was never that formatting matters less — it is that a broken
formatter stops deciding whether anyone finds out the tests passed.

**When you add a check, ask what it makes invisible if it fails.** That
question is now written at the top of `ci.yml`.

### Two smaller rules that fell out

**Pin the formatter.** `rust-toolchain.toml` pins the channel, so rustfmt's
output cannot change under CI on a schedule nobody controls and turn a
untouched repo red at the first step of a job whose real work is tests.

**Install the CLI that reads your own CI.** The deeper cause is that nobody
could see the Actions tab from where the work happened, so every push was
fire-and-forget. Splitting jobs makes a failure legible; it does not make
anyone look at it. `gh run list` after a push, reported in the same breath as
the commit, is a five-minute install against a failure mode that cost a day.

## 2026-08-23 — The same mirror image, one layer in: a healed hole that could never heal

**Severity:** high (caught before the milestone closed; `resolved_at` was
unreachable in shipped code).

Fourth instance this week of "check the mirror image", and the first where the
defect was inside the *fix* for an earlier instance of itself.

0.5.8 corrected the blocking detector (see *The gap detector was the outage*,
above): a hole is recorded in `ledger_replay_gap` and the entry is accepted.
`resolved_at` was added in the same change, because a hole that later fills is
not a loss. It could never fire.

`checkContiguity` refused any `entry_seq` at or below the outlet's high-water
mark as a reused mark. But **"below the cursor" and "already taken" are
different questions**, and they differ in exactly one case: the late arrival
that fills a recorded hole. Record 1, then 3 — cursor 3, hole open at 2. Then 2
arrives, and is refused, because 2 ≤ 3. The blocking detector, one layer
further in, inside the code written to remove blocking detectors.

The comment above the function asserted the refusal was "unreachable through
the edge's own path". It reasoned correctly that the edge never *reuses* a
mark, and then treated that as proof that nothing below the cursor could be
legitimate. The reasoning was sound and the conclusion was wrong, which is the
worst combination a comment can have: it reads as having been thought about.

### The distinction was already written down, in dead code

`Repository.GetLedgerEntryBySeq` was declared, implemented, and called by
nothing. Its doc comment: *"so a replayed envelope for an already-ingested
entry_seq can be told apart from a genuine conflicting write."* That is exactly
the distinction the caller was missing. Someone saw it, named it, built the
tool for it, and never wired it up.

**An unused method whose doc comment describes a distinction the callers do not
make is a bug report.** Not dead weight to tidy away — evidence that the shape
was understood and the wiring was forgotten. Deleting it silently would have
removed the only written trace of the missing idea.

### And: one stream tested, one stream trusted

`checkContiguity` is shared by both ranged streams, so the defect was never
ledger-specific. It survived because only the ledger side had tests at all —
and 0.5.8 minted the two counters independently *precisely so the streams could
diverge*. **A shared function tested through one caller has a reputation it has
not earned.** Both streams are covered now.

## 2026-08-23 — A test that was green in CI and red for every human

**Severity:** medium (no product defect; a whole milestone of misleading local
failures).

`TestBuildRouter_SyncConfigEndToEnd` passed on a fresh PostgreSQL and failed on
every subsequent run against the same one, with a duplicate key on
`app_user_pkey`. CI never saw it, because CI's postgres service container is
new on every run. Locally it failed on the second `go test` and read as a
regression in whatever had just been changed — it cost this session real time
before being traced.

The cause is a *correct* change breaking a fixture in silence. The test used
fixed ids plus a `t.Cleanup` deleting the fixture in FK-safe order, first
statement `DELETE FROM audit_event`. Contracts 0.5.0 then made `audit_event`
append-only — rightly — so that statement became one the database refuses
forever. Every `Exec` in the block discarded its error, so the chain stopped at
statement one and said nothing; `audit_event` went on pinning `app_user`
through `actor_user_id`, and the fixed `userID` collided on the next run.

### The lesson, stated for the next reader

**A test whose correctness depends on a clean database passes forever in CI and
fails for every human.** The freshness CI provides for free is a precondition
nobody wrote down, so the test's real requirements were invisible until someone
ran it twice. Anything that touches a persistent store must either mint unique
ids or make its own isolated database — never assume the state it starts from.
The criterion 6 test added the same day creates and drops a database per test
for exactly this reason.

**A cleanup that swallows its errors is worse than no cleanup**: it reads as
tidiness that is happening. The repair here was to mint unique ids and delete
the cleanup rather than fix it — unique ids remove the reason it existed, and
no cleanup can ever delete an append-only table.

**And the shape underneath, which this repo keeps paying for:** a guarantee was
added on one side of a boundary while a fixture on the other side still assumed
the old rule. The guarantee was right. What made it expensive was the discarded
error that would have said so on day one.

## 2026-08-24 — The falsification pass failed on the harness, not the assertion

**Severity:** medium (no product defect; the criterion-6 test could not be
falsified on demand, which is the only thing that makes it evidence).

Criterion 6's test was green. To trust it, the round trip was broken
deliberately — `entry.Note` replaced with a typed nil in the cloud's INSERT, so
the row stored would differ from the row sent by exactly one field. The expected
result was the storage-fidelity byte-compare failing and naming `note`.

What happened instead was `os error 32` — the process cannot access the file
because it is being used by another process — while spawning `cmd/api`.

Both tests in the target call `start_cloud`, which ran `go build -o
target/holler-api-e2e.exe` and then spawned it. Cargo runs the tests on parallel
threads of one process, so two builds wrote the same output path while a third
thread was executing it. **The race was invisible for as long as the Go sources
did not change**: `go build` is a no-op against a warm cache, the write never
happens, and the collision cannot occur. The one action that makes it fire is
editing Go — which is to say, the falsification pass itself. A test harness that
breaks precisely when you try to falsify it is indistinguishable from a test
that cannot be falsified.

`OnceLock::get_or_init` now serialises the build: it happens once per test
binary, every other thread blocks until it finishes, and afterwards each test
spawns an executable nobody is still writing. Concurrent reads of one exe were
always fine; concurrent writes were never allowed.

With that fixed the falsification landed as designed. The storage comparison
failed, printed both objects, and named `note` as the difference — while the
201-echo comparison **passed**, because the handler echoes the entry it was
handed and never consults the database. That is the whole argument for keeping
two checks: one field, dropped server-side after the echo, is invisible to wire
fidelity and caught by storage fidelity. Had the test asserted only the echo, a
GST-relevant column could go missing with a green tick.

### The lessons, stated for the next reader

**Run the falsification pass. Then check what actually failed.** A red test
during falsification is not the confirmation you are looking for — the failure
has to be the assertion under test, at the field you broke. Here the first red
was the harness, and stopping at "it went red" would have recorded a proof that
had not happened.

**A `--all-targets` clippy does not lint a gated target.** `required-features`
hides the target from the style job as thoroughly as from `cargo test`, so the
criterion-6 test would have compiled for the first time inside the acceptance
job, where a lint error reads as a failed acceptance criterion. The style job
now passes `--features cloud-e2e`; `scripts/check-gated-tests.mjs` was already
guarding the same blindness one job over, on the test side.

---

## 2026-08-27 — Twenty minutes of manual clicking found what every suite passes over

**Severity:** medium. Six defects, none caught by any test, two of them blocking the acceptance criterion they sat behind.

### What happened

M4 acceptance criterion 4 — "an ingredient crossing its reorder level is visible to a human on the POS" — had stood at *EDGE MET / SURFACE UNOBSERVED* for days. The component was mounted on the right screens, the route was registered, the pure logic was unit-tested. Closing it needed one person to launch the shipped POS and look.

Launching it took three fixes, none predicted:

1. **The seeded dev principal could not reach the screens.** `StockDeductionGapsScreen` is gated on `inventory.manage` and the count/wastage screens on `inventory.count`; the seeded cashier carried neither, so the criterion's own surface rendered a not-permitted panel.
2. **The Tauri `MenuItem` DTO was missing `tax_profile_id` and `hsn_sac`.** `MenuItemSchema` marks both `.nullable()`, which in Zod is *not* `.optional()` — a missing key fails `.parse` exactly like a wrong type. Every `list_menu_items` call rejected.
3. **A rejected menu query is indistinguishable from a slow one.** `PosScreen` renders "Loading menu…" on `!hydrated`, and `hydrate` only runs on `isSuccess`. The query's `isError` is never surfaced, so the DTO bug presented as a permanent spinner with no error anywhere.

Then, with the POS finally usable, roughly twenty minutes of ordinary clicking found four more: DINE_IN accepts an order with no table selected; the cart does not clear after a successful send and its per-item controls stay enabled on a non-amendable order while Send correctly greys out; "Beverages" appears twice in the category list; and "Kitchen Prep (internal — not sold)" is orderable from the till despite its own name. All four are M1/M2 ordering surface. All four are filed in `docs/backlog.md`, not fixed mid-milestone.

### Root cause

Nobody had driven the ordering screen by hand since M1. Every one of these is invisible to the suites by construction:

- The DTO drift is a **cross-language wire type with no drift check**. The TS↔Go drift suite covers `packages/contracts`; nothing compares the Tauri DTOs in `apps/pos/src-tauri/src/dto.rs` against the Zod schemas they must satisfy. This is the 0.4.6 OpenAPI drift — *the same three `MenuItem` fields* — one layer further out. A shape that crosses a language boundary with no machine check will drift, and it will drift at exactly the field most recently added.
- The permission gap needed a **specific principal reaching a specific screen**; unit tests construct their own principals and never ask whether the seeded one can log in and navigate.
- The four ordering defects are **judgement about what a cashier sees**. A test asserting "DINE_IN order created" passes whether or not a table was chosen; only a person asks who is going to close that table at billing.

The deeper pattern is the one this log keeps recording in new costumes: the repo is well defended against regressions in things someone once looked at, and undefended against things nobody has ever looked at. Criterion 4's evidence chain was component-mounted, route-registered, logic-unit-tested — three true statements that together still did not mean a human could see a low-stock warning.

### Rules

- **Drive the shipped surface by hand at every milestone boundary, not only at the end.** Twenty minutes found six defects here. The cost of not doing it is not the defects — it is that they are found by whoever finally looks, at whatever moment that happens to be, which was the eve of an acceptance sign-off.
- **A wire type that crosses a language boundary needs a machine check, or it is drift waiting to happen.** `dto.rs` ↔ Zod schemas is now the second instance of the identical failure on the identical three fields. Add the check or expect a third.
- **A failed query must not render as a loading state.** Keying a spinner off `!loaded` rather than `isError` converts every failure into an infinite wait, which is the hardest possible symptom to diagnose and the easiest to ship.
- **Nullable is not optional.** In Zod, `.nullable()` requires the key to be *present* and null. A Rust `Option<T>` that is simply absent from the struct fails the parse — it does not serialise as `null`, it does not serialise at all.
- **An unreachable screen is an unmet criterion, no matter what is behind it.** Check that the seeded principal can actually reach every surface a criterion names, before claiming the surface works.

---

## 2026-08-27 — A test assertion that defended a bug for two milestones

**Severity:** medium. Every VOLUME quantity in the POS was displayed 1000x understated, under a green test.

### What happened

`formatMicroQuantity` divided every dimension by 1e6 and labelled VOLUME "ml". The edge stores micro-units of a BASE unit and the base differs -- gram, LITRE, piece -- so a VOLUME value divided by 1e6 is LITRES. Every volume on every stock screen read 1000x low: Soda Water's `litres(5)` reorder level rendered as "5ml", and a 20ml cream deduction rendered as "0.02ml".

It was found by a human reading the low-stock banner and asking why soda water had a five-millilitre reorder point.

The formatter had a test. The test asserted:

    expect(formatMicroQuantity(1_500_000, "VOLUME")).toBe("1.5ml");

1,500,000 micro-litres is 1500ml. The assertion was wrong, and it was wrong in exactly the direction of the defect, so it passed. The module's own doc comment carried the same error in its worked example.

### Root cause

The unit convention is genuinely asymmetric and the asymmetry is correct: `grams(n)` and `pieces(n)` multiply by 1e6, `litres(n)` by 1e6, `millilitres(n)` by 1e3. Whoever wrote the formatter read "VOLUME in millilitres" from the surrounding comment, applied the mass scale, and wrote a test that agreed with the code rather than with the unit system. Test, implementation and documentation were mutually consistent and all three were wrong.

Storage, entry and recipe authoring were correct throughout, so no deduction was ever affected. Only what a human read was wrong -- which is the half that criterion 4 and the variance report depend on.

### Rules

- **A wrong assertion is worse than no test, because it makes the defect look verified.** A missing test leaves a known gap; a wrong one closes the gap on paper and redirects everyone who might have looked. This one survived two milestones and was found by reading a number on a screen, not by any suite.
- **When a test and an implementation are written together, they share the author's misunderstanding.** Derive the expected value from the SPEC -- here, the unit definitions in `edge/database/src/inventory/units.rs` -- not from what the function currently returns. An assertion whose expected value was obtained by running the code proves only that the code is deterministic.
- **A unit is not a scale.** "VOLUME in millilitres" and "VOLUME stored as micro-litres" differ by 1000 and read almost identically in a comment. Where a base unit differs across dimensions, name the base unit at every boundary that converts.

---

## 2026-08-28 — Two quantity fields that named no unit

**Severity:** medium. Every physical count and wastage entry was typed into a field whose unit was 1000x off for VOLUME items, with no indication anywhere on screen.

### What happened

`StockCountScreen` labelled its input "Counted quantity (whole units)" and named no unit. `WastageScreen` named the unit only in a label. Entry is grams for MASS, millilitres for VOLUME, pieces for COUNT (`human_quantity_to_micro`, `apps/pos/src-tauri/src/commands/inventory.rs`).

Someone counting oil in litres types 5 and records five millilitres. That figure goes straight into the variance report as a real variance, and a `COUNT_ADJUSTMENT` ledger entry posts against it.

Found by driving the shipped POS by hand — the same way the previous three entries in this log were found. A 90,000 g entry made through the count field is what prompted the question.

### Root cause

Two distinct errors were being conflated as one, and only the first is a labelling problem.

1. **Reading.** The field named no unit, so a correct reading was unavailable.
2. **Intent.** A person counting stock is counting, not reading a form. They type the number they hold in their head, in the unit they are holding it in. A label they have stopped reading does not stop them — and labels stop being read on the second use of a screen.

The fix therefore is not a better label. It is a live restatement under the input that changes as the digits land: `Counting 5,000 millilitres of Sunflower Oil on hand`. It costs one line, restricts nothing, and is legible at the moment the intent forms rather than at the moment the screen opens.

A third error surfaced only when the two screens were put side by side. Wastage is a **movement** and a count is a **balance**; parallel verbs hide that. "Counting 5 millilitres" reads just as naturally as "recording that 5ml was used", and someone reading it that way enters a consumption figure into a balance field. That is a 100% variance error that looks entirely reasonable on screen, and nothing downstream distinguishes it from a real one. Two words — "on hand" — separate the two.

### Rules

- **A quantity input must state the unit it will record, and restate the value in that unit as it is typed.** The label is for the first use of the screen; the echo is for every use after that. Where a dimension's entry unit is 1000x from the unit a human thinks in, the echo is the only thing standing between intent and the ledger.
- **Echo, do not convert.** The restatement repeats what was typed and does no arithmetic beyond digit grouping. Re-deriving the edge's micro-unit conversion in TypeScript is how the two drift — the edge is the authority on quantity exactly as it is on money.
- **Group the digits.** Magnitude is itself a signal: five litres entered correctly reads "5,000", entered wrongly it reads "5". A run of zeroes that a human must count is a worse check than one they can see.
- **Spell the unit out.** "millilitres", not "ml". A symbol is glanceable-past, and this line exists specifically to be read by someone who has stopped reading.
- **Name what kind of quantity a field holds when a screen has siblings.** Balance and movement are different questions with identical-looking answers. The distinction is invisible in the number and invisible in the verb.

### Method that came out of it — enumerate sinks, not surfaces

Confirming the fix required proving no third quantity-entry screen existed. Listing screens is recall plus confirmation bias; listing write paths is a search over a closed set:

- Two Tauri commands accept a human quantity: `record_wastage`, `add_or_update_stock_count_line`.
- Exactly one non-test `INSERT INTO stock_ledger_entry` exists (`edge/database/src/deduction/ledger.rs`).
- Four origins reach it: `RECIPE`, `MODIFIER_DELTA` (automatic, from confirm), `WASTAGE`, `COUNT_ADJUSTMENT`.

The same enumeration answered a question nobody had asked: `devseed.rs` writes no ledger rows at all, so a stocked item can only have been stocked by a count — which located the incident's entry point without relying on anyone's memory of it. Now in CLAUDE.md; applies to permission checks, audit writes, print paths and sync emitters identically.

### The eleventh instance of "the contract permits it, nothing produces it"

`scripts/check-contract-field-consumers.mjs` was written on 2026-08-27 against five instances. M4 closes at **eleven**: `stock_ledger_entry.entry_type` permits six values no path writes — `PURCHASE`, `TRANSFER_IN`, `TRANSFER_OUT`, `RETURN_TO_VENDOR`, `PRODUCTION_CONSUMPTION`, `PRODUCTION_OUTPUT`.

Measured, not assumed: all six appear in the consumer roots only in a doc comment enumerating the CHECK constraint (`edge/database/src/model.rs:1248-1250`), plus `"PURCHASE"` once in a test fixture (`edge/database/src/stock/variance.rs:150`).

That measurement is the finding. **Widening the consumer check to enum values without first narrowing its corpus would report all six green** — a doc comment listing the permitted values is indistinguishable from a branch acting on one, under a grep. This is the DECLARED-versus-ACTED-ON gap the script's own header admits to, arriving a second time from a different direction, and it means the check must be built in a specific order or it ships inert. Carried into the M5 handoff as such.

## 2026-08-31 — A broken outer loop makes every inner fix unfalsifiable

`e2e-scenario` had no green run in the last 57, back to at least 2026-08-12. It
took three fixes, and the order they became visible in is the whole finding.

1. The harness minted its own `BAR` station and collided with devseed's on
   `UNIQUE (station.outlet_id, station.code)`. It died at startup.
2. Behind that, Node 20 has no global `WebSocket`, so every KDS-touching
   scenario threw `ReferenceError`.
3. Behind *that*, **the job had no build step.** The orchestrator spawns the
   harness with `cargo run`, which compiles on demand *inside* the 180s ready
   timeout — and `rust-cache` does not save on failure. So the job failed, saved
   no cache, and compiled cold again on the next run.

Fault 3 is a closed loop, and it is what makes this worth writing down. Fixes 1
and 2 were both correct, and both landed into a job that would time out
regardless. **The verdict did not move, and "the verdict did not move" read as
evidence the diagnosis was wrong.** It was not. It was evidence that nothing
about the job could be measured at all.

**A BROKEN OUTER LOOP MAKES EVERY INNER FIX UNFALSIFIABLE.** This is the
same family as green-on-absent-data (2026-08-30) and the harness-not-assertion
failure (2026-08-24), one level up: there, the test could not see the defect;
here, the *harness itself* was what made every result meaningless. When the
apparatus is broken, a red verdict carries no information about the code, and
neither does a green one.

What follows from it:

- **When a job has never been green, do not assume the current error is the only
  one; assume it is the first of N.** Budget for a stack, not a bug.
- **Fix the measuring apparatus before crediting or discrediting any fix made
  under it.** Until the loop closes, no verdict distinguishes a good change from
  a bad one — so a change that "did nothing" has not been tested.
- **Do not raise the timeout before a green run.** Raising it would have masked
  fault 3 into a slow pass and left the cold-compile loop in place permanently,
  buying a green tick at the cost of the diagnosis. A timeout is a symptom
  reporter; widening it deletes the report.

Two more from the same sweep, both cheap and both cost days:

- **A SWALLOWED STDERR COSTS DAYS.** The harness child's stderr is `inherit`ed
  and absorbed by vitest, so no cargo output ever reached the job log. A cold
  build was therefore indistinguishable from a hung harness, and was reported as
  the latter for eleven days. The bridge's two timeouts — startup (180s) and
  per-request (30s) — also emitted the *same sentence*, so the failure could not
  say which phase it was in. Both fixed; each timeout now names its phase.
- **ONE ROOT CAUSE SPANNED TWO JOBS THAT LOOKED UNRELATED.** A `WebSocket`
  `ReferenceError` in `lan-integration` and a UNIQUE constraint violation in
  `e2e-scenario` were one Node pin apart. Distinct symptoms are not evidence of
  distinct causes.

And the failure mode that let it hide: **`e2e-scenario` fails quietly.** The run
completes, all 50 scenarios execute, the invariant count reads zero violations,
and the `WebSocket` errors land in a "fatal (harness-level, not invariant)"
bucket. The summary looks like a passing run that happened to fail. **An
invariant whose subject never occurred is worse than no invariant** — it reports
zero violations, which is what success looks like. For eleven days every
KDS-touching scenario had none.

## 2026-09-02 — An acceptance criterion satisfied by either of two definitions cannot tell you which one you built

M5 criterion 7: *"Weighted average cost after two receipts at different prices
matches an independently computed figure."* It passed. The figure was right.

And it could not have told us what the number **means**.

The criterion tests that the average **moves** when prices differ. It does not
test what is being averaged, over what range, or against what definition. Two
completely different products satisfy it identically:

- a **lifetime cumulative purchase-weighted average**, over every receipt an
  outlet has ever recorded; and
- a **weighted average cost of stock on hand**, bounded by a snapshot mark —
  which is what an owner reading "average cost" generally means.

Holler implements the first. Nobody decided that. Excluding outbound rows was a
real decision, argued in `procurement/cost.rs`. The *unbounded over all time*
property was never chosen at all — the words "on hand", "moving average" and
"periodic average" appear in no design document in this repository. It is the
consequence of writing the simplest query that satisfies the criterion.

So an undocumented product definition reached acceptance, in a milestone whose
headline claim is food costing, and the criterion that was supposed to gate it
was blind to the difference by construction.

**Same family as the M4 line about a test that constructs its own subject.**
There, the test could not detect that nothing else constructed the subject. Here,
the criterion cannot detect which of two subjects it is measuring. Both are
green-on-the-wrong-question, and both survived precisely because the assertion
was true.

### The rule, and it applies to every future criterion

**Read every acceptance criterion against the question: how many different
products would satisfy this sentence?** If the answer is more than one, the
criterion is testing that something happened, not that the right thing happened,
and the definition it leaves open is the one that will ship undocumented.

Criterion 7's honest form would have named the definition: *"weighted average
cost of stock on hand"* or *"lifetime purchase-weighted average cost"*. Either
would have forced the question during design instead of during a post-hoc audit
of why the number looked high.

### Two smaller findings from the same investigation, both worth keeping

- **The dataset that passed was chosen to make the average vary, not to make the
  rounding fail.** Three receipts at 10, 10 and 18 paise per gram divide evenly,
  so the per-receipt rounding defect (±0.5 paise, +20% at a 2.5 paise/g price)
  was invisible. A fixture chosen to exercise one property will pass over every
  property it does not exercise. **Pick fixture values that are hostile to the
  arithmetic, not merely different from each other.**
- **A guard written over a table with no matching rows asserts nothing.** The
  first version of the count-adjustment regression guard checked
  `COUNT(*) = 0 WHERE origin = 'COUNT_ADJUSTMENT' AND unit_cost_paise IS NOT NULL`
  — in a fixture that contained no count adjustments at all. It passed with the
  defect deliberately introduced. It now opens, counts and completes a real
  count, with a fixture assertion that an adjustment was actually posted before
  anything is concluded from its absence. **Green on absent data, in the guard
  written to prevent green on absent data.**

## 2026-09-02 — A host that routes only some streams is indistinguishable from a working one

ADR-020 shipped with the headline "the sync worker has a host" — the first time in
five milestones that anything in the product constructed a `SyncWorker`. The
drain ran at both ends of a trading day, both its claims were falsified with
deliberately-red runs, and CI was green on all sixteen jobs.

It hosted **one of three pumps**.

`worker::pump_outbox` routes orders and table sessions. A goods receipt is
`("goods_receipt_note", "GoodsReceiptRecorded")`, which that router does not map
at all: it returns the row as `unrouted_skipped` and leaves it pending. Purchase
returns and transfers go through `pump_procurement`, carrying the per-entry retry
budget; ledger entries and stock gaps through `pump_ranged_streams`. So every
GRN, return, transfer and ledger row would have sat in the outbox while the host
reported success.

**The failure reads as its own opposite.** The drain printed
`drain published 0 row(s)` — which is exactly what an empty outbox prints. Not
silence, which someone might question, but a plausible number meaning the reverse
of the truth. The ordering trap this ADR already warned about at least does
nothing visibly; this one does nothing and produces a reassuring report.

**Nothing caught it, and the reason matters.** `cloud_replay` is green, has been
for milestones, and drives `pump_ranged_streams` **directly** — it exercises the
worker, not the host. Every test of the sync path constructs its own pump. So the
question "does anything call the other two?" was structurally unaskable from
inside the suite, in the same way "does anything construct the worker at all?"
was before ADR-020.

Same family as the M4 line — *a test that constructs its own subject cannot
detect that nothing else constructs it* — one level along: **a test that
constructs its own subject cannot detect that its caller constructs only some of
them.** Hosting is not a property any test of the hosted thing can see.

It was found by asking which code path a goods receipt actually takes while
writing the procedure to close criterion 6 — not by a test, and not by review.

### What changed beyond the fix

Routing all three pumps fixes the instance and leaves the pattern: the next
stream added hits the same wall and reads the same way. So the counter changed
too. The drain now reports **published / unrouted / refused separately, per
stream**, and a non-zero `unrouted` prints its own line saying the rows have no
route and this is not an empty queue. `published` alone was never a report — it
was a number that could not distinguish "nothing to send" from "nothing sendable".

And the drain's claim about itself is no longer the only evidence: every pass now
also prints the pending count **read straight from `local_outbox`**, so the
table's number and the drain's number sit side by side in the same terminal. A
mis-routing drain cannot make those two agree.

## 2026-09-02 — A test condition the environment cannot produce is not a weak test, it is no test

Every acceptance procedure in this project since Milestone 1 has said some form of
**"with the network disconnected"**, and the operator has dutifully switched WiFi
off before receiving, ordering or billing.

**The cloud is `http://localhost:8080`.** Traffic to it never leaves the machine.
Turning WiFi off — or unplugging every cable, or disabling every adapter — changes
nothing about its reachability. The step passes identically in all cases, because
it is not connected to the thing it claims to control.

So the offline condition was never established, in any run, in any milestone. It
was found when a backend process died for an unrelated reason and the operator
noticed that *this* was the only thing that had ever made the cloud unreachable.

**The failure is not that the test was weak. It is that it could not fail.** A
condition the environment cannot produce yields a step that is green regardless
of the code, which is indistinguishable from a step that is green because the
code is right.

Same family, third instance this milestone:

- **Criterion 7** was satisfied by either of two cost definitions, so it could not
  report which one shipped.
- **`cloud_replay`** proved replay while nothing hosted the worker, because it
  constructed its own pump.
- **This**: an offline test against a loopback cloud.

All three assert something true. None of them can distinguish the world where the
product works from the world where it does not — which is the only thing an
acceptance criterion is for.

### The rule

**Before running an acceptance step that names a precondition, establish the
precondition and VERIFY IT INDEPENDENTLY — with a check that fails when the
precondition is absent.** "WiFi is off" is a description of an action. "A request
to the cloud base URL is refused" is the precondition, and it is the only form
that can be wrong out loud.

Offline against a localhost cloud is produced by stopping the process that serves
it, or pointing the client at an address nothing binds — never by touching the
network stack.

### And the symptom was not the defect

The same run turned up a `500` on order replay, caused by the cloud seeding 2
menu items where the edge seeds 43. **Seeding the cloud menu would have made the
500 disappear, the stream drain, and everything look healthy — while shipping
both real defects underneath**: a client-data failure reported as a server fault,
and one unreplayable row stranding 120 behind it.

The dev-seed drift was the STIMULUS. The defects were what it exposed. Fixing the
stimulus is how both would have shipped, and it would have looked like a fix.

## 2026-09-02 — "Offline" was reported for a request that was never sent

The M5 criterion 6 run produced this, in one shutdown drain:

```text
holler-pos: shutdown drain [orders] published=0 unrouted=36 refused=0
holler-pos: shutdown outbox drain found no route to the cloud; 8 row(s) sent
holler-pos: shutdown drain [procurement] published=6 unrouted=0 refused=0
holler-pos: shutdown drain [stock] published=2 unrouted=0 refused=0
```

The orders stream "found no route to the cloud" and then, in the same pass,
through the same client, in the same process, procurement and stock published
eight rows. The backend's request log settles it: during that entire drain it
received six `POST /procurement/goods-receipts`, two
`POST /inventory/ledger-entries`, and **nothing whatsoever from the orders
stream**. Not a failed request — no request.

The cloud was reachable. The label was wrong.

**Cause:** `ureq` pools connections. The startup drain ran at 11:03 and the
shutdown drain at 11:18, so the pooled keep-alive socket had been closed by the
server long before it was reused. Reuse failed at the transport layer, and
`client.rs` mapped every `ureq::Error::Transport` straight to
`SyncError::HttpTransport`, which the worker reports as `StopReason::Offline`.
One attempt, no retry, and the verdict was final.

**Why a false offline is expensive rather than merely wrong.** The outbox drains
in order, so a stream that stops strands every row behind it. The first stream in
any drain after an idle period would therefore hit the dead pooled socket, stop,
and leave its rows pending — while every stream after it succeeded, because the
next request opened a fresh connection. At an outlet that reads as "sync is
broken and the network is down", on a night when the network was fine.

**The fix is one retry, and the reasoning for the number matters.** A transport
failure on the first attempt after idling is not evidence of anything — it is
equally consistent with a dead pool entry and a severed uplink, and only a second
attempt separates them. But it is *one* retry, not a loop: the shutdown drain is
bounded, an outlet closing with no uplink is the normal case (ADR-020), and a
retry loop would spend the budget rediscovering what two attempts already
established. An HTTP status is never retried — the server answered, and asking
again does not change its mind.

### The generalisation, which is the point

**A diagnosis is not a measurement.** `Offline` is a conclusion the client draws
from one failed syscall, and it was stated to the operator with the same
confidence as `published=6`, which is a count of things that actually happened.
Three distinct situations — no listener, listening but refusing, a socket the
peer already closed — were collapsed into one word, and the word chosen was the
one that blames the customer's network.

The same session had already built the discriminator: `scripts/check-cloud-unreachable.ps1`
refuses to say "offline" unless three independent probes agree, and reports
REACHABLE when it cannot classify a failure. That fail-closed instinct belongs in
the worker's status reporting too, and is filed.

### And a correction, recorded because the process matters more than the answer

While diagnosing the same run I asserted that `LanServerHandle::shutdown()` hangs
forever, because it dials its own bind address and `0.0.0.0` is not a dialable
target. The dial failure is real and was proven. **The hang was not.** The
process had already exited; my "still running" reading was taken mid-shutdown,
and the heartbeat join alone takes up to five seconds. I built a mechanism on one
true fact and one stale observation, and stated it as the explanation.

The operator caught it by asking a simple question I had not asked myself — *are
you sure closing the window kills it?* — and the answer was no, it had died, and
the thing still reachable on port 5173 was Vite, not the POS.

**A plausible mechanism that explains the symptom is not the same as the cause.**
The undialable bind address is still a latent defect and is filed on its own
merits; it just did not cause this.

## 2026-09-02 — Acceptance evidence that lives only in a session is lost when the session is

The machine hung. The session restarted. The next session was asked to pick up
where it stopped, read the git log, read `docs/RESUME.md`, and produced a
criteria table saying M5 criteria 1, 3, 4 and 6 were unobserved and criterion 6
should be re-run.

All four had been observed that morning, on real screens, by the operator.

The reconstruction was not hedged. It was stated in the same register as a read
of the record — a table, with verdicts, offering to resume the run. And the
strongest evidence against it was already in the context window: `262e03a` is the
transport-retry fix asked for **after** criterion 6 passed, and its own commit
message opens *"The M5 criterion 6 shutdown drain reported..."*. The session held
the commit that came after the evidence and still concluded the evidence did not
exist.

**The record was the chat.** Four criteria, their preconditions, the figures
(`GRN/20260902/0002`, Atta 400000 g → 500000 g, `4 sack → 100000 g`,
`published=6`, outbox 126 → 120, `line_total_paise = 950000`) — none of it was
committed anywhere. Git held the *fixes the run produced* and none of the run.

### Why this is the same family as the rest of this log

*A test whose subject nothing else constructs cannot detect that nothing else
constructs it.* Here: **the fact existed and the record of it did not**, and
nothing in the repository could tell the difference between "observed and
unrecorded" and "never observed". Both look identical to a fresh session — and
the fresh session resolved the ambiguity in the direction that discards work, and
proposed re-running a criterion that had passed.

The M5 pattern, now four instances, is one sentence: **green on absent data.** A
criterion satisfied by either cost definition. A `cloud_replay` proving replay
while nothing hosted the worker. An offline test against a loopback cloud. And
now an acceptance table derived from a repository that contains no acceptance
evidence — confidently, because nothing was there to contradict it.

### The rule

**A milestone does not close until its acceptance evidence is committed to the
repository. The chat is not the record.** Every criterion: what was observed, how
the precondition was established and independently verified, who observed it, and
on what date. In CLAUDE.md, in all three builder agent files, and in the
verifier's rubric as an automatic FAIL.

Three corollaries, each earned in this session:

- **Cite the artefact, never the conversation** — the screen, the row, the
  request log, the PID.
- **When two reports of the same run disagree, record the contradiction as
  UNRESOLVED with the query that settles it.** The pre-drain baseline named the
  pending receipts `0001`/`0002` and the post-drain comparison named them
  `0002`/`0003`. One is wrong, neither store is readable now, and the honest
  entry is the open question plus the SQL — not a guess written in the register's
  voice.
- **A verifier judges a committed file, not an agent's account of a run.**

And the smaller lesson from the verification that followed: `make check-seams`
and a bare `cargo test` at the repository root both failed here — no `make` on
PATH in the Bash tool, no workspace manifest at the root — and **both exited 0
through a pipe**. Two "green" lines were reported before the pipeline was
noticed. A command that cannot run is not a command that passed.
