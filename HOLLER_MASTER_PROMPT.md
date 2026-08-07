# HOLLER — STATE-OF-THE-ART RESTAURANT OPERATING SYSTEM FOR INDIA
# MASTER PROMPT v2

You are acting simultaneously as:

* Principal Software Architect
* Staff Backend Engineer
* Staff Frontend Engineer
* Distributed Systems Engineer
* Restaurant POS Domain Expert
* Database Architect
* DevSecOps Engineer
* Indian GST/restaurant-compliance-aware software architect
* Product Designer
* QA/Test Automation Architect

Your task is to **design and incrementally build a production-grade restaurant operating system named HOLLER from scratch**, for Indian restaurants, cafés, QSRs, cloud kitchens, fine dining and multi-outlet chains.

Holler must compete technically and functionally with systems such as Petpooja, Restroworks/Posist, UrbanPiper, DineOpen, Bhojan Setu, DotPe, GoFrugal, SlickPOS, TMBill. Do NOT clone any proprietary product or interface. Use the capabilities expected from modern restaurant-management systems and create a cleaner, faster, modular architecture.

Core philosophy:

> HOLLER MUST CONTINUE RUNNING EVEN WHEN THE INTERNET DOES NOT.

Holler feels instantaneous at the counter and remains functional during internet outages. The architecture is LOCAL-FIRST, not CLOUD-DEPENDENT.

---

# 0. HOW THIS DOCUMENT MUST BE USED (READ FIRST)

This is the **master specification**, not a per-session context file. Loading this entire document into every agent session wastes context and money. Therefore:

## 0.1 First action: decompose this document

As part of MILESTONE 0, split this specification into the repository:

* `CLAUDE.md` (≤150 lines) — tech stack summary, coding rules (§83–§84), money/time/identifier conventions (§72–§74), agent working style and response rules (§85, §95), directory ownership map, test/build commands, and the current milestone's scope + exclusion list. **This is the only file every agent always reads.**
* `docs/spec/<context>.md` — one file per bounded context: `ordering.md`, `menu.md`, `kitchen.md`, `tables.md`, `inventory.md`, `procurement.md`, `payments.md`, `aggregators.md`, `compliance.md`, `crm-loyalty.md`, `sync.md`, `hardware-printing.md`, `reporting.md`, `multi-outlet.md`, `security-rbac.md`. Copy the relevant sections of this document into these files verbatim, enriched as design progresses. **An agent working on a module loads only CLAUDE.md + its assigned spec file(s) + `packages/contracts/`.**
* `docs/vision.md` — product vision, principles, competitive positioning (§1, §2, §88, §93). Read by humans and by the orchestrator when planning, never by builder agents.

## 0.2 Progressive disclosure rule

Never feed a builder agent spec content outside its assigned module. An agent implementing the KOT router must not receive loyalty, WhatsApp, or voice-POS content.

## 0.3 Scope guard

If any single task appears to require modifying more than ~15 files, STOP, output the plan and file list, and wait for confirmation instead of proceeding. Large blast radius means the task was decomposed incorrectly.

---

# 1. PRODUCT VISION

Holler should eventually become a complete **Restaurant Operating System** covering:

1. Point of Sale
2. Table management
3. Order management
4. KOT management
5. Kitchen Display System
6. Online aggregator integration
7. Inventory
8. Recipes
9. Procurement
10. Central kitchen
11. Payments
12. GST invoicing
13. Settlement/reconciliation
14. Customer CRM
15. Loyalty
16. Online ordering
17. QR ordering
18. Reservations
19. Staff management
20. Multi-outlet management
21. Analytics
22. Menu engineering
23. Accounting exports/integrations
24. AI-assisted operations

Holler is NOT merely a billing application. Architect it as an extensible **Restaurant Commerce and Operations Platform**.

---

# 2. PRIMARY PRODUCT PRINCIPLES

Apply these principles to every architectural decision.

## 2.1 Local-first

Core restaurant operations must work without internet connectivity. Offline operation MUST support: dine-in orders, takeaway orders, local delivery orders, table management, KOT generation, KDS, modifiers, discounts subject to permissions, taxes, bill generation, cash payments, offline/manual card records, inventory deduction, shift operations, printing, basic customer lookup from locally cached data.

Cloud services enhance the system rather than being required for restaurant operation.

## 2.2 Extremely fast

Performance is a first-class feature. Targets:

* menu interaction perceived as instantaneous
* add-to-cart <50 ms
* table retrieval <100 ms
* order persistence <100 ms locally
* KOT creation <100 ms
* LAN POS→KDS propagation <250 ms
* GST bill computation <100 ms
* invoice creation <300 ms
* typical screen transitions <100 ms
* initial launch <2 seconds on recommended hardware

Avoid unnecessary network round trips. Never make a cloud request in the critical order-entry path unless absolutely necessary.

## 2.3 Zero lost orders

Orders are financial records. Never silently lose or overwrite: orders, KOTs, payments, refunds, aggregator events, inventory transactions, stock transfers. Every important operation generates a durable event/audit record.

## 2.4 Idempotency

External integrations WILL resend webhooks. Every externally initiated transaction must support idempotency: Swiggy order events, Zomato order events, Razorpay webhooks, refund callbacks, settlement imports, menu synchronization requests. Duplicate delivery of the same event MUST NOT create duplicate orders, payments, KOTs, stock deductions, or refunds.

## 2.5 Immutable financial history

Never modify historical financial records destructively. Corrections use: cancellation, void, credit note, refund, adjustment, reversal. Maintain complete audit history.

## 2.6 Contracts first (NEW)

Shared interfaces are defined and frozen **before** parallel implementation begins. See §3.7 and MILESTONE 0.5. Builder agents implement against contracts; they never invent or modify them.

---

# 3. RECOMMENDED TECHNOLOGY ARCHITECTURE

Prefer this architecture unless strong technical evidence suggests otherwise.

## 3.1 POS desktop

Use: **Tauri, React, TypeScript, Rust, SQLite**.

Tauri provides a much smaller runtime footprint than Electron while allowing a modern web UI.

Rust handles: local database access, printer abstraction, device interfaces, synchronization, LAN communication, local background services, encryption, filesystem operations, hardware interfaces where appropriate.

React/TypeScript handles presentation. Use: Vite, TanStack Query, Zustand, TanStack Router, React Hook Form, Zod. Avoid unnecessarily large dependency chains.

## 3.2 Cloud backend

Use: **Go, PostgreSQL, Redis**. Event/message system: **NATS JetStream**. Do NOT begin with Kafka unless actual scale demands it.

Backend is initially a **Modular Monolith** with strongly isolated bounded contexts. Do NOT create 40 microservices for architectural fashion. Each module has clear internal interfaces so it can later become a service if required.

## 3.3 Web administration

Use: React, TypeScript, Vite, TanStack Query, TanStack Router. Responsive owner/admin dashboards.

## 3.4 KDS

Initially a responsive web/PWA application. Optionally package later using Tauri or native Android. KDS must work on the restaurant LAN whenever the internet is unavailable.

## 3.5 Waiter application

**DECIDED: Flutter.** Android-first, because affordable Android phones/tablets dominate restaurant staff use in India. Do not write an evaluation of Flutter vs React Native; the decision is made. Record it as an ADR and proceed.

## 3.6 Databases

Cloud: PostgreSQL. Restaurant edge/local: SQLite in WAL mode. Explicit synchronization between edge SQLite and cloud PostgreSQL (§50). Never expose local SQLite files directly to UI code.

## 3.7 Shared contracts layer (NEW)

`packages/contracts/` is the single source of truth for all cross-boundary shapes:

* TypeScript types + Zod schemas for all API request/response bodies and realtime events
* OpenAPI specification generated from or validated against these schemas
* Event payload definitions for every event in §49 (JSON Schema)
* Canonical order model (§16) as a versioned schema
* SQLite and PostgreSQL migration files (schema ownership documented per module)
* Go structs generated or mirrored from the same definitions, with drift-detection tests

Rules:

1. Contract changes are **serialized**: only the orchestrator/main session (or the architect agent) edits contracts, never a parallel builder.
2. Every contract change increments a version and is recorded in `docs/adr/` if it changes semantics.
3. CI includes a contract-drift check: Go, TypeScript, and Rust representations must round-trip the same fixtures.
4. Builder agents treat `packages/contracts/` as read-only.

## 3.8 Development hardware profile (NEW)

Primary dev machine: Windows laptop, Intel i7-9750H, 16 GB RAM, GTX 1050. Consequences:

* Docker services (PostgreSQL, Redis, NATS) and the Go backend run inside **WSL2**.
* Tauri/Rust Windows builds run on the **Windows side** (MSVC toolchain).
* Cap concurrent agent sessions at **3**.
* Keep Docker Compose memory limits explicit; prefer Alpine-based images.
* Repository lives in the WSL2 filesystem (`~/code/holler`) for I/O speed; Windows-side Tauri builds use a checkout or worktree accessible to Windows when needed.

---

# 4. HIGH-LEVEL ARCHITECTURE

Implement approximately:

```
                  ┌──────────────────────────────┐
                  │        HOLLER CLOUD          │
                  │ API Gateway / Auth           │
                  │ Orders                       │
                  │ Menu                         │
                  │ Inventory                    │
                  │ Aggregators                  │
                  │ Payments                     │
                  │ Analytics                    │
                  │ CRM                          │
                  │ Multi-outlet                 │
                  │ Integrations                 │
                  │ PostgreSQL / Redis / NATS    │
                  └──────────────┬───────────────┘
                                 │
                           Secure Sync
                                 │
                      Internet available?
                          /           \
                        YES            NO
                        │               │
           ┌────────────▼────────────────────┐
           │      HOLLER EDGE NODE           │
           │ SQLite                          │
           │ Sync Engine                     │
           │ Local Event Log                 │
           │ Printer Service                 │
           │ KDS Gateway                     │
           │ Device Gateway                  │
           │ Local WebSocket Server          │
           │ LAN Discovery                   │
           └────────────┬────────────────────┘
                        │ LAN
         ┌──────────────┼───────────────┐
         │              │               │
         ▼              ▼               ▼
       POS #1          POS #2           KDS
         │                              │
         ▼                              ▼
      Cashier                       Kitchen
         │
         ├──────── Waiter devices
         ├──────── QR orders
         └──────── Printers
```

Aggregator traffic arriving through the cloud syncs reliably into the local restaurant edge node and subsequently to KDS/POS.

---

# 5. MULTI-TENANT DOMAIN MODEL

Hierarchy:

```
Organisation
└── Brand
    └── Outlet
        ├── Revenue Centers
        ├── Floors
        ├── Tables
        ├── Kitchens
        ├── Stations
        ├── Registers
        └── Devices
```

Revenue center examples: restaurant, bar, bakery, room service, takeaway, delivery, banquet, food-court counter.

Never assume one restaurant equals one outlet.

---

# 6. AUTHENTICATION AND RBAC

Roles: Platform Super Admin, Organisation Owner, Brand Admin, Regional Manager, Outlet Manager, Accountant, Inventory Manager, Purchase Manager, Chef, Kitchen Staff, Captain, Waiter, Cashier, Delivery Staff, Auditor.

Granular permissions, e.g.:

```
order.create  order.modify  order.cancel  order.void
bill.discount  bill.discount.override  bill.reprint  bill.cancel
payment.refund  cash_drawer.open
inventory.adjust  inventory.transfer  recipe.modify
purchase.approve
reports.view_cost  reports.view_profit
user.manage
```

Sensitive actions optionally require manager PIN approval.

Audit record for every sensitive action: who, what, when, where, device, old value, new value, reason.

---

# 7. POINT OF SALE

Extremely fast touch-first POS. Order types: Dine In, Takeaway, Delivery, Aggregator, QR Order, Room Service, Catering. Order creation must require as few interactions as possible.

---

# 8. POS INTERFACE

Primary screen — LEFT: categories. CENTER: large menu item grid. RIGHT: current cart/order. TOP: search, order type, customer/table. BOTTOM: subtotal, taxes, discount, payment/hold/send-KOT actions.

Support: keyboard operation, touch operation, barcode input, PLU codes, favorites, item shortcuts, recent items, menu search, fuzzy search, configurable quick keys.

A trained cashier should be able to create a common order almost entirely from muscle memory.

---

# 9. MENU ENGINE

Entities: Menu, Category, Subcategory, MenuItem, Variant, ModifierGroup, Modifier, Combo, TaxProfile, PriceBook, AvailabilityRule, OrderType, Channel, KitchenStation.

Support: multiple menus, breakfast/lunch/dinner menus, day-of-week menus, happy hour, outlet-specific menus, aggregator menus, different prices by channel / outlet / order type.

Example — Butter Chicken: Dine-in ₹410, Takeaway ₹420, Zomato ₹459, Swiggy ₹459. Do not duplicate the underlying product; use **channel price books**.

---

# 10. MODIFIERS

Support complex modifier trees:

```
Pizza
├── Size: Regular | Medium | Large
├── Crust: Thin | Cheese Burst
└── Toppings: Paneer | Mushroom | Jalapeño
```

Support: required modifiers, optional modifiers, min selection, max selection, repeated modifiers, nested modifiers, price deltas, recipe implications.

Modifiers must affect inventory: if "Extra Paneer +50g" is selected, another 50g of paneer is deducted.

---

# 11. TABLE MANAGEMENT

Visual floor plans. Table states: AVAILABLE, OCCUPIED, ORDERED, KOT_SENT, FOOD_READY, BILL_REQUESTED, PAYMENT_PENDING, PAID, DIRTY, RESERVED.

Support: move table, merge tables, split tables, merge orders, transfer items, seat count, waiter assignment, reservation assignment. Multiple guests at the same table may require separate checks.

---

# 12. KOT SYSTEM

KOT = Kitchen Order Ticket. Never treat KOT as "print the entire order." Each menu item maps to a production station:

```
Butter Chicken → MAIN_KITCHEN
Beer → BAR
Garlic Naan → TANDOOR
Brownie → DESSERT
```

An order containing all of these produces separate station tickets.

Maintain: KOT ID, Order ID, Sequence, Station, Items, Modifiers, Notes, Timestamp, User, Status. Statuses: NEW, ACKNOWLEDGED, PREPARING, READY, SERVED, CANCELLED.

Maintain change history. Example: KOT #132 (Butter Chicken x2), later KOT #132-A (Garlic Naan x2), later cancellation KOT #132-C (Butter Chicken x1). Kitchen must see explicit additions/cancellations.

---

# 13. KITCHEN DISPLAY SYSTEM

Dedicated KDS. Cards display: order number, channel, table/customer, elapsed time, items, modifiers, allergies/notes, priority, rider status where relevant.

Flow: NEW → ACCEPTED → PREPARING → READY → SERVED.

Configurable SLA urgency, e.g. GREEN <8 min, AMBER 8–12, RED >12. Do not depend exclusively on color; also show time/status for accessibility.

---

# 14. KITCHEN STATIONS

Stations: Kitchen, Tandoor, Chinese, Bar, Dessert, Beverage, Packaging. Allow an item to route to more than one station. Implement expo/pass screen showing all components necessary before an order is considered ready.

---

# 15. AGGREGATOR SYNC

THIS IS A CORE HOLLER FEATURE. Create an independent bounded context: `aggregator_gateway`. Do not put Zomato-specific or Swiggy-specific logic inside core Order code.

```
interface AggregatorProvider {
  ConnectOutlet(); DisconnectOutlet();
  FetchMenu(); PublishMenu();
  UpdateItemAvailability(); UpdateStoreAvailability();
  ReceiveOrder(); AcceptOrder(); RejectOrder();
  UpdatePreparationTime(); MarkFoodReady();
  CancelOrder();
  GetRiderStatus();
  FetchSettlementData();
}
```

Adapters: ZomatoAdapter, SwiggyAdapter, UrbanPiperAdapter, MockAggregatorAdapter. Future: ONDCAdapter, MagicpinAdapter, DirectOrderingAdapter.

IMPORTANT: Never scrape Swiggy or Zomato websites. Never use unofficial/private APIs. Use only officially authorized partner APIs. Because direct platform API access may require commercial/partner approval, the architecture MUST support intermediary providers such as UrbanPiper while retaining the ability to use direct APIs later.

---

# 16. NORMALIZED ORDER MODEL

Every incoming channel maps into Holler's canonical order format (versioned schema in `packages/contracts/`):

```
CanonicalOrder {
  holler_order_id, external_order_id, source, outlet_id,
  customer, delivery_address,
  items[], modifiers[],
  subtotal, discount, packaging, delivery_charge, taxes,
  aggregator_discount, merchant_discount, total,
  payment_status, payment_source,
  preparation_time, rider,
  timestamps, source_payload
}
```

Store the raw external payload for audit/debug purposes.

---

# 17. AGGREGATOR EVENT FLOW

```
Swiggy → Webhook → Aggregator Gateway
  ├── verify authenticity
  ├── deduplicate
  ├── normalize
  └── persist raw event
→ Order Service → Outlet Sync → Holler Edge → KOT Router
  ├── Kitchen
  ├── Tandoor
  └── Packaging
→ KDS
```

Design retry/dead-letter mechanisms. Never silently discard malformed external messages.

---

# 18. MENU SYNCHRONIZATION

Holler becomes the master menu if the restaurant chooses that mode:

```
        HOLLER Master Menu Catalog
      ┌─────────┼─────────┐
      ▼         ▼         ▼
   Swiggy     Zomato     QR/Web
```

Synchronize: item names, categories, prices, variants, modifiers, taxes/charges where API supports them, availability, store hours, images, descriptions. Provide channel-specific overrides.

---

# 19. ITEM SNOOZE / STOCK-OUT

If paneer stock hits zero, Holler determines which menu items depend on paneer (Paneer Tikka, Kadai Paneer, Paneer Butter Masala, Paneer Roll) and optionally marks them unavailable across POS, QR menu, Holler Direct, Swiggy, Zomato. Restoring stock optionally restores channel availability. Allow manager override.

---

# 20. INVENTORY ARCHITECTURE

Do NOT create basic "menu item stock." Create a proper food inventory engine:

```
Raw Material → Semi-Finished Product → Recipe → Menu Item
```

Example: Tomato, Onion, Cashew, Butter, Cream, Spices → Makhani Gravy → Butter Chicken.

---

# 21. INVENTORY ITEM MODEL

Each raw material supports: SKU, Name, Category, Base unit, Purchase unit, Conversion, Yield percentage, Wastage percentage, Current cost, Weighted average cost, Last purchase price, Reorder level, Par level, Supplier, Storage location, Batch, Expiry, Tax, Outlet.

Units: kg, g, litre, ml, piece, dozen, packet, bottle, tray, portion. Conversions explicit (1 bag flour = 25 kg). Never rely solely on display units; internally normalize quantities.

---

# 22. RECIPES

```
Recipe { ingredients, quantity, unit, yield, preparation loss, sub-recipes }
```

Example — BUTTER CHICKEN, one serving: Chicken 220 g, Makhani gravy 180 ml, Butter 20 g, Cream 30 ml, Kasuri methi 2 g. Confirming one Butter Chicken for production produces exactly those theoretical inventory deductions.

---

# 23. SEMI-FINISHED INVENTORY

Support batch production. Example — Makhani Gravy Batch: input Tomato 5 kg, Onion 2 kg, Cashew 500 g, Butter 500 g, Spices 100 g; output 8 litres. Track: input cost, production yield, actual vs expected output, variance, wastage, batch number, production timestamp, expiry.

---

# 24. INVENTORY LEDGER

Immutable stock ledger. Never merely overwrite `stock = 25`. Record transactions:

```
PURCHASE +50kg | CONSUMPTION -3kg | WASTAGE -1kg
TRANSFER_OUT -5kg | TRANSFER_IN +5kg | ADJUSTMENT -0.5kg
RETURN_TO_VENDOR -2kg | PRODUCTION_CONSUMPTION -10kg | PRODUCTION_OUTPUT +8kg
```

Current stock is derived from ledger entries. For performance, maintain projections/materialized balances, but the ledger remains source of truth.

---

# 25. THEORETICAL VS ACTUAL CONSUMPTION

Essential. THEORETICAL consumption from recipes × items sold; ACTUAL from Opening Stock + Purchases + Transfers In − Transfers Out − Closing Stock. Produce Variance Quantity, Value, %. Example: Chicken theoretical 38.4 kg, actual 42.1 kg → variance 3.7 kg. Highlight potential over-portioning, waste, spoilage, recipe error, theft/pilferage.

---

# 26. FOOD COST

Calculate: ingredient cost, recipe cost, menu food cost, food cost %, contribution margin. Example: selling price ₹420, recipe cost ₹126 → food cost 30%. Track changing purchase costs automatically. Menu engineering matrix: STAR, PLOWHORSE, PUZZLE, DOG based on profitability and popularity.

---

# 27. PROCUREMENT

Entities: Supplier, Purchase Requisition, RFQ, Purchase Order, Goods Receipt Note, Supplier Invoice, Purchase Return, Supplier Credit, Payment status.

Flow: Stock Low → Purchase Requisition → Approval → Purchase Order → Supplier → GRN → Inventory → Invoice → Accounts. Support approval limits.

---

# 28. CENTRAL KITCHEN

Model central kitchen as inventory/production location. Flow: Outlet raises indent → Central kitchen approves → Production → Dispatch → Goods in transit → Outlet receives → Inventory updated. Maintain variance between dispatched and received quantities.

---

# 29. BATCH AND EXPIRY

Support FEFO (First Expiry, First Out). Track: batch, manufacture date, expiry date, quantity, supplier, purchase document, storage location. Alerts: expiring in 1 / 3 / 7 days.

---

# 30. WASTAGE

Record: ingredient, quantity, value, reason, employee, timestamp, photo optional, manager approval. Reasons: Spoilage, Overproduction, Preparation loss, Customer return, Kitchen mistake, Breakage, Expired, Unknown. Generate wastage reports.

---

# 31. GST AND INDIAN COMPLIANCE ENGINE

Do NOT scatter tax percentages throughout the application. Create: TaxEngine, TaxRule, TaxProfile, ComplianceVersion. Tax rules are effective-date/version based.

Support: CGST, SGST, IGST, cess where relevant, restaurant tax configurations, different tax rules per business scenario, tax-inclusive prices, tax-exclusive prices, rounding.

Store snapshots of the rules used for every invoice. Historical bills must remain reproducible even after tax rules change.

---

# 32. ELECTRONIC COMMERCE OPERATOR GST HANDLING

Aggregator-originated restaurant orders can have different compliance treatment from direct restaurant supplies. Every order must know: channel, tax liability party, tax profile, operator, operator GST identifiers where required, supply classification.

Do not incorrectly combine direct dine-in sales and ECO supplies in compliance reporting. Provide GST reporting datasets separating: directly taxable supplies, ECO-originated supplies where the ECO bears statutory liability, refunds, cancellations, credit notes.

Do not attempt to file GST returns automatically in the first releases. Generate accountant-friendly validated reports/export files first.

---

# 33. GST INVOICE

Invoice supports: restaurant legal name, trade name, address, GSTIN, FSSAI number, invoice number, invoice date/time, table/order identifier, item descriptions, HSN/SAC where applicable, taxable value, CGST, SGST, IGST, discount, round-off, grand total, payment mode, place of supply where applicable, QR/payment information, footer/legal text.

Invoice numbering must be configurable and concurrency-safe. Never generate duplicate invoice numbers.

---

# 34. PAYMENT ARCHITECTURE

Separate bounded context: Payment Service. Never store only `order.paymentMethod = "UPI"`. Use: Payment, PaymentAttempt, PaymentAllocation, Refund, Settlement, ReconciliationRecord.

---

# 35. PAYMENT METHODS

Cash, UPI, Credit Card, Debit Card, Wallet, Gift Card, Loyalty Points, Bank Transfer, Aggregator-paid, House Account, Credit, Multiple/Split payment. Example: ₹2,000 bill = ₹500 cash + ₹1,000 UPI + ₹500 card.

---

# 36. RAZORPAY INTEGRATION

Implement RazorpayAdapter. Where APIs/products permit: dynamic UPI QR, payment creation, payment status, webhook handling, refunds, settlement retrieval, UTR tracking, payment links.

Never store card details. Verify webhook signatures. Store external IDs. All webhook processing must be idempotent.

---

# 37. PAYMENT RECONCILIATION

Different from payment acceptance. Example: order ₹1,000, captured ₹1,000, gateway fee ₹20, GST on fee ₹3.60, expected settlement ₹976.40, actual ₹976.40 → RECONCILED.

States: UNMATCHED, MATCHED, PARTIALLY_MATCHED, RECONCILED, DISPUTED.

---

# 38. AGGREGATOR RECONCILIATION

Track per settlement: gross order value, merchant discount, platform discount, commission, commission tax, delivery charges where applicable, packaging, adjustments, refund, cancellation, TDS/TCS where applicable, other fees, net settlement, settlement reference, settlement date.

Build order-level reconciliation. Owner sees Expected Receivable vs Actual Settlement with discrepancies highlighted.

---

# 39. CASH DRAWER AND SHIFTS

Shift Start, Opening Cash, Cash Sales, Cash Refunds, Paid In, Paid Out, Expected Cash, Actual Closing Cash, Variance. Require reason for variance. Support cashier-specific registers.

---

# 40. ONLINE/DIRECT ORDERING (FUTURE MODULE)

Holler Direct: `restaurant.holler.app` or customer's custom domain. Online menu, pickup, delivery, scheduled orders, payments, offers, loyalty, customer account, order tracking. Orders use the same Order Service as POS/aggregators — do not build separate order logic.

---

# 41. QR ORDERING

Each table can have QR: `/o/{outlet}/{table-token}`. Customer can: browse, order, repeat order, request waiter, request water, request bill, pay. Orders appear directly in Holler. Restaurant chooses: auto-send to kitchen, or waiter approval required.

---

# 42. CRM

Customer profile: phone, name, email, birthday optional, anniversary optional, visits, orders, lifetime value, average ticket, favorite items, preferred outlet, last visit, loyalty points, consent preferences. Privacy considered carefully; do not collect unnecessary personal information.

---

# 43. LOYALTY

Points, cashback, visit rewards, tiers, coupons, wallet, referral, campaigns. Examples: ₹1 = 1 point, or 5% cashback. Support expiry policies.

---

# 44. WHATSAPP INTEGRATION

Adapter for official WhatsApp Business APIs only. Uses: digital invoices, order confirmation, reservation confirmation, feedback, loyalty notifications, opt-in marketing. Never implement bulk spam workflows. Maintain consent.

---

# 45. RESERVATIONS

Fields: reservation date/time, guest count, customer, table assignment, special requests, deposit, source, status. Status: BOOKED, CONFIRMED, ARRIVED, SEATED, NO_SHOW, CANCELLED. Integrate into table management.

---

# 46. MULTI-OUTLET MANAGEMENT

Central admin manages: menus, recipes, pricing, tax rules, staff, suppliers, inventory templates, promotions, analytics. Allow inheritance: Brand Menu → Outlet Override → Channel Override. Avoid manually duplicating data per outlet.

---

# 47. REPORTING

At minimum: sales summary, hourly sales, day-part sales, outlet comparison, channel sales, payment report, tax report, discount report, cancelled orders, voided items, employee sales, table turnover, average order value, menu item performance, category performance, kitchen preparation time, aggregator performance, stock report, consumption report, wastage, food cost, inventory variance, purchase report, supplier report, settlement report, cashier reconciliation.

Exports: CSV, XLSX, PDF where appropriate.

---

# 48. ANALYTICS DATA MODEL

Separate operational and analytical workloads eventually. Initially: PostgreSQL reporting tables/materialized views. At scale consider ClickHouse — do not introduce it until justified.

---

# 49. EVENT MODEL

Important business events are immutable: OrderCreated, ItemAdded, ItemRemoved, KOTCreated, OrderAccepted, OrderPrepared, OrderReady, InvoiceCreated, PaymentReceived, PaymentRefunded, StockConsumed, StockAdjusted, PurchaseReceived, AggregatorOrderReceived, SettlementReceived.

Publish via **transactional outbox**. Never `commit → publish` without an outbox or equivalent consistency mechanism. All event payload schemas live in `packages/contracts/`.

---

# 50. LOCAL ↔ CLOUD SYNCHRONIZATION

Every locally created record has: id, tenant_id, outlet_id, device_id, created_at, updated_at, version, sync_status. Use UUIDv7 or ULID-style sortable identifiers. Synchronization must be resumable:

```
local operation → SQLite transaction → local outbox → sync worker
→ cloud API → cloud transaction → ack → mark synchronized
```

Never delete local transactions immediately after synchronization.

## 50.1 Authority rule (NEW — resolves ambiguity; do not redesign)

* **Cloud is the source of truth for catalog and configuration**: menu, price books, tax profiles, users, roles, outlet settings. These sync **down** to edge, versioned; the edge applies the latest authorized version.
* **Edge is the source of truth for operational transactions**: orders, KOTs, payments, shifts, stock movements. These sync **up** to cloud, append-only.
* Transactions are never merged; they are replayed. Config is never appended; it is versioned and replaced.
* Do NOT introduce CRDTs or bidirectional merge machinery. This authority split plus §51's per-aggregate policies is the design.

---

# 51. CONFLICT RESOLUTION

Explicit policy per aggregate:

* Financial transactions: append-only; never last-write-wins.
* Menu description: version-based merge/admin resolution.
* Inventory transaction: append-only ledger.
* Availability: latest authorized version.
* Order: state machine and command validation.

Document the conflict policy for every major aggregate in its spec file.

---

# 52. ORDER STATE MACHINE

```
DRAFT → CONFIRMED → SENT_TO_KITCHEN → PREPARING → READY
→ SERVED → BILLED → PAID → CLOSED
Alternative: CANCELLED
```

Prevent illegal transitions (e.g., CLOSED → DRAFT must never happen).

---

# 53. HARDWARE ABSTRACTION

Device Service. Printer adapters: ESC/POS, Network, USB, Bluetooth where supported. Support 80mm receipt, 58mm receipt, KOT printers, label printers. Future: weighing scales, barcode scanners, customer displays, cash drawer, payment terminals. Hardware-specific code must never contaminate domain services.

---

# 54. PRINTING

Template engine. Templates: GST invoice, KOT, proforma bill, receipt, settlement summary, purchase order, GRN. Printer routing: Tandoor → Printer A, Bar → Printer B, Main Kitchen → Printer C. Automatic retry and print spool. Failure to print must be visible. Do not duplicate a KOT because a printer acknowledged late.

---

# 55. OBSERVABILITY

Structured logs, metrics, distributed tracing, error tracking, audit events. Metrics: orders/minute, KOT latency, sync delay, aggregator failures, payment failures, printer failures, inventory sync failures, API latency, database latency. Use OpenTelemetry.

---

# 56. SECURITY

Follow OWASP principles. Required: TLS, Argon2id passwords, short-lived access tokens, refresh token rotation, secure secret storage, rate limits, RBAC, tenant isolation, audit logs, webhook signature verification, CSRF protection where relevant, XSS protections, SQL injection prevention, encrypted backups.

Do NOT log: passwords, tokens, card data, sensitive payment secrets.

---

# 57. TENANT ISOLATION

Every tenant-owned table must be securely scoped. No API request may retrieve another organization's records by changing an ID. Create automated tests specifically for cross-tenant access.

---

# 58. DATABASE MIGRATIONS

All schema changes use migrations. Never manually alter production schema. Every migration tested. Production migrations support safe upgrade paths. Migration ownership per module is documented in `packages/contracts/`.

---

# 59. BACKUPS

Cloud PostgreSQL: automated backups, PITR where deployment supports it. Local outlet: encrypted SQLite backups, cloud synchronization, local snapshot strategy. Document restore procedures. A backup never restored in testing is not a valid backup strategy.

---

# 60. AI CAPABILITIES (DESIGN ONLY — DO NOT SCAFFOLD)

AI is supplementary. Never place an LLM in critical billing logic. Core calculations must be deterministic.

**Until MILESTONE 10: do not create modules, directories, placeholder files, or stub interfaces for AI features.** Design mentions in ADRs are the only permitted artifact.

Future Holler Intelligence features:

* Natural-language analytics ("Why did food cost increase last week?", "Which five items became less profitable?", "Which outlet had abnormal discounting yesterday?", "How much paneer will we need this weekend?")
* Demand forecasting (item demand, ingredient consumption, hourly volumes) using historical data, weekday, season, weather if integrated, holidays, festivals, events, promotions, aggregator demand
* Procurement recommendations from current stock, forecast, lead time, par level, expiry, historical wastage
* Anomaly detection: unusual discounts, abnormally frequent voids, inventory shrinkage, refund patterns, cashier discrepancies, food-cost anomalies

AI must explain its reasoning using observable business data.

---

# 61. INDIA-AWARE FORECASTING (FUTURE)

Calendar features should eventually understand: Diwali, Holi, Navratri, Ramadan/Eid, Christmas, New Year, regional festivals, IPL/cricket events, local holidays. Restaurant demand is highly calendar-dependent.

---

# 62. OPTIONAL VOICE POS (FUTURE — DO NOT SCAFFOLD)

"Holler, add two masala dosa, one without onion." Pipeline: Speech Recognition → Intent Parsing → Menu Entity Resolution → Modifier Resolution → Confirmation → Order Command. Never automatically send uncertain speech results to the kitchen; confidence threshold + cashier confirmation is mandatory. **No code, stubs, or directories for this before it is explicitly scheduled.**

---

# 63. UI DESIGN LANGUAGE

Fast, calm, clean, touch-friendly, information dense without clutter. Avoid excessive animations, glassmorphism, gradients, huge whitespace, decorative cards. Restaurant POS users operate under pressure — functional density is desirable. Large touch targets. Critical operations obvious.

---

# 64. ERROR DESIGN

Never display "Something went wrong." Instead:

* "Kitchen printer unavailable — KOT saved and queued for retry."
* "Internet unavailable — order stored locally."
* "Swiggy synchronization delayed — retrying automatically."
* "Payment received but settlement not yet verified."

Errors tell staff whether intervention is necessary.

---

# 65. DEVELOPMENT METHODOLOGY

TDD. Every module requires: unit tests, integration tests, contract tests where integrations exist, critical end-to-end tests. Do NOT write the entire product first and tests later.

---

# 66. TEST CRITICAL FINANCIAL LOGIC

Property-based tests cover: tax calculations, rounding, discounts, split payments, refund allocation, inventory unit conversions, recipe deduction, settlement calculations. Use generated/random test data where useful.

---

# 67. FAILURE TESTS

Explicitly test: internet disconnect during billing; internet returns after 6 hours; cloud API unavailable; duplicate aggregator webhook; duplicate Razorpay webhook; printer offline; printer out of paper; KDS reconnect; power failure after local order commit; sync retry; payment received twice; payment callback delayed; two terminals edit same order; menu modified while offline; inventory adjustment during offline period; outlet clock temporarily incorrect.

---

# 68. LOAD TESTS

Simulate: 10 POS terminals, 20 waiter devices, 6 KDS displays, 5 printers, 500 orders/hour, multiple aggregator channels. Measure P50/P95/P99 for: order creation, KOT propagation, payment confirmation, menu query, inventory deduction.

---

# 69. REPOSITORY STRUCTURE

```
holler/
├── CLAUDE.md
├── apps/
│   ├── pos/            # Tauri + React POS
│   ├── admin/          # Web admin
│   ├── kds/            # PWA KDS
│   ├── waiter/         # Flutter
│   └── customer-ordering/
├── edge/
│   ├── sync/
│   ├── printer/
│   ├── device/
│   └── database/
├── backend/
│   ├── cmd/
│   ├── internal/
│   │   ├── auth/ tenant/ outlet/ menu/ ordering/ kitchen/
│   │   ├── inventory/ procurement/ payments/ aggregators/
│   │   ├── compliance/ reporting/ crm/
│   └── migrations/
├── packages/
│   ├── contracts/      # SOURCE OF TRUTH for cross-boundary shapes (§3.7)
│   ├── ui/
│   ├── validation/
│   └── generated/
├── deployments/
│   ├── docker/ kubernetes/ terraform/
├── docs/
│   ├── vision.md
│   ├── spec/           # per-bounded-context specs (§0.1)
│   ├── architecture/ adr/ domain/ api/
├── tests/
│   ├── integration/ e2e/ load/
├── docker-compose.yml
├── Makefile
└── README.md
```

Modify if technically justified.

---

# 70. API DESIGN

REST initially for business APIs. Generate OpenAPI documentation (validated against `packages/contracts/`). WebSockets/SSE/local WebSocket transport for realtime as appropriate.

```
POST /orders            GET /orders/:id
POST /orders/:id/items  POST /orders/:id/send-to-kitchen
POST /orders/:id/cancel
POST /payments          POST /payments/:id/refund
GET  /inventory/items   POST /inventory/adjustments
POST /aggregators/webhooks/:provider
```

Do not expose database CRUD directly. Expose business commands.

---

# 71. DATABASE DESIGN

Proper relational modeling. No giant JSON blobs for core entities. JSONB acceptable for: external payloads, provider-specific metadata, configuration extensions. Core financial/operational entities in typed relational tables with foreign keys, constraints, unique constraints, indexes.

---

# 72. MONEY REPRESENTATION

Never use IEEE floating-point for money. Store INR in **paise as integers** (₹125.50 = 12550). Recipe quantities and unit conversions use fixed-precision decimal/numeric types.

---

# 73. TIME

Timestamps in UTC. Outlet timezone stored separately; render in outlet local time. Business-day calculation supports restaurants operating beyond midnight (e.g., trading day 06:00 → next day 05:59 counts as one business day).

---

# 74. IDENTIFIERS

Internally: globally unique sortable identifiers (UUIDv7/ULID). Human-facing numbers shorter: Order #A184, KOT #829, Invoice FY26/PNQ/001423. Never expose sequential database primary keys as security identifiers.

---

# 75. CONFIGURATION

Outlet settings: currency, timezone, business-day cutoff, rounding, service charge, tax profile, printer routing, KDS routing, order numbering, invoice numbering, payment providers, aggregator providers, menu, receipt template.

---

# 76. IMPORT/MIGRATION

Onboarding is a competitive differentiator. Import tools for: menu CSV/XLSX, inventory, recipes, customers, suppliers, opening stock. Eventually migration utilities from common POS exports. Import must validate data and generate error reports.

---

# 77. DEMO DATA

Realistic Indian restaurant demo data: "Holler Kitchen — Pune". Categories: Starters, North Indian, South Indian, Chinese, Breads, Rice, Beverages, Desserts. Include ~100 menu items, recipes, ingredients, suppliers, tables, staff, inventory, sample orders.

---

# 78. DEVOPS

Local development works with one command: `make dev` (or equivalent). Docker Compose for PostgreSQL, Redis, NATS, backend — **running inside WSL2** per §3.8, with explicit memory limits. Frontend runs natively for fast HMR.

CI pipeline: lint, format, unit tests, integration tests, **contract-drift check (§3.7)**, build, security scanning.

---

# 79. DEPLOYMENT

Initially AWS with portable containers: CloudFront, ALB, ECS/Fargate (Kubernetes only when justified), RDS PostgreSQL, ElastiCache, S3, CloudWatch/OpenTelemetry. Avoid premature Kubernetes complexity. Infrastructure via Terraform.

---

# 80. ARCHITECTURAL DECISION RECORDS

Maintain `docs/adr/`. Initial set:

```
ADR-001 Local First Architecture
ADR-002 Tauri for POS
ADR-003 SQLite WAL
ADR-004 Go Backend
ADR-005 NATS JetStream
ADR-006 PostgreSQL Multi-Tenancy
ADR-007 Transactional Outbox
ADR-008 Contracts-First Development     (NEW)
ADR-009 Sync Authority Split            (NEW — records §50.1)
ADR-010 Flutter for Waiter App          (NEW — records §3.5)
```

Each ADR: Context, Decision, Alternatives, Consequences.

---

# 81. IMPLEMENTATION PHASES

Do NOT attempt the whole application at once. Every milestone now carries an explicit **EXCLUDES** list. Agents must not scaffold, stub, or "prepare for" excluded items.

## MILESTONE 0 — Foundation

Deliver: repository, architecture docs, domain glossary, ADR structure (ADR-001…010), CI, Docker Compose (WSL2), PostgreSQL, Redis, NATS, backend skeleton, POS shell, KDS shell. **Also: decompose this master prompt per §0.1 into CLAUDE.md + docs/spec/ + docs/vision.md.**

No fake production implementation.

EXCLUDES: any business logic, any UI beyond empty shells, aggregators, payments, inventory, CRM, loyalty, AI.

## MILESTONE 0.5 — Contracts for the Vertical Slice (NEW)

Deliver, in `packages/contracts/`:

* SQLite schema for: outlet, device, menu (category/item/variant/modifier), order, order_item, kot, local_outbox, sync_state
* PostgreSQL migrations for: tenant, outlet, menu, order (mirroring the canonical shapes)
* TypeScript + Zod + Go types for: CanonicalOrder (§16), OrderCommand set, KOT, the M0–M2 event payloads (OrderCreated, ItemAdded, KOTCreated, OrderReady), sync envelope
* OpenAPI spec covering the §70 order endpoints needed for the slice
* Contract fixtures + round-trip drift tests wired into CI

Acceptance: contracts compile in Go, TypeScript, and Rust representations; fixtures round-trip identically; CI drift check passes.

**Contracts are frozen after this milestone.** Subsequent changes only via orchestrator/architect with an ADR note and version bump. Builder agents treat contracts as read-only.

EXCLUDES: implementation of any service using these contracts.

## MILESTONE 1 — Core POS

Deliver: organisation, outlet, users, RBAC, menu, categories, modifiers, tables, order creation, local SQLite, basic synchronization.

Acceptance: internet may be disconnected and the cashier can still create restaurant orders.

EXCLUDES: aggregators, payments beyond cash, inventory, recipes, loyalty, CRM, multi-outlet UI, reservations, QR ordering, reporting beyond a basic order list.

## MILESTONE 2 — Kitchen

Deliver: KOT, station routing, printer abstraction, KDS, LAN realtime delivery, order status.

Acceptance: POS → kitchen propagation below target latency on LAN.

EXCLUDES: aggregator KOTs, expo screen polish, label printers, waiter app.

## MILESTONE 3 — Billing

Deliver: tax engine, GST invoice, discounts, split bills, split payments, cash shift, invoice numbering. Extensive financial tests required (§66).

EXCLUDES: online payment gateways, settlement, reconciliation, ECO reporting outputs (model the fields now, report later).

## MILESTONE 4 — Inventory

Deliver: raw materials, units, recipes, subrecipes, inventory ledger, automatic consumption, wastage, stock counts, variance.

Acceptance: selling one menu item creates correct recipe-level stock ledger entries.

EXCLUDES: procurement, central kitchen, batch/expiry alerts (model fields, defer alerting).

## MILESTONE 5 — Procurement

Deliver: suppliers, PO, GRN, purchase, returns, stock transfers, central kitchen.

## MILESTONE 6 — Aggregators

Implement in order: MockAggregatorAdapter → UrbanPiperAdapter → official direct adapters when access/approval is available.

Deliver: incoming orders, canonical normalization, KOT, menu synchronization, availability synchronization, order statuses, retry, deduplication.

## MILESTONE 7 — Payments

Deliver: payment domain, Razorpay adapter, UPI QR, refund, settlement import, payment reconciliation.

## MILESTONE 8 — Multi-Outlet + Analytics

Deliver: brand management, central menu, central recipes, outlet overrides, executive dashboard, cost analytics.

## MILESTONE 9 — Customer Experience

Deliver: QR ordering, direct ordering, CRM, loyalty, reservations.

## MILESTONE 10 — Holler Intelligence

Deliver: forecasting, anomaly detection, natural language analytics, procurement recommendations. AI must never alter financial records without explicit authorized action.

---

# 82. DEFINITION OF DONE

A module is NOT complete because the UI renders. Definition of Done:

domain model implemented; database migration created; validation implemented; authorization implemented; audit events implemented; unit tests passing; integration tests passing; error handling implemented; observability implemented; documentation updated; no critical security warnings; no TODO mock business logic; performance reasonable; offline behavior tested where relevant; **contracts respected without modification**.

---

# 83. CODING RULES

Strict typing. Avoid `any`. Avoid global state unless justified. Business logic outside UI components. Database logic outside HTTP handlers. Provider-specific integrations behind interfaces. No magic numbers. No hard-coded taxes. No hard-coded restaurant IDs. No hard-coded URLs. No secrets committed. No duplicated business logic. Avoid giant services/classes/files. Use domain terminology consistently.

---

# 84. CODE QUALITY

Whenever generating code:

1. Show the filename.
2. Produce complete compilable code.
3. Update relevant tests.
4. Update migration if schema changed (respecting contract ownership).
5. Update documentation if architecture changed.
6. Run tests.
7. Fix failures.
8. Do not claim success without actual verification.

Never replace unfinished logic with `// TODO implement later` unless intentionally marking functionality outside the current milestone — and never for excluded-list items, which must not exist at all.

---

# 85. AGENT WORKING STYLE

You are not here to repeatedly brainstorm Holler. You are here to BUILD it.

Do not repeatedly ask "Would you like me to continue?" Proceed milestone-by-milestone. Only stop for clarification when a truly blocking architectural decision cannot safely be inferred. For ordinary implementation decisions, make a professional engineering decision and record it in an ADR when appropriate.

Additional rules for multi-agent operation:

* Builder agents receive: CLAUDE.md, their assigned `docs/spec/` file(s), and read-only `packages/contracts/`. Nothing else.
* Contract and shared-migration changes are performed only by the orchestrator/architect session, serialized, never in parallel.
* Respect the §0.3 scope guard (~15 file limit per task).
* Respect the current milestone's EXCLUDES list absolutely.
* Verification is performed by a separate read-only verification pass before any merge; a builder's own claim of success is insufficient.

---

# 86. SOURCE-CONTROL WORKFLOW

Small meaningful commits:

```
feat(ordering): add local order aggregate
feat(kitchen): implement station-aware KOT routing
feat(sync): add durable local outbox
test(payments): cover duplicate webhook handling
fix(inventory): prevent repeated recipe consumption
```

---

# 87. PERFORMANCE BUDGET

Maintain explicit benchmarks. Reject implementation changes causing major regressions. Target restaurant edge hardware includes modest devices, not developer workstations. Test using realistic lower-powered restaurant hardware profiles. Optimize: startup, SQLite queries, search, rendering, KDS propagation, printing, sync.

---

# 88. COMPETITIVE REQUIREMENT

Holler should ultimately meet or exceed leading restaurant POS platforms in: POS speed, offline operation, KOT, KDS, table management, online orders, aggregator integration, menu synchronization, inventory, recipe costing, central kitchen, procurement, payments, settlement reconciliation, GST reporting, multi-outlet operations, analytics.

Holler's specific competitive advantages:

1. exceptional speed
2. true local-first operation
3. no lost orders
4. restaurant-LAN resilience
5. sophisticated ingredient inventory
6. extremely transparent reconciliation
7. open integration architecture
8. explainable AI analytics
9. modern API-first architecture
10. lower operational complexity

---

# 89. FUTURE PLUGIN ARCHITECTURE

Design provider interfaces now so future integrations do not modify core domains: Tally, Zoho Books, QuickBooks, Razorpay, Pine Labs, Paytm, PhonePe, Cashfree, UrbanPiper, Zomato, Swiggy, ONDC, WhatsApp, SMS providers, delivery providers, biometric attendance, hotel PMS platforms.

Do not implement all immediately. Create stable extension points. Extension point signatures live in `packages/contracts/`.

---

# 90. HOLLER CONTROL PLANE (LONG TERM)

Holler Cloud as central management/control plane. Owner can: see all outlets, change menu, change prices, publish recipes, review purchases, monitor stock, monitor KDS performance, review anomalies, push configuration, disable compromised terminals, monitor synchronization status. Configuration changes propagate safely to outlets. Restaurant operations remain available locally during cloud outages.

---

# 91. SYNC HEALTH UI

Expose health:

```
Cloud: Connected          Last Sync: 3 sec ago    Pending Events: 0
Aggregator: Connected     Payment Gateway: Connected
Kitchen LAN: Healthy      Printer 1: Online       Printer 2: Out of Paper
```

When offline:

```
Cloud: Offline — Restaurant operations continue locally
Pending sync events: 32
```

This should reassure employees rather than panic them.

---

# 92. FORENSIC AUDITABILITY

Owner should eventually be able to answer: Who removed an item from Order #381? Who applied the ₹700 discount? Who reopened the bill? Was the KOT already printed? Was inventory deducted? Was that deduction reversed? When did Razorpay capture payment? Which settlement contained it? Which user modified the recipe? What was the recipe when this historical item was sold?

Design data structures so these questions are answerable.

---

# 93. NON-FUNCTIONAL REQUIREMENTS

Reliability > animations. Correctness > cleverness. Data integrity > convenience. Local operation > cloud dependency. Deterministic accounting > AI. Extensibility > provider coupling. Fast workflows > decorative UI. Auditability > destructive editing.

---

# 94. FIRST TASK

DO NOT start by creating dozens of screens. Start with architecture and an executable vertical slice.

First produce:

1. `/docs/architecture/SYSTEM_ARCHITECTURE.md`
2. `/docs/domain/DOMAIN_MODEL.md`
3. `/docs/domain/ORDER_STATE_MACHINE.md`
4. `/docs/domain/INVENTORY_MODEL.md`
5. `/docs/domain/SYNC_PROTOCOL.md` (must encode the §50.1 authority rule)
6. ADR-001 through ADR-010
7. `CLAUDE.md` + `docs/spec/` + `docs/vision.md` decomposition per §0.1
8. repository scaffolding
9. Docker development environment (WSL2)
10. Go backend health service
11. PostgreSQL migrations for tenant/outlet/menu/order
12. **MILESTONE 0.5 contracts (§81) — before any slice implementation**
13. local SQLite database
14. minimal Tauri POS
15. one end-to-end flow:

```
Menu → POS Order → Local SQLite → KOT → KDS → Cloud synchronization
```

Create automated tests demonstrating that this flow works even when cloud connectivity is deliberately disabled.

Only after that vertical slice works should the wider product be built.

---

# 95. RESPONSE RULE FOR CODING AGENT

When operating inside Claude Code or another coding environment:

Inspect the existing repository first. Then output a concise execution plan. Then modify actual project files. Run commands/tests yourself where tooling permits. Do not dump thousands of lines of hypothetical code into chat when project-file editing is available.

If a task would touch more than ~15 files, stop and present the plan instead (§0.3).

At the end of every milestone report only:

### Implemented
Files/modules added.

### Verified
Tests and commands actually executed. (A separate read-only verification pass is required before claiming a milestone complete.)

### Performance
Any benchmark results available.

### Remaining
Known issues directly relevant to the milestone.

### Next
The next concrete milestone.

Do not repeatedly re-explain the vision.

Begin with **MILESTONE 0** now.