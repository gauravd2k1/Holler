# Spec: Kitchen

Owns: KOT system, kitchen stations, KDS, expo/pass.
Source: HOLLER_MASTER_PROMPT.md §12, §13, §14.

## KOT (Kitchen Order Ticket)
Never "print the entire order" — each menu item routes to a production station (e.g. Butter Chicken → MAIN_KITCHEN, Beer → BAR, Garlic Naan → TANDOOR, Brownie → DESSERT). One order can produce multiple station tickets.

Fields: KOT ID, Order ID, Sequence, Station, Items, Modifiers, Notes, Timestamp, User, Status.
Statuses: NEW, ACKNOWLEDGED, PREPARING, READY, SERVED, CANCELLED.

Maintain change history explicitly (additions/cancellations visible to kitchen), e.g. KOT #132 → #132-A (addition) → #132-C (cancellation).

## KDS
Cards: order number, channel, table/customer, elapsed time, items, modifiers, allergies/notes, priority, rider status.
Flow: NEW → ACCEPTED → PREPARING → READY → SERVED.
SLA urgency thresholds configurable (e.g. GREEN <8min, AMBER 8–12, RED >12) — never color-only, always show time/status too.

## Stations
Kitchen, Tandoor, Chinese, Bar, Dessert, Beverage, Packaging. An item may route to more than one station. Expo/pass screen shows all components required before an order counts as ready.

## Performance target
LAN POS→KDS propagation <250ms.

## Cross-context dependencies
- Ordering (docs/spec/ordering.md) triggers KOT creation on send-to-kitchen.
- Hardware/Printing (docs/spec/hardware-printing.md) for KOT printer routing.
- Sync (docs/spec/sync.md) — KOTs are edge-authoritative, append-only.
