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

## The register

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
| VV-009 | `d805218` | POS — **Tauri release window**, Kitchen panel on an order | Bump the ticket to READY on the KDS. Return to the till and read the order's Kitchen view. | It reads READY. **Record whether a remount was needed** — that is the open question (D14), not an aside. | OPEN | | | |
| VV-010 | pre-existing | POS — **Tauri release window**, window chrome | Look at the title bar and the taskbar icon. | A real title and a real icon. Today the icon is a 16×16 placeholder from the scaffold (D23). | OPEN | | | Expected FAIL until artwork exists |

## What is NOT in here

Anything with no rendered surface — a drift check, a migration, a sync route —
is proved by an executed test and named in its commit. The register is for
pixels a person can be wrong about.
