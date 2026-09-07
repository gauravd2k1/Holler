package aggregators

import (
	"context"
	"errors"
	"fmt"

	"github.com/jackc/pgx/v5"

	"github.com/holler/backend/internal/platform/postgres"
	"github.com/holler/backend/internal/platform/storage"
	contracts "github.com/holler/contracts"
)

// Repository is the persistence boundary for inbound aggregator documents.
type Repository interface {
	// AlreadySeen reports whether this platform message has been processed.
	//
	// PLATFORMS RETRY, AND A RETRY MUST NOT PRODUCE A SECOND DOCUMENT. The
	// UNIQUE (tenant_id, platform, message_id) key IS the mechanism -- an
	// insert that conflicts is a duplicate, and that is the whole check. Asking
	// first and inserting second would be a race; this inserts and reads the
	// conflict.
	RecordCallback(ctx context.Context, tx pgx.Tx, id, tenantID, platform, messageID, action string) (fresh bool, err error)

	// UpsertDocument applies a document REPLACE-NOT-MERGE at its version. An
	// older or equal document_version is IGNORED OUTRIGHT rather than merged --
	// the GET /sync/config precedent. Merging would make the reader a second
	// writer of a cloud-authoritative row.
	UpsertDocument(ctx context.Context, tx pgx.Tx, doc contracts.AggregatorOrder) (applied bool, err error)

	ResolveMenuItem(ctx context.Context, tenantID, outletID, platform, externalItemID string) (*string, error)

	WithTx(ctx context.Context, fn func(tx pgx.Tx) error) error
}

type pgRepository struct{ pool postgres.Pool }

func NewRepository(pool postgres.Pool) Repository { return &pgRepository{pool: pool} }

func (r *pgRepository) WithTx(ctx context.Context, fn func(tx pgx.Tx) error) error {
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return storage.Wrap("aggregators: beginning transaction", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()
	if err := fn(tx); err != nil {
		return err
	}
	return storage.Wrap("aggregators: committing", tx.Commit(ctx))
}

func (r *pgRepository) RecordCallback(ctx context.Context, tx pgx.Tx, id, tenantID, platform, messageID, action string) (bool, error) {
	// ON CONFLICT DO NOTHING plus a rows-affected read: one statement, no race.
	tag, err := tx.Exec(ctx,
		`INSERT INTO aggregator_callback_receipt (id, tenant_id, platform, message_id, action)
		 VALUES ($1, $2, $3, $4, $5)
		 ON CONFLICT (tenant_id, platform, message_id) DO NOTHING`,
		id, tenantID, platform, messageID, action,
	)
	if err != nil {
		return false, storage.Wrap("aggregators: recording callback", err)
	}
	return tag.RowsAffected() == 1, nil
}

func (r *pgRepository) UpsertDocument(ctx context.Context, tx pgx.Tx, doc contracts.AggregatorOrder) (bool, error) {
	// REPLACE-NOT-MERGE, guarded on document_version. The WHERE clause on the
	// DO UPDATE is what makes an older document a no-op instead of a
	// regression: a platform that retries an earlier state must not walk the
	// document backwards.
	//
	// accepted_at and local_order_id are DELIBERATELY NOT in the update list.
	// They are written by the operator-confirmed accept at the till, and a
	// later platform message must never clear the fact that a human accepted
	// this order (ADR-022 addendum §2).
	tag, err := tx.Exec(ctx,
		`INSERT INTO aggregator_order
		   (id, tenant_id, outlet_id, platform, external_order_id, platform_status,
		    document_version, raw_payload, stated_total_paise, received_at, business_date)
		 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)
		 ON CONFLICT (tenant_id, platform, external_order_id) DO UPDATE SET
		    platform_status    = EXCLUDED.platform_status,
		    document_version   = EXCLUDED.document_version,
		    raw_payload        = EXCLUDED.raw_payload,
		    stated_total_paise = EXCLUDED.stated_total_paise,
		    received_at        = EXCLUDED.received_at,
		    updated_at         = now()
		 WHERE EXCLUDED.document_version > aggregator_order.document_version`,
		doc.ID, doc.TenantID, doc.OutletID, doc.Platform, doc.ExternalOrderID,
		doc.PlatformStatus, doc.DocumentVersion, doc.RawPayload, doc.StatedTotalPaise,
		doc.ReceivedAt, doc.BusinessDate,
	)
	if err != nil {
		return false, storage.Wrap("aggregators: upserting document", err)
	}
	if tag.RowsAffected() == 0 {
		// An older or equal version. Not an error: a platform retrying an
		// earlier state is ordinary, and refusing it would turn a normal
		// condition into an alarm.
		return false, nil
	}

	// Lines are replaced wholesale with the document, never merged: a document
	// is a snapshot of what the platform currently says, and half of an old one
	// beside half of a new one is a state neither system ever described.
	if _, err := tx.Exec(ctx,
		`DELETE FROM aggregator_order_line WHERE aggregator_order_id = $1`, doc.ID); err != nil {
		return false, storage.Wrap("aggregators: clearing lines", err)
	}
	for _, l := range doc.Lines {
		if _, err := tx.Exec(ctx,
			`INSERT INTO aggregator_order_line
			   (id, aggregator_order_id, line_number, external_item_id, external_item_name,
			    menu_item_id, quantity, stated_unit_price_paise)
			 VALUES ($1,$2,$3,$4,$5,$6,$7,$8)`,
			l.ID, doc.ID, l.LineNumber, l.ExternalItemID, l.ExternalItemName,
			// NULL where nothing resolved, and the NULL is load-bearing: an
			// unmappable line is recorded, not refused (ADR-022 rule 4).
			l.MenuItemID, l.Quantity, l.StatedUnitPricePaise,
		); err != nil {
			return false, storage.Wrap("aggregators: inserting line", err)
		}
	}
	return true, nil
}

func (r *pgRepository) ResolveMenuItem(ctx context.Context, tenantID, outletID, platform, externalItemID string) (*string, error) {
	var menuItemID string
	err := r.pool.QueryRow(ctx,
		`SELECT menu_item_id FROM aggregator_item_map
		  WHERE tenant_id = $1 AND outlet_id = $2 AND platform = $3 AND external_item_id = $4`,
		tenantID, outletID, platform, externalItemID,
	).Scan(&menuItemID)
	if errors.Is(err, pgx.ErrNoRows) {
		// NOT AN ERROR. No mapping is the ordinary case for a new dish, and the
		// line is recorded with the platform's own name so a human can see what
		// was ordered.
		return nil, nil
	}
	if err != nil {
		return nil, fmt.Errorf("aggregators: resolving menu item: %w", err)
	}
	return &menuItemID, nil
}
