package outlet

import (
	"context"
	"crypto/rand"
	"encoding/base64"
	"errors"
	"fmt"
	"strings"
	"time"

	"github.com/holler/backend/internal/auth"
	"github.com/holler/backend/internal/platform/crypto"
	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/platform/id"
	contracts "github.com/holler/contracts"
	"github.com/jackc/pgx/v5"
)

// deviceTokenSecretBytes is 256 bits of entropy for the per-device token
// (ADR-017 "high-entropy token"), before base64url encoding.
const deviceTokenSecretBytes = 32

// DeviceService implements device enrollment, rotation, revocation and
// credential verification (ADR-017). It is deliberately separate from
// Service (outlet CRUD) even though both share a Postgres pool: enrollment
// is a security boundary, not an outlet business command.
type DeviceService struct {
	outlets Repository
	devices DeviceRepository
	auditor auth.AuditRecorder
	now     func() time.Time
}

func NewDeviceService(outlets Repository, devices DeviceRepository, auditor auth.AuditRecorder) *DeviceService {
	return &DeviceService{outlets: outlets, devices: devices, auditor: auditor, now: time.Now}
}

// EnrolledDevice is what a successful enrollment or rotation returns: the
// device row plus the credential's PLAINTEXT token. Token is populated only
// by EnrollDevice/RotateCredential — there is no method anywhere in this
// package that reads a token back a second time (ADR-017 §1, a structural
// guarantee: no query in device_postgres.go selects token_hash back out as
// plaintext, because token_hash is never the plaintext to begin with).
type EnrolledDevice struct {
	Device       Device
	CredentialID string
	Token        string
}

// EnrollDevice registers (or re-enrolls) a device at outletID and mints it a
// fresh credential. If a device already exists at (outletID, name) and
// already holds an active credential, callers must call RotateCredential
// instead — enrollment never silently replaces a live credential out from
// under a device that is still using it.
func (s *DeviceService) EnrollDevice(ctx context.Context, principal Principal, outletID string, kind DeviceKind, name, label string, actorUserID *string) (EnrolledDevice, error) {
	if principal.TenantID == "" {
		return EnrolledDevice{}, httpx.ErrUnauthorized
	}
	outletID = strings.TrimSpace(outletID)
	name = strings.TrimSpace(name)
	if outletID == "" || name == "" {
		return EnrolledDevice{}, fmt.Errorf("%w: outlet_id and name are required", httpx.ErrInvalidInput)
	}
	if !isValidDeviceKind(kind) {
		return EnrolledDevice{}, fmt.Errorf("%w: unknown device kind %q", httpx.ErrInvalidInput, kind)
	}
	if _, err := s.outlets.GetByID(ctx, principal.TenantID, outletID); err != nil {
		return EnrolledDevice{}, err
	}

	now := s.now().UTC()
	device, err := s.devices.FindDeviceByOutletAndName(ctx, principal.TenantID, outletID, name)
	switch {
	case errors.Is(err, httpx.ErrNotFound):
		device = Device{
			ID:         id.New(),
			OutletID:   outletID,
			Kind:       kind,
			Name:       name,
			EnrolledAt: &now,
			CreatedAt:  now,
			UpdatedAt:  now,
		}
		if err := s.devices.InsertDevice(ctx, principal.TenantID, device); err != nil {
			return EnrolledDevice{}, err
		}
	case err != nil:
		return EnrolledDevice{}, err
	default:
		if device.RevokedAt != nil {
			return EnrolledDevice{}, fmt.Errorf("%w: device is revoked", httpx.ErrConflict)
		}
		active, err := s.devices.HasActiveCredential(ctx, device.ID)
		if err != nil {
			return EnrolledDevice{}, err
		}
		if active {
			return EnrolledDevice{}, fmt.Errorf("%w: device already enrolled; rotate its credential instead", httpx.ErrConflict)
		}
		if err := s.devices.MarkDeviceEnrolled(ctx, device.ID, now); err != nil {
			return EnrolledDevice{}, err
		}
	}

	// The bump and the credential insert must commit together or not at all
	// (T13 retry, DEFECT 1): a crash between them would leave a committed
	// credential no edge ever learns about, and a retried enroll then
	// reports "device already enrolled" even though the device can never
	// authenticate. Since contracts 0.4.5 the order also matters on its own
	// terms: the row must carry the version the outlet is bumped TO, so the
	// bump runs FIRST and its returned version is threaded into the insert
	// (ADR-017 0.4.5 addendum) — never insert-then-bump.
	var issued issuedCredential
	err = s.devices.WithTx(ctx, func(tx pgx.Tx) error {
		newVersion, txErr := s.devices.BumpOutletConfigVersion(ctx, tx, outletID)
		if txErr != nil {
			return txErr
		}
		issued, txErr = s.issueCredential(ctx, tx, principal.TenantID, outletID, device.ID, label, now, newVersion)
		return txErr
	})
	if err != nil {
		return EnrolledDevice{}, err
	}

	s.audit(ctx, principal.TenantID, actorUserID, "device.enroll", device.ID, map[string]interface{}{
		"device_id": device.ID, "outlet_id": outletID, "kind": string(kind), "name": name,
		"credential_id": issued.CredentialID,
	})

	return EnrolledDevice{Device: device, CredentialID: issued.CredentialID, Token: issued.Token}, nil
}

// RotateCredential revokes deviceID's current active credential and mints a
// fresh one. It appends a new row and stamps revoked_at on the old one
// rather than updating in place, so a compromised credential leaves a trail
// rather than being overwritten out of history (0008_device_enrollment.sql).
func (s *DeviceService) RotateCredential(ctx context.Context, principal Principal, deviceID, label string, actorUserID *string) (EnrolledDevice, error) {
	if principal.TenantID == "" {
		return EnrolledDevice{}, httpx.ErrUnauthorized
	}
	device, err := s.devices.GetDevice(ctx, principal.TenantID, deviceID)
	if err != nil {
		return EnrolledDevice{}, err
	}
	if device.RevokedAt != nil {
		return EnrolledDevice{}, fmt.Errorf("%w: device is revoked", httpx.ErrConflict)
	}

	now := s.now().UTC()
	// The bump, the revoke of the old credential and the insert of the new
	// one must all commit together or not at all (T13 retry, DEFECT 1) — see
	// EnrollDevice. The bump runs FIRST (ADR-017 0.4.5 addendum): both the
	// revoked row and the fresh row must carry the version the outlet is
	// bumped TO.
	var issued issuedCredential
	err = s.devices.WithTx(ctx, func(tx pgx.Tx) error {
		newVersion, txErr := s.devices.BumpOutletConfigVersion(ctx, tx, device.OutletID)
		if txErr != nil {
			return txErr
		}
		if txErr := s.devices.RevokeActiveCredential(ctx, tx, device.ID, now, newVersion); txErr != nil {
			return txErr
		}
		issued, txErr = s.issueCredential(ctx, tx, principal.TenantID, device.OutletID, device.ID, label, now, newVersion)
		return txErr
	})
	if err != nil {
		return EnrolledDevice{}, err
	}

	s.audit(ctx, principal.TenantID, actorUserID, "device.credential.rotate", device.ID, map[string]interface{}{
		"device_id": device.ID, "credential_id": issued.CredentialID,
	})
	return EnrolledDevice{Device: device, CredentialID: issued.CredentialID, Token: issued.Token}, nil
}

// RevokeCredential revokes deviceID's current active credential without
// issuing a replacement. The device row itself is untouched — it remains
// registered, simply unable to authenticate until re-enrolled or rotated.
func (s *DeviceService) RevokeCredential(ctx context.Context, principal Principal, deviceID string, actorUserID *string) error {
	if principal.TenantID == "" {
		return httpx.ErrUnauthorized
	}
	device, err := s.devices.GetDevice(ctx, principal.TenantID, deviceID)
	if err != nil {
		return err
	}
	now := s.now().UTC()
	// The bump and the revoke must commit together or not at all (T13 retry,
	// DEFECT 1) — see EnrollDevice. The bump runs FIRST (ADR-017 0.4.5
	// addendum): the revoked row must carry the version the outlet is
	// bumped TO, or the revocation would never reach an edge pulling
	// GET /sync/config — the edge would keep honouring a credential the
	// cloud has revoked.
	err = s.devices.WithTx(ctx, func(tx pgx.Tx) error {
		newVersion, txErr := s.devices.BumpOutletConfigVersion(ctx, tx, device.OutletID)
		if txErr != nil {
			return txErr
		}
		return s.devices.RevokeActiveCredential(ctx, tx, device.ID, now, newVersion)
	})
	if err != nil {
		return err
	}
	s.audit(ctx, principal.TenantID, actorUserID, "device.credential.revoke", device.ID, map[string]interface{}{
		"device_id": device.ID,
	})
	return nil
}

// issuedCredential is the private mint result; only issueCredential and its
// two exported callers ever see a Token.
type issuedCredential struct {
	CredentialID string
	Token        string
}

// issueCredential mints a fresh high-entropy token, hashes it with Argon2id
// and persists only the hash. The plaintext is composed of the credential's
// own id (a public lookup key, not a secret) plus the random secret, so
// VerifyToken can find the row by id instead of scanning every active
// credential. configVersion must be the value BumpOutletConfigVersion just
// returned within the SAME transaction (ADR-017 0.4.5 addendum) — every
// caller of issueCredential bumps before calling it.
func (s *DeviceService) issueCredential(ctx context.Context, tx pgx.Tx, tenantID, outletID, deviceID, label string, now time.Time, configVersion int) (issuedCredential, error) {
	credentialID := id.New()
	secret, err := generateDeviceSecret()
	if err != nil {
		return issuedCredential{}, err
	}
	hash, err := crypto.HashPassword(secret)
	if err != nil {
		return issuedCredential{}, err
	}

	cred := DeviceCredential{
		ID:            credentialID,
		DeviceID:      deviceID,
		TenantID:      tenantID,
		OutletID:      outletID,
		Label:         label,
		CreatedAt:     now,
		ConfigVersion: configVersion,
	}
	if err := s.devices.InsertCredential(ctx, tx, cred, hash); err != nil {
		return issuedCredential{}, err
	}
	return issuedCredential{CredentialID: credentialID, Token: credentialID + "." + secret}, nil
}

func generateDeviceSecret() (string, error) {
	buf := make([]byte, deviceTokenSecretBytes)
	if _, err := rand.Read(buf); err != nil {
		return "", fmt.Errorf("outlet: generating device token: %w", err)
	}
	return base64.RawURLEncoding.EncodeToString(buf), nil
}

// DevicePrincipal is what a verified device credential resolves to. TenantID
// and OutletID come from the device_credential row, never from anything the
// caller supplied — the fix for ADR-017 hole 1: a mis-enrolled edge node can
// no longer mislabel its own envelopes, because the labels are not its to
// give.
type DevicePrincipal struct {
	DeviceID string
	TenantID string
	OutletID string
}

// ErrInvalidDeviceToken is returned for every verification failure — unknown
// credential id, malformed token, revoked credential, expired credential,
// revoked device, wrong secret — so a caller can never distinguish which
// case occurred from the error alone.
var ErrInvalidDeviceToken = fmt.Errorf("%w: invalid device token", httpx.ErrUnauthorized)

// VerifyToken authenticates a presented device token, returning the
// DevicePrincipal it resolves to. The token format is
// "<credential_id>.<secret>": the id is a public lookup key (a UUIDv7, not a
// secret), and the secret is what is Argon2id-verified against token_hash —
// this makes verification a single indexed lookup rather than a scan over
// every active credential.
func (s *DeviceService) VerifyToken(ctx context.Context, token string) (DevicePrincipal, error) {
	credentialID, secret, ok := strings.Cut(token, ".")
	if !ok || credentialID == "" || secret == "" {
		return DevicePrincipal{}, ErrInvalidDeviceToken
	}
	if _, err := id.Parse(credentialID); err != nil {
		return DevicePrincipal{}, ErrInvalidDeviceToken
	}

	row, err := s.devices.findCredentialForVerify(ctx, credentialID)
	if err != nil {
		return DevicePrincipal{}, ErrInvalidDeviceToken
	}
	if row.credRevokedAt != nil || row.deviceRevoked != nil {
		return DevicePrincipal{}, ErrInvalidDeviceToken
	}
	if row.expiresAt != nil && !row.expiresAt.After(s.now().UTC()) {
		return DevicePrincipal{}, ErrInvalidDeviceToken
	}
	if err := crypto.VerifyPassword(secret, row.tokenHash); err != nil {
		return DevicePrincipal{}, ErrInvalidDeviceToken
	}

	_ = s.devices.touchCredentialLastUsed(ctx, row.credentialID, s.now().UTC())

	return DevicePrincipal{DeviceID: row.deviceID, TenantID: row.tenantID, OutletID: row.outletID}, nil
}

// ListEdgeDeviceCredentials resolves the device_credentials array of
// GET /sync/config (T13, ADR-017 0.4.3 amendment): every credential enrolled
// at outletID whose OWN config_version exceeds sinceVersion — active,
// revoked AND expired, hash intact — so a KDS can verify a LAN handshake
// against its local cache with the uplink down. Like VerifyToken, this is
// one of the few places in this package that ever touches a hash; unlike
// VerifyToken, the hash here is deliberately RETURNED to the caller, because
// the caller is GET /sync/config itself — the one route in this backend
// permitted to carry credential material on the wire (mirrors
// auth.Service.ListEdgeUserCache's identical carve-out for
// password_hash/pin_hash).
//
// Since contracts 0.4.5, device_credential carries its OWN config_version
// column (packages/contracts/postgres/0010_device_credential_config_version.sql),
// stamped at write time by InsertCredential/RevokeActiveCredential to the
// value the outlet was just bumped to. Filtering is therefore row-granular
// like every sibling config table (station, printer, menu_item_station,
// restaurant_table) — an unrelated config change elsewhere in the outlet no
// longer re-sends the whole credential collection.
//
// The early return below is still a valid optimisation, not a
// reintroduction of outlet-granular filtering: no credential row's
// config_version can ever exceed the outlet's own current config_version
// (every write stamps a value returned by the SAME bump, and versions only
// increase), so sinceVersion >= out.ConfigVersion guarantees zero rows would
// satisfy WHERE config_version > sinceVersion — it only skips the query,
// never changes the result.
func (s *DeviceService) ListEdgeDeviceCredentials(ctx context.Context, tenantID, outletID string, sinceVersion int) ([]contracts.EdgeDeviceCredential, error) {
	if tenantID == "" {
		return nil, httpx.ErrUnauthorized
	}
	out, err := s.outlets.GetByID(ctx, tenantID, outletID)
	if err != nil {
		return nil, err
	}
	if sinceVersion >= out.ConfigVersion {
		return []contracts.EdgeDeviceCredential{}, nil
	}

	return s.devices.ListEdgeCredentials(ctx, tenantID, outletID, sinceVersion)
}

// audit is a best-effort wrapper: enrollment/rotation/revocation succeed or
// fail on the credential mutation itself, never on whether the audit write
// landed, exactly like every other context's fire-and-forget _ =
// auditor.Audit(...) call (see backend/internal/kitchen/service.go).
func (s *DeviceService) audit(ctx context.Context, tenantID string, actorUserID *string, action, deviceID string, newValue map[string]interface{}) {
	if s.auditor == nil {
		return
	}
	_ = s.auditor.Audit(ctx, auth.AuditInput{
		TenantID:    tenantID,
		ActorUserID: actorUserID,
		Action:      action,
		EntityType:  "device",
		EntityID:    &deviceID,
		NewValue:    newValue,
	})
}
