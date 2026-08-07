# Order State Machine

Source: docs/spec/ordering.md, HOLLER_MASTER_PROMPT.md §52.

## States
```
DRAFT → CONFIRMED → SENT_TO_KITCHEN → PREPARING → READY
→ SERVED → BILLED → PAID → CLOSED
Alternative: CANCELLED
```

## Transition table
| From | To | Trigger | Notes |
|---|---|---|---|
| DRAFT | CONFIRMED | cashier/waiter confirms cart | items/modifiers locked for pricing snapshot |
| DRAFT | CANCELLED | order abandoned before confirmation | no KOT exists yet |
| CONFIRMED | SENT_TO_KITCHEN | send-to-kitchen action | generates KOT(s), see docs/spec/kitchen.md |
| CONFIRMED | CANCELLED | cashier/manager cancels | audit record required (docs/spec/security-rbac.md) |
| SENT_TO_KITCHEN | PREPARING | kitchen acknowledges (KDS) | per-station KOT status also progresses |
| SENT_TO_KITCHEN | CANCELLED | manager void with reason | must produce explicit cancellation KOT, not silent removal |
| PREPARING | READY | all stations report READY (expo/pass) | see docs/spec/kitchen.md §Stations |
| READY | SERVED | waiter marks served | |
| SERVED | BILLED | bill requested/generated | triggers compliance/invoice generation |
| BILLED | PAID | payment(s) allocated in full | see docs/spec/payments.md |
| PAID | CLOSED | shift/day-end reconciliation or immediate close | |
| any pre-PAID state | CANCELLED | authorized cancellation | never silently skips states |

## Invariants
- Illegal transitions (e.g. `CLOSED → DRAFT`, `PAID → DRAFT`) are rejected at the command layer, not merely hidden in the UI.
- Every transition is command-validated and produces an audit trail entry (who/what/when/where/device/old/new/reason) per docs/spec/security-rbac.md.
- Financial states (BILLED, PAID, CLOSED) are never destructively edited — corrections happen via void/refund/credit-note/adjustment (see docs/vision.md — immutable financial history).
- Multiple guests at one table may require independent per-check state machines (split checks) — see docs/spec/tables.md.
- Order state is edge-authoritative; sync to cloud is append-only replay of state-transition events, never a merged/overwritten row (docs/spec/sync.md).
