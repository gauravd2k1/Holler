package outlet

import (
	"context"
	"time"

	contracts "github.com/holler/contracts"
	"github.com/jackc/pgx/v5"
)

// DeviceKind mirrors the device.kind CHECK constraint in
// packages/contracts/postgres/0008_device_enrollment.sql.
type DeviceKind string

const (
	DeviceKindPOS           DeviceKind = "POS"
	DeviceKindKDS           DeviceKind = "KDS"
	DeviceKindWaiter        DeviceKind = "WAITER"
	DeviceKindPrinterBridge DeviceKind = "PRINTER_BRIDGE"
)

func isValidDeviceKind(k DeviceKind) bool {
	switch k {
	case DeviceKindPOS, DeviceKindKDS, DeviceKindWaiter, DeviceKindPrinterBridge:
		return true
	default:
		return false
	}
}

// Device is the cloud's registry row for one outlet device (ADR-017). It is
// never replicated as config and never replayed from the edge — enrollment
// is the only writer.
type Device struct {
	ID         string
	OutletID   string
	Kind       DeviceKind
	Name       string
	EnrolledAt *time.Time
	RevokedAt  *time.Time
	LastSeenAt *time.Time
	CreatedAt  time.Time
	UpdatedAt  time.Time
}

// DeviceCredential is a device_credential row MINUS token_hash: no exported
// method in this package ever returns a hash, and no route reads a plaintext
// token back after enrollment — that is a structural guarantee, not a policy
// (ADR-017 §1).
type DeviceCredential struct {
	ID         string
	DeviceID   string
	TenantID   string
	OutletID   string
	Label      string
	CreatedAt  time.Time
	ExpiresAt  *time.Time
	RevokedAt  *time.Time
	LastUsedAt *time.Time
}

// deviceCredentialVerifyRow is the ONLY place in this package a token hash
// may be scanned into memory, mirroring auth.credentialRow's rule for
// password_hash/pin_hash. It never leaves verifyCredential.
type deviceCredentialVerifyRow struct {
	credentialID  string
	deviceID      string
	tenantID      string
	outletID      string
	tokenHash     string
	credRevokedAt *time.Time
	expiresAt     *time.Time
	deviceRevoked *time.Time
}

// DeviceRepository is the persistence boundary device enrollment depends on.
// Every device/device_credential row is tenant/outlet scoped exactly like
// Repository — a mistaken query must never be able to return, mutate or
// verify another tenant's device.
type DeviceRepository interface {
	// WithTx runs fn inside a single Postgres transaction, committing iff fn
	// returns nil and rolling back otherwise. Mirrors
	// backend/internal/compliance's Repository.WithTx exactly (T13 retry,
	// DEFECT 1): a device credential mutation and the outlet config_version
	// bump that announces it must land in one commit, never as two
	// independent pool calls — a crash between them would leave a live
	// credential whose change is invisible to every edge pulling
	// GET /sync/config at or above the un-bumped version.
	WithTx(ctx context.Context, fn func(tx pgx.Tx) error) error

	// InsertDevice creates device under outletID, but only if outletID
	// belongs to tenantID (mirrors Repository.Insert's WHERE EXISTS pattern).
	InsertDevice(ctx context.Context, tenantID string, d Device) error
	// FindDeviceByOutletAndName returns httpx.ErrNotFound if no device with
	// that name exists at outletID for tenantID.
	FindDeviceByOutletAndName(ctx context.Context, tenantID, outletID, name string) (Device, error)
	// GetDevice returns httpx.ErrNotFound unless deviceID belongs (via its
	// outlet) to tenantID.
	GetDevice(ctx context.Context, tenantID, deviceID string) (Device, error)
	// MarkDeviceEnrolled stamps enrolled_at the first time a device receives
	// a credential; a no-op if already set.
	MarkDeviceEnrolled(ctx context.Context, deviceID string, now time.Time) error

	// InsertCredential adds a new credential row within tx. The caller is
	// responsible for having revoked any prior active credential first —
	// this method does not enforce idx_device_credential_active itself,
	// relying on the partial unique index to fail the insert if it would
	// violate the "one active credential per device" invariant. Runs inside
	// tx so the insert and the outlet config_version bump that must
	// accompany it commit or roll back together (T13 retry, DEFECT 1).
	InsertCredential(ctx context.Context, tx pgx.Tx, c DeviceCredential, tokenHash string) error
	// RevokeActiveCredential stamps revoked_at on deviceID's current active
	// credential (revoked_at IS NULL), if any, within tx. Not an error if
	// none exists. Runs inside tx for the same reason as InsertCredential.
	RevokeActiveCredential(ctx context.Context, tx pgx.Tx, deviceID string, now time.Time) error
	// HasActiveCredential reports whether deviceID currently holds a live
	// credential.
	HasActiveCredential(ctx context.Context, deviceID string) (bool, error)
	// findCredentialForVerify loads the row VerifyToken needs to check a
	// presented secret. Unexported return type: callers outside this package
	// cannot obtain a token hash through this interface.
	findCredentialForVerify(ctx context.Context, credentialID string) (deviceCredentialVerifyRow, error)
	// touchCredentialLastUsed best-effort records that credentialID
	// authenticated a request. Callers should not fail the request if this
	// errors.
	touchCredentialLastUsed(ctx context.Context, credentialID string, now time.Time) error

	// BumpOutletConfigVersion increments outlet.config_version by exactly
	// one, mirroring backend/internal/tables/kitchen/compliance's own copy of
	// the same statement, and runs inside tx (T13 retry, DEFECT 1) so it
	// commits atomically with the InsertCredential/RevokeActiveCredential
	// call it is always paired with. device_credential carries no
	// config_version column of its own (packages/contracts/postgres/
	// 0008_device_enrollment.sql predates ADR-017's 0.4.3 amendment and was
	// not revised) — T13's ListEdgeCredentials reports the OUTLET's
	// config_version for every credential row rather than a per-row one, so
	// this bump is what makes a credential mutation observable to an edge
	// pulling with since_version.
	BumpOutletConfigVersion(ctx context.Context, tx pgx.Tx, outletID string) (int, error)

	// ListEdgeCredentials returns EVERY device_credential row for outletID —
	// active, revoked AND expired — WITH credential_hash. This is the one
	// exception to this package's otherwise-total "no exported method ever
	// returns a hash" rule (see DeviceCredential's own doc comment above),
	// mirroring auth.Repository.ListEdgeUserCache's identical exception for
	// password_hash/pin_hash (ADR-011 applied to devices, ADR-017 0.4.3
	// amendment). A revoked/expired row is never filtered out here — the
	// edge learns a credential is dead by syncing it, not by its absence.
	ListEdgeCredentials(ctx context.Context, tenantID, outletID string) ([]contracts.EdgeDeviceCredential, error)
}
