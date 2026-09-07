package httpx

import (
	"net/http"
	"net/http/httptest"
	"testing"
)

// The defect these pin, in the form it actually appeared: `apps/admin` on
// http://localhost:5175 could not sign in, the browser said "Failed to fetch",
// the console showed no HTTP status, and the backend log showed nothing useful
// — because a preflight that never gets an allow header is refused by the
// browser before the page sees a response.
//
// Watched failing first with the middleware removed from the chain: every
// assertion below that names a header fails on an empty string.

func handlerOK() http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.WriteHeader(http.StatusOK)
		_, _ = w.Write([]byte(`{"ok":true}`))
	})
}

func TestCORS_PreflightFromAnAllowedOriginIsAnswered(t *testing.T) {
	h := CORS([]string{"http://localhost:5175"})(handlerOK())

	req := httptest.NewRequest(http.MethodOptions, "/menu/items", nil)
	req.Header.Set("Origin", "http://localhost:5175")
	req.Header.Set("Access-Control-Request-Method", "PATCH")
	req.Header.Set("Access-Control-Request-Headers", "authorization,content-type")
	rec := httptest.NewRecorder()
	h.ServeHTTP(rec, req)

	if rec.Code != http.StatusNoContent {
		t.Errorf("preflight status = %d, want 204", rec.Code)
	}
	if got := rec.Header().Get("Access-Control-Allow-Origin"); got != "http://localhost:5175" {
		t.Errorf("Allow-Origin = %q, want the exact origin", got)
	}
	// Authorization is the one that matters. This API is bearer-token only, so
	// without it every authenticated request fails preflight and the app looks
	// broken in a way no server log explains.
	if got := rec.Header().Get("Access-Control-Allow-Headers"); got == "" {
		t.Error("Allow-Headers is empty; a bearer-token API must allow Authorization or nothing authenticated can be called from a browser")
	}
	if got := rec.Header().Get("Access-Control-Allow-Methods"); got == "" {
		t.Error("Allow-Methods is empty; PATCH would be refused by the browser")
	}
}

// A preflight must NOT reach the router. It carries no Authorization header,
// so the auth middleware would 401 it, and a 401 on an OPTIONS reads to the
// browser as a CORS failure with no detail.
func TestCORS_PreflightNeverReachesTheHandler(t *testing.T) {
	reached := false
	h := CORS([]string{"http://localhost:5175"})(http.HandlerFunc(func(http.ResponseWriter, *http.Request) {
		reached = true
	}))

	req := httptest.NewRequest(http.MethodOptions, "/menu/items", nil)
	req.Header.Set("Origin", "http://localhost:5175")
	h.ServeHTTP(httptest.NewRecorder(), req)

	if reached {
		t.Error("the preflight reached the router; it must be answered by the middleware, or auth will 401 it")
	}
}

// THE SECURITY HALF. An origin that is not on the list gets no allow header,
// so the browser refuses to hand the response to that page.
//
// Note what this does NOT assert: the request is not rejected. A disallowed
// origin still reaches the router, because refusing outright would break every
// caller that sends no Origin at all — the edge sync client, curl, the health
// probe. Enforcement is the browser's job and the absent header is the whole
// mechanism.
func TestCORS_UnknownOriginGetsNoAllowHeader(t *testing.T) {
	h := CORS([]string{"http://localhost:5175"})(handlerOK())

	req := httptest.NewRequest(http.MethodGet, "/menu/items", nil)
	req.Header.Set("Origin", "https://evil.example.com")
	rec := httptest.NewRecorder()
	h.ServeHTTP(rec, req)

	if got := rec.Header().Get("Access-Control-Allow-Origin"); got != "" {
		t.Errorf("Allow-Origin = %q for an unlisted origin, want empty — reflecting an arbitrary Origin lets any page make authenticated requests from a logged-in operator's browser", got)
	}
}

// FAIL CLOSED. An unset allowlist serves no browser rather than serving every
// browser. A deployment that forgot to configure this must break visibly for
// the admin console, never silently open the API to any origin.
func TestCORS_EmptyAllowlistServesNoOrigin(t *testing.T) {
	h := CORS(nil)(handlerOK())

	req := httptest.NewRequest(http.MethodGet, "/menu/items", nil)
	req.Header.Set("Origin", "http://localhost:5175")
	rec := httptest.NewRecorder()
	h.ServeHTTP(rec, req)

	if got := rec.Header().Get("Access-Control-Allow-Origin"); got != "" {
		t.Errorf("Allow-Origin = %q with an empty allowlist, want empty", got)
	}
}

// Existing callers are unaffected. The POS, the KDS and the edge sync client
// send no Origin header at all, and this middleware must be invisible to them
// — including when no allowlist is configured.
func TestCORS_RequestWithNoOriginIsUntouched(t *testing.T) {
	h := CORS([]string{"http://localhost:5175"})(handlerOK())

	req := httptest.NewRequest(http.MethodGet, "/health", nil)
	rec := httptest.NewRecorder()
	h.ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Errorf("status = %d for an origin-less request, want 200", rec.Code)
	}
	if got := rec.Header().Get("Access-Control-Allow-Origin"); got != "" {
		t.Errorf("Allow-Origin = %q for a request with no Origin, want empty", got)
	}
}

// The response varies by Origin, so a cache that ignored that would serve one
// origin's headers to another.
func TestCORS_VariesOnOrigin(t *testing.T) {
	h := CORS([]string{"http://localhost:5175"})(handlerOK())

	req := httptest.NewRequest(http.MethodGet, "/menu/items", nil)
	req.Header.Set("Origin", "http://localhost:5175")
	rec := httptest.NewRecorder()
	h.ServeHTTP(rec, req)

	if got := rec.Header().Get("Vary"); got != "Origin" {
		t.Errorf("Vary = %q, want Origin", got)
	}
}
