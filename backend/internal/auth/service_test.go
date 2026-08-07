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
	f.users[uid] = credentialRow{id: uid, tenantID: tenantID, email: email, fullName: fullName, passwordHash: passwordHash, isActive: true, createdAt: now, updatedAt: now}
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

// fakeAuditor records Audit calls in-memory for assertions.
type fakeAuditor struct {
	calls []AuditInput
}

func (f *fakeAuditor) Audit(ctx context.Context, input AuditInput) error {
	f.calls = append(f.calls, input)
	return nil
}

func newTestService(t *testing.T) (*Service, *fakeRepo, *fakeAuditor) {
	t.Helper()
	repo := newFakeRepo()
	auditor := &fakeAuditor{}
	signer := NewTokenSigner([]byte("test-signing-key-not-a-secret"))
	refresh := NewRefreshStore()
	svc := NewService(repo, signer, refresh, auditor, time.Minute, time.Hour)
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

	result, err := svc.Login(context.Background(), tenantID, "cashier@example.com", "correct-horse-battery-staple", outletID)
	if err != nil {
		t.Fatalf("expected login success, got %v", err)
	}
	if result.AccessToken == "" || result.RefreshToken == "" {
		t.Fatal("expected non-empty tokens")
	}
	if !result.Principal.HasPermission(string(PermissionOrderCreate)) {
		t.Fatal("expected principal to carry order.create from tenant-wide role")
	}
}

func TestLogin_WrongPassword_And_NoSuchUser_SameError(t *testing.T) {
	svc, repo, _ := newTestService(t)
	tenantID := id.New()
	userID := id.New()
	hash := mustHash(t, "correct-password")
	repo.CreateUser(context.Background(), userID, tenantID, "someone@example.com", "Someone", hash, time.Now())

	_, err1 := svc.Login(context.Background(), tenantID, "someone@example.com", "wrong-password", id.New())
	_, err2 := svc.Login(context.Background(), tenantID, "nobody@example.com", "irrelevant-password", id.New())

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

	resultA, err := svc.Login(context.Background(), tenantID, "manager@example.com", "password12345", outletA)
	if err != nil {
		t.Fatalf("login at outlet A: %v", err)
	}
	if !resultA.Principal.HasPermission(string(PermissionOrderCreate)) {
		t.Error("expected tenant-wide permission to apply at outlet A")
	}
	if !resultA.Principal.HasPermission(string(PermissionMenuManage)) {
		t.Error("expected outlet-scoped permission to apply at its own outlet")
	}

	resultB, err := svc.Login(context.Background(), tenantID, "manager@example.com", "password12345", outletB)
	if err != nil {
		t.Fatalf("login at outlet B: %v", err)
	}
	if !resultB.Principal.HasPermission(string(PermissionOrderCreate)) {
		t.Error("expected tenant-wide permission to apply at outlet B too")
	}
	if resultB.Principal.HasPermission(string(PermissionMenuManage)) {
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

	login, err := svc.Login(context.Background(), tenantID, "user@example.com", "password12345", outletID)
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

	login, err := svc.Login(context.Background(), tenantID, "user@example.com", "password12345", outletID)
	if err != nil {
		t.Fatalf("login: %v", err)
	}

	svc.Logout(login.RefreshToken)

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
