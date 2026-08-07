// Package crypto owns credential hashing. Argon2id per
// docs/spec/security-rbac.md §Security baseline. The encoded string format is
// shared infrastructure because the edge verifies these same hashes offline
// (ADR-011) — one format, one implementation, never duplicated per module.
package crypto

import (
	"crypto/rand"
	"crypto/subtle"
	"encoding/base64"
	"errors"
	"fmt"
	"strings"

	"golang.org/x/crypto/argon2"
)

// Argon2id parameters. Named constants, not magic numbers; tuned for a
// shop-floor device that must verify a login quickly while offline.
const (
	argonTime    uint32 = 2
	argonMemory  uint32 = 64 * 1024 // 64 MiB
	argonThreads uint8  = 4
	argonKeyLen  uint32 = 32
	saltLen             = 16
)

var (
	// ErrMismatch is returned when a credential does not match. Callers must
	// not distinguish it from "user not found" in any response.
	ErrMismatch = errors.New("crypto: credential mismatch")
	// ErrMalformedHash indicates a stored hash could not be parsed.
	ErrMalformedHash = errors.New("crypto: malformed hash")
)

// HashPassword returns the PHC-style encoded Argon2id string stored in
// app_user.password_hash / app_user.pin_hash. Never log the return value.
func HashPassword(plaintext string) (string, error) {
	salt := make([]byte, saltLen)
	if _, err := rand.Read(salt); err != nil {
		return "", fmt.Errorf("crypto: reading salt: %w", err)
	}

	key := argon2.IDKey([]byte(plaintext), salt, argonTime, argonMemory, argonThreads, argonKeyLen)

	return fmt.Sprintf("$argon2id$v=%d$m=%d,t=%d,p=%d$%s$%s",
		argon2.Version, argonMemory, argonTime, argonThreads,
		base64.RawStdEncoding.EncodeToString(salt),
		base64.RawStdEncoding.EncodeToString(key),
	), nil
}

// VerifyPassword compares a plaintext against an encoded hash in constant
// time. It returns ErrMismatch on failure and never reveals which component
// differed.
func VerifyPassword(plaintext, encoded string) error {
	parts := strings.Split(encoded, "$")
	if len(parts) != 6 || parts[1] != "argon2id" {
		return ErrMalformedHash
	}

	var version int
	if _, err := fmt.Sscanf(parts[2], "v=%d", &version); err != nil || version != argon2.Version {
		return ErrMalformedHash
	}

	var memory, time uint32
	var threads uint8
	if _, err := fmt.Sscanf(parts[3], "m=%d,t=%d,p=%d", &memory, &time, &threads); err != nil {
		return ErrMalformedHash
	}

	salt, err := base64.RawStdEncoding.DecodeString(parts[4])
	if err != nil {
		return ErrMalformedHash
	}
	want, err := base64.RawStdEncoding.DecodeString(parts[5])
	if err != nil {
		return ErrMalformedHash
	}

	got := argon2.IDKey([]byte(plaintext), salt, time, memory, threads, uint32(len(want)))
	if subtle.ConstantTimeCompare(got, want) != 1 {
		return ErrMismatch
	}
	return nil
}
