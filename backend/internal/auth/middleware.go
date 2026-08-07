package auth

import (
	"context"
	"net/http"
	"strings"

	"github.com/holler/backend/internal/platform/httpx"
)

type principalContextKey struct{}

// WithPrincipal returns a context carrying p. Exported so tests and other
// middleware chains can inject a principal directly.
func WithPrincipal(ctx context.Context, p AuthenticatedPrincipal) context.Context {
	return context.WithValue(ctx, principalContextKey{}, p)
}

// PrincipalFromContext retrieves the principal stashed by the RBAC
// middleware (or WithPrincipal in tests). Other bounded contexts call this
// to learn who is making the current request.
func PrincipalFromContext(ctx context.Context) (AuthenticatedPrincipal, bool) {
	p, ok := ctx.Value(principalContextKey{}).(AuthenticatedPrincipal)
	return p, ok
}

// Authenticate returns middleware that resolves the caller's
// AuthenticatedPrincipal from the Authorization: Bearer <access_token>
// header and stores it in the request context. It does not itself enforce
// any permission — pair it with RequirePermission per route.
func Authenticate(tokens *TokenSigner) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			header := r.Header.Get("Authorization")
			token, ok := strings.CutPrefix(header, "Bearer ")
			if !ok || token == "" {
				httpx.Error(w, httpx.ErrUnauthorized)
				return
			}
			principal, err := tokens.VerifyAccessToken(token)
			if err != nil {
				httpx.Error(w, httpx.ErrUnauthorized)
				return
			}
			ctx := WithPrincipal(r.Context(), principal)
			next.ServeHTTP(w, r.WithContext(ctx))
		})
	}
}

// RequirePermission returns middleware enforcing that the request's
// principal (already resolved by Authenticate) holds permission. It must be
// mounted after Authenticate.
func RequirePermission(permission Permission) func(http.Handler) http.Handler {
	return func(next http.Handler) http.Handler {
		return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
			principal, ok := PrincipalFromContext(r.Context())
			if !ok {
				httpx.Error(w, httpx.ErrUnauthorized)
				return
			}
			if !hasPermission(principal, string(permission)) {
				httpx.Error(w, httpx.ErrForbidden)
				return
			}
			next.ServeHTTP(w, r)
		})
	}
}
