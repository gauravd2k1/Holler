# Spec: Payments

Owns: payment acceptance, split payments, Razorpay integration, reconciliation, cash drawer/shifts.
Source: HOLLER_MASTER_PROMPT.md §34–§39.

## Domain model
Never `order.paymentMethod = "UPI"` as a single field. Use: Payment, PaymentAttempt, PaymentAllocation, Refund, Settlement, ReconciliationRecord.

## Methods
Cash, UPI, Credit Card, Debit Card, Wallet, Gift Card, Loyalty Points, Bank Transfer, Aggregator-paid, House Account, Credit, Split. Example: ₹2,000 = ₹500 cash + ₹1,000 UPI + ₹500 card.

## Razorpay adapter
Dynamic UPI QR, payment creation/status, webhook handling, refunds, settlement retrieval, UTR tracking, payment links. Never store card details. Verify webhook signatures. Store external IDs. All webhook processing idempotent.

## Reconciliation
Distinct from acceptance. Example: order ₹1,000, captured ₹1,000, gateway fee ₹20 + GST ₹3.60 → expected settlement ₹976.40. States: UNMATCHED, MATCHED, PARTIALLY_MATCHED, RECONCILED, DISPUTED.

## Cash drawer & shifts
Shift Start, Opening Cash, Cash Sales, Cash Refunds, Paid In, Paid Out, Expected Cash, Actual Closing Cash, Variance (reason required). Cashier-specific registers.

## Conflict policy
Financial transactions: append-only; never last-write-wins.

## Milestone note
Milestone 3 covers cash + split payments only; online gateways/settlement/reconciliation land in Milestone 7.
