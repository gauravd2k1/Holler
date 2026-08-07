package auth

import (
	"context"
	"errors"
	"sync"
	"time"
)

// Login attempt budget (ADR-012). Named constants, not inline literals: five
// attempts per fifteen-minute fixed window is the Milestone 1 policy value:
// generous enough that a cashier fumbling a PIN once or twice is unaffected,
// tight enough to make online credential-stuffing against a single account
// impractical.
const (
	LoginRateLimitAttempts = 5
	LoginRateLimitWindow   = 15 * time.Minute
)

// ErrRateLimited is returned by CheckLoginRateLimit when the caller has
// exhausted its budget, or when the backing store is unavailable — ADR-012
// requires the login path to fail closed, so an unavailable limiter is
// indistinguishable from "too many attempts" to the caller. The HTTP layer
// must map this to the same response either way and must never let it
// reveal whether the attempted account exists.
var ErrRateLimited = errors.New("auth: rate limited")

// RateLimiter is the seam ADR-012 asks for: the domain depends on this
// small interface, not on a specific backing store. Redis is the intended
// production implementation (already in the stack, survives restart, works
// across instances) — see the KNOWN LIMITATION note on InMemoryRateLimiter
// below for why this pass ships the in-memory implementation instead.
type RateLimiter interface {
	// Allow reports whether one more attempt under key is permitted within
	// the trailing window, given a maximum of limit attempts per window, and
	// records this attempt if so. A non-nil error means the backing store
	// could not be reached; callers on the login path MUST treat that as
	// "not allowed" (fail closed), never as "allowed".
	Allow(ctx context.Context, key string, limit int, window time.Duration) (bool, error)
}

// InMemoryRateLimiter is a fixed-window RateLimiter backed by a process-local
// map.
//
// KNOWN LIMITATION (reported to orchestrator): adding a Redis client would
// require a backend/go.mod change, which this pass is scoped not to make
// (no Redis dependency exists in backend/go.mod today). ADR-012 names Redis
// as the intended backing store — same shape of gap as the refresh-token
// store before contracts 0.2.1 added its table. This in-memory
// implementation is behaviour-correct for a single backend instance (which
// is all Milestone 1 runs) but, like the old refresh store, does not share
// budget across instances or survive a restart. Swapping in a Redis-backed
// RateLimiter later is a single-file change behind this interface.
type InMemoryRateLimiter struct {
	mu      sync.Mutex
	windows map[string]*rateWindow
	now     func() time.Time
}

type rateWindow struct {
	count   int
	resetAt time.Time
}

func NewInMemoryRateLimiter() *InMemoryRateLimiter {
	return &InMemoryRateLimiter{windows: make(map[string]*rateWindow), now: time.Now}
}

func (l *InMemoryRateLimiter) Allow(ctx context.Context, key string, limit int, window time.Duration) (bool, error) {
	l.mu.Lock()
	defer l.mu.Unlock()

	now := l.now()
	w, ok := l.windows[key]
	if !ok || now.After(w.resetAt) {
		w = &rateWindow{count: 0, resetAt: now.Add(window)}
		l.windows[key] = w
	}

	if w.count >= limit {
		return false, nil
	}
	w.count++
	return true, nil
}
