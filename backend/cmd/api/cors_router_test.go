package main

import (
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/holler/backend/internal/platform/config"
)

// CORS through the REAL router, not the middleware in isolation.
//
// WHY BOTH. `internal/platform/httpx` unit-tests the middleware's behaviour;
// this asserts it is actually WIRED, and in the right position. Those are
// different failures: a correct middleware mounted after `auth.Authenticate`
// would pass every unit test and still break the admin console, because the
// preflight carries no Authorization header, would 401, and the browser would
// report that as the same opaque "Failed to fetch" the console started with.
//
// This is the closest thing to "verify with an actual request from the browser"
// that runs without a browser: a real preflight, over a real HTTP connection,
// against the real route table.
func TestRouter_PreflightFromTheAdminOriginIsAllowed(t *testing.T) {
	pool := setupIntegrationPool(t)

	const adminOrigin = "http://localhost:5175"
	cfg := config.Config{
		Port:               "0",
		DatabaseURL:        "unused-in-test",
		AccessTokenTTL:     15 * time.Minute,
		RefreshTokenTTL:    720 * time.Hour,
		TokenSigningKey:    []byte("integration-test-signing-key-not-for-prod"),
		AllowedCORSOrigins: []string{adminOrigin},
	}
	server := httptest.NewServer(buildRouter(pool, cfg))
	defer server.Close()

	// The exact preflight a browser sends before the admin console's login
	// POST: a cross-origin request with a JSON body and custom headers is
	// never "simple", so this always happens first.
	req, err := http.NewRequest(http.MethodOptions, server.URL+"/auth/login", nil)
	if err != nil {
		t.Fatalf("building the preflight: %v", err)
	}
	req.Header.Set("Origin", adminOrigin)
	req.Header.Set("Access-Control-Request-Method", "POST")
	req.Header.Set("Access-Control-Request-Headers", "content-type,x-tenant-id")

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("preflight request: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode != http.StatusNoContent {
		t.Errorf("preflight status = %d, want 204 — anything else means the preflight reached a handler instead of the CORS middleware", resp.StatusCode)
	}
	if got := resp.Header.Get("Access-Control-Allow-Origin"); got != adminOrigin {
		t.Errorf("Allow-Origin = %q, want %q", got, adminOrigin)
	}
	if got := resp.Header.Get("Access-Control-Allow-Headers"); got == "" {
		t.Error("Allow-Headers is empty on the real router; the console cannot send Authorization or X-Tenant-ID")
	}
}

// An authenticated GET must carry the allow header on its RESPONSE too, not
// just on the preflight. A preflight that passes and a response that arrives
// without the header is a page that still sees nothing — and it is a plausible
// wiring mistake, since the two are handled by different branches.
func TestRouter_AuthenticatedResponseCarriesTheAllowHeader(t *testing.T) {
	pool := setupIntegrationPool(t)

	const adminOrigin = "http://localhost:5175"
	cfg := config.Config{
		Port:               "0",
		DatabaseURL:        "unused-in-test",
		AccessTokenTTL:     15 * time.Minute,
		RefreshTokenTTL:    720 * time.Hour,
		TokenSigningKey:    []byte("integration-test-signing-key-not-for-prod"),
		AllowedCORSOrigins: []string{adminOrigin},
	}
	server := httptest.NewServer(buildRouter(pool, cfg))
	defer server.Close()

	// No token: this 401s, and that is fine. What is being asserted is that the
	// CORS header is present on the way out REGARDLESS of the status — a page
	// that cannot read a 401 cannot show the operator why sign-in failed, which
	// is its own small version of the same defect.
	req, err := http.NewRequest(http.MethodGet, server.URL+"/menu/items?outlet_id=x", nil)
	if err != nil {
		t.Fatalf("building the request: %v", err)
	}
	req.Header.Set("Origin", adminOrigin)

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		t.Fatalf("request: %v", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if got := resp.Header.Get("Access-Control-Allow-Origin"); got != adminOrigin {
		t.Errorf("Allow-Origin = %q on a %d response, want %q — the header must be present whatever the status", got, resp.StatusCode, adminOrigin)
	}
}
