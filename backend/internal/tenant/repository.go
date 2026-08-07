package tenant

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/platform/postgres"
	"github.com/jackc/pgx/v5"
)

// Repository is the persistence boundary the service depends on, so the
// service can be unit-tested against a fake without a live database.
type Repository interface {
	InsertTenant(ctx context.Context, t Tenant) error
	InsertBrand(ctx context.Context, b Brand) error
	// GetBrand returns httpx.ErrNotFound when no brand with this id exists
	// for this tenant — including when the brand exists under a different
	// tenant. The caller never learns whether the id exists elsewhere
	// (docs/spec/security-rbac.md §Tenant isolation).
	GetBrand(ctx context.Context, tenantID, brandID string) (Brand, error)
}

// PostgresRepository is the Repository implementation backed by the
// packages/contracts/postgres schema.
type PostgresRepository struct {
	pool postgres.Pool
}

func NewPostgresRepository(pool postgres.Pool) *PostgresRepository {
	return &PostgresRepository{pool: pool}
}

func (r *PostgresRepository) InsertTenant(ctx context.Context, t Tenant) error {
	_, err := r.pool.Exec(ctx,
		`INSERT INTO tenant (id, name, created_at, updated_at) VALUES ($1, $2, $3, $4)`,
		t.ID, t.Name, t.CreatedAt, t.UpdatedAt,
	)
	if err != nil {
		return fmt.Errorf("tenant: inserting tenant: %w", err)
	}
	return nil
}

func (r *PostgresRepository) InsertBrand(ctx context.Context, b Brand) error {
	_, err := r.pool.Exec(ctx,
		`INSERT INTO brand (id, tenant_id, name, created_at, updated_at) VALUES ($1, $2, $3, $4, $5)`,
		b.ID, b.TenantID, b.Name, b.CreatedAt, b.UpdatedAt,
	)
	if err != nil {
		return fmt.Errorf("tenant: inserting brand: %w", err)
	}
	return nil
}

func (r *PostgresRepository) GetBrand(ctx context.Context, tenantID, brandID string) (Brand, error) {
	var b Brand
	var createdAt, updatedAt time.Time
	err := r.pool.QueryRow(ctx,
		`SELECT id, tenant_id, name, created_at, updated_at
		 FROM brand WHERE id = $1 AND tenant_id = $2`,
		brandID, tenantID,
	).Scan(&b.ID, &b.TenantID, &b.Name, &createdAt, &updatedAt)
	if err != nil {
		if errors.Is(err, pgx.ErrNoRows) {
			return Brand{}, httpx.ErrNotFound
		}
		return Brand{}, fmt.Errorf("tenant: querying brand: %w", err)
	}
	b.CreatedAt, b.UpdatedAt = createdAt, updatedAt
	return b, nil
}
