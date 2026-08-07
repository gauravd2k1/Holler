package outlet

import "context"

// Principal is the minimal authenticated-caller shape this package needs.
// Authentication itself belongs to backend/internal/auth (T2); this package
// never imports that package. Whatever wires the HTTP router is responsible
// for calling WithPrincipal on the request context once a token is
// verified, using the AuthenticatedPrincipal contract in
// packages/contracts/openapi/openapi.yaml as the source of truth for the
// fields.
type Principal struct {
	UserID   string
	TenantID string
}

type principalCtxKey struct{}

// WithPrincipal attaches an authenticated principal to ctx.
func WithPrincipal(ctx context.Context, p Principal) context.Context {
	return context.WithValue(ctx, principalCtxKey{}, p)
}

// PrincipalFromContext reads the principal attached by WithPrincipal. ok is
// false when no principal is present, which handlers must treat as
// httpx.ErrUnauthorized, never as an anonymous/tenant-less request.
func PrincipalFromContext(ctx context.Context) (Principal, bool) {
	p, ok := ctx.Value(principalCtxKey{}).(Principal)
	return p, ok
}
