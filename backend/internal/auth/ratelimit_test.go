package auth

import (
	"context"
	"errors"
	"testing"
	"time"
)

func TestInMemoryRateLimiter_AllowsUnderLimitDeniesOver(t *testing.T) {
	limiter := NewInMemoryRateLimiter()
	ctx := context.Background()

	for i := 0; i < 3; i++ {
		allowed, err := limiter.Allow(ctx, "k", 3, time.Minute)
		if err != nil {
			t.Fatalf("attempt %d: %v", i, err)
		}
		if !allowed {
			t.Fatalf("attempt %d: expected allowed", i)
		}
	}

	allowed, err := limiter.Allow(ctx, "k", 3, time.Minute)
	if err != nil {
		t.Fatalf("4th attempt: %v", err)
	}
	if allowed {
		t.Fatal("expected the 4th attempt to be denied")
	}
}

func TestInMemoryRateLimiter_WindowResets(t *testing.T) {
	now := time.Now()
	limiter := NewInMemoryRateLimiter()
	limiter.now = func() time.Time { return now }
	ctx := context.Background()

	for i := 0; i < 2; i++ {
		if allowed, err := limiter.Allow(ctx, "k", 2, time.Minute); err != nil || !allowed {
			t.Fatalf("attempt %d: allowed=%v err=%v", i, allowed, err)
		}
	}
	if allowed, _ := limiter.Allow(ctx, "k", 2, time.Minute); allowed {
		t.Fatal("expected the 3rd attempt within the window to be denied")
	}

	now = now.Add(time.Minute + time.Second)
	if allowed, err := limiter.Allow(ctx, "k", 2, time.Minute); err != nil || !allowed {
		t.Fatalf("expected a fresh window to allow again: allowed=%v err=%v", allowed, err)
	}
}

// alwaysErrorLimiter simulates a Redis outage: every call fails.
type alwaysErrorLimiter struct{}

func (alwaysErrorLimiter) Allow(ctx context.Context, key string, limit int, window time.Duration) (bool, error) {
	return false, errors.New("ratelimit: backing store unavailable")
}

// TestLogin_FailsClosedWhenLimiterUnavailable proves ADR-012's fail-closed
// requirement: if the rate limiter's backing store errors, login must be
// denied, never allowed through.
func TestLogin_FailsClosedWhenLimiterUnavailable(t *testing.T) {
	repo := newFakeRepo()
	signer := NewTokenSigner([]byte("k"))
	refresh := NewInMemoryRefreshStore()
	svc := NewService(repo, signer, refresh, alwaysErrorLimiter{}, nil, time.Minute, time.Hour)

	tenantID := "tenant-1"
	userID := "user-1"
	hash := mustHash(t, "correct-password")
	repo.CreateUser(context.Background(), userID, tenantID, "user@example.com", "User", hash, time.Now())

	// Even with fully correct credentials, an unavailable limiter must deny.
	if _, err := svc.Login(context.Background(), testClientIP, tenantID, "user@example.com", "correct-password", "outlet-1"); err == nil {
		t.Fatal("expected login to fail closed when the rate limiter is unavailable")
	}
}

// TestLogin_RateLimitRejectionDoesNotLeakAccountExistence proves that a
// rate-limited response is identical whether or not the targeted account
// exists.
func TestLogin_RateLimitRejectionDoesNotLeakAccountExistence(t *testing.T) {
	repo := newFakeRepo()
	signer := NewTokenSigner([]byte("k"))
	refresh := NewInMemoryRefreshStore()
	limiter := NewInMemoryRateLimiter()
	svc := NewService(repo, signer, refresh, limiter, nil, time.Minute, time.Hour)

	tenantID := "tenant-1"
	existingID := "user-1"
	hash := mustHash(t, "correct-password")
	repo.CreateUser(context.Background(), existingID, tenantID, "exists@example.com", "Exists", hash, time.Now())

	ip := "203.0.113.50"
	// Exhaust the budget for this IP+tenant (and IP-alone) key.
	for i := 0; i < LoginRateLimitAttempts; i++ {
		svc.Login(context.Background(), ip, tenantID, "exists@example.com", "wrong-password", "outlet-1")
	}

	_, errExisting := svc.Login(context.Background(), ip, tenantID, "exists@example.com", "correct-password", "outlet-1")
	_, errMissing := svc.Login(context.Background(), ip, tenantID, "nobody@example.com", "irrelevant", "outlet-1")

	if errExisting == nil || errMissing == nil {
		t.Fatal("expected both attempts to be rejected once the budget is exhausted")
	}
	if errExisting.Error() != errMissing.Error() {
		t.Fatalf("rate-limit rejection must not distinguish account existence: %v vs %v", errExisting, errMissing)
	}
	if !errors.Is(errExisting, ErrRateLimited) || !errors.Is(errMissing, ErrRateLimited) {
		t.Fatalf("expected ErrRateLimited, got %v / %v", errExisting, errMissing)
	}
}

// TestLogin_HeaderRotationDoesNotResetRateLimitCounter proves the ADR-012
// mitigation: an attacker who rotates X-Tenant-ID (simulated here by calling
// Login with a fresh tenantID each time) from the same IP still exhausts a
// shared budget, because the limiter also checks an IP-only key that does
// not vary with tenant.
func TestLogin_HeaderRotationDoesNotResetRateLimitCounter(t *testing.T) {
	repo := newFakeRepo()
	signer := NewTokenSigner([]byte("k"))
	refresh := NewInMemoryRefreshStore()
	limiter := NewInMemoryRateLimiter()
	svc := NewService(repo, signer, refresh, limiter, nil, time.Minute, time.Hour)

	ip := "203.0.113.77"

	// Exhaust the IP-only budget using a fresh tenant on every attempt, as an
	// attacker rotating X-Tenant-ID would.
	for i := 0; i < LoginRateLimitAttempts; i++ {
		tenantID := "tenant-rotating-" + string(rune('a'+i))
		if _, err := svc.Login(context.Background(), ip, tenantID, "nobody@example.com", "irrelevant", "outlet-1"); err == nil {
			t.Fatalf("attempt %d: expected a failure (no such user), got success", i)
		}
	}

	// One more attempt, again with a brand-new tenant, must still be denied
	// because the IP-only counter is exhausted — rotating the header did not
	// buy a fresh budget.
	_, err := svc.Login(context.Background(), ip, "tenant-rotating-final", "nobody@example.com", "irrelevant", "outlet-1")
	if !errors.Is(err, ErrRateLimited) {
		t.Fatalf("expected header rotation to still hit the shared IP budget, got %v", err)
	}
}
