// Package config loads backend configuration from the environment. No URL,
// credential, tax rate or outlet id is ever hard-coded (CLAUDE.md §Coding
// rules); every deployment-specific value arrives here.
package config

import (
	"fmt"
	"os"
	"strconv"
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
}

// Load reads configuration from the environment, applying defaults only for
// values that are safe to default. Secrets have no defaults — a missing
// signing key is a startup error, never a generated fallback.
func Load() (Config, error) {
	cfg := Config{
		Port:         envOr("PORT", "8080"),
		DatabaseURL:  os.Getenv("DATABASE_URL"),
		ContractsDir: envOr("CONTRACTS_DIR", "../packages/contracts/postgres"),
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

	return cfg, nil
}

func envOr(key, fallback string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return fallback
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
