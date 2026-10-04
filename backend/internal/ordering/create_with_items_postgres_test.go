package ordering_test

import (
	"context"
	"testing"

	"github.com/holler/backend/internal/ordering"
	"github.com/holler/backend/internal/platform/id"
	contracts "github.com/holler/contracts"
)

// TestPostgresRepository_CreateEnvelopeWithItemsPersistsEveryLine is the
// falsifier for the VV-004 divergence: an order created at the till WITH its
// lines already on it arrived in the cloud as a header carrying a total and
// ZERO order_item rows.
//
// The till puts the whole CanonicalOrder, items included, into the
// OrderCreated event (apps/pos/src-tauri/src/commands/orders.rs). Measured on
// the live edge database 2026-10-04, all three orders that have ever replayed
// carried payload_items = 1 and all three OrderCreated rows were `sent`; the
// cloud's order_item set matched the SEPARATELY sent ItemAdded events exactly
// and nothing else. #A2 therefore stood at total_paise = 194500 against
// count(order_item) = 0, and the admin console could not name a single thing
// that had been sold.
//
// WHY NO EXISTING TEST SEES THIS. Every cloud-side ingest test creates its
// order with `Items: []contracts.OrderItem{}` (orderFor in postgres_test.go,
// newOrder in service_test.go) and appends lines afterwards through
// AppendItem, so the create-with-items path has never been exercised on
// either the fake or real Postgres. Same shape as contracts 0.5.9
// (source_stock_count_id, a field the fixture left null) and 0.8.2
// (display_number, dropped on ingest): a fidelity test proves fidelity only
// for the fields its fixture populates. Green on absent data, third
// occurrence.
//
// Two lines rather than one, deliberately: a one-line fixture cannot tell
// "the items are persisted" apart from "the first item is persisted".
func TestPostgresRepository_CreateEnvelopeWithItemsPersistsEveryLine(t *testing.T) {
	pool := setupPool(t)
	fx := newFixture(t, pool)
	svc := ordering.NewService(ordering.NewPostgresRepository(pool))

	orderID := id.New()
	firstItemID := id.New()
	secondItemID := id.New()

	order := orderFor(orderID, fx.outletID)
	// variant_id stays nil on both lines. order_item_variant_id_fkey is the
	// constraint that actually refused M6 C3's fixture — the cloud holds one
	// variant row per outlet against the edge's one per item — and this test
	// is about line persistence, not about variant referential integrity.
	order.Items = []contracts.OrderItem{
		{
			ID:             firstItemID,
			MenuItemID:     fx.menuItemID,
			Quantity:       1,
			UnitPricePaise: 32000,
			LineTotalPaise: 32000,
		},
		{
			ID:             secondItemID,
			MenuItemID:     fx.menuItemID,
			Quantity:       2,
			UnitPricePaise: 32000,
			LineTotalPaise: 64000,
		},
	}
	order.SubtotalPaise = 96000
	order.TotalPaise = 96000

	stored, err := svc.IngestOrder(context.Background(), fx.tenantID, envelopeFor(orderID, fx.tenantID, fx.outletID, 1), order)
	if err != nil {
		t.Fatalf("IngestOrder: %v", err)
	}

	// The 201 echo is asserted as well as the stored rows. The echo is built
	// from GetByID, which reads order_item back out, so an echo showing two
	// lines cannot be the handler reflecting its own decoded input.
	if len(stored.Items) != 2 {
		t.Fatalf("create echo carries %d lines, want 2 — the cloud dropped the items the create envelope handed it", len(stored.Items))
	}

	var count int
	if err := pool.QueryRow(context.Background(), `SELECT count(*) FROM order_item WHERE order_id = $1`, orderID).Scan(&count); err != nil {
		t.Fatalf("counting order_item rows: %v", err)
	}
	if count != 2 {
		t.Fatalf("order_item holds %d rows for this order, want 2", count)
	}

	// The money, not just the row count: VV-004's symptom was a total with
	// nothing under it, so the sum of the stored lines is the assertion that
	// matters to the screen.
	var lineSum int64
	if err := pool.QueryRow(context.Background(), `SELECT coalesce(sum(line_total_paise), 0) FROM order_item WHERE order_id = $1`, orderID).Scan(&lineSum); err != nil {
		t.Fatalf("summing order_item line totals: %v", err)
	}
	if lineSum != 96000 {
		t.Fatalf("stored lines sum to %d paise, want 96000 — the header total would stand against a different number", lineSum)
	}

	// Both ids specifically, not just a count of two: a create that inserted
	// the same line twice would satisfy a count.
	seen := map[string]bool{}
	for _, it := range stored.Items {
		seen[it.ID] = true
	}
	if !seen[firstItemID] || !seen[secondItemID] {
		t.Fatalf("stored lines are not the two that were sent: got %v", seen)
	}
}

// TestPostgresRepository_CreateEnvelopeWithItemsIsIdempotent is the
// at-least-once half. The edge's outbox is at-least-once (contracts 0.8.3,
// ADR-028) and a redelivered OrderCreated must not double the lines — the
// same guarantee TestPostgresRepository_DuplicateOrderEnvelopeIsIdempotent
// makes for the header, which says nothing about items it never inserted.
func TestPostgresRepository_CreateEnvelopeWithItemsIsIdempotent(t *testing.T) {
	pool := setupPool(t)
	fx := newFixture(t, pool)
	svc := ordering.NewService(ordering.NewPostgresRepository(pool))

	orderID := id.New()
	order := orderFor(orderID, fx.outletID)
	order.Items = []contracts.OrderItem{
		{
			ID:             id.New(),
			MenuItemID:     fx.menuItemID,
			Quantity:       1,
			UnitPricePaise: 32000,
			LineTotalPaise: 32000,
		},
	}
	order.SubtotalPaise = 32000
	order.TotalPaise = 32000
	env := envelopeFor(orderID, fx.tenantID, fx.outletID, 1)

	if _, err := svc.IngestOrder(context.Background(), fx.tenantID, env, order); err != nil {
		t.Fatalf("first IngestOrder: %v", err)
	}
	stored, err := svc.IngestOrder(context.Background(), fx.tenantID, env, order)
	if err != nil {
		t.Fatalf("redelivered IngestOrder: %v", err)
	}

	if len(stored.Items) != 1 {
		t.Fatalf("redelivery left %d lines, want 1", len(stored.Items))
	}

	var count int
	if err := pool.QueryRow(context.Background(), `SELECT count(*) FROM order_item WHERE order_id = $1`, orderID).Scan(&count); err != nil {
		t.Fatalf("counting order_item rows: %v", err)
	}
	if count != 1 {
		t.Fatalf("order_item holds %d rows after redelivery, want 1", count)
	}
}
