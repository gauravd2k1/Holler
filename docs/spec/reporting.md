# Spec: Reporting & Analytics

Owns: operational reports, analytics data model.
Source: HOLLER_MASTER_PROMPT.md §47–§48.

## Reports (minimum set)
Sales summary, hourly sales, day-part sales, outlet comparison, channel sales, payment report, tax report, discount report, cancelled orders, voided items, employee sales, table turnover, average order value, menu item performance, category performance, kitchen prep time, aggregator performance, stock report, consumption report, wastage, food cost, inventory variance, purchase report, supplier report, settlement report, cashier reconciliation.
Exports: CSV, XLSX, PDF where appropriate.

## Analytics data model
Keep operational and analytical workloads separate eventually. Start with PostgreSQL reporting tables/materialized views; introduce ClickHouse only when scale justifies it.

Budgets: outlet-level budget targets (daily/monthly/yearly) with
budget-vs-actual in the reporting views. (Competitive parity — Recaho, M8.)

## Milestone note
Milestone 1 ships only a basic order list; full reporting is built out through Milestone 8 (multi-outlet/analytics) and beyond.
