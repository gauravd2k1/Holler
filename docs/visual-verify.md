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
| **VV-017** | 1 | Search finds a dish from **any** section, each result labelled with its section |
| **VV-018** | 1 | No "(internal -- not sold)" category anywhere in the rail |
| **VV-019** | 1 | The rail's longest category names read in full, no `…` |

**VV-012/013/014 are D14, which is FIXED (`97bc3dc`) and has never been seen in
the Tauri release window.** Expect it to work; the remount workaround in
`docs/demo-script.md` → "Known on stage" is the fallback, not the expectation.
The open-defect register's D14 row still reads OPEN and is stale — it was
compiled about two hours before the fix landed.

### VV-017/018/019 — the till's menu screen

Three changes in one commit, one rebuild. **All three are CSS or render-path
changes that no test suite can see** — the POS unit suite (264 tests), `tsc`
and `eslint` all pass either way, which is exactly why they need a row each.

| ID | Commit | Open | Steps | Pass condition | Status | Evidence | Verified by | Date |
|---|---|---|---|---|---|---|---|---|
| VV-017 | this commit | POS — **Tauri release window**, main ordering screen | Select **Sushi Platter** in the left rail (a section with no Thai dish in it). Type `Thai` into **Search menu…**. | Results appear **from other sections** — `Thai Grilled Chicken Salad`, `Pad Thai` and so on — **not an empty grid**. Each card shows its **section name under the dish name**. Clear the box: the grid returns to Sushi Platter's own two items and **no section label is shown on them**. | PASS | `docs/evidence/VV-017-search-cross-section.png` | Operator | 2026-10-01. Before this commit the search filtered the SELECTED category only, so this exact sequence returned nothing. Falsified by the operator on the live till, 2026-09-17 Observed in the Tauri release window: Sushi Platter selected, `Thai` returned 10 dishes across 7 other sections, each labelled with its section; clearing the box returned the grid to Sushi Platter only, unlabelled. |
| VV-018 | this commit | POS — **Tauri release window**, category rail | Scroll the left rail from top to bottom. | **No category named "… (internal -- not sold)" appears** — neither `Kitchen Prep` nor `Test fixtures`. Every other section still appears and still opens. | PASS | `docs/evidence/VV-010-window-chrome.png` | Operator | 2026-10-01. Hidden by a NAME MATCH in PosScreen, deliberately, so tonight needs no re-emit or reset. A category renamed without the word "internal" comes back — the real fix is a flag on the row, filed post-demo Operator scrolled the rail top to bottom: no category containing "internal" or "not sold". |
| VV-019 | this commit | POS — **Tauri release window**, category rail | Read the rail's longest names: `Tartar & Carpaccio`, `Wok Poultry & Meat`, `Non Veg Tapas`, `Sushi Roll (4pc)`. | Each reads in full, **with no `…`**. The item grid and the cart are unchanged in width and nothing below the rail is clipped. | PASS | `docs/evidence/VV-019-rail-long-names.png` | Operator | 2026-10-01. 160px → 220px. 13 of 48 names were clipped at 160px; the longest sold name is 23 chars All four named categories read in full, no ellipsis, rail not clipped. |

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
| VV-003 | `1c0de90` | POS — **Tauri release window**, order list and sync banner | Open the order list. Force a blocked row if the banner is empty (or read VV-001's). | An order number renders **`#A2`**, never `##A2`, on both surfaces. | OPEN | `docs/evidence/VV-003-order-number.png` | | **2026-10-01/03, operator, ONE SURFACE OF TWO.** The order list renders `#A1` and `#A2`, single `#`, correct. The sync banner half is NOT taken -- the banner has carried no rows in this sitting, so there has been nothing to read it on. Closes with VV-001. |
| VV-004 | `cc11b88` (D12) | Admin console — browser, Orders tab | After the operator's `demo-reset.ps1 -Force`, take one order end to end, then open Orders. | **No order shows a non-zero total with zero lines.** 21 did before the reset. | FAIL | `docs/evidence/VV-004-admin-no-empty-orders.jpg` | Operator + agent-driven Chrome | 2026-10-04. **FAILS, and it is the exact shape the row was written to catch.** Admin Orders shows **#A2 at Rs1945.00 with "no lines synced"** -- a non-zero total and zero lines. Confirmed in Postgres, not just on screen: `order.total_paise = 194500` against `count(order_item) = 0`. **#A1 is wrong in a different and worse way**: the cloud holds TWO lines (Avocado, line total Rs427.00; Salmon, Rs575.00, summing to Rs1002.00) while the till shows ONE item at Rs575.00 and its KOT read `1x Salmon`; and the order header reads Rs415.00, which is neither the sum of its own lines nor either line total -- it is Avocado's UNIT PRICE (41500). So the header total, the line set and the till disagree three ways on one order. `ledger_replay_gap` is empty, so nothing flagged any of it. Filed as its own backlog row; the mechanism is NOT established and is written there as the open question rather than guessed at. |
| VV-005 | `52d8930`, `78c87f5` | Captain page — **a real phone on the hotspot**, plus the KDS screen | Send a round for table T1. Without clearing the table, Send a second round. | The second round **appends to the open order** and the KDS shows a **second ticket carrying only the new round** — not a second order, not a repeat of round one. | OPEN | | | |
| VV-006 | `c246a9d` | Invoice screen — **Tauri release window** — and the rendered receipt PDF | Bill an order split cash + UPI. Open the PDF the print writes. | The UPI QR is present on **both**, and both name the same payee. | PASS | `docs/evidence/VV-006-upi-qr.png` + `docs/evidence/VV-006-receipt.txt` | Operator | 2026-10-04. Bill #A2, 1895.00, split CASH 1800.00 + UPI 95.00. A UPI QR appears on BOTH the invoice screen and the printed receipt and both name the same payee, "Shinjuku Yakitori", which is what this row asks. The row PASSES on its own terms and TWO DEFECTS ARE FILED SEPARATELY rather than folded into it, because neither is what it was written to catch: the QR encodes the FULL bill on a split tender, and it is printed on the receipt of an ALREADY-PAID invoice. See docs/backlog.md. Note the print only reached a file at all after HOLLER_PRINTER_FILE_SINK_DIR was set in the launching shell -- the first attempt wrote nothing AND showed no banner. |
| VV-007 | `807552c` | POS — **Tauri release window**, till header and bill | Sign in and look at the header; bill an order and read the receipt. | The restaurant name, address and GSTIN come from `seed/outlet.toml` and agree on every surface. **`logo_path` stays unset** — a set one has never been rendered. | PASS | `docs/evidence/VV-007-header.png` + `docs/evidence/VV-006-receipt.txt` | Operator | 2026-10-04. Taken in two halves, as the row's own wording requires -- it spans the header AND the receipt. Header (2026-10-01): the till shows the brand name "Shinjuku Yakitori", matching seed/outlet.toml. The address and GSTIN are deliberately NOT on the till header; seed/outlet.toml:44 says they print on the invoice and the receipt, so the sitting document's "read the till header: name, address, GSTIN" was over-specified. Receipt (2026-10-04): legal_name "Shinjuku Yakitori Hospitality Pvt Ltd", address "Camp / Pune 411001", GSTIN 27AAAAA0000A1Z5, FSSAI 11522998000123 -- every one from seed/outlet.toml, none hardcoded. Place of Supply "Maharashtra (27)" agrees with the GSTIN's first two digits. HSN/SAC 9963 present on the line, without which the invoice could not have issued at all. |
| VV-008 | M6 item 6 | KDS — browser on a **second device over the hotspot** | Load the KDS, send a ticket from the till, bump it. | The ticket renders, the bump sticks, and **no raw UUID, dev label or internal note** is on screen. | OPEN | | | The KDS has never been observed on a second device |
| VV-009 | `d805218` | POS — **Tauri release window**, Kitchen panel on an order | Bump the ticket to READY on the KDS. Return to the till and read the order's Kitchen view. | It reads READY. **Record whether a remount was needed** — that is the open question (D14), not an aside. | **FAIL — closed on evidence** | `docs/evidence/VV-009.png` | Operator | 2026-09-16 |
| VV-012 | D14 part 1 | POS — **Tauri release window**, Kitchen panel left OPEN | With the panel open and untouched, acknowledge ticket #1 on the KDS. Do not switch panels, do not alt-tab. | The status changes **on its own**, within a second or two. **No remount.** Alt-tabbing is not a valid way to run this — `refetchOnWindowFocus` is false, so a focus change refetches nothing and proves nothing. | OPEN | | | |
| VV-013 | D14 part 2 | POS — **Tauri release window**, Kitchen panel | Force a stale view: with the panel open, acknowledge on the KDS, then **immediately** press the till's own action for that ticket. | If a rejection happens at all, the row is **already corrected** when the error appears and the offending button is gone. The error must never sit beside the stale status that caused it. | OPEN | | | Hard to force once VV-012 works — that is the point; this path is the backstop for when live update fails |
| VV-014 | D14 part 3 | POS — **Tauri release window**, Kitchen panel | Look at a ticket in each status. | Buttons read as **verbs** — Acknowledge, Start preparing, Mark ready, Mark served, Cancel ticket — never as statuses. Only moves legal from the current status are offered. The status badge is **coloured AND worded**, never colour alone. | OPEN | `docs/evidence/VV-014-kot-buttons.png` | | **2026-10-03, operator, PARTIAL -- 2 of 5 statuses seen, so the row stays OPEN.** A READY ticket on the till offers exactly one action, `Mark served` -- a verb, and the only legal move. A SERVED ticket offers NOTHING, correctly: it is terminal. Badges are worded as well as coloured (green `Ready`, outlined `Served`), so the colour-only failure does not occur. NEW / ACKNOWLEDGED / PREPARING were never seen ON THE TILL -- those buttons only appeared on the KDS, and this row is about the till. Closing it needs a run that watches one ticket through those three. This run is also what exposed the unreachable SERVED ORDER status now in docs/backlog.md. |
| VV-020 | this commit (B2-T1) | POS — **Tauri release window**, order list with a Kitchen panel **COLLAPSED** | Open the order list. Leave every Kitchen panel collapsed and **do not touch the window**. Bump a ticket to READY on the KDS. Wait. Then expand that order's Kitchen panel. | The order row's state reflects the bump **without the panel ever having been open**, and expanding it shows READY immediately. **The falsifier is the collapsed panel**: before this commit the only listener lived in `KotsPanel`, so a collapsed panel meant nothing in the process was subscribed — the pre-fix binary passes any version of this test that has a panel open, which is why VV-012's wording is not sufficient for it. Second half, same run: with the order list open and untouched, place an order from the captain phone and confirm it appears (the 15s fallback poll, not the event — a created-but-unsent order broadcasts nothing). | OPEN | `docs/evidence/VV-020a-collapsed-panel.png` | | 264 POS tests, `tsc`, eslint and `pnpm build` all pass with the listener mounted or unmounted. No suite can see this **2026-10-01/03, operator, PART A PASSES; ROW STAYS OPEN FOR PART B.** First attempt was DISCARDED rather than counted: the panel had been expanded between the send and the bump, and "the row updated on its own" is indistinguishable from "expanding it fetched the status and it stayed". A second order (#A2) was run clean -- sent to the kitchen, panel collapsed with Hide Kitchen, POS then untouched, ticket taken to Ready on the KDS, and the row showed Ready by itself after 5-10s with the panel never reopened. Note for whoever runs this next: **the Kitchen panel AUTO-EXPANDS when an order is sent**, so it must be collapsed deliberately before the bump or the falsifier is destroyed. Part B (captain phone order appearing via the 15s fallback poll) is NOT taken -- no captain device has been paired in this sitting. |
| VV-010 | pre-existing | POS — **Tauri release window**, window chrome | Look at the title bar and the taskbar icon. | A real title and a real icon. Today the icon is a 16×16 placeholder from the scaffold (D23). | PASS | `docs/evidence/VV-010-window-chrome.png + docs/evidence/VV-010-taskbar-icon.png` | Operator | 2026-10-01. Expected FAIL until artwork exists **The stated expectation was STALE.** Title reads `Holler POS` and the icon is the real Holler mark in both the title bar and the taskbar; `apps/pos/src-tauri/icons/` carries full artwork (icon.ico 77KB, icon.png 188KB), not the 16x16 scaffold placeholder D23 describes. |
| VV-015 | `e640f3d` (D11) | Admin console — browser, `http://localhost:5175`, Orders tab | **After a reseed from `e640f3d` or later**, take an order on the till, let it replay, then open Orders. | The order **names the device that took it** — "Dev Till 1" — not a blank, not a raw UUID. Before D11 the cloud had no `device` row for the till and four orders resolved to nothing. | FAIL | `docs/evidence/VV-015-admin-device-name.jpg` | Operator + agent-driven Chrome | 2026-10-04. Needs the reseed; the row is seeded UNENROLLED, which is correct and must not read as a device that has enrolled **FAILS: there is no device column at all.** Admin Orders renders Order, Taken, Type, Status, Items, Payment, Total -- and nothing naming the device. The row expects "Dev Till 1". This is not a blank value or a raw UUID, which is what the row anticipated; the column does not exist on the screen. |
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
