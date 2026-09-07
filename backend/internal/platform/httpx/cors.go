package httpx

import (
	"net/http"
	"strings"
)

// CORS headers for browser clients on a different origin.
//
// WHY THIS DID NOT EXIST UNTIL M6 PHASE B. Every browser that had ever called
// this API was same-origin or not a browser at all: the POS is a Tauri window
// with its own scheme, the KDS talks to the edge over the LAN, and the edge
// sync client is a Rust process with no origin and no preflight. `apps/admin`
// on a Vite dev server is the first cross-origin browser client this API has
// ever served, and it failed with "Failed to fetch" — which is the browser
// refusing to hand the response to the page, not the request failing.
//
// THE ALLOWLIST IS EXACT AND IT FAILS CLOSED. No wildcard, not even in
// development:
//
//   - `Access-Control-Allow-Origin: *` cannot be combined with credentials,
//     and reflecting an arbitrary Origin back is the same thing wearing a
//     disguise — it lets any page on the internet make authenticated requests
//     to this API from a logged-in operator's browser.
//   - An unset allowlist emits NO CORS headers at all, so the failure mode of
//     a misconfigured deployment is "the admin console cannot reach the API",
//     never "any origin can". A wrong config that reads as working is the
//     defect this whole milestone kept finding.
//
// A dev default of "http://localhost:5175" was considered and rejected for the
// same reason `dev-bootstrap.ps1` has no default database key: a default is
// consent by omission, and the one that ships is whichever nobody had to
// choose.
func CORS(allowedOrigins []string) func(http.Handler) http.Handler {
	allowed := make(map[string]struct{}, len(allowedOrigins))
	for _, o := range allowedOrigins {
		trimmed := strings.TrimSpace(o)
		if trimmed != "" {
			allowed[trimmed] = struct{}{}
		}
	}

	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			origin := r.Header.Get("Origin")

			// Not a cross-origin browser request, or an origin we do not
			// serve. Either way: no CORS headers, and the request continues to
			// the router. A disallowed origin is NOT rejected here — the
			// browser enforces the absence of the header, and refusing the
			// request outright would break every non-browser caller that sends
			// no Origin at all.
			if origin == "" {
				next.ServeHTTP(w, r)
				return
			}
			if _, ok := allowed[origin]; !ok {
				next.ServeHTTP(w, r)
				return
			}

			h := w.Header()
			h.Set("Access-Control-Allow-Origin", origin)
			// The response varies by Origin, so a cache that ignored this
			// would serve one origin's CORS headers to another.
			h.Add("Vary", "Origin")

			// Authorization is the one that matters: this API is bearer-token
			// only, so without it every authenticated request fails preflight.
			// X-Tenant-ID is ADR-012's time-boxed interim and is sent on login
			// alone.
			h.Set("Access-Control-Allow-Headers", "Authorization, Content-Type, X-Tenant-ID")
			h.Set("Access-Control-Allow-Methods", "GET, POST, PATCH, PUT, DELETE, OPTIONS")

			// DELIBERATELY ABSENT: Access-Control-Allow-Credentials. This API
			// authenticates with a bearer token in a header, never a cookie,
			// so credentialed CORS buys nothing and would commit us to a
			// cookie model plus the CSRF handling that comes with it.

			// A preflight is answered here and never reaches the router: it
			// carries no Authorization header, so letting it through would
			// meet the auth middleware and 401, and the browser would report
			// that as a CORS failure with no useful detail.
			if r.Method == http.MethodOptions {
				h.Set("Access-Control-Max-Age", "600")
				w.WriteHeader(http.StatusNoContent)
				return
			}

			next.ServeHTTP(w, r)
		})
	}
}
