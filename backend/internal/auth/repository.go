package auth

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"time"

	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/platform/postgres"
	"github.com/jackc/pgx/v5"
)

// Repository is the Postgres-backed persistence for app_user, role,
// role_permission, user_role and audit_event (packages/contracts/postgres/
// 0002_m1_identity_tables.sql, read-only schema).
type Repository struct {
	pool postgres.Pool
}

func NewRepository(pool postgres.Pool) *Repository {
	return &Repository{pool: pool}
}

// credentialRow is the only place in this package password_hash /
// pin_hash may be scanned into memory. It is never returned from any
// exported Repository method.
type credentialRow struct {
	id           string
	tenantID     string
	email        string
	fullName     string
	passwordHash string
	// pinHash and configVersion are populated only by ListUsersForEdgeCache
	// (the /sync/config users export, ADR-015); FindUserByEmailForAuth and
	// FindUserByIDForAuth leave them zero-valued since the login/refresh
	// flows that call them never need either.
	pinHash       *string
	configVersion int
	isActive      bool
	createdAt     time.Time
	updatedAt     time.Time
}

// FindUserByEmailForAuth loads a user (with credential hash) for login
// verification. Callers must discard the hash after verifying and never let
// it escape this package.
func (r *Repository) FindUserByEmailForAuth(ctx context.Context, tenantID, email string) (credentialRow, error) {
	var row credentialRow
	err := r.pool.QueryRow(ctx, `
		SELECT id, tenant_id, email, full_name, password_hash, is_active, created_at, updated_at
		FROM app_user
		WHERE tenant_id = $1 AND email = $2
	`, tenantID, email).Scan(&row.id, &row.tenantID, &row.email, &row.fullName, &row.passwordHash, &row.isActive, &row.createdAt, &row.updatedAt)
	if errors.Is(err, pgx.ErrNoRows) {
		return credentialRow{}, httpx.ErrNotFound
	}
	if err != nil {
		return credentialRow{}, fmt.Errorf("auth: finding user by email: %w", err)
	}
	return row, nil
}

// FindUserByIDForAuth is used by refresh flows that already hold a verified
// user id and need current active/role state, not a fresh password check.
func (r *Repository) FindUserByIDForAuth(ctx context.Context, userID string) (credentialRow, error) {
	var row credentialRow
	err := r.pool.QueryRow(ctx, `
		SELECT id, tenant_id, email, full_name, password_hash, is_active, created_at, updated_at
		FROM app_user
		WHERE id = $1
	`, userID).Scan(&row.id, &row.tenantID, &row.email, &row.fullName, &row.passwordHash, &row.isActive, &row.createdAt, &row.updatedAt)
	if errors.Is(err, pgx.ErrNoRows) {
		return credentialRow{}, httpx.ErrNotFound
	}
	if err != nil {
		return credentialRow{}, fmt.Errorf("auth: finding user by id: %w", err)
	}
	return row, nil
}

// ListUsersForEdgeCache returns every user of tenantID eligible to log in at
// outletID (via a tenant-wide role assignment or one scoped to outletID)
// whose config_version is newer than sinceVersion, with password_hash and
// pin_hash populated. It is the credential-bearing counterpart to ListUsers,
// scoped to callers that populate GET /sync/config's users array (ADR-015)
// — no other caller may use it.
func (r *Repository) ListUsersForEdgeCache(ctx context.Context, tenantID, outletID string, sinceVersion int) ([]credentialRow, error) {
	rows, err := r.pool.Query(ctx, `
		SELECT DISTINCT u.id, u.tenant_id, u.email, u.full_name, u.password_hash, u.pin_hash,
			u.is_active, u.config_version, u.updated_at
		FROM app_user u
		JOIN user_role ur ON ur.user_id = u.id
		WHERE u.tenant_id = $1
		  AND (ur.outlet_id = $2 OR ur.outlet_id IS NULL)
		  AND u.config_version > $3
		ORDER BY u.updated_at
	`, tenantID, outletID, sinceVersion)
	if err != nil {
		return nil, fmt.Errorf("auth: listing users for edge cache: %w", err)
	}
	defer rows.Close()

	var out []credentialRow
	for rows.Next() {
		var row credentialRow
		if err := rows.Scan(&row.id, &row.tenantID, &row.email, &row.fullName, &row.passwordHash,
			&row.pinHash, &row.isActive, &row.configVersion, &row.updatedAt); err != nil {
			return nil, fmt.Errorf("auth: scanning edge cache user: %w", err)
		}
		out = append(out, row)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("auth: iterating edge cache users: %w", err)
	}
	return out, nil
}

// CreateUser inserts a new app_user row. id is caller-supplied (UUIDv7,
// §74/openapi.yaml POST /users). config_version starts at 1, not 0, so the
// row is visible to a GET /sync/config pull made with since_version=0 — a
// row minted at 0 would never satisfy config_version > since_version on a
// node's very first sync (ADR-015).
func (r *Repository) CreateUser(ctx context.Context, id, tenantID, email, fullName, passwordHash string, now time.Time) error {
	_, err := r.pool.Exec(ctx, `
		INSERT INTO app_user (id, tenant_id, email, full_name, password_hash, is_active, config_version, created_at, updated_at)
		VALUES ($1, $2, $3, $4, $5, TRUE, 1, $6, $6)
	`, id, tenantID, email, fullName, passwordHash, now)
	if err != nil {
		return fmt.Errorf("auth: creating user: %w", err)
	}
	return nil
}

// UpdatePassword sets a user's password_hash and bumps config_version, so a
// changed credential reaches the edge cache on the next /sync/config pull
// exactly like a role change does (ADR-017 §4). Previously config_version
// bumped only on create and role change, so a compromised password change
// never reached an offline cashier's cached credential.
func (r *Repository) UpdatePassword(ctx context.Context, userID, passwordHash string, now time.Time) error {
	tag, err := r.pool.Exec(ctx, `
		UPDATE app_user SET password_hash = $2, config_version = config_version + 1, updated_at = $3
		WHERE id = $1
	`, userID, passwordHash, now)
	if err != nil {
		return fmt.Errorf("auth: updating password: %w", err)
	}
	if tag.RowsAffected() == 0 {
		return httpx.ErrNotFound
	}
	return nil
}

// UpdatePin sets a user's pin_hash and bumps config_version, for the same
// reason UpdatePassword does (ADR-017 §4).
func (r *Repository) UpdatePin(ctx context.Context, userID, pinHash string, now time.Time) error {
	tag, err := r.pool.Exec(ctx, `
		UPDATE app_user SET pin_hash = $2, config_version = config_version + 1, updated_at = $3
		WHERE id = $1
	`, userID, pinHash, now)
	if err != nil {
		return fmt.Errorf("auth: updating pin: %w", err)
	}
	if tag.RowsAffected() == 0 {
		return httpx.ErrNotFound
	}
	return nil
}

// ListUsers returns every user of a tenant with their role assignments.
func (r *Repository) ListUsers(ctx context.Context, tenantID string) ([]User, error) {
	rows, err := r.pool.Query(ctx, `
		SELECT id, tenant_id, email, full_name, is_active, config_version, created_at, updated_at
		FROM app_user
		WHERE tenant_id = $1
		ORDER BY created_at
	`, tenantID)
	if err != nil {
		return nil, fmt.Errorf("auth: listing users: %w", err)
	}
	defer rows.Close()

	var users []User
	for rows.Next() {
		var u User
		if err := rows.Scan(&u.ID, &u.TenantID, &u.Email, &u.FullName, &u.IsActive, &u.ConfigVersion, &u.CreatedAt, &u.UpdatedAt); err != nil {
			return nil, fmt.Errorf("auth: scanning user: %w", err)
		}
		u.SchemaVersion = schemaVersion
		users = append(users, u)
	}
	if err := rows.Err(); err != nil {
		return nil, fmt.Errorf("auth: iterating users: %w", err)
	}

	for i := range users {
		roles, err := r.RolesForUser(ctx, users[i].ID)
		if err != nil {
			return nil, err
		}
		users[i].Roles = roles
	}
	return users, nil
}

// GetUser loads one user with its role assignments.
func (r *Repository) GetUser(ctx context.Context, tenantID, userID string) (User, error) {
	var u User
	err := r.pool.QueryRow(ctx, `
		SELECT id, tenant_id, email, full_name, is_active, config_version, created_at, updated_at
		FROM app_user
		WHERE tenant_id = $1 AND id = $2
	`, tenantID, userID).Scan(&u.ID, &u.TenantID, &u.Email, &u.FullName, &u.IsActive, &u.ConfigVersion, &u.CreatedAt, &u.UpdatedAt)
	if errors.Is(err, pgx.ErrNoRows) {
		return User{}, httpx.ErrNotFound
	}
	if err != nil {
		return User{}, fmt.Errorf("auth: getting user: %w", err)
	}
	u.SchemaVersion = schemaVersion
	roles, err := r.RolesForUser(ctx, u.ID)
	if err != nil {
		return User{}, err
	}
	u.Roles = roles
	return u, nil
}

// RolesForUser returns a user's role assignments, joined to role code.
func (r *Repository) RolesForUser(ctx context.Context, userID string) ([]RoleAssignment, error) {
	rows, err := r.pool.Query(ctx, `
		SELECT ur.id, ur.role_id, r.code, ur.outlet_id
		FROM user_role ur
		JOIN role r ON r.id = ur.role_id
		WHERE ur.user_id = $1
	`, userID)
	if err != nil {
		return nil, fmt.Errorf("auth: listing user roles: %w", err)
	}
	defer rows.Close()

	var assignments []RoleAssignment
	for rows.Next() {
		var a RoleAssignment
		var code string
		if err := rows.Scan(&a.ID, &a.RoleID, &code, &a.OutletID); err != nil {
			return nil, fmt.Errorf("auth: scanning user role: %w", err)
		}
		a.RoleCode = RoleCode(code)
		assignments = append(assignments, a)
	}
	return assignments, rows.Err()
}

// PermissionsForRole returns the permission set granted to a role.
func (r *Repository) PermissionsForRole(ctx context.Context, roleID string) ([]Permission, error) {
	rows, err := r.pool.Query(ctx, `SELECT permission FROM role_permission WHERE role_id = $1`, roleID)
	if err != nil {
		return nil, fmt.Errorf("auth: listing role permissions: %w", err)
	}
	defer rows.Close()

	var perms []Permission
	for rows.Next() {
		var p string
		if err := rows.Scan(&p); err != nil {
			return nil, fmt.Errorf("auth: scanning role permission: %w", err)
		}
		perms = append(perms, Permission(p))
	}
	return perms, rows.Err()
}

// ReplaceUserRoles deletes and re-inserts a user's role_role assignments in
// one transaction (PUT /users/{id}/roles is a full replace). It also bumps
// app_user.config_version: a role change changes the permission set
// ListEdgeUserCache resolves for this user, so the edge cache must see it as
// a newer row (ADR-015) exactly like menu/tables bump their owning outlet's
// config_version on every mutation.
func (r *Repository) ReplaceUserRoles(ctx context.Context, userID string, assignments []RoleAssignment, now time.Time) error {
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return fmt.Errorf("auth: begin replace roles: %w", err)
	}
	defer tx.Rollback(ctx) //nolint:errcheck

	if _, err := tx.Exec(ctx, `DELETE FROM user_role WHERE user_id = $1`, userID); err != nil {
		return fmt.Errorf("auth: clearing user roles: %w", err)
	}
	for _, a := range assignments {
		if _, err := tx.Exec(ctx, `
			INSERT INTO user_role (id, user_id, role_id, outlet_id, created_at)
			VALUES ($1, $2, $3, $4, $5)
		`, a.ID, userID, a.RoleID, a.OutletID, now); err != nil {
			return fmt.Errorf("auth: inserting user role: %w", err)
		}
	}
	if _, err := tx.Exec(ctx, `
		UPDATE app_user SET config_version = config_version + 1, updated_at = $2 WHERE id = $1
	`, userID, now); err != nil {
		return fmt.Errorf("auth: bumping user config_version: %w", err)
	}
	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("auth: commit replace roles: %w", err)
	}
	return nil
}

// ListRoles returns a tenant's roles with their permissions.
func (r *Repository) ListRoles(ctx context.Context, tenantID string) ([]Role, error) {
	rows, err := r.pool.Query(ctx, `
		SELECT id, tenant_id, code, name FROM role WHERE tenant_id = $1 ORDER BY code
	`, tenantID)
	if err != nil {
		return nil, fmt.Errorf("auth: listing roles: %w", err)
	}
	defer rows.Close()

	var roles []Role
	for rows.Next() {
		var role Role
		var code string
		if err := rows.Scan(&role.ID, &role.TenantID, &code, &role.Name); err != nil {
			return nil, fmt.Errorf("auth: scanning role: %w", err)
		}
		role.Code = RoleCode(code)
		role.SchemaVersion = schemaVersion
		roles = append(roles, role)
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}

	for i := range roles {
		perms, err := r.PermissionsForRole(ctx, roles[i].ID)
		if err != nil {
			return nil, err
		}
		roles[i].Permissions = perms
	}
	return roles, nil
}

// GetRole fetches a single role by id, scoped to a tenant.
func (r *Repository) GetRole(ctx context.Context, tenantID, roleID string) (Role, error) {
	var role Role
	var code string
	err := r.pool.QueryRow(ctx, `
		SELECT id, tenant_id, code, name FROM role WHERE tenant_id = $1 AND id = $2
	`, tenantID, roleID).Scan(&role.ID, &role.TenantID, &code, &role.Name)
	if errors.Is(err, pgx.ErrNoRows) {
		return Role{}, httpx.ErrNotFound
	}
	if err != nil {
		return Role{}, fmt.Errorf("auth: getting role: %w", err)
	}
	role.Code = RoleCode(code)
	role.SchemaVersion = schemaVersion
	perms, err := r.PermissionsForRole(ctx, role.ID)
	if err != nil {
		return Role{}, err
	}
	role.Permissions = perms
	return role, nil
}

// SeedRole inserts a role and its permission set if the (tenant_id, code)
// pair does not already exist. Idempotent so it is safe to call on every
// tenant creation / backend startup.
func (r *Repository) SeedRole(ctx context.Context, id, tenantID string, code RoleCode, name string, perms []Permission, now time.Time) error {
	tx, err := r.pool.Begin(ctx)
	if err != nil {
		return fmt.Errorf("auth: begin seed role: %w", err)
	}
	defer tx.Rollback(ctx) //nolint:errcheck

	var roleID string
	err = tx.QueryRow(ctx, `SELECT id FROM role WHERE tenant_id = $1 AND code = $2`, tenantID, string(code)).Scan(&roleID)
	switch {
	case errors.Is(err, pgx.ErrNoRows):
		roleID = id
		if _, err := tx.Exec(ctx, `
			INSERT INTO role (id, tenant_id, code, name, created_at, updated_at)
			VALUES ($1, $2, $3, $4, $5, $5)
		`, roleID, tenantID, string(code), name, now); err != nil {
			return fmt.Errorf("auth: inserting role: %w", err)
		}
	case err != nil:
		return fmt.Errorf("auth: checking existing role: %w", err)
	default:
		// Already seeded; leave permissions as-is so a later manual edit isn't
		// clobbered by a restart.
		return tx.Commit(ctx)
	}

	for _, p := range perms {
		if _, err := tx.Exec(ctx, `
			INSERT INTO role_permission (role_id, permission) VALUES ($1, $2)
			ON CONFLICT DO NOTHING
		`, roleID, string(p)); err != nil {
			return fmt.Errorf("auth: inserting role permission: %w", err)
		}
	}
	return tx.Commit(ctx)
}

// RecordAudit persists a pre-redacted audit event. Redaction itself happens
// in the audit helper (audit.go), never here — this is the last line of
// defense, not the first.
func (r *Repository) RecordAudit(ctx context.Context, e AuditEvent) error {
	oldJSON, err := marshalRedacted(e.OldValue)
	if err != nil {
		return err
	}
	newJSON, err := marshalRedacted(e.NewValue)
	if err != nil {
		return err
	}
	_, err = r.pool.Exec(ctx, `
		INSERT INTO audit_event (id, tenant_id, outlet_id, actor_user_id, device_id, action, entity_type, entity_id, old_value, new_value, reason, occurred_at)
		VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
	`, e.ID, e.TenantID, e.OutletID, e.ActorUserID, e.DeviceID, e.Action, e.EntityType, e.EntityID, oldJSON, newJSON, e.Reason, e.OccurredAt)
	if err != nil {
		return fmt.Errorf("auth: recording audit event: %w", err)
	}
	return nil
}

func marshalRedacted(v map[string]interface{}) ([]byte, error) {
	if v == nil {
		return nil, nil
	}
	b, err := json.Marshal(v)
	if err != nil {
		return nil, fmt.Errorf("auth: marshaling audit value: %w", err)
	}
	return b, nil
}
