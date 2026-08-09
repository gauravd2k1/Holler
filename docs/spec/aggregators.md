# Spec: Aggregators

Owns: `aggregator_gateway` bounded context — Swiggy/Zomato/UrbanPiper/ONDC integration.
Source: HOLLER_MASTER_PROMPT.md §15, §17–§19, §38.

Core Holler feature. Aggregator-specific logic never lives inside core Order code.

## Provider interface
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
Only officially authorized partner APIs — never scraping or private APIs. Architecture must support intermediary providers (UrbanPiper) as well as future direct APIs.

## Event flow
```
Swiggy → Webhook → Aggregator Gateway
  ├── verify authenticity
  ├── deduplicate
  ├── normalize (into CanonicalOrder — see docs/spec/ordering.md)
  └── persist raw event
→ Order Service → Outlet Sync → Holler Edge → KOT Router → KDS
```
Retry/dead-letter for malformed messages — never silently discard.

Configurable auto-accept mode: outlet may enable automatic acceptance of
incoming aggregator orders and automatic KOT print, per channel. Manual
accept/reject remains the default. (Competitive parity — Recaho, M6.)

## Menu sync
Holler can be master catalog, publishing to Swiggy/Zomato/QR-Web with channel-specific overrides (name, category, price, variants, modifiers, tax/charges where supported, availability, hours, images, descriptions).

## Item snooze / stock-out
Zero stock on an ingredient (e.g. paneer) can auto-snoozes dependent items across POS/QR/Direct/Swiggy/Zomato; restock can auto-restore. Manager override always available.

## Aggregator reconciliation
Per settlement: gross order value, merchant discount, platform discount, commission (+tax), delivery/packaging charges, adjustments, refunds, cancellations, TDS/TCS, other fees, net settlement, reference, date. Owner view: Expected Receivable vs Actual Settlement with discrepancies highlighted.

## Milestone order
MockAggregatorAdapter → UrbanPiperAdapter → direct adapters when partner access is approved (Milestone 6).
