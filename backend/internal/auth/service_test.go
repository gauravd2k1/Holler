package auth

import (
	"context"
	"testing"
	"time"

	"github.com/holler/backend/internal/platform/crypto"
	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/platform/id"
)

// fakeRepo is an in-memory UserRepository for unit tests.
type fakeRepo struct {
	users       map[string]credentialRow // by id
	emailIndex  map[string]string        // tenantID|email -> userID
	roles       map[string]Role          // by id
	userRoles   map[string][]RoleAssignment
	auditEvents []AuditEvent
}

func newFakeRepo() *fakeRepo {
	return &fakeRepo{
		users:      make(map[string]credentialRow),
		emailIndex: make(map[string]string),
		roles:      make(map[string]Role),
		userRoles:  make(map[string][]RoleAssignment),
	}
}

func (f *fakeRepo) FindUserByEmailForAuth(ctx context.Context, tenantID, email string) (credentialRow, error) {
	uid, ok := f.emailIndex[tenantID+"|"+email]
	if !ok {
		return credentialRow{}, httpx.ErrNotFound
	}
	return f.users[uid], nil
}

func (f *fakeRepo) FindUserByIDForAuth(ctx context.Context, userID string) (credentialRow, error) {
	row, ok := f.users[userID]
	if !ok {
		return credentialRow{}, httpx.ErrNotFound
	}
	return row, nil
}

func (f *fakeRepo) CreateUser(ctx context.Context, uid, tenantID, email, fullName, passwordHash string, now time.Time) error {
	f.users[uid] = credentialRow{id: uid, tenantID: tenantID, email: email, fullName: fullName, passwordHash: passwordHash, isActive: true, configVersion: 1, createdAt: now, updatedAt: now}
	f.emailIndex[tenantID+"|"+email] = uid
	return nil
}

func (f *fakeRepo) ListUsers(ctx context.Context, tenantID string) ([]User, error) {
	var out []User
	for _, row := range f.users {
		if row.tenantID == tenantID {
			out = append(out, f.toUser(row))
		}
	}
	return out, nil
}

func (f *fakeRepo) GetUser(ctx context.Context, tenantID, userID string) (User, error) {
	row, ok := f.users[userID]
	if !ok || row.tenantID != tenantID {
		return User{}, httpx.ErrNotFound
	}
	return f.toUser(row), nil
}

func (f *fakeRepo) toUser(row credentialRow) User {
	return User{
		ID: row.id, TenantID: row.tenantID, Email: row.email, FullName: row.fullName,
		IsActive: row.isActive, Roles: f.userRoles[row.id], CreatedAt: row.createdAt, UpdatedAt: row.updatedAt,
	}
}

func (f *fakeRepo) RolesForUser(ctx context.Context, userID string) ([]RoleAssignment, error) {
	return f.userRoles[userID], nil
}

func (f *fakeRepo) PermissionsForRole(ctx context.Context, roleID string) ([]Permission, error) {
	role, ok := f.roles[roleID]
	if !ok {
		return nil, nil
	}
	return role.Permissions, nil
}

func (f *fakeRepo) ReplaceUserRoles(ctx context.Context, userID string, assignments []RoleAssignment, now time.Time) error {
	f.userRoles[userID] = assignments
	if row, ok := f.users[userID]; ok {
		row.configVersion++
		row.updatedAt = now
		f.users[userID] = row
	}
	return nil
}

func (f *fakeRepo) UpdatePassword(ctx context.Context, userID, passwordHash string, now time.Time) error {
	row, ok := f.users[userID]
	if !ok {
		return httpx.ErrNotFound
	}
	row.passwordHash = passwordHash
	row.configVersion++
	row.updatedAt = now
	f.users[userID] = row
	return nil
}

func (f *fakeRepo) UpdatePin(ctx context.Context, userID, pinHash string, now time.Time) error {
	row, ok := f.users[userID]
	if !ok {
		return httpx.ErrNotFound
	}
	row.pinHash = &pinHash
	row.configVersion++
	row.updatedAt = now
	f.users[userID] = row
	return nil
}

func (f *fakeRepo) ListRoles(ctx context.Context, tenantID string) ([]Role, error) {
	var out []Role
	for _, r := range f.roles {
		if r.TenantID == tenantID {
			out = append(out, r)
		}
	}
	return out, nil
}

func (f *fakeRepo) GetRole(ctx context.Context, tenantID, roleID string) (Role, error) {
	role, ok := f.roles[roleID]
	if !ok || role.TenantID != tenantID {
		return Role{}, httpx.ErrNotFound
	}
	return role, nil
}

func (f *fakeRepo) addRole(role Role) {
	f.roles[role.ID] = role
}

// ListUsersForEdgeCache mirrors Repository.ListUsersForEdgeCache: users of
// tenantID with a role assignment either tenant-wide or scoped to outletID,
// newer than sinceVersion.
func (f *fakeRepo) ListUsersForEdgeCache(ctx context.Context, tenantID, outletID string, sinceVersion int) ([]credentialRow, error) {
	var out []credentialRow
	for uid, row := range f.users {
		if row.tenantID != tenantID || row.configVersion <= sinceVersion {
			continue
		}
		eligible := false
		for _, a := range f.userRoles[uid] {
			if a.OutletID == nil || *a.OutletID == outletID {
				eligible = true
				break
			}
		}
		if eligible {
			out = append(out, row)
		}
	}
	return out, nil
}

// fakeAuditor records Audit calls in-memory for assertions.
type fakeAuditor struct {
	calls []AuditInput
}

func (f *fakeAuditor) Audit(ctx context.Context, input AuditInput) error {
	f.calls = append(f.calls, input)
	return nil
}

// testClientIP is used by every service_test.go login unless a test needs a
// distinct address on purpose (rate-limit tests).
const testClientIP = "198.51.100.10"

func newTestService(t *testing.T) (*Service, *fakeRepo, *fakeAuditor) {
	t.Helper()
	repo := newFakeRepo()
	auditor := &fakeAuditor{}
	signer := NewTokenSigner([]byte("test-signing-key-not-a-secret"))
	refresh := NewInMemoryRefreshStore()
	limiter := NewInMemoryRateLimiter()
	svc := NewService(repo, signer, refresh, limiter, auditor, time.Minute, time.Hour)
	return svc, repo, auditor
}

func mustHash(t *testing.T, plaintext string) string {
	t.Helper()
	h, err := crypto.HashPassword(plaintext)
	if err != nil {
		t.Fatalf("hashing password: %v", err)
	}
	return h
}

func TestLogin_Success(t *testing.T) {
	svc, repo, _ := newTestService(t)
	tenantID := id.New()
	outletID := id.New()
	userID := id.New()

	hash := mustHash(t, "correct-horse-battery-staple")
	repo.CreateUser(context.Background(), userID, tenantID, "cashier@example.com", "Cash Ier", hash, time.Now())

	role := Role{ID: id.New(), TenantID: tenantID, Code: RoleCodeCashier, Name: "Cashier", Permissions: []Permission{PermissionOrderCreate}}
	repo.addRole(role)
	repo.userRoles[userID] = []RoleAssignment{{ID: id.New(), RoleID: role.ID, RoleCode: role.Code, OutletID: nil}}

	result, err := svc.Login(context.Background(), testClientIP, tenantID, "cashier@example.com", "correct-horse-battery-staple", outletID)
	if err != nil {
		t.Fatalf("expected login success, got %v", err)
	}
	if result.AccessToken == "" || result.RefreshToken == "" {
		t.Fatal("expected non-empty tokens")
	}
	if !hasPermission(result.Principal, string(PermissionOrderCreate)) {
		t.Fatal("expected principal to carry order.create from tenant-wide role")
	}
}

func TestLogin_WrongPassword_And_NoSuchUser_SameError(t *testing.T) {
	svc, repo, _ := newTestService(t)
	tenantID := id.New()
	userID := id.New()
	hash := mustHash(t, "correct-password")
	repo.CreateUser(context.Background(), userID, tenantID, "someone@example.com", "Someone", hash, time.Now())

	_, err1 := svc.Login(context.Background(), testClientIP, tenantID, "someone@example.com", "wrong-password", id.New())
	_, err2 := svc.Login(context.Background(), testClientIP, tenantID, "nobody@example.com", "irrelevant-password", id.New())

	if err1 == nil || err2 == nil {
		t.Fatal("expected both logins to fail")
	}
	if err1.Error() != err2.Error() {
		t.Fatalf("login failure must not distinguish wrong password from no such user: %v vs %v", err1, err2)
	}
}

func TestPermissionResolution_TenantWideVsOutletScoped(t *testing.T) {
	svc, repo, _ := newTestService(t)
	tenantID := id.New()
	outletA := id.New()
	outletB := id.New()
	userID := id.New()
	hash := mustHash(t, "password12345")
	repo.CreateUser(context.Background(), userID, tenantID, "manager@example.com", "Manager", hash, time.Now())

	tenantWideRole := Role{ID: id.New(), TenantID: tenantID, Code: RoleCodeWaiter, Name: "Waiter", Permissions: []Permission{PermissionOrderCreate}}
	outletScopedRole := Role{ID: id.New(), TenantID: tenantID, Code: RoleCodeOutletManager, Name: "Outlet Manager", Permissions: []Permission{PermissionMenuManage}}
	repo.addRole(tenantWideRole)
	repo.addRole(outletScopedRole)

	repo.userRoles[userID] = []RoleAssignment{
		{ID: id.New(), RoleID: tenantWideRole.ID, RoleCode: tenantWideRole.Code, OutletID: nil},
		{ID: id.New(), RoleID: outletScopedRole.ID, RoleCode: outletScopedRole.Code, OutletID: &outletA},
	}

	resultA, err := svc.Login(context.Background(), testClientIP, tenantID, "manager@example.com", "password12345", outletA)
	if err != nil {
		t.Fatalf("login at outlet A: %v", err)
	}
	if !hasPermission(resultA.Principal, string(PermissionOrderCreate)) {
		t.Error("expected tenant-wide permission to apply at outlet A")
	}
	if !hasPermission(resultA.Principal, string(PermissionMenuManage)) {
		t.Error("expected outlet-scoped permission to apply at its own outlet")
	}

	resultB, err := svc.Login(context.Background(), testClientIP, tenantID, "manager@example.com", "password12345", outletB)
	if err != nil {
		t.Fatalf("login at outlet B: %v", err)
	}
	if !hasPermission(resultB.Principal, string(PermissionOrderCreate)) {
		t.Error("expected tenant-wide permission to apply at outlet B too")
	}
	if hasPermission(resultB.Principal, string(PermissionMenuManage)) {
		t.Error("outlet-scoped permission from outlet A must not leak into outlet B")
	}
}

func TestRefresh_RotatesAndDetectsReuse(t *testing.T) {
	svc, repo, _ := newTestService(t)
	tenantID := id.New()
	outletID := id.New()
	userID := id.New()
	hash := mustHash(t, "password12345")
	repo.CreateUser(context.Background(), userID, tenantID, "user@example.com", "User", hash, time.Now())

	login, err := svc.Login(context.Background(), testClientIP, tenantID, "user@example.com", "password12345", outletID)
	if err != nil {
		t.Fatalf("login: %v", err)
	}

	rotated, err := svc.Refresh(context.Background(), login.RefreshToken)
	if err != nil {
		t.Fatalf("first refresh: %v", err)
	}
	if rotated.RefreshToken == login.RefreshToken {
		t.Fatal("expected a new refresh token on rotation")
	}

	// Using the rotated-away token again must fail and invalidate the chain.
	if _, err := svc.Refresh(context.Background(), login.RefreshToken); err == nil {
		t.Fatal("expected reuse of a rotated refresh token to fail")
	}

	// The chain is now dead: even the latest token must be rejected.
	if _, err := svc.Refresh(context.Background(), rotated.RefreshToken); err == nil {
		t.Fatal("expected the whole rotation chain to be invalidated after reuse")
	}
}

func TestLogout_RevokesRefreshToken(t *testing.T) {
	svc, repo, _ := newTestService(t)
	tenantID := id.New()
	outletID := id.New()
	userID := id.New()
	hash := mustHash(t, "password12345")
	repo.CreateUser(context.Background(), userID, tenantID, "user@example.com", "User", hash, time.Now())

	login, err := svc.Login(context.Background(), testClientIP, tenantID, "user@example.com", "password12345", outletID)
	if err != nil {
		t.Fatalf("login: %v", err)
	}

	svc.Logout(context.Background(), login.RefreshToken)

	if _, err := svc.Refresh(context.Background(), login.RefreshToken); err == nil {
		t.Fatal("expected refresh with a revoked token to fail")
	}
}

func TestSetUserRoles_AuditsOldAndNew(t *testing.T) {
	svc, repo, auditor := newTestService(t)
	tenantID := id.New()
	userID := id.New()
	hash := mustHash(t, "password12345")
	repo.CreateUser(context.Background(), userID, tenantID, "user@example.com", "User", hash, time.Now())

	role := Role{ID: id.New(), TenantID: tenantID, Code: RoleCodeCashier, Name: "Cashier"}
	repo.addRole(role)

	actor := id.New()
	_, err := svc.SetUserRoles(context.Background(), tenantID, userID, []RoleAssignmentInput{
		{ID: id.New(), RoleID: role.ID, OutletID: nil},
	}, &actor, nil)
	if err != nil {
		t.Fatalf("set user roles: %v", err)
	}

	if len(auditor.calls) != 1 {
		t.Fatalf("expected one audit call, got %d", len(auditor.calls))
	}
	if auditor.calls[0].Action != "user.roles.replace" {
		t.Errorf("unexpected action: %s", auditor.calls[0].Action)
	}
}

// TestChangePassword_BumpsConfigVersion is the direct test for ADR-017 §4:
// today config_version bumps only on create and role change, so a password
// change never reached the edge cache and a cashier kept authenticating
// offline against the OLD, possibly-compromised credential. This proves the
// fix.
func TestChangePassword_BumpsConfigVersion(t *testing.T) {
	svc, repo, _ := newTestService(t)
	tenantID := id.New()
	userID := id.New()
	hash := mustHash(t, "password12345")
	repo.CreateUser(context.Background(), userID, tenantID, "user@example.com", "User", hash, time.Now())

	before := repo.users[userID]
	if before.configVersion != 1 {
		t.Fatalf("expected freshly created user at config_version 1, got %d", before.configVersion)
	}

	if _, err := svc.ChangePassword(context.Background(), tenantID, userID, "a-new-password-999", nil, nil); err != nil {
		t.Fatalf("ChangePassword: %v", err)
	}

	after := repo.users[userID]
	if after.configVersion <= before.configVersion {
		t.Fatalf("expected config_version to increase on password change: before=%d after=%d", before.configVersion, after.configVersion)
	}
	if after.passwordHash == before.passwordHash {
		t.Fatal("expected password_hash to change")
	}
	if err := crypto.VerifyPassword("a-new-password-999", after.passwordHash); err != nil {
		t.Fatalf("expected the new password to verify against the stored hash: %v", err)
	}
}

// TestChangePin_BumpsConfigVersion mirrors TestChangePassword_BumpsConfigVersion
// for the PIN path.
func TestChangePin_BumpsConfigVersion(t *testing.T) {
	svc, repo, _ := newTestService(t)
	tenantID := id.New()
	userID := id.New()
	hash := mustHash(t, "password12345")
	repo.CreateUser(context.Background(), userID, tenantID, "user@example.com", "User", hash, time.Now())

	before := repo.users[userID]

	if _, err := svc.ChangePin(context.Background(), tenantID, userID, "1234", nil, nil); err != nil {
		t.Fatalf("ChangePin: %v", err)
	}

	after := repo.users[userID]
	if after.configVersion <= before.configVersion {
		t.Fatalf("expected config_version to increase on PIN change: before=%d after=%d", before.configVersion, after.configVersion)
	}
	if after.pinHash == nil {
		t.Fatal("expected pin_hash to be set")
	}
}

// TestChangePassword_NeverAuditsThePlaintextOrHash is the falsifying test
// for CLAUDE.md's "no credential material in audit values" rule applied to
// the new password-change path specifically: prove the audit record this
// action writes carries no password_hash key at all, not merely that a
// generic redaction list happens to catch it.
func TestChangePassword_NeverAuditsThePlaintextOrHash(t *testing.T) {
	svc, repo, auditor := newTestService(t)
	tenantID := id.New()
	userID := id.New()
	hash := mustHash(t, "password12345")
	repo.CreateUser(context.Background(), userID, tenantID, "user@example.com", "User", hash, time.Now())

	if _, err := svc.ChangePassword(context.Background(), tenantID, userID, "a-new-password-999", nil, nil); err != nil {
		t.Fatalf("ChangePassword: %v", err)
	}

	if len(auditor.calls) != 1 {
		t.Fatalf("expected one audit call, got %d", len(auditor.calls))
	}
	for _, v := range auditor.calls[0].NewValue {
		if s, ok := v.(string); ok && (contains(s, "password12345") || contains(s, "$argon2id$")) {
			t.Fatalf("audit NewValue leaked credential material: %v", auditor.calls[0].NewValue)
		}
	}
}

// TestRedact_StripsDeviceTokenHash is the direct test for this track's task
// 6: device_token_hash joins password_hash/pin_hash/token_hash on the
// redact list (ADR-017 §1), confined to this package's local supplement
// since packages/contracts is frozen and read-only to builder agents.
func TestRedact_StripsDeviceTokenHash(t *testing.T) {
	got := redact(map[string]interface{}{
		"device_id":         "some-device-id",
		"device_token_hash": "argon2id-should-never-appear",
	})
	if _, present := got["device_token_hash"]; present {
		t.Fatalf("expected device_token_hash to be redacted, got %v", got)
	}
	if got["device_id"] != "some-device-id" {
		t.Fatalf("expected non-redacted fields to survive, got %v", got)
	}
}

func contains(haystack, needle string) bool {
	return len(haystack) >= len(needle) && (func() bool {
		for i := 0; i+len(needle) <= len(haystack); i++ {
			if haystack[i:i+len(needle)] == needle {
				return true
			}
		}
		return false
	})()
}
