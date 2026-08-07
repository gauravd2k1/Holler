package auth

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"time"

	"github.com/holler/backend/internal/platform/id"
	"github.com/holler/backend/internal/platform/postgres"
	"github.com/jackc/pgx/v5"
)

// PostgresRefreshStore persists refresh token rotation state in the
// refresh_token table (packages/contracts/postgres/0003_refresh_token.sql).
// It is the RefreshStore implementation used in production: state survives
// a restart and is shared correctly across backend instances because it
// lives in Postgres, not in process memory.
//
// The opaque token itself is never persisted or logged — only
// sha256(token) (hex-encoded) is written to token_hash, matching the
// migration's comment that the token is NEVER stored.
type PostgresRefreshStore struct {
	pool postgres.Pool
	now  func() time.Time
}

func NewPostgresRefreshStore(pool postgres.Pool) *PostgresRefreshStore {
	return &PostgresRefreshStore{pool: pool, now: time.Now}
}

func hashToken(token string) string {
	sum := sha256.Sum256([]byte(token))
	return hex.EncodeToString(sum[:])
}

// Issue starts a new rotation family and returns the first refresh token.
func (s *PostgresRefreshStore) Issue(ctx context.Context, userID, outletID string, ttl time.Duration) (string, error) {
	token, err := randomToken()
	if err != nil {
		return "", err
	}
	familyID := id.New()
	now := s.now().UTC()

	_, err = s.pool.Exec(ctx, `
		INSERT INTO refresh_token (id, family_id, user_id, outlet_id, token_hash, issued_at, expires_at, created_at)
		VALUES ($1, $2, $3, $4, $5, $6, $7, $6)
	`, id.New(), familyID, userID, outletID, hashToken(token), now, now.Add(ttl))
	if err != nil {
		return "", fmt.Errorf("auth: issuing refresh token: %w", err)
	}
	return token, nil
}

type refreshTokenRow struct {
	id        string
	familyID  string
	userID    string
	outletID  string
	expiresAt time.Time
	usedAt    *time.Time
	revokedAt *time.Time
}

// Rotate consumes token and issues its successor in the same family, in one
// transaction (rotate → set used_at + replaced_by_id and insert the
// successor). Reuse of an already-used or revoked token revokes every row
// sharing family_id and returns ErrInvalidToken.
func (s *PostgresRefreshStore) Rotate(ctx context.Context, token string, ttl time.Duration) (newToken, userID, outletID string, err error) {
	tx, err := s.pool.Begin(ctx)
	if err != nil {
		return "", "", "", fmt.Errorf("auth: begin rotate: %w", err)
	}
	defer tx.Rollback(ctx) //nolint:errcheck

	var row refreshTokenRow
	var outlet *string
	dbErr := tx.QueryRow(ctx, `
		SELECT id, family_id, user_id, outlet_id, expires_at, used_at, revoked_at
		FROM refresh_token
		WHERE token_hash = $1
		FOR UPDATE
	`, hashToken(token)).Scan(&row.id, &row.familyID, &row.userID, &outlet, &row.expiresAt, &row.usedAt, &row.revokedAt)
	if errors.Is(dbErr, pgx.ErrNoRows) {
		return "", "", "", ErrInvalidToken
	}
	if dbErr != nil {
		return "", "", "", fmt.Errorf("auth: looking up refresh token: %w", dbErr)
	}
	if outlet != nil {
		row.outletID = *outlet
	}

	now := s.now().UTC()

	// Reuse of an already-rotated (used_at set) or already-revoked token is a
	// stolen-token signal: revoke the whole family and refuse.
	if row.usedAt != nil || row.revokedAt != nil || now.After(row.expiresAt) {
		if _, revokeErr := tx.Exec(ctx, `
			UPDATE refresh_token SET revoked_at = $1 WHERE family_id = $2 AND revoked_at IS NULL
		`, now, row.familyID); revokeErr != nil {
			return "", "", "", fmt.Errorf("auth: revoking family on reuse: %w", revokeErr)
		}
		if commitErr := tx.Commit(ctx); commitErr != nil {
			return "", "", "", fmt.Errorf("auth: commit revoke on reuse: %w", commitErr)
		}
		return "", "", "", ErrInvalidToken
	}

	next, genErr := randomToken()
	if genErr != nil {
		return "", "", "", genErr
	}
	nextID := id.New()

	if _, err := tx.Exec(ctx, `
		INSERT INTO refresh_token (id, family_id, user_id, outlet_id, token_hash, issued_at, expires_at, created_at)
		VALUES ($1, $2, $3, $4, $5, $6, $7, $6)
	`, nextID, row.familyID, row.userID, outlet, hashToken(next), now, now.Add(ttl)); err != nil {
		return "", "", "", fmt.Errorf("auth: inserting rotated refresh token: %w", err)
	}

	if _, err := tx.Exec(ctx, `
		UPDATE refresh_token SET used_at = $1, replaced_by_id = $2 WHERE id = $3
	`, now, nextID, row.id); err != nil {
		return "", "", "", fmt.Errorf("auth: marking refresh token used: %w", err)
	}

	if err := tx.Commit(ctx); err != nil {
		return "", "", "", fmt.Errorf("auth: commit rotate: %w", err)
	}

	return next, row.userID, row.outletID, nil
}

// Revoke invalidates a single refresh token's whole family (logout).
func (s *PostgresRefreshStore) Revoke(ctx context.Context, token string) {
	now := s.now().UTC()
	_, _ = s.pool.Exec(ctx, `
		UPDATE refresh_token SET revoked_at = $1
		WHERE revoked_at IS NULL AND family_id = (
			SELECT family_id FROM refresh_token WHERE token_hash = $2
		)
	`, now, hashToken(token))
}
