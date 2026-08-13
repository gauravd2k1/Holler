package auth

import (
	"context"
	"fmt"
	"strings"
	"time"

	"github.com/holler/backend/internal/platform/crypto"
	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/platform/id"
)

const schemaVersion = 1

// UserRepository is what Service needs from persistence. *Repository
// satisfies it; tests use a fake.
type UserRepository interface {
	FindUserByEmailForAuth(ctx context.Context, tenantID, email string) (credentialRow, error)
	FindUserByIDForAuth(ctx context.Context, userID string) (credentialRow, error)
	CreateUser(ctx context.Context, id, tenantID, email, fullName, passwordHash string, now time.Time) error
	ListUsers(ctx context.Context, tenantID string) ([]User, error)
	GetUser(ctx context.Context, tenantID, userID string) (User, error)
	RolesForUser(ctx context.Context, userID string) ([]RoleAssignment, error)
	PermissionsForRole(ctx context.Context, roleID string) ([]Permission, error)
	ReplaceUserRoles(ctx context.Context, userID string, assignments []RoleAssignment, now time.Time) error
	ListRoles(ctx context.Context, tenantID string) ([]Role, error)
	GetRole(ctx context.Context, tenantID, roleID string) (Role, error)
	ListUsersForEdgeCache(ctx context.Context, tenantID, outletID string, sinceVersion int) ([]credentialRow, error)
	UpdatePassword(ctx context.Context, userID, passwordHash string, now time.Time) error
	UpdatePin(ctx context.Context, userID, pinHash string, now time.Time) error
}

// Service implements login, refresh rotation, logout, user/role management
// and permission resolution.
type Service struct {
	repo       UserRepository
	tokens     *TokenSigner
	refresh    RefreshStore
	limiter    RateLimiter
	auditor    AuditRecorder
	accessTTL  time.Duration
	refreshTTL time.Duration
	now        func() time.Time
}

func NewService(repo UserRepository, tokens *TokenSigner, refresh RefreshStore, limiter RateLimiter, auditor AuditRecorder, accessTTL, refreshTTL time.Duration) *Service {
	return &Service{
		repo:       repo,
		tokens:     tokens,
		refresh:    refresh,
		limiter:    limiter,
		auditor:    auditor,
		accessTTL:  accessTTL,
		refreshTTL: refreshTTL,
		now:        time.Now,
	}
}

// LoginResult is what a successful login returns.
type LoginResult struct {
	AccessToken  string
	RefreshToken string
	Principal    AuthenticatedPrincipal
}

// Login authenticates a user against a tenant + outlet and, on success,
// resolves their effective permissions at that outlet. Failure never
// distinguishes "no such user" from "wrong password" — and, per ADR-012,
// nor does rate-limit rejection: it returns the identical ErrRateLimited
// regardless of whether clientIP+tenantID's target account exists.
//
// clientIP + tenantID is the rate-limit key (ADR-012): keying on IP as well
// as tenant means rotating X-Tenant-ID cannot reset an attacker's budget,
// because a second, IP-only check always applies too.
func (s *Service) Login(ctx context.Context, clientIP, tenantID, email, password, outletID string) (LoginResult, error) {
	if err := s.checkLoginRateLimit(ctx, clientIP, tenantID); err != nil {
		return LoginResult{}, err
	}

	row, err := s.repo.FindUserByEmailForAuth(ctx, tenantID, strings.ToLower(email))
	if err != nil {
		// Do the same work whether the user exists or not, so timing does not
		// leak account existence, and always return the same generic error.
		_ = crypto.VerifyPassword(password, dummyHash)
		return LoginResult{}, httpx.ErrUnauthorized
	}
	if !row.isActive {
		return LoginResult{}, httpx.ErrUnauthorized
	}
	if err := crypto.VerifyPassword(password, row.passwordHash); err != nil {
		return LoginResult{}, httpx.ErrUnauthorized
	}

	principal, err := s.resolvePrincipal(ctx, row.id, row.tenantID, row.fullName, outletID)
	if err != nil {
		return LoginResult{}, err
	}

	access, err := s.tokens.IssueAccessToken(principal, s.accessTTL)
	if err != nil {
		return LoginResult{}, err
	}
	refreshToken, err := s.refresh.Issue(ctx, row.id, outletID, s.refreshTTL)
	if err != nil {
		return LoginResult{}, err
	}

	return LoginResult{AccessToken: access, RefreshToken: refreshToken, Principal: principal}, nil
}

// dummyHash is a validly-formatted but unusable Argon2id hash used to make a
// failed lookup take roughly the same time as a real verify.
const dummyHash = "$argon2id$v=19$m=65536,t=2,p=4$AAAAAAAAAAAAAAAAAAAAAA$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"

// checkLoginRateLimit enforces the ADR-012 login budget on two keys: IP
// alone, and IP+tenant together. Both must allow the attempt. The IP-alone
// check is what stops rotating X-Tenant-ID from resetting the budget — it
// does not vary with the header, so an attacker cycling tenants against one
// IP still exhausts a single shared counter. A limiter error (backing store
// unavailable) fails closed: the attempt is denied, never allowed through.
func (s *Service) checkLoginRateLimit(ctx context.Context, clientIP, tenantID string) error {
	if s.limiter == nil {
		return nil
	}

	ipAllowed, err := s.limiter.Allow(ctx, "login:ip:"+clientIP, LoginRateLimitAttempts, LoginRateLimitWindow)
	if err != nil {
		return ErrRateLimited
	}
	if !ipAllowed {
		return ErrRateLimited
	}

	compositeAllowed, err := s.limiter.Allow(ctx, "login:ip:"+clientIP+"|tenant:"+tenantID, LoginRateLimitAttempts, LoginRateLimitWindow)
	if err != nil {
		return ErrRateLimited
	}
	if !compositeAllowed {
		return ErrRateLimited
	}

	return nil
}

// Refresh rotates a refresh token. Reuse of an already-rotated token
// invalidates the whole chain and returns ErrInvalidToken via
// httpx.ErrUnauthorized.
func (s *Service) Refresh(ctx context.Context, refreshToken string) (LoginResult, error) {
	next, userID, outletID, err := s.refresh.Rotate(ctx, refreshToken, s.refreshTTL)
	if err != nil {
		return LoginResult{}, httpx.ErrUnauthorized
	}

	row, err := s.repo.FindUserByIDForAuth(ctx, userID)
	if err != nil || !row.isActive {
		return LoginResult{}, httpx.ErrUnauthorized
	}

	principal, err := s.resolvePrincipal(ctx, row.id, row.tenantID, row.fullName, outletID)
	if err != nil {
		return LoginResult{}, err
	}

	access, err := s.tokens.IssueAccessToken(principal, s.accessTTL)
	if err != nil {
		return LoginResult{}, err
	}
	return LoginResult{AccessToken: access, RefreshToken: next, Principal: principal}, nil
}

// Logout revokes the refresh token's whole rotation family.
func (s *Service) Logout(ctx context.Context, refreshToken string) {
	s.refresh.Revoke(ctx, refreshToken)
}

// resolvePrincipal computes the effective permission set for a user at a
// given outlet: the union of every tenant-wide role's permissions plus every
// role assigned specifically at that outlet.
func (s *Service) resolvePrincipal(ctx context.Context, userID, tenantID, fullName, outletID string) (AuthenticatedPrincipal, error) {
	permissions, err := s.resolveUserPermissions(ctx, userID, outletID)
	if err != nil {
		return AuthenticatedPrincipal{}, err
	}

	return AuthenticatedPrincipal{
		UserID:               userID,
		TenantID:             tenantID,
		OutletID:             outletID,
		FullName:             fullName,
		Permissions:          permissions,
		AuthenticatedOffline: false,
		SchemaVersion:        schemaVersion,
	}, nil
}

// resolveUserPermissions is the shared core of resolvePrincipal and
// ListEdgeUserCache: the union of every tenant-wide role's permissions plus
// every role assigned specifically at outletID, for one user.
func (s *Service) resolveUserPermissions(ctx context.Context, userID, outletID string) ([]Permission, error) {
	assignments, err := s.repo.RolesForUser(ctx, userID)
	if err != nil {
		return nil, err
	}

	seen := make(map[Permission]struct{})
	for _, a := range assignments {
		if a.OutletID != nil && *a.OutletID != outletID {
			continue
		}
		perms, err := s.repo.PermissionsForRole(ctx, a.RoleID)
		if err != nil {
			return nil, err
		}
		for _, p := range perms {
			seen[p] = struct{}{}
		}
	}

	permissions := make([]Permission, 0, len(seen))
	for p := range seen {
		permissions = append(permissions, p)
	}
	return permissions, nil
}

// edgeUserCacheSchemaVersion is EdgeUserCacheEntry's schema_version
// (packages/contracts/openapi/openapi.yaml EdgeUserCacheEntry, added 0.3.1).
const edgeUserCacheSchemaVersion = 1

// ListEdgeUserCache resolves the users array of GET /sync/config (ADR-015):
// every user of tenantID eligible to log in at outletID, with password_hash
// and pin_hash carried verbatim and permissions resolved server-side into a
// flat claim set, because the edge holds no role table. This is the ONLY
// exported method in this package that returns either hash — callers outside
// the /sync/config composite handler must not exist.
func (s *Service) ListEdgeUserCache(ctx context.Context, tenantID, outletID string, sinceVersion int) ([]EdgeUserCacheEntry, error) {
	rows, err := s.repo.ListUsersForEdgeCache(ctx, tenantID, outletID, sinceVersion)
	if err != nil {
		return nil, err
	}

	entries := make([]EdgeUserCacheEntry, 0, len(rows))
	for _, row := range rows {
		permissions, err := s.resolveUserPermissions(ctx, row.id, outletID)
		if err != nil {
			return nil, err
		}
		entries = append(entries, EdgeUserCacheEntry{
			ID:            row.id,
			TenantID:      row.tenantID,
			OutletID:      outletID,
			Email:         row.email,
			FullName:      row.fullName,
			PasswordHash:  row.passwordHash,
			PinHash:       row.pinHash,
			IsActive:      row.isActive,
			Permissions:   permissions,
			ConfigVersion: row.configVersion,
			UpdatedAt:     row.updatedAt,
			SchemaVersion: edgeUserCacheSchemaVersion,
		})
	}
	return entries, nil
}

// ResolvePermissions is exported for tests and for callers that already hold
// role assignments without a full login round trip.
func ResolvePermissions(assignments []RoleAssignment, permsByRole map[string][]Permission, outletID string) []Permission {
	seen := make(map[Permission]struct{})
	for _, a := range assignments {
		if a.OutletID != nil && *a.OutletID != outletID {
			continue
		}
		for _, p := range permsByRole[a.RoleID] {
			seen[p] = struct{}{}
		}
	}
	out := make([]Permission, 0, len(seen))
	for p := range seen {
		out = append(out, p)
	}
	return out
}

// CreateUser creates a new tenant user with a hashed password. actor/device
// identify who performed the action for the audit trail.
func (s *Service) CreateUser(ctx context.Context, tenantID, userID, email, fullName, password string, actorUserID *string, deviceID *string) (User, error) {
	if email == "" || fullName == "" || password == "" {
		return User{}, fmt.Errorf("%w: email, full_name and password are required", httpx.ErrInvalidInput)
	}
	hash, err := crypto.HashPassword(password)
	if err != nil {
		return User{}, err
	}
	now := s.now().UTC()
	normalizedEmail := strings.ToLower(email)
	if err := s.repo.CreateUser(ctx, userID, tenantID, normalizedEmail, fullName, hash, now); err != nil {
		return User{}, err
	}

	if s.auditor != nil {
		_ = s.auditor.Audit(ctx, AuditInput{
			TenantID:    tenantID,
			ActorUserID: actorUserID,
			DeviceID:    deviceID,
			Action:      "user.create",
			EntityType:  "app_user",
			EntityID:    &userID,
			NewValue: map[string]interface{}{
				"id":        userID,
				"email":     normalizedEmail,
				"full_name": fullName,
			},
		})
	}

	return s.repo.GetUser(ctx, tenantID, userID)
}

// ChangePassword hashes newPassword and persists it, bumping the user's
// config_version so the change reaches the edge cache on the next
// /sync/config pull (ADR-017 §4) — otherwise a cashier keeps authenticating
// offline against the OLD credential indefinitely, including one changed
// because it was compromised. The plaintext password never enters the audit
// value; only the fact that a change occurred is recorded.
func (s *Service) ChangePassword(ctx context.Context, tenantID, userID, newPassword string, actorUserID *string, deviceID *string) (User, error) {
	if newPassword == "" {
		return User{}, fmt.Errorf("%w: password is required", httpx.ErrInvalidInput)
	}
	hash, err := crypto.HashPassword(newPassword)
	if err != nil {
		return User{}, err
	}
	now := s.now().UTC()
	if err := s.repo.UpdatePassword(ctx, userID, hash, now); err != nil {
		return User{}, err
	}

	if s.auditor != nil {
		_ = s.auditor.Audit(ctx, AuditInput{
			TenantID:    tenantID,
			ActorUserID: actorUserID,
			DeviceID:    deviceID,
			Action:      "user.password.change",
			EntityType:  "app_user",
			EntityID:    &userID,
			NewValue:    map[string]interface{}{"id": userID},
		})
	}
	return s.repo.GetUser(ctx, tenantID, userID)
}

// ChangePin is ChangePassword's PIN counterpart.
func (s *Service) ChangePin(ctx context.Context, tenantID, userID, newPin string, actorUserID *string, deviceID *string) (User, error) {
	if newPin == "" {
		return User{}, fmt.Errorf("%w: pin is required", httpx.ErrInvalidInput)
	}
	hash, err := crypto.HashPassword(newPin)
	if err != nil {
		return User{}, err
	}
	now := s.now().UTC()
	if err := s.repo.UpdatePin(ctx, userID, hash, now); err != nil {
		return User{}, err
	}

	if s.auditor != nil {
		_ = s.auditor.Audit(ctx, AuditInput{
			TenantID:    tenantID,
			ActorUserID: actorUserID,
			DeviceID:    deviceID,
			Action:      "user.pin.change",
			EntityType:  "app_user",
			EntityID:    &userID,
			NewValue:    map[string]interface{}{"id": userID},
		})
	}
	return s.repo.GetUser(ctx, tenantID, userID)
}

// ListUsers returns every user in tenantID.
func (s *Service) ListUsers(ctx context.Context, tenantID string) ([]User, error) {
	return s.repo.ListUsers(ctx, tenantID)
}

// ListRoles returns every role in tenantID.
func (s *Service) ListRoles(ctx context.Context, tenantID string) ([]Role, error) {
	return s.repo.ListRoles(ctx, tenantID)
}

// RoleAssignmentInput is one row of the PUT /users/{id}/roles body.
type RoleAssignmentInput struct {
	ID       string
	RoleID   string
	OutletID *string
}

// SetUserRoles replaces a user's role assignments and records an audit
// event capturing the before/after role sets.
func (s *Service) SetUserRoles(ctx context.Context, tenantID, userID string, inputs []RoleAssignmentInput, actorUserID *string, deviceID *string) (User, error) {
	before, err := s.repo.GetUser(ctx, tenantID, userID)
	if err != nil {
		return User{}, err
	}

	assignments := make([]RoleAssignment, 0, len(inputs))
	for _, in := range inputs {
		role, err := s.repo.GetRole(ctx, tenantID, in.RoleID)
		if err != nil {
			return User{}, fmt.Errorf("%w: unknown role_id", httpx.ErrInvalidInput)
		}
		assignments = append(assignments, RoleAssignment{
			ID:       in.ID,
			RoleID:   in.RoleID,
			RoleCode: role.Code,
			OutletID: in.OutletID,
		})
	}

	now := s.now().UTC()
	if err := s.repo.ReplaceUserRoles(ctx, userID, assignments, now); err != nil {
		return User{}, err
	}

	if s.auditor != nil {
		_ = s.auditor.Audit(ctx, AuditInput{
			TenantID:    tenantID,
			ActorUserID: actorUserID,
			DeviceID:    deviceID,
			Action:      "user.roles.replace",
			EntityType:  "app_user",
			EntityID:    &userID,
			OldValue:    rolesToAuditValue(before.Roles),
			NewValue:    rolesToAuditValue(assignments),
		})
	}

	return s.repo.GetUser(ctx, tenantID, userID)
}

func rolesToAuditValue(assignments []RoleAssignment) map[string]interface{} {
	roles := make([]map[string]interface{}, 0, len(assignments))
	for _, a := range assignments {
		roles = append(roles, map[string]interface{}{
			"role_id":   a.RoleID,
			"role_code": string(a.RoleCode),
			"outlet_id": a.OutletID,
		})
	}
	return map[string]interface{}{"roles": roles}
}

// NewUserID is exported so handlers/tests can mint ids consistently through
// backend/internal/platform/id.
func NewUserID() string { return id.New() }
