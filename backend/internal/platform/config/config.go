// Package config loads backend configuration from the environment. No URL,
// credential, tax rate or outlet id is ever hard-coded (CLAUDE.md §Coding
// rules); every deployment-specific value arrives here.
package config

import (
	"fmt"
	"os"
	"strconv"
	"strings"
	"time"
)

type Config struct {
	Port            string
	DatabaseURL     string
	AccessTokenTTL  time.Duration
	RefreshTokenTTL time.Duration
	TokenSigningKey []byte
	// ContractsDir points at the frozen packages/contracts/postgres
	// migrations directory postgres.Migrate applies at startup. Never a
	// literal in cmd/api — always sourced from here, so a deployment can
	// relocate the contracts checkout without a code change.
	ContractsDir string

	// AllowedCORSOrigins is the exact allowlist of browser origins this API
	// serves cross-origin, from HOLLER_CORS_ALLOWED_ORIGINS (comma-separated).
	//
	// NO DEFAULT, AND EMPTY IS A REAL ANSWER: with nothing set the API emits no
	// CORS headers at all and every cross-origin browser request fails. That is
	// the correct failure for a misconfigured deployment. A convenience default
	// of localhost:5175 was considered and rejected -- the same reasoning that
	// removed the hardcoded default database key from dev-bootstrap.ps1: a
	// default is consent by omission, and the one that ships is whichever
	// nobody had to choose.
	AllowedCORSOrigins []string

	// The login budget (ADR-012), from HOLLER_LOGIN_RATE_LIMIT_ATTEMPTS and
	// HOLLER_LOGIN_RATE_LIMIT_WINDOW. Defaults are the policy values in
	// auth.LoginRateLimitAttempts / auth.LoginRateLimitWindow, repeated here
	// rather than imported because platform packages do not depend on
	// bounded contexts; auth's own constants stay the source of truth and a
	// test pins the two together.
	//
	// EXISTS FOR THE DEMO BUILD. A rate-limited login is indistinguishable
	// from a wrong password by design, so five fumbled attempts lock every
	// client on one IP for fifteen minutes with nothing a human can act on.
	// A deployment may widen that; nothing here narrows it, and an unset or
	// unparseable value leaves the defaults in force.
	LoginRateLimitAttempts int
	LoginRateLimitWindow   time.Duration
}

// Load reads configuration from the environment, applying defaults only for
// values that are safe to default. Secrets have no defaults — a missing
// signing key is a startup error, never a generated fallback.
func Load() (Config, error) {
	cfg := Config{
		Port:               envOr("PORT", "8080"),
		DatabaseURL:        os.Getenv("DATABASE_URL"),
		ContractsDir:       envOr("CONTRACTS_DIR", "../packages/contracts/postgres"),
		AllowedCORSOrigins: splitAndTrim(os.Getenv("HOLLER_CORS_ALLOWED_ORIGINS")),
	}

	if cfg.DatabaseURL == "" {
		return Config{}, fmt.Errorf("config: DATABASE_URL is required")
	}

	key := os.Getenv("TOKEN_SIGNING_KEY")
	if key == "" {
		return Config{}, fmt.Errorf("config: TOKEN_SIGNING_KEY is required")
	}
	cfg.TokenSigningKey = []byte(key)

	var err error
	if cfg.AccessTokenTTL, err = durationEnvOr("ACCESS_TOKEN_TTL", 15*time.Minute); err != nil {
		return Config{}, err
	}
	if cfg.RefreshTokenTTL, err = durationEnvOr("REFRESH_TOKEN_TTL", 720*time.Hour); err != nil {
		return Config{}, err
	}
	if cfg.LoginRateLimitWindow, err = durationEnvOr("HOLLER_LOGIN_RATE_LIMIT_WINDOW", 15*time.Minute); err != nil {
		return Config{}, err
	}
	if cfg.LoginRateLimitAttempts, err = intEnvOr("HOLLER_LOGIN_RATE_LIMIT_ATTEMPTS", 5); err != nil {
		return Config{}, err
	}

	return cfg, nil
}

func envOr(key, fallback string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return fallback
}

// intEnvOr parses a positive integer from the environment. A value that is
// absent leaves the default in place; one that is present and unparseable, or
// not positive, is a STARTUP ERROR rather than a silent fallback -- a limiter
// budget of zero or a typo would otherwise read as "no limit" or as the
// default, and neither is what the operator wrote down.
func intEnvOr(key string, fallback int) (int, error) {
	raw := strings.TrimSpace(os.Getenv(key))
	if raw == "" {
		return fallback, nil
	}
	value, err := strconv.Atoi(raw)
	if err != nil {
		return 0, fmt.Errorf("config: %s is not an integer: %w", key, err)
	}
	if value <= 0 {
		return 0, fmt.Errorf("config: %s must be positive, got %d", key, value)
	}
	return value, nil
}

func durationEnvOr(key string, fallback time.Duration) (time.Duration, error) {
	raw := os.Getenv(key)
	if raw == "" {
		return fallback, nil
	}
	d, err := time.ParseDuration(raw)
	if err != nil {
		return 0, fmt.Errorf("config: %s is not a duration: %w", key, err)
	}
	return d, nil
}

// IntEnvOr is exposed for modules that need a tunable numeric knob rather than
// a magic number at the call site.
func IntEnvOr(key string, fallback int) (int, error) {
	raw := os.Getenv(key)
	if raw == "" {
		return fallback, nil
	}
	n, err := strconv.Atoi(raw)
	if err != nil {
		return 0, fmt.Errorf("config: %s is not an integer: %w", key, err)
	}
	return n, nil
}

// splitAndTrim turns "a, b ,c" into ["a","b","c"], dropping empties so a
// trailing comma or a blank variable yields an EMPTY allowlist rather than one
// containing "" -- an empty-string origin would never match a real Origin
// header, but it would make the list look populated to anyone printing it.
func splitAndTrim(raw string) []string {
	if strings.TrimSpace(raw) == "" {
		return nil
	}
	parts := strings.Split(raw, ",")
	out := make([]string, 0, len(parts))
	for _, p := range parts {
		if trimmed := strings.TrimSpace(p); trimmed != "" {
			out = append(out, trimmed)
		}
	}
	return out
}
