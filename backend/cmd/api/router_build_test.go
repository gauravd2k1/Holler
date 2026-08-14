package main

import (
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/holler/backend/internal/platform/config"
)

// TestBuildRouter_DoesNotPanicOnMount is the guard T13 was asked for: the
// double-Mount() panic T8 was trying to avoid when it split tables/ordering/
// kitchen's Mount from MountIngest happens at ROUTE-BUILD time, not request
// time (go-chi/chi/v5's own mux.go: mounting the same pattern twice via
// Route()/Mount() panics as soon as the second Mount() call runs). A request
// -level test cannot see that failure mode at all — only actually calling
// buildRouter can. No Postgres connection is required: buildRouter only
// wires repository/service/handler values and mounts routes: it never
// issues a query, so this test needs no HOLLER_TEST_DATABASE_URL gate and
// always runs.
func TestBuildRouter_DoesNotPanicOnMount(t *testing.T) {
	cfg := config.Config{
		Port: "0", DatabaseURL: "unused-in-test",
		AccessTokenTTL: 15 * time.Minute, RefreshTokenTTL: 720 * time.Hour,
		TokenSigningKey: []byte("router-build-test-signing-key-not-for-prod"),
	}

	defer func() {
		if r := recover(); r != nil {
			t.Fatalf("buildRouter panicked (likely a chi double-Mount() collision on a shared pattern): %v", r)
		}
	}()

	router := buildRouter(nil, cfg)
	if router == nil {
		t.Fatal("buildRouter returned a nil router")
	}
}

// TestTableSessionEnvelopeRoutes_TolerateTrailingSlash is the T13 Task 1
// regression guard: T8 split tables' MountEnvelopeIngest/MountEnvelopeRead
// off r.Route() onto plain r.Post/r.Get to dodge the panic above, which
// silently dropped the trailing-slash tolerance the old
// r.Route(...).Get("/") form gave for free (chi's Mount() accepts both
// "/x/table-sessions" and "/x/table-sessions/"). This asserts both forms
// resolve to a mounted route rather than 404 — a bare 404 here means the
// pattern was never registered; a 401 means chi matched the route and
// handed off to auth middleware, which is what we want to observe since
// this test carries no credential and no Postgres pool.
func TestTableSessionEnvelopeRoutes_TolerateTrailingSlash(t *testing.T) {
	cfg := config.Config{
		Port: "0", DatabaseURL: "unused-in-test",
		AccessTokenTTL: 15 * time.Minute, RefreshTokenTTL: 720 * time.Hour,
		TokenSigningKey: []byte("router-build-test-signing-key-not-for-prod"),
	}
	router := buildRouter(nil, cfg)

	cases := []struct {
		method string
		path   string
	}{
		{http.MethodGet, "/outlets/out-1/table-sessions"},
		{http.MethodGet, "/outlets/out-1/table-sessions/"},
		{http.MethodGet, "/outlets/out-1/table-sessions/sess-1"},
		{http.MethodGet, "/outlets/out-1/table-sessions/sess-1/"},
		{http.MethodPost, "/outlets/out-1/table-sessions"},
		{http.MethodPost, "/outlets/out-1/table-sessions/"},
		{http.MethodPost, "/outlets/out-1/table-sessions/sess-1"},
		{http.MethodPost, "/outlets/out-1/table-sessions/sess-1/"},
	}
	for _, tc := range cases {
		req := httptest.NewRequest(tc.method, tc.path, nil)
		rec := httptest.NewRecorder()
		router.ServeHTTP(rec, req)
		if rec.Code == http.StatusNotFound {
			t.Errorf("%s %s: got 404 — route not registered for this slash variant", tc.method, tc.path)
		}
	}
}
