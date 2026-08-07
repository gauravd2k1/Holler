# Spec: Compliance (GST / Indian Regulatory)

Owns: TaxEngine, GST invoicing, ECO tax handling.
Source: HOLLER_MASTER_PROMPT.md §31–§33.

## Tax engine
Never scatter tax percentages through the app. Entities: TaxEngine, TaxRule, TaxProfile, ComplianceVersion — rules are effective-date/version based. Supports CGST/SGST/IGST/cess, tax-inclusive and -exclusive pricing, rounding. Every invoice stores a snapshot of the rules used, so historical bills stay reproducible after rule changes.

## Electronic Commerce Operator (ECO) handling
Every order records: channel, tax liability party, tax profile, operator, operator GST identifiers where required, supply classification. Direct dine-in sales and ECO-originated supplies must never be combined in compliance reporting. Reporting datasets separate: directly taxable supplies, ECO-liable supplies, refunds, cancellations, credit notes.
Do not auto-file GST returns in early releases — produce accountant-friendly validated export files first.

## GST invoice fields
Legal name, trade name, address, GSTIN, FSSAI number, invoice number, date/time, table/order id, item descriptions, HSN/SAC, taxable value, CGST/SGST/IGST, discount, round-off, grand total, payment mode, place of supply, QR/payment info, footer/legal text. Invoice numbering is configurable and concurrency-safe — duplicates are never generated.

## Cross-context dependencies
- Payments (docs/spec/payments.md) for grand total / payment mode.
- Ordering (docs/spec/ordering.md) — CanonicalOrder carries channel/tax-liability fields.
