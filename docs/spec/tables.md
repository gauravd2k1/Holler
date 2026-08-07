# Spec: Tables

Owns: table/floor management, reservations linkage.
Source: HOLLER_MASTER_PROMPT.md §11, §45.

## Table management
Visual floor plans. States: AVAILABLE, OCCUPIED, ORDERED, KOT_SENT, FOOD_READY, BILL_REQUESTED, PAYMENT_PENDING, PAID, DIRTY, RESERVED.
Operations: move table, merge tables, split tables, merge orders, transfer items, seat count, waiter assignment, reservation assignment. Multiple guests at one table may need separate checks.

## Reservations
Fields: reservation date/time, guest count, customer, table assignment, special requests, deposit, source, status.
Status: BOOKED, CONFIRMED, ARRIVED, SEATED, NO_SHOW, CANCELLED. Integrates into table state.

## Cross-context dependencies
- Ordering (docs/spec/ordering.md) for dine-in order association.
- CRM (docs/spec/crm-loyalty.md) for customer identity on reservation.
