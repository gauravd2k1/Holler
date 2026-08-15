package outlet

import (
	"context"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/holler/backend/internal/platform/httpx"
	contracts "github.com/holler/contracts"
	"github.com/jackc/pgx/v5"
)

// fakeDeviceRepo is an in-memory DeviceRepository. Its findCredentialForVerify
// / touchCredentialLastUsed methods are unexported, so — like the real
// PostgresRepository — only code inside package outlet can construct
// something satisfying DeviceRepository.
type fakeDeviceRepo struct {
	devices        map[string]Device
	credentials    map[string]DeviceCredential // credentialID -> row
	tokenHashes    map[string]string           // credentialID -> hash
	outletVersions map[string]int              // outletID -> config_version, mirrors fakeRepo's outlets map
	bumpCalls      int

	// failBumpForOutlet, when non-empty, makes BumpOutletConfigVersion
	// return an error for that outletID. Combined with WithTx's
	// snapshot/restore below, this is what lets
	// TestEnrollDevice_CredentialAndBumpAreAtomic (and its rotate/revoke
	// siblings) falsify a regression where the bump is ever pulled back out
	// of the transaction (T13 retry, DEFECT 1): if InsertCredential/
	// RevokeActiveCredential were called directly against the maps instead
	// of through WithTx's snapshot, this failure would no longer roll them
	// back.
	failBumpForOutlet string
}

func newFakeDeviceRepo() *fakeDeviceRepo {
	return &fakeDeviceRepo{
		devices:        map[string]Device{},
		credentials:    map[string]DeviceCredential{},
		tokenHashes:    map[string]string{},
		outletVersions: map[string]int{},
	}
}

// WithTx snapshots credentials/tokenHashes/outletVersions before running fn
// and restores that snapshot if fn returns an error, so the in-memory fake
// gives the same all-or-nothing guarantee a real Postgres transaction does.
// This is what makes TestEnrollDevice_CredentialAndBumpAreAtomic (and its
// rotate/revoke siblings) an actual regression test rather than one that
// merely exercises the happy path (T13 retry, DEFECT 1).
func (f *fakeDeviceRepo) WithTx(_ context.Context, fn func(tx pgx.Tx) error) error {
	credsBackup := make(map[string]DeviceCredential, len(f.credentials))
	for k, v := range f.credentials {
		credsBackup[k] = v
	}
	hashesBackup := make(map[string]string, len(f.tokenHashes))
	for k, v := range f.tokenHashes {
		hashesBackup[k] = v
	}
	versionsBackup := make(map[string]int, len(f.outletVersions))
	for k, v := range f.outletVersions {
		versionsBackup[k] = v
	}

	if err := fn(nil); err != nil {
		f.credentials = credsBackup
		f.tokenHashes = hashesBackup
		f.outletVersions = versionsBackup
		return err
	}
	return nil
}

func (f *fakeDeviceRepo) InsertDevice(_ context.Context, _ string, d Device) error {
	for _, existing := range f.devices {
		if existing.OutletID == d.OutletID && existing.Name == d.Name {
			return httpx.ErrConflict
		}
	}
	f.devices[d.ID] = d
	return nil
}

func (f *fakeDeviceRepo) FindDeviceByOutletAndName(_ context.Context, _, outletID, name string) (Device, error) {
	for _, d := range f.devices {
		if d.OutletID == outletID && d.Name == name {
			return d, nil
		}
	}
	return Device{}, httpx.ErrNotFound
}

func (f *fakeDeviceRepo) GetDevice(_ context.Context, _, deviceID string) (Device, error) {
	d, ok := f.devices[deviceID]
	if !ok {
		return Device{}, httpx.ErrNotFound
	}
	return d, nil
}

func (f *fakeDeviceRepo) MarkDeviceEnrolled(_ context.Context, deviceID string, now time.Time) error {
	d, ok := f.devices[deviceID]
	if !ok {
		return httpx.ErrNotFound
	}
	if d.EnrolledAt == nil {
		d.EnrolledAt = &now
		f.devices[deviceID] = d
	}
	return nil
}

func (f *fakeDeviceRepo) InsertCredential(_ context.Context, _ pgx.Tx, c DeviceCredential, tokenHash string) error {
	for _, existing := range f.credentials {
		if existing.DeviceID == c.DeviceID && existing.RevokedAt == nil {
			return httpx.ErrConflict
		}
	}
	f.credentials[c.ID] = c
	f.tokenHashes[c.ID] = tokenHash
	return nil
}

func (f *fakeDeviceRepo) RevokeActiveCredential(_ context.Context, _ pgx.Tx, deviceID string, now time.Time, configVersion int) error {
	for id, c := range f.credentials {
		if c.DeviceID == deviceID && c.RevokedAt == nil {
			c.RevokedAt = &now
			c.ConfigVersion = configVersion
			f.credentials[id] = c
		}
	}
	return nil
}

func (f *fakeDeviceRepo) HasActiveCredential(_ context.Context, deviceID string) (bool, error) {
	for _, c := range f.credentials {
		if c.DeviceID == deviceID && c.RevokedAt == nil {
			return true, nil
		}
	}
	return false, nil
}

func (f *fakeDeviceRepo) findCredentialForVerify(_ context.Context, credentialID string) (deviceCredentialVerifyRow, error) {
	c, ok := f.credentials[credentialID]
	if !ok {
		return deviceCredentialVerifyRow{}, httpx.ErrUnauthorized
	}
	d := f.devices[c.DeviceID]
	return deviceCredentialVerifyRow{
		credentialID:  c.ID,
		deviceID:      c.DeviceID,
		tenantID:      c.TenantID,
		outletID:      c.OutletID,
		tokenHash:     f.tokenHashes[c.ID],
		credRevokedAt: c.RevokedAt,
		expiresAt:     c.ExpiresAt,
		deviceRevoked: d.RevokedAt,
	}, nil
}

func (f *fakeDeviceRepo) touchCredentialLastUsed(_ context.Context, credentialID string, now time.Time) error {
	c, ok := f.credentials[credentialID]
	if !ok {
		return httpx.ErrNotFound
	}
	c.LastUsedAt = &now
	f.credentials[credentialID] = c
	return nil
}

// BumpOutletConfigVersion mutates f.outletVersions directly, which
// newDeviceTestFixture wires to mirror f.outlets' own Outlet.ConfigVersion
// field on every call — in production both DeviceRepository and Repository
// are the same *PostgresRepository over the same outlet row, so a unit test
// splitting them into two fakes must keep them in sync by hand.
func (f *fakeDeviceRepo) BumpOutletConfigVersion(_ context.Context, _ pgx.Tx, outletID string) (int, error) {
	if f.failBumpForOutlet != "" && f.failBumpForOutlet == outletID {
		return 0, errors.New("simulated config_version bump failure")
	}
	f.bumpCalls++
	f.outletVersions[outletID]++
	return f.outletVersions[outletID], nil
}

func (f *fakeDeviceRepo) ListEdgeCredentials(_ context.Context, tenantID, outletID string, sinceVersion int) ([]contracts.EdgeDeviceCredential, error) {
	out := make([]contracts.EdgeDeviceCredential, 0)
	for _, c := range f.credentials {
		if c.OutletID != outletID || c.TenantID != tenantID || c.ConfigVersion <= sinceVersion {
			continue
		}
		d := f.devices[c.DeviceID]
		out = append(out, contracts.EdgeDeviceCredential{
			CredentialID:   c.ID,
			DeviceID:       c.DeviceID,
			TenantID:       c.TenantID,
			OutletID:       c.OutletID,
			CredentialHash: f.tokenHashes[c.ID],
			DeviceKind:     string(d.Kind),
			RevokedAt:      formatEdgeTimestamp(c.RevokedAt),
			ExpiresAt:      formatEdgeTimestamp(c.ExpiresAt),
			SchemaVersion:  1,
			ConfigVersion:  c.ConfigVersion,
		})
	}
	return out, nil
}

// newDeviceTestFixture wires outlets (Repository) and devices
// (DeviceRepository) so a BumpOutletConfigVersion call through EITHER fake
// is visible through the OTHER's GetByID/ConfigVersion — see
// fakeDeviceRepo.BumpOutletConfigVersion's own doc comment for why this
// syncing is necessary only in the test double, never in production.
func newDeviceTestFixture() (*fakeRepo, *fakeDeviceRepo, *DeviceService) {
	outlets := newFakeRepo()
	outlets.brandTenant["brand-a"] = "tenant-a"
	outlets.outlets["outlet-a"] = Outlet{ID: "outlet-a", BrandID: "brand-a"}
	devices := newFakeDeviceRepo()
	devices.outletVersions["outlet-a"] = 0
	outlets.onConfigVersionRead = func(outletID string) int { return devices.outletVersions[outletID] }
	svc := NewDeviceService(outlets, devices, nil)
	return outlets, devices, svc
}

func TestEnrollDevice_ReturnsTokenExactlyOnceAndVerifies(t *testing.T) {
	_, _, svc := newDeviceTestFixture()
	principal := Principal{TenantID: "tenant-a", UserID: "user-1"}

	enrolled, err := svc.EnrollDevice(context.Background(), principal, "outlet-a", DeviceKindPOS, "POS-1", "install visit 1", nil)
	if err != nil {
		t.Fatalf("EnrollDevice: %v", err)
	}
	if enrolled.Token == "" {
		t.Fatal("expected a non-empty plaintext token")
	}
	if !strings.Contains(enrolled.Token, ".") {
		t.Fatalf("expected token to carry a credential-id prefix, got %q", enrolled.Token)
	}

	principalOut, err := svc.VerifyToken(context.Background(), enrolled.Token)
	if err != nil {
		t.Fatalf("VerifyToken: %v", err)
	}
	if principalOut.DeviceID != enrolled.Device.ID || principalOut.TenantID != "tenant-a" || principalOut.OutletID != "outlet-a" {
		t.Fatalf("unexpected device principal: %+v", principalOut)
	}
}

// TestVerifyToken_ResolvesTenantFromCredentialNotCaller is the direct test
// for ADR-017 hole 1: a caller cannot supply its own tenant_id/outlet_id —
// VerifyToken only ever returns what the credential row says.
func TestVerifyToken_ResolvesTenantFromCredentialNotCaller(t *testing.T) {
	outlets, _, svc := newDeviceTestFixture()
	outlets.brandTenant["brand-b"] = "tenant-b"
	outlets.outlets["outlet-b"] = Outlet{ID: "outlet-b", BrandID: "brand-b"}

	enrolled, err := svc.EnrollDevice(context.Background(), Principal{TenantID: "tenant-b"}, "outlet-b", DeviceKindKDS, "KDS-1", "", nil)
	if err != nil {
		t.Fatalf("EnrollDevice: %v", err)
	}

	p, err := svc.VerifyToken(context.Background(), enrolled.Token)
	if err != nil {
		t.Fatalf("VerifyToken: %v", err)
	}
	if p.TenantID != "tenant-b" || p.OutletID != "outlet-b" {
		t.Fatalf("expected tenant/outlet resolved from the credential row, got %+v", p)
	}
}

func TestVerifyToken_RejectsUnknownToken(t *testing.T) {
	_, _, svc := newDeviceTestFixture()
	if _, err := svc.VerifyToken(context.Background(), "not-a-real-token"); !errors.Is(err, httpx.ErrUnauthorized) {
		t.Fatalf("expected ErrUnauthorized for a malformed token, got %v", err)
	}
	if _, err := svc.VerifyToken(context.Background(), "00000000-0000-7000-8000-000000000000.bogus-secret"); !errors.Is(err, httpx.ErrUnauthorized) {
		t.Fatalf("expected ErrUnauthorized for an unknown credential id, got %v", err)
	}
}

func TestVerifyToken_RejectsWrongSecret(t *testing.T) {
	_, _, svc := newDeviceTestFixture()
	enrolled, err := svc.EnrollDevice(context.Background(), Principal{TenantID: "tenant-a"}, "outlet-a", DeviceKindPOS, "POS-1", "", nil)
	if err != nil {
		t.Fatalf("EnrollDevice: %v", err)
	}
	credentialID := strings.SplitN(enrolled.Token, ".", 2)[0]
	tampered := credentialID + ".wrong-secret-entirely"

	if _, err := svc.VerifyToken(context.Background(), tampered); !errors.Is(err, httpx.ErrUnauthorized) {
		t.Fatalf("expected ErrUnauthorized for a wrong secret, got %v", err)
	}
}

func TestEnrollDevice_SecondEnrollWithActiveCredentialConflicts(t *testing.T) {
	_, _, svc := newDeviceTestFixture()
	principal := Principal{TenantID: "tenant-a"}

	if _, err := svc.EnrollDevice(context.Background(), principal, "outlet-a", DeviceKindPOS, "POS-1", "", nil); err != nil {
		t.Fatalf("first EnrollDevice: %v", err)
	}
	_, err := svc.EnrollDevice(context.Background(), principal, "outlet-a", DeviceKindPOS, "POS-1", "", nil)
	if !errors.Is(err, httpx.ErrConflict) {
		t.Fatalf("expected ErrConflict re-enrolling a device with a live credential, got %v", err)
	}
}

// TestRotateCredential_InvalidatesThePriorToken is the falsifying test for
// rotation: after rotating, the OLD plaintext token must stop verifying —
// otherwise "revoke before issue" would be a claim the code does not keep.
func TestRotateCredential_InvalidatesThePriorToken(t *testing.T) {
	_, _, svc := newDeviceTestFixture()
	principal := Principal{TenantID: "tenant-a"}

	enrolled, err := svc.EnrollDevice(context.Background(), principal, "outlet-a", DeviceKindPOS, "POS-1", "", nil)
	if err != nil {
		t.Fatalf("EnrollDevice: %v", err)
	}
	oldToken := enrolled.Token

	rotated, err := svc.RotateCredential(context.Background(), principal, enrolled.Device.ID, "rotation 1", nil)
	if err != nil {
		t.Fatalf("RotateCredential: %v", err)
	}
	if rotated.Token == oldToken {
		t.Fatal("expected a fresh token from rotation")
	}

	if _, err := svc.VerifyToken(context.Background(), oldToken); !errors.Is(err, httpx.ErrUnauthorized) {
		t.Fatalf("expected the old token to be rejected after rotation, got %v", err)
	}
	if _, err := svc.VerifyToken(context.Background(), rotated.Token); err != nil {
		t.Fatalf("expected the new token to verify, got %v", err)
	}
}

// TestRevokeCredential_StopsAuthentication is the falsifying test for
// revocation without replacement.
func TestRevokeCredential_StopsAuthentication(t *testing.T) {
	_, _, svc := newDeviceTestFixture()
	principal := Principal{TenantID: "tenant-a"}

	enrolled, err := svc.EnrollDevice(context.Background(), principal, "outlet-a", DeviceKindPOS, "POS-1", "", nil)
	if err != nil {
		t.Fatalf("EnrollDevice: %v", err)
	}

	if err := svc.RevokeCredential(context.Background(), principal, enrolled.Device.ID, nil); err != nil {
		t.Fatalf("RevokeCredential: %v", err)
	}

	if _, err := svc.VerifyToken(context.Background(), enrolled.Token); !errors.Is(err, httpx.ErrUnauthorized) {
		t.Fatalf("expected a revoked credential's token to be rejected, got %v", err)
	}
}

func TestEnrollDevice_RejectsCrossTenantOutlet(t *testing.T) {
	_, _, svc := newDeviceTestFixture()
	_, err := svc.EnrollDevice(context.Background(), Principal{TenantID: "tenant-b"}, "outlet-a", DeviceKindPOS, "POS-1", "", nil)
	if !errors.Is(err, httpx.ErrNotFound) {
		t.Fatalf("expected ErrNotFound enrolling a device at another tenant's outlet, got %v", err)
	}
}

func TestEnrollDevice_RejectsUnknownKind(t *testing.T) {
	_, _, svc := newDeviceTestFixture()
	_, err := svc.EnrollDevice(context.Background(), Principal{TenantID: "tenant-a"}, "outlet-a", DeviceKind("TOASTER"), "T-1", "", nil)
	if !errors.Is(err, httpx.ErrInvalidInput) {
		t.Fatalf("expected ErrInvalidInput for an unknown device kind, got %v", err)
	}
}

// --- T13 retry, DEFECT 1: credential write / config_version bump atomicity -

// TestEnrollDevice_CredentialAndBumpAreAtomic is the falsifying test for
// DEFECT 1: if InsertCredential and BumpOutletConfigVersion are ever split
// back into two independent, non-transactional calls, the credential insert
// will have already been committed by the time the (simulated) bump
// failure is observed, and this test goes red because the credential is
// still present after EnrollDevice returned an error.
func TestEnrollDevice_CredentialAndBumpAreAtomic(t *testing.T) {
	_, devices, svc := newDeviceTestFixture()
	devices.failBumpForOutlet = "outlet-a"
	principal := Principal{TenantID: "tenant-a"}

	_, err := svc.EnrollDevice(context.Background(), principal, "outlet-a", DeviceKindPOS, "POS-1", "", nil)
	if err == nil {
		t.Fatal("expected EnrollDevice to fail when the config_version bump fails")
	}
	if len(devices.credentials) != 0 {
		t.Fatalf("expected the credential insert to roll back with the failed bump, got %d credentials", len(devices.credentials))
	}
}

// TestRotateCredential_RevokeInsertAndBumpAreAtomic is DEFECT 1's falsifying
// test for RotateCredential: a failed bump must roll back both the revoke of
// the old credential and the insert of the new one, leaving the device
// exactly as it was before rotation was attempted.
func TestRotateCredential_RevokeInsertAndBumpAreAtomic(t *testing.T) {
	_, devices, svc := newDeviceTestFixture()
	principal := Principal{TenantID: "tenant-a"}

	enrolled, err := svc.EnrollDevice(context.Background(), principal, "outlet-a", DeviceKindPOS, "POS-1", "", nil)
	if err != nil {
		t.Fatalf("EnrollDevice: %v", err)
	}

	devices.failBumpForOutlet = "outlet-a"
	if _, err := svc.RotateCredential(context.Background(), principal, enrolled.Device.ID, "rotation 1", nil); err == nil {
		t.Fatal("expected RotateCredential to fail when the config_version bump fails")
	}

	if len(devices.credentials) != 1 {
		t.Fatalf("expected exactly the original credential to survive the rolled-back rotation, got %d", len(devices.credentials))
	}
	if _, err := svc.VerifyToken(context.Background(), enrolled.Token); err != nil {
		t.Fatalf("expected the pre-rotation token to still verify after the rolled-back rotation, got %v", err)
	}
}

// TestRevokeCredential_RevokeAndBumpAreAtomic is DEFECT 1's falsifying test
// for RevokeCredential: a failed bump must roll back the revoke, leaving the
// original token still able to authenticate.
func TestRevokeCredential_RevokeAndBumpAreAtomic(t *testing.T) {
	_, devices, svc := newDeviceTestFixture()
	principal := Principal{TenantID: "tenant-a"}

	enrolled, err := svc.EnrollDevice(context.Background(), principal, "outlet-a", DeviceKindPOS, "POS-1", "", nil)
	if err != nil {
		t.Fatalf("EnrollDevice: %v", err)
	}

	devices.failBumpForOutlet = "outlet-a"
	if err := svc.RevokeCredential(context.Background(), principal, enrolled.Device.ID, nil); err == nil {
		t.Fatal("expected RevokeCredential to fail when the config_version bump fails")
	}

	if _, err := svc.VerifyToken(context.Background(), enrolled.Token); err != nil {
		t.Fatalf("expected the token to still verify after the rolled-back revoke, got %v", err)
	}
}

// --- ListEdgeDeviceCredentials (T13, ADR-017 0.4.3 amendment) ---------------

func TestListEdgeDeviceCredentials_BumpsAndFiltersBySinceVersion(t *testing.T) {
	outlets, devices, svc := newDeviceTestFixture()
	principal := Principal{TenantID: "tenant-a"}

	before := devices.outletVersions["outlet-a"]
	enrolled, err := svc.EnrollDevice(context.Background(), principal, "outlet-a", DeviceKindKDS, "KDS-1", "", nil)
	if err != nil {
		t.Fatalf("EnrollDevice: %v", err)
	}
	afterEnroll := devices.outletVersions["outlet-a"]
	if afterEnroll != before+1 {
		t.Fatalf("expected exactly one config_version bump on enroll, before=%d after=%d", before, afterEnroll)
	}
	_ = outlets

	// A pull at the CURRENT config_version excludes the credential — the
	// same since_version contract every other config aggregate obeys.
	atCurrent, err := svc.ListEdgeDeviceCredentials(context.Background(), "tenant-a", "outlet-a", afterEnroll)
	if err != nil {
		t.Fatalf("ListEdgeDeviceCredentials at current version: %v", err)
	}
	if len(atCurrent) != 0 {
		t.Fatalf("expected no credentials at the current watermark, got %+v", atCurrent)
	}

	// A pull BELOW the current config_version returns it, hash intact.
	stale, err := svc.ListEdgeDeviceCredentials(context.Background(), "tenant-a", "outlet-a", before)
	if err != nil {
		t.Fatalf("ListEdgeDeviceCredentials below watermark: %v", err)
	}
	if len(stale) != 1 || stale[0].DeviceID != enrolled.Device.ID {
		t.Fatalf("expected exactly the enrolled device's credential, got %+v", stale)
	}
	if stale[0].CredentialHash == "" || stale[0].CredentialHash == enrolled.Token {
		t.Fatalf("expected a non-empty hash distinct from the plaintext token, got %q", stale[0].CredentialHash)
	}
	if stale[0].ConfigVersion != afterEnroll {
		t.Fatalf("expected the row's reported config_version to equal the outlet's, got %d want %d", stale[0].ConfigVersion, afterEnroll)
	}
}

// TestListEdgeDeviceCredentials_RevokedCredentialStillReturned is the direct
// test for ADR-017's explicit failure mode: "a revoked credential must not
// look merely un-synced". A caller that filtered on revoked_at IS NULL would
// make a revoked credential indistinguishable from one never synced at all.
func TestListEdgeDeviceCredentials_RevokedCredentialStillReturned(t *testing.T) {
	_, _, svc := newDeviceTestFixture()
	principal := Principal{TenantID: "tenant-a"}

	enrolled, err := svc.EnrollDevice(context.Background(), principal, "outlet-a", DeviceKindKDS, "KDS-1", "", nil)
	if err != nil {
		t.Fatalf("EnrollDevice: %v", err)
	}
	if err := svc.RevokeCredential(context.Background(), principal, enrolled.Device.ID, nil); err != nil {
		t.Fatalf("RevokeCredential: %v", err)
	}

	creds, err := svc.ListEdgeDeviceCredentials(context.Background(), "tenant-a", "outlet-a", 0)
	if err != nil {
		t.Fatalf("ListEdgeDeviceCredentials: %v", err)
	}
	if len(creds) != 1 {
		t.Fatalf("expected the revoked credential to still be present, got %+v", creds)
	}
	if creds[0].RevokedAt == nil {
		t.Fatal("expected revoked_at to be populated, not the row dropped")
	}
}
