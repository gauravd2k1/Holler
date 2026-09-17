# Visual verification register

**A gate, not a log.** Everything in this project that a person LOOKS AT is
verified here, by a person, in the runtime that ships — and nowhere else.

## Why it exists

CLAUDE.md records that there are **four runtimes, not three**: the build
output, the dev server, the browser, and the Tauri release window. Every
incident behind that rule was a change that was green in one runtime and broken
in another — the KDS detached-global crash (browser only), the POS white screen
(dev server only), and the release binary that had been fetching its UI from
`localhost:5173` for three sessions while every existence and identity check
passed.

An agent cannot close a row here. `run-dev.ps1` refuses under `CLAUDECODE`, and
a Tauri window launched from a tool with redirected stdio never appears. **So
the rule for agents is: when a window cannot be opened, add a row and say
UNVERIFIED. Never a claim.**

## Rules

1. **Any commit that changes something rendered — POS, admin, captain, KDS, or
   the receipt — adds a row in the same commit. No exceptions.** A rendered
   change with no row is an incomplete commit, the same way a contract change
   with no consumer update is.
2. **A row closes only with a screenshot path** under `docs/evidence/`.
   "Seen in the dev server" does not close a row; neither does "the test
   passes", and neither does an agent's description of what the code should
   draw. Record what the screen said, not what the change intends.
3. **A row names the runtime to open**, because that is the thing that keeps
   being got wrong. "The POS" is not a runtime; "the Tauri release window" is.
4. **A FAIL is recorded, not overwritten.** Add the fix's row beneath it rather
   than flipping the old one, so the register shows what was seen and when.
5. **The verifier is a person and is named.** "Operator" is enough; an agent
   never appears in that column.

## Status vocabulary

| Status | Meaning |
|---|---|
| `OPEN` | Not yet looked at by anyone. |
| `PASS` | Observed, with a screenshot path in the Evidence column. |
| `FAIL` | Observed and wrong. A new row is added for the fix. |
| `UNVERIFIABLE` | No person can currently open the runtime it needs (no hardware, no device). Says why. |

## TONIGHT — 2026-09-17. The rows that gate the demo.

**These ten and no others.** Every other row in the register is **post-demo**:
not cut, not failed, simply not in tonight's path and not to be chased during a
rehearsal. Run them inside the three clean runs
(`demo-up.ps1 -Release -Fresh …`), not as a separate pass.

| Row | Gates which step | One-line pass condition |
|---|---|---|
| VV-001 | 4 | Banner empties after the cloud restarts; no `conflict (HTTP 409)` ever appears |
| VV-002 | 4, 5 | The order is in admin Orders and is **not** DRAFT |
| VV-003 | 1, 5 | `#A2`, never `##A2` |
| VV-004 | 5 | No order with a total and **zero lines** |
| VV-011 | 1, 4 | Muted "kept locally" line present; **attention list EMPTY** |
| VV-012 | 1 | Kitchen panel open and untouched: the till's status changes **on its own** after a KDS bump |
| VV-013 | 1 | A refused move shows its error with the row **already corrected** |
| VV-014 | 1 | Buttons are **verbs**, only legal moves offered, badge coloured **and** worded |
| VV-015 | 5 | Admin Orders names the device — **"Dev Till 1"** |
| **VV-016** | **fix 5** | **See below — the drain.** |

**VV-012/013/014 are D14, which is FIXED (`97bc3dc`) and has never been seen in
the Tauri release window.** Expect it to work; the remount workaround in
`docs/demo-script.md` → "Known on stage" is the fallback, not the expectation.
The open-defect register's D14 row still reads OPEN and is stale — it was
compiled about two hours before the fix landed.

### VV-016 — the fix-5 drain (`ca9ac49`, `aa79396`)

| ID | Commit | Open | Steps | Pass condition | Status | Evidence | Verified by | Date |
|---|---|---|---|---|---|---|---|---|
| VV-016 | `ca9ac49`, `aa79396` | POS — **Tauri release window**, sync banner; plus the cloud database | **A. After a clean `demo-up.ps1 -Release -Fresh …`**, let the till drain. Then, in the cloud: `SELECT count(*), min(entry_seq), max(entry_seq) FROM stock_ledger_entry WHERE outlet_id = '<demo outlet>';` **B.** Record one wastage on the till. Let it drain. Re-run the query. | **A: 45 rows, `entry_seq` 1–45, carrying the EDGE's row ids** — a freshly seeded cloud starts with **zero** ledger rows and every one of the 45 arrives by replay. Banner: **attention list EMPTY**, `no_route` count only. **B: 46 rows, max `entry_seq` = 46**, attention list still EMPTY. **No `conflict (HTTP 409)` at any point.** | OPEN | | | The defect this replaces: cloud and edge each held the same 45 movements under DIFFERENT ids, so the till's first replay missed on id, INSERTed, and hit `UNIQUE (outlet_id, entry_seq)` — a 409 on stage before any real movement existed |

**An empty cloud ledger immediately after seeding is the INTENDED state**, not a
missing step. If the query in A returns 45 rows *before* the till has drained,
something is seeding them cloud-side again and fix 5 has regressed.

---

## The register

**Everything below that is not in tonight's ten is POST-DEMO.**

| ID | Commit | Open | Steps | Pass condition | Status | Evidence | Verified by | Date |
|---|---|---|---|---|---|---|---|---|
| VV-001 | `1c0de90`, `df977f8` | POS — **Tauri release window** | Take an order with the cloud stopped. Restart the cloud (`scripts/dev-up.ps1`, new pid). Wait one pump interval (60s). | The sync banner **empties**. No `conflict (HTTP 409)` line appears at any point. | OPEN | | | |
| VV-002 | `1c0de90`, `df977f8` | Admin console — browser, `http://localhost:5175`, Orders tab | After VV-001, open Orders and find that order. | The order is listed and its status is **not DRAFT** — it reads what the till showed (SENT_TO_KITCHEN or later). | OPEN | | | |
| VV-003 | `1c0de90` | POS — **Tauri release window**, order list and sync banner | Open the order list. Force a blocked row if the banner is empty (or read VV-001's). | An order number renders **`#A2`**, never `##A2`, on both surfaces. | OPEN | | | |
| VV-004 | `cc11b88` (D12) | Admin console — browser, Orders tab | After the operator's `demo-reset.ps1 -Force`, take one order end to end, then open Orders. | **No order shows a non-zero total with zero lines.** 21 did before the reset. | OPEN | | | |
| VV-005 | `52d8930`, `78c87f5` | Captain page — **a real phone on the hotspot**, plus the KDS screen | Send a round for table T1. Without clearing the table, Send a second round. | The second round **appends to the open order** and the KDS shows a **second ticket carrying only the new round** — not a second order, not a repeat of round one. | OPEN | | | |
| VV-006 | `c246a9d` | Invoice screen — **Tauri release window** — and the rendered receipt PDF | Bill an order split cash + UPI. Open the PDF the print writes. | The UPI QR is present on **both**, and both name the same payee. | OPEN | | | |
| VV-007 | `807552c` | POS — **Tauri release window**, till header and bill | Sign in and look at the header; bill an order and read the receipt. | The restaurant name, address and GSTIN come from `seed/outlet.toml` and agree on every surface. **`logo_path` stays unset** — a set one has never been rendered. | OPEN | | | |
| VV-008 | M6 item 6 | KDS — browser on a **second device over the hotspot** | Load the KDS, send a ticket from the till, bump it. | The ticket renders, the bump sticks, and **no raw UUID, dev label or internal note** is on screen. | OPEN | | | The KDS has never been observed on a second device |
| VV-009 | `d805218` | POS — **Tauri release window**, Kitchen panel on an order | Bump the ticket to READY on the KDS. Return to the till and read the order's Kitchen view. | It reads READY. **Record whether a remount was needed** — that is the open question (D14), not an aside. | **FAIL — closed on evidence** | `docs/evidence/VV-009.png` | Operator | 2026-09-16 |
| VV-012 | D14 part 1 | POS — **Tauri release window**, Kitchen panel left OPEN | With the panel open and untouched, acknowledge ticket #1 on the KDS. Do not switch panels, do not alt-tab. | The status changes **on its own**, within a second or two. **No remount.** Alt-tabbing is not a valid way to run this — `refetchOnWindowFocus` is false, so a focus change refetches nothing and proves nothing. | OPEN | | | |
| VV-013 | D14 part 2 | POS — **Tauri release window**, Kitchen panel | Force a stale view: with the panel open, acknowledge on the KDS, then **immediately** press the till's own action for that ticket. | If a rejection happens at all, the row is **already corrected** when the error appears and the offending button is gone. The error must never sit beside the stale status that caused it. | OPEN | | | Hard to force once VV-012 works — that is the point; this path is the backstop for when live update fails |
| VV-014 | D14 part 3 | POS — **Tauri release window**, Kitchen panel | Look at a ticket in each status. | Buttons read as **verbs** — Acknowledge, Start preparing, Mark ready, Mark served, Cancel ticket — never as statuses. Only moves legal from the current status are offered. The status badge is **coloured AND worded**, never colour alone. | OPEN | | | |
| VV-010 | pre-existing | POS — **Tauri release window**, window chrome | Look at the title bar and the taskbar icon. | A real title and a real icon. Today the icon is a 16×16 placeholder from the scaffold (D23). | OPEN | | | Expected FAIL until artwork exists |
| VV-015 | `e640f3d` (D11) | Admin console — browser, `http://localhost:5175`, Orders tab | **After a reseed from `e640f3d` or later**, take an order on the till, let it replay, then open Orders. | The order **names the device that took it** — "Dev Till 1" — not a blank, not a raw UUID. Before D11 the cloud had no `device` row for the till and four orders resolved to nothing. | OPEN | | | Needs the reseed; the row is seeded UNENROLLED, which is correct and must not read as a device that has enrolled |
| VV-011 | D8a + the split | POS — **Tauri release window**, sync banner | Send an order to the kitchen, then bump its ticket to READY on the KDS. Watch the banner through the whole demo story. | A muted line reads **"N records kept locally — the cloud has no route for them yet. Nothing to do."** and **the attention list stays EMPTY** — no red count, no growing list of rows nobody can act on. If a real failure happens as well, the attention list holds exactly that row and the muted count carries the rest. | OPEN | | | Needs a build after this commit; not in the 12:27 binary |

## A note on VV-009

**Recorded as FAIL on the operator's observation, 2026-09-16:** the KDS
acknowledged ticket #1; the till's Kitchen panel still showed **New** and still
offered **"Acknowledged"**; pressing it produced *"This ticket cannot move to
that status from where it is now."* — the edge refusing a move the screen had
offered from stale state.

**CLOSED as a FAIL on `docs/evidence/VV-009.png`**, which the operator
committed. The FAIL is kept rather than flipped: this row records what was
seen, and VV-012/013/014 record the fix.

**THE REMOUNT VARIANT WAS NOT OBSERVED, and is now moot.** The reading after
switching panels was reported with both options still in it — "[Acknowledged →
stale-until-remount | New → stale-after-remount]" — and no one went back for
it. **Operator ruling: not observed, superseded by `97bc3dc`, the fix does not
depend on it.** Part 1 makes the view live so a remount is never needed, and
part 2 repairs the view on any rejection whatever the cause. It is written down
as unobserved rather than resolved by guessing, because a reconstruction stated
with the confidence of a reading is the thing `docs/m5-acceptance.md` exists to
stop.


## What is NOT in here

Anything with no rendered surface — a drift check, a migration, a sync route —
is proved by an executed test and named in its commit. The register is for
pixels a person can be wrong about.
