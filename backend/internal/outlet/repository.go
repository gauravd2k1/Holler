package outlet

import (
	"context"
	"errors"
	"fmt"

	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/platform/postgres"
	"github.com/jackc/pgx/v5"
)

// Repository is the persistence boundary the service depends on. Every
// method that reads or writes an outlet takes tenantID as an explicit,
// mandatory parameter and every implementation must use it in the query —
// never merely as a post-hoc check on the loaded row — so a mistaken query
// can never silently return another tenant's data.
type Repository interface {
	// Insert creates outlet under brandID, but only if brandID belongs to
	// tenantID. Returns httpx.ErrNotFound if the brand does not exist for
	// this tenant, so a caller cannot distinguish "brand does not exist"
	// from "brand belongs to another tenant".
	Insert(ctx context.Context, tenantID string, o Outlet) error
	// ListByTenant returns every outlet whose brand belongs to tenantID.
	ListByTenant(ctx context.Context, tenantID string) ([]Outlet, error)
	// GetByID returns httpx.ErrNotFound unless outletID's outlet belongs
	// (via its brand) to tenantID.
	GetByID(ctx context.Context, tenantID, outletID string) (Outlet, error)
}

// PostgresRepository is the Repository implementation backed by the
// packages/contracts/postgres schema.
type PostgresRepository struct {
	pool postgres.Pool
}

func NewPostgresRepository(pool postgres.Pool) *PostgresRepository {
	return &PostgresRepository{pool: pool}
}

func (r *PostgresRepository) Insert(ctx context.Context, tenantID string, o Outlet) error {
	tag, err := r.pool.Exec(ctx,
		`INSERT INTO outlet (id, brand_id, name, timezone, config_version, created_at, updated_at)
		 SELECT $1, $2, $3, $4, $5, $6, $7
		 WHERE EXISTS (SELECT 1 FROM brand WHERE id = $2 AND tenant_id = $8)`,
		o.ID, o.BrandID, o.Name, o.Timezone, o.ConfigVersion, o.CreatedAt, o.UpdatedAt, tenantID,
	)
	if err != nil {
		return fmt.Errorf("outlet: inserting outlet: %w", err)
	}
	if tag.RowsAffected() == 0 {
		return httpx.ErrNotFound
	}
	return nil
}

func (r *PostgresRepository) ListByTenant(ctx context.Context, tenantID string) ([]Outlet, error) {
	rows, err := r.pool.Query(ctx,
		`SELECT o.id, o.brand_id, o.name, o.timezone, o.config_version, o.created_at, o.updated_at
		 FROM outlet o
		 JOIN brand b ON b.id = o.brand_id
		 WHERE b.tenant_id = $1
		 ORDER BY o.name`,
		tenantID,
	)
	if err != nil {
		return nil, fmt.Errorf("outlet: listing outlets: %w", err)
	}
	defer rows.Close()

	var outlets []Outlet
	for rows.Next() {
		var o Outlet
		if err := rows.Scan(&o.ID, &o.BrandID, &o.Name, &o.Timezone, &o.ConfigVersion, &o.CreatedAt, &o.UpdatedAt); err != nil {
			return nil, fmt.Errorf("outlet: scanning outlet: %w", err)
		}
		outlets = append(outlets, o)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("outlet: iterating outlets: %w", err)
	}
	return outlets, nil
}

func (r *PostgresRepository) GetByID(ctx context.Context, tenantID, outletID string) (Outlet, error) {
	var o Outlet
	err := r.pool.QueryRow(ctx,
		`SELECT o.id, o.brand_id, o.name, o.timezone, o.config_version, o.created_at, o.updated_at
		 FROM outlet o
		 JOIN brand b ON b.id = o.brand_id
		 WHERE o.id = $1 AND b.tenant_id = $2`,
		outletID, tenantID,
	).Scan(&o.ID, &o.BrandID, &o.Name, &o.Timezone, &o.ConfigVersion, &o.CreatedAt, &o.UpdatedAt)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return Outlet{}, httpx.ErrNotFound
		}
		return Outlet{}, fmt.Errorf("outlet: querying outlet: %w", err)
	}
	return o, nil
}
