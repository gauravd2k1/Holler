package outlet

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/platform/storage"
	contracts "github.com/holler/contracts"
	"github.com/jackc/pgx/v5"
)

// PostgresRepository also implements DeviceRepository, over
// packages/contracts/postgres/0008_device_enrollment.sql. Kept in this
// dedicated file (rather than repository.go) because device enrollment is a
// distinct concern from outlet CRUD, even though both share one Go type and
// one Postgres pool.
var _ DeviceRepository = (*PostgresRepository)(nil)

// WithTx runs fn inside a single Postgres transaction, mirroring
// backend/internal/compliance's pgRepository.WithTx exactly (T13 retry,
// DEFECT 1) — do not invent a second transaction idiom in this package.
func (r *PostgresRepository) WithTx(ctx context.Context, fn func(tx pgx.Tx) error) error {
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return fmt.Errorf("outlet: begin tx: %w", err)
	}
	if err := fn(tx); err != nil {
		_ = tx.Rollback(ctx)
		return err
	}
	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("outlet: commit tx: %w", err)
	}
	return nil
}

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
		if storage.IsUniqueViolation(err) {
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

func (r *PostgresRepository) InsertCredential(ctx context.Context, tx pgx.Tx, c DeviceCredential, tokenHash string) error {
	_, err := tx.Exec(ctx, `
		INSERT INTO device_credential (id, device_id, tenant_id, outlet_id, token_hash, label, created_at, expires_at, config_version)
		VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
	`, c.ID, c.DeviceID, c.TenantID, c.OutletID, tokenHash, c.Label, c.CreatedAt, c.ExpiresAt, c.ConfigVersion)
	if err != nil {
		if storage.IsUniqueViolation(err) {
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

func (r *PostgresRepository) RevokeActiveCredential(ctx context.Context, tx pgx.Tx, deviceID string, now time.Time, configVersion int) error {
	_, err := tx.Exec(ctx, `
		UPDATE device_credential SET revoked_at = $2, config_version = $3
		WHERE device_id = $1 AND revoked_at IS NULL
	`, deviceID, now, configVersion)
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

// BumpOutletConfigVersion increments outlet.config_version by exactly one —
// see DeviceRepository's doc comment for why device credential mutations
// need this (T13, ADR-017 0.4.3 amendment) and why it now runs inside tx
// (T13 retry, DEFECT 1).
func (r *PostgresRepository) BumpOutletConfigVersion(ctx context.Context, tx pgx.Tx, outletID string) (int, error) {
	var newVersion int
	err := tx.QueryRow(ctx,
		`UPDATE outlet SET config_version = config_version + 1, updated_at = now()
		 WHERE id = $1 RETURNING config_version`,
		outletID,
	).Scan(&newVersion)
	if errors.Is(err, pgx.ErrNoRows) {
		return 0, fmt.Errorf("%w: outlet %s", httpx.ErrNotFound, outletID)
	}
	if err != nil {
		return 0, fmt.Errorf("outlet: bumping outlet config_version: %w", err)
	}
	return newVersion, nil
}

// ListEdgeCredentials returns every device_credential row for outletID whose
// OWN config_version exceeds sinceVersion, scoped to tenantID via the row's
// own tenant_id (denormalized onto device_credential at enrollment time,
// exactly like outlet_id — see 0008_device_enrollment.sql) AND to outletID
// (0010_device_credential_config_version.sql's idx_device_credential_outlet_
// version supports this predicate) — deliberately keeping BOTH predicates:
// tenant_id alone is not enough to keep one outlet's Argon2id hashes from
// another outlet under the same tenant
// (TestPostgresListEdgeDeviceCredentials_ScopesToOutletNotJustTenant). Never
// filters on revoked_at/expires_at: a dead credential still syncs (ADR-017
// 0.4.3 amendment) — rejection is decided by those fields at the edge, never
// by a row's absence from this result. Since contracts 0.4.5 every row
// carries its OWN config_version, stamped at write time by
// InsertCredential/RevokeActiveCredential to the value the outlet was just
// bumped to — this method no longer needs to (and must not) overwrite it.
func (r *PostgresRepository) ListEdgeCredentials(ctx context.Context, tenantID, outletID string, sinceVersion int) ([]contracts.EdgeDeviceCredential, error) {
	rows, err := r.pool.Query(ctx, `
		SELECT dc.id, dc.device_id, dc.tenant_id, dc.outlet_id, dc.token_hash, d.kind, dc.revoked_at, dc.expires_at, dc.config_version
		FROM device_credential dc
		JOIN device d ON d.id = dc.device_id
		WHERE dc.outlet_id = $1 AND dc.tenant_id = $2 AND dc.config_version > $3
		ORDER BY dc.created_at
	`, outletID, tenantID, sinceVersion)
	if err != nil {
		return nil, fmt.Errorf("outlet: listing edge device credentials: %w", err)
	}
	defer rows.Close()

	out := make([]contracts.EdgeDeviceCredential, 0)
	for rows.Next() {
		var c contracts.EdgeDeviceCredential
		var revokedAt, expiresAt *time.Time
		if err := rows.Scan(&c.CredentialID, &c.DeviceID, &c.TenantID, &c.OutletID, &c.CredentialHash,
			&c.DeviceKind, &revokedAt, &expiresAt, &c.ConfigVersion); err != nil {
			return nil, fmt.Errorf("outlet: scanning edge device credential: %w", err)
		}
		c.RevokedAt = formatEdgeTimestamp(revokedAt)
		c.ExpiresAt = formatEdgeTimestamp(expiresAt)
		c.SchemaVersion = 1
		out = append(out, c)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("outlet: iterating edge device credentials: %w", err)
	}
	return out, nil
}

// formatEdgeTimestamp renders a nullable Postgres timestamptz as the RFC3339
// string EdgeDeviceCredential.RevokedAt/ExpiresAt carry on the wire (unlike
// most timestamps in this codebase, packages/contracts/go/identity.go types
// these as *string rather than *time.Time).
func formatEdgeTimestamp(t *time.Time) *string {
	if t == nil {
		return nil
	}
	s := t.UTC().Format(time.RFC3339Nano)
	return &s
}
