package contracts

// M6 Phase C, contracts 0.8.0 (ADR-022 ACCEPTED 2026-09-08).
// Mirrors src/types/aggregator.ts.
//
// TWO AGGREGATES, NOT ONE. AggregatorOrder is the inbound DOCUMENT from an
// external platform: CLOUD-AUTHORITATIVE, syncing DOWN, replaced wholesale at a
// newer DocumentVersion and never field-merged. The local `order` created from
// it stays EDGE-AUTHORITATIVE and syncs up exactly as every other order does,
// linked by ExternalOrderID.
//
// The guarantee that falls out of the split, and it is published: A NEW
// AGGREGATOR ORDER CANNOT ARRIVE WHILE THE UPLINK IS DOWN; ONE THAT HAS ALREADY
// ARRIVED IS FULLY OPERABLE OFFLINE.
//
// NOTHING IN THIS FILE NAMES A PLATFORM. Platform is a string carrying data,
// not a constant set carrying code: a new platform must not require a contract
// change, and the drift check keeps platform vocabulary inside its own adapter
// module.

// AggregatorOrderLine is a CHILD ROW, not an aggregate. It travels inside its
// parent's payload and has no sync direction of its own -- the InvoiceLine and
// GrnLine precedent.
type AggregatorOrderLine struct {
	ID                string `json:"id"`
	AggregatorOrderID string `json:"aggregator_order_id"`
	LineNumber        int    `json:"line_number"`

	// What the platform called it, kept whatever else happens. When the mapping
	// fails this is the only description of what the customer actually ordered.
	ExternalItemID   string `json:"external_item_id"`
	ExternalItemName string `json:"external_item_name"`

	// NULLABLE, AND THE NULL IS LOAD-BEARING. A line that cannot be mapped to a
	// menu item is RECORDED, NOT REFUSED (ADR-022 rule 4, the grn_gap
	// precedent). Refusing a delivery order that is already cooking is the
	// outage, not the protection -- so no CHECK ties a line to a menu item in
	// either store, and none may be added.
	MenuItemID *string `json:"menu_item_id"`

	Quantity int `json:"quantity"`
	// Integer paise. Nullable because not every inbound shape prices its lines.
	StatedUnitPricePaise *int64 `json:"stated_unit_price_paise"`

	SchemaVersion int `json:"schema_version"`
}

type AggregatorOrder struct {
	ID       string `json:"id"`
	TenantID string `json:"tenant_id"`
	OutletID string `json:"outlet_id"`

	// Data, not code. See the file header.
	Platform string `json:"platform"`
	// Tenant- and platform-scoped, never global: two platforms can and do issue
	// the same id, so a global unique would reject the second as a duplicate of
	// an unrelated order.
	ExternalOrderID string `json:"external_order_id"`

	// The platform's own state string, verbatim and unmapped. A platform status
	// NEVER writes order.status -- one writer, as ADR-014 requires for
	// kot.status. Kept as the platform's own string so a status we have never
	// seen is recorded rather than coerced into the nearest local one.
	PlatformStatus string `json:"platform_status"`

	// Replace-not-merge compares on this: a newer document replaces the row
	// WHOLESALE, an older or equal one is ignored outright.
	DocumentVersion int64 `json:"document_version"`

	// The raw inbound payload, kept whole: the record of what an external
	// system actually asked for, and the thing anyone reaches for in a dispute.
	// Storing only our parse of it makes a mapping bug unfalsifiable after the
	// fact.
	RawPayload map[string]any `json:"raw_payload"`

	StatedTotalPaise *int64 `json:"stated_total_paise"`

	ReceivedAt   string `json:"received_at"`
	BusinessDate string `json:"business_date"`

	// NULL means "arrived, not yet accepted" -- a visible operational state,
	// not a missing value. Creation of the local order is OPERATOR-CONFIRMED,
	// never automatic (ADR-022 addendum §2): an unmappable document must not
	// put unresolved lines into a kitchen.
	AcceptedAt   *string `json:"accepted_at"`
	LocalOrderID *string `json:"local_order_id"`

	Lines []AggregatorOrderLine `json:"lines"`

	SchemaVersion int `json:"schema_version"`
}
