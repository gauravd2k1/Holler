package outlet

import (
	"context"
	"net/http"
	"strings"

	"github.com/holler/backend/internal/platform/httpx"
)

// deviceVerifier is the minimal seam DeviceAuthenticate needs. *DeviceService
// satisfies it; tests use a fake so this middleware can be exercised without
// a Postgres pool.
type deviceVerifier interface {
	VerifyToken(ctx context.Context, token string) (DevicePrincipal, error)
}

type devicePrincipalContextKey struct{}

// WithDevicePrincipal returns a context carrying p. Exported so tests can
// inject a device principal directly, mirroring auth.WithPrincipal.
func WithDevicePrincipal(ctx context.Context, p DevicePrincipal) context.Context {
	return context.WithValue(ctx, devicePrincipalContextKey{}, p)
}

// DevicePrincipalFromContext retrieves the principal DeviceAuthenticate
// resolved. ok is false when no device credential was verified on this
// request.
func DevicePrincipalFromContext(ctx context.Context) (DevicePrincipal, bool) {
	p, ok := ctx.Value(devicePrincipalContextKey{}).(DevicePrincipal)
	return p, ok
}

// DeviceAuthenticate returns middleware that resolves the caller's
// DevicePrincipal from the Authorization: Bearer <device_token> header and
// stores it in the request context. Unlike auth.Authenticate, there is no
// separate RequirePermission step — a verified device credential IS the
// authorization for the routes this middleware guards (ADR-017 §2: this is
// the gate for GET /sync/config, the one route carrying Argon2id password
// and PIN hashes, and it must stop accepting a human bearer token entirely,
// not merely prefer a device one).
func DeviceAuthenticate(verifier deviceVerifier) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			header := r.Header.Get("Authorization")
			token, ok := strings.CutPrefix(header, "Bearer ")
			if !ok || token == "" {
				httpx.Error(w, httpx.ErrUnauthorized)
				return
			}
			principal, err := verifier.VerifyToken(r.Context(), token)
			if err != nil {
				httpx.Error(w, httpx.ErrUnauthorized)
				return
			}
			ctx := WithDevicePrincipal(r.Context(), principal)
			next.ServeHTTP(w, r.WithContext(ctx))
		})
	}
}
