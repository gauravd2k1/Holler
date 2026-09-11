package config

import (
	"testing"
	"time"
)

// setRequired sets the two values Load refuses to default, so each test below
// exercises only the value it is about.
func setRequired(t *testing.T) {
	t.Helper()
	t.Setenv("DATABASE_URL", "postgres://user:pass@127.0.0.1:5432/db?sslmode=disable")
	t.Setenv("TOKEN_SIGNING_KEY", "test-signing-key")
}

// The login budget defaults to the ADR-012 policy. These literals are
// deliberately repeated rather than imported from internal/auth: a platform
// package does not depend on a bounded context, so the two copies are pinned
// by asserting the numbers, and a change to either side without the other
// fails here rather than at a demo.
func TestLoad_LoginRateLimitDefaultsToThePolicy(t *testing.T) {
	setRequired(t)

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load: %v", err)
	}
	if cfg.LoginRateLimitAttempts != 5 {
		t.Fatalf("default attempts: want 5 (auth.LoginRateLimitAttempts), got %d", cfg.LoginRateLimitAttempts)
	}
	if cfg.LoginRateLimitWindow != 15*time.Minute {
		t.Fatalf("default window: want 15m (auth.LoginRateLimitWindow), got %s", cfg.LoginRateLimitWindow)
	}
}

func TestLoad_LoginRateLimitReadsTheEnvironment(t *testing.T) {
	setRequired(t)
	t.Setenv("HOLLER_LOGIN_RATE_LIMIT_ATTEMPTS", "50")
	t.Setenv("HOLLER_LOGIN_RATE_LIMIT_WINDOW", "2m")

	cfg, err := Load()
	if err != nil {
		t.Fatalf("Load: %v", err)
	}
	if cfg.LoginRateLimitAttempts != 50 {
		t.Fatalf("attempts: want 50, got %d", cfg.LoginRateLimitAttempts)
	}
	if cfg.LoginRateLimitWindow != 2*time.Minute {
		t.Fatalf("window: want 2m, got %s", cfg.LoginRateLimitWindow)
	}
}

// A malformed or non-positive budget is a STARTUP ERROR, never a silent
// fallback to the default: an operator who typed the value wrong, or wrote 0
// meaning "off", must find out at boot rather than discover at a demo that the
// limiter is doing something they did not ask for.
func TestLoad_LoginRateLimitRefusesGarbageRatherThanDefaulting(t *testing.T) {
	for _, value := range []string{"lots", "0", "-3", "5.5"} {
		t.Run(value, func(t *testing.T) {
			setRequired(t)
			t.Setenv("HOLLER_LOGIN_RATE_LIMIT_ATTEMPTS", value)

			if _, err := Load(); err == nil {
				t.Fatalf("HOLLER_LOGIN_RATE_LIMIT_ATTEMPTS=%q must fail Load, not fall back to the default", value)
			}
		})
	}
}
