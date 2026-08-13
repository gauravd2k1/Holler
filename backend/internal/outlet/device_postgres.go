package outlet

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/holler/backend/internal/platform/httpx"
	"github.com/jackc/pgx/v5"
	"github.com/jackc/pgx/v5/pgconn"
)

// pgUniqueViolation is the PostgreSQL SQLSTATE for a unique_violation,
// mirroring backend/internal/kitchen/repository.go and
// backend/internal/tables/repository.go's own copy of the same constant —
// each bounded context owns its persistence layer independently.
const pgUniqueViolation = "23505"

func isUniqueViolation(err error) bool {
	var pgErr *pgconn.PgError
	return errors.As(err, &pgErr) && pgErr.Code == pgUniqueViolation
}

// PostgresRepository also implements DeviceRepository, over
// packages/contracts/postgres/0008_device_enrollment.sql. Kept in this
// dedicated file (rather than repository.go) because device enrollment is a
// distinct concern from outlet CRUD, even though both share one Go type and
// one Postgres pool.
var _ DeviceRepository = (*PostgresRepository)(nil)

func (r *PostgresRepository) InsertDevice(ctx context.Context, tenantID string, d Device) error {
	tag, err := r.pool.Exec(ctx, `
		INSERT INTO device (id, outlet_id, kind, name, enrolled_at, created_at, updated_at)
		SELECT $1, $2, $3, $4, $5, $6, $7
		WHERE EXISTS (
			SELECT 1 FROM outlet o JOIN brand b ON b.id = o.brand_id
			WHERE o.id = $2 AND b.tenant_id = $8
		)
	`, d.ID, d.OutletID, string(d.Kind), d.Name, d.EnrolledAt, d.CreatedAt, d.UpdatedAt, tenantID)
	if err != nil {
		if isUniqueViolation(err) {
			return fmt.Errorf("%w: a device named %q already exists at this outlet", httpx.ErrConflict, d.Name)
		}
		return fmt.Errorf("outlet: inserting device: %w", err)
	}
	if tag.RowsAffected() == 0 {
		return httpx.ErrNotFound
	}
	return nil
}

func (r *PostgresRepository) FindDeviceByOutletAndName(ctx context.Context, tenantID, outletID, name string) (Device, error) {
	var d Device
	err := r.pool.QueryRow(ctx, `
		SELECT d.id, d.outlet_id, d.kind, d.name, d.enrolled_at, d.revoked_at, d.last_seen_at, d.created_at, d.updated_at
		FROM device d
		JOIN outlet o ON o.id = d.outlet_id
		JOIN brand b ON b.id = o.brand_id
		WHERE d.outlet_id = $1 AND d.name = $2 AND b.tenant_id = $3
	`, outletID, name, tenantID).Scan(&d.ID, &d.OutletID, &d.Kind, &d.Name, &d.EnrolledAt, &d.RevokedAt, &d.LastSeenAt, &d.CreatedAt, &d.UpdatedAt)
	if errors.Is(err, pgx.ErrNoRows) {
		return Device{}, httpx.ErrNotFound
	}
	if err != nil {
		return Device{}, fmt.Errorf("outlet: finding device by name: %w", err)
	}
	return d, nil
}

func (r *PostgresRepository) GetDevice(ctx context.Context, tenantID, deviceID string) (Device, error) {
	var d Device
	err := r.pool.QueryRow(ctx, `
		SELECT d.id, d.outlet_id, d.kind, d.name, d.enrolled_at, d.revoked_at, d.last_seen_at, d.created_at, d.updated_at
		FROM device d
		JOIN outlet o ON o.id = d.outlet_id
		JOIN brand b ON b.id = o.brand_id
		WHERE d.id = $1 AND b.tenant_id = $2
	`, deviceID, tenantID).Scan(&d.ID, &d.OutletID, &d.Kind, &d.Name, &d.EnrolledAt, &d.RevokedAt, &d.LastSeenAt, &d.CreatedAt, &d.UpdatedAt)
	if errors.Is(err, pgx.ErrNoRows) {
		return Device{}, httpx.ErrNotFound
	}
	if err != nil {
		return Device{}, fmt.Errorf("outlet: getting device: %w", err)
	}
	return d, nil
}

func (r *PostgresRepository) MarkDeviceEnrolled(ctx context.Context, deviceID string, now time.Time) error {
	_, err := r.pool.Exec(ctx, `
		UPDATE device SET enrolled_at = COALESCE(enrolled_at, $2), updated_at = $2 WHERE id = $1
	`, deviceID, now)
	if err != nil {
		return fmt.Errorf("outlet: marking device enrolled: %w", err)
	}
	return nil
}

func (r *PostgresRepository) InsertCredential(ctx context.Context, c DeviceCredential, tokenHash string) error {
	_, err := r.pool.Exec(ctx, `
		INSERT INTO device_credential (id, device_id, tenant_id, outlet_id, token_hash, label, created_at, expires_at)
		VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
	`, c.ID, c.DeviceID, c.TenantID, c.OutletID, tokenHash, c.Label, c.CreatedAt, c.ExpiresAt)
	if err != nil {
		if isUniqueViolation(err) {
			// idx_device_credential_active: two live credentials for one
			// device is a bug, not a state (packages/contracts/postgres/
			// 0008_device_enrollment.sql). The caller failed to revoke the
			// prior credential first.
			return fmt.Errorf("%w: device already holds an active credential", httpx.ErrConflict)
		}
		return fmt.Errorf("outlet: inserting device credential: %w", err)
	}
	return nil
}

func (r *PostgresRepository) RevokeActiveCredential(ctx context.Context, deviceID string, now time.Time) error {
	_, err := r.pool.Exec(ctx, `
		UPDATE device_credential SET revoked_at = $2
		WHERE device_id = $1 AND revoked_at IS NULL
	`, deviceID, now)
	if err != nil {
		return fmt.Errorf("outlet: revoking active device credential: %w", err)
	}
	return nil
}

func (r *PostgresRepository) HasActiveCredential(ctx context.Context, deviceID string) (bool, error) {
	var exists bool
	err := r.pool.QueryRow(ctx, `
		SELECT EXISTS (SELECT 1 FROM device_credential WHERE device_id = $1 AND revoked_at IS NULL)
	`, deviceID).Scan(&exists)
	if err != nil {
		return false, fmt.Errorf("outlet: checking active device credential: %w", err)
	}
	return exists, nil
}

func (r *PostgresRepository) findCredentialForVerify(ctx context.Context, credentialID string) (deviceCredentialVerifyRow, error) {
	var row deviceCredentialVerifyRow
	err := r.pool.QueryRow(ctx, `
		SELECT dc.id, dc.device_id, dc.tenant_id, dc.outlet_id, dc.token_hash, dc.revoked_at, dc.expires_at, d.revoked_at
		FROM device_credential dc
		JOIN device d ON d.id = dc.device_id
		WHERE dc.id = $1
	`, credentialID).Scan(&row.credentialID, &row.deviceID, &row.tenantID, &row.outletID, &row.tokenHash,
		&row.credRevokedAt, &row.expiresAt, &row.deviceRevoked)
	if errors.Is(err, pgx.ErrNoRows) {
		return deviceCredentialVerifyRow{}, httpx.ErrUnauthorized
	}
	if err != nil {
		return deviceCredentialVerifyRow{}, fmt.Errorf("outlet: finding device credential: %w", err)
	}
	return row, nil
}

func (r *PostgresRepository) touchCredentialLastUsed(ctx context.Context, credentialID string, now time.Time) error {
	_, err := r.pool.Exec(ctx, `
		UPDATE device_credential SET last_used_at = $2 WHERE id = $1
	`, credentialID, now)
	if err != nil {
		return fmt.Errorf("outlet: touching device credential last_used_at: %w", err)
	}
	return nil
}
