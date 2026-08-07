package auth

import (
	"crypto/hmac"
	"crypto/rand"
	"crypto/sha256"
	"crypto/subtle"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"sync"
	"time"
)

// ErrInvalidToken covers every way a presented token can fail: malformed,
// bad signature, expired, or a reused/revoked refresh token. Callers must not
// distinguish these cases in a response (docs/spec/security-rbac.md mirrors
// the login rule: do not leak which check failed).
var ErrInvalidToken = errors.New("auth: invalid token")

// accessClaims is the payload of a short-lived access token. It carries the
// full resolved principal so a request can be authorized without a database
// round trip per request.
type accessClaims struct {
	Principal AuthenticatedPrincipal `json:"principal"`
	ExpiresAt int64                  `json:"exp"`
}

// TokenSigner issues and verifies HMAC-signed access tokens. The signing key
// comes from platform/config.Config.TokenSigningKey — never a literal.
type TokenSigner struct {
	key []byte
	now func() time.Time
}

func NewTokenSigner(key []byte) *TokenSigner {
	if len(key) == 0 {
		panic("auth: token signing key must not be empty")
	}
	return &TokenSigner{key: key, now: time.Now}
}

// IssueAccessToken returns an opaque, HMAC-signed access token encoding the
// principal, valid for ttl.
func (s *TokenSigner) IssueAccessToken(principal AuthenticatedPrincipal, ttl time.Duration) (string, error) {
	claims := accessClaims{Principal: principal, ExpiresAt: s.now().Add(ttl).Unix()}
	payload, err := json.Marshal(claims)
	if err != nil {
		return "", fmt.Errorf("auth: marshaling access claims: %w", err)
	}
	return s.sign(payload), nil
}

// VerifyAccessToken checks the signature and expiry and returns the embedded
// principal.
func (s *TokenSigner) VerifyAccessToken(token string) (AuthenticatedPrincipal, error) {
	payload, err := s.verify(token)
	if err != nil {
		return AuthenticatedPrincipal{}, err
	}
	var claims accessClaims
	if err := json.Unmarshal(payload, &claims); err != nil {
		return AuthenticatedPrincipal{}, ErrInvalidToken
	}
	if s.now().Unix() > claims.ExpiresAt {
		return AuthenticatedPrincipal{}, ErrInvalidToken
	}
	return claims.Principal, nil
}

func (s *TokenSigner) sign(payload []byte) string {
	mac := hmac.New(sha256.New, s.key)
	mac.Write(payload)
	sig := mac.Sum(nil)
	return base64.RawURLEncoding.EncodeToString(payload) + "." + base64.RawURLEncoding.EncodeToString(sig)
}

func (s *TokenSigner) verify(token string) ([]byte, error) {
	parts := strings.SplitN(token, ".", 2)
	if len(parts) != 2 {
		return nil, ErrInvalidToken
	}
	payload, err := base64.RawURLEncoding.DecodeString(parts[0])
	if err != nil {
		return nil, ErrInvalidToken
	}
	sig, err := base64.RawURLEncoding.DecodeString(parts[1])
	if err != nil {
		return nil, ErrInvalidToken
	}
	mac := hmac.New(sha256.New, s.key)
	mac.Write(payload)
	want := mac.Sum(nil)
	if subtle.ConstantTimeCompare(sig, want) != 1 {
		return nil, ErrInvalidToken
	}
	return payload, nil
}

// refreshEntry is one link in a refresh token rotation chain.
type refreshEntry struct {
	familyID  string
	userID    string
	outletID  string
	expiresAt time.Time
	used      bool
	revoked   bool
}

// RefreshStore issues and rotates refresh tokens, detecting reuse of an
// already-rotated token by invalidating its whole family.
//
// KNOWN LIMITATION (reported to orchestrator): packages/contracts/postgres/
// 0002_m1_identity_tables.sql has no refresh_token table, so this store is
// in-process and does not survive a restart or scale across backend
// instances. A future contracts change should add a refresh_token table
// (token_hash, family_id, user_id, outlet_id, expires_at, used_at, revoked_at)
// so RefreshStore can be backed by Postgres without changing this interface.
type RefreshStore struct {
	mu      sync.Mutex
	entries map[string]*refreshEntry // token -> entry
	now     func() time.Time
}

func NewRefreshStore() *RefreshStore {
	return &RefreshStore{entries: make(map[string]*refreshEntry), now: time.Now}
}

// Issue starts a new rotation family and returns the first refresh token.
func (s *RefreshStore) Issue(userID, outletID string, ttl time.Duration) (string, error) {
	token, err := randomToken()
	if err != nil {
		return "", err
	}
	familyID, err := randomToken()
	if err != nil {
		return "", err
	}

	s.mu.Lock()
	defer s.mu.Unlock()
	s.entries[token] = &refreshEntry{
		familyID:  familyID,
		userID:    userID,
		outletID:  outletID,
		expiresAt: s.now().Add(ttl),
	}
	return token, nil
}

// Rotate consumes token and issues its successor in the same family. If
// token was already used (reuse of a rotated token — a stolen-token signal)
// or is otherwise invalid, the whole family is revoked and ErrInvalidToken
// is returned.
func (s *RefreshStore) Rotate(token string, ttl time.Duration) (newToken, userID, outletID string, err error) {
	s.mu.Lock()
	defer s.mu.Unlock()

	entry, ok := s.entries[token]
	if !ok {
		return "", "", "", ErrInvalidToken
	}
	if entry.revoked || entry.used || s.now().After(entry.expiresAt) {
		s.revokeFamilyLocked(entry.familyID)
		return "", "", "", ErrInvalidToken
	}

	entry.used = true

	next, genErr := randomToken()
	if genErr != nil {
		return "", "", "", genErr
	}
	s.entries[next] = &refreshEntry{
		familyID:  entry.familyID,
		userID:    entry.userID,
		outletID:  entry.outletID,
		expiresAt: s.now().Add(ttl),
	}
	return next, entry.userID, entry.outletID, nil
}

// Revoke invalidates a single refresh token's whole family (logout).
func (s *RefreshStore) Revoke(token string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	entry, ok := s.entries[token]
	if !ok {
		return
	}
	s.revokeFamilyLocked(entry.familyID)
}

func (s *RefreshStore) revokeFamilyLocked(familyID string) {
	for _, e := range s.entries {
		if e.familyID == familyID {
			e.revoked = true
		}
	}
}

func randomToken() (string, error) {
	buf := make([]byte, 32)
	if _, err := rand.Read(buf); err != nil {
		return "", fmt.Errorf("auth: generating token: %w", err)
	}
	return base64.RawURLEncoding.EncodeToString(buf), nil
}
