# POS order-screen UI pass — before/after

Evidence for the 2026-09-14 UI pass on `apps/pos`. Four states, each shot twice:
once from a build of the parent commit (`before-*`) and once from this branch
(`after-*`), at 1440x900.

| State | Before | After |
|---|---|---|
| Empty cart | `before-01-empty-cart.png` | `after-01-empty-cart.png` |
| Three lines, one carrying a modifier | `before-02-three-lines.png` | `after-02-three-lines.png` |
| Table selected | `before-03-table-selected.png` | `after-03-table-selected.png` |
| Orders list, no orders | `before-04-orders-empty.png` | `after-04-orders-empty.png` |

## How they were taken, and what that means they do and do not prove

A scratch Playwright harness (session scratchpad, deliberately **not**
committed — it is a one-off, and a committed harness nothing runs is a
maintenance obligation with no owner) served each built `dist` over plain HTTP
on **port 5399** and installed a fake `window.__TAURI_INTERNALS__` through
`addInitScript`, which runs before the bundle evaluates. Every Tauri command
the screens call returned a fixture.

Port 5399 is a scratch port. Nothing in this pass started, stopped or bound
5173, 5175, 8080, 9310 or 9320 (CLAUDE.md standing rule).

**What these prove: layout, type, spacing, colour, and the empty and disabled
states, in a real browser rendering the real production bundle.** That is the
runtime this pass changed, and it is a different runtime from `pnpm build` and
from the unit suite — CLAUDE.md's build-green-is-not-dev-works rule.

**What they do NOT prove: anything about the Tauri window or the edge.** The
data is fixtures, so no command, permission check, price or tax figure here was
computed by the edge. A card tap in these shots resolves against a fixture
variant, not against the real menu. The till still has to be looked at on the
release binary — that is the operator's run, not this.

The three-line fixture carries one modifier (`Extra spicy`, +₹25.00) because
the modifier line beneath a cart line is one of the things the pass restyled,
and a fixture that omits it proves nothing about it — the same reason
contracts 0.5.9 needed a populated provenance row.
