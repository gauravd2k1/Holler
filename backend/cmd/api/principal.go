package main

import (
	"net/http"

	"github.com/holler/backend/internal/auth"
	"github.com/holler/backend/internal/menu"
	"github.com/holler/backend/internal/outlet"
	"github.com/holler/backend/internal/tables"
)

// bridgeDownstreamPrincipals re-publishes the auth.AuthenticatedPrincipal
// that auth.Authenticate already resolved into the context shapes
// backend/internal/outlet, backend/internal/menu and backend/internal/tables
// each define for themselves (per their own package docs: "Authentication
// itself belongs to backend/internal/auth; this package never imports that
// package"). Composing those three independently-built contexts onto one
// router is exactly the wiring CLAUDE.md assigns to the composition root, not
// to any one context.
//
// backend/internal/ordering and backend/internal/kitchen read
// auth.PrincipalFromContext directly and need no bridging.
//
// Must run after auth.Authenticate in the middleware chain. If no principal
// is present yet (an unauthenticated request slipping through, or a route
// mounted outside the authenticated group by mistake) this is a no-op —
// downstream permission checks in each context already fail closed
// (PrincipalFromContext ok=false -> httpx.ErrUnauthorized) so there is
// nothing to enforce here.
func bridgeDownstreamPrincipals(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		p, ok := auth.PrincipalFromContext(r.Context())
		if !ok {
			next.ServeHTTP(w, r)
			return
		}

		ctx := r.Context()
		ctx = outlet.WithPrincipal(ctx, outlet.Principal{UserID: p.UserID, TenantID: p.TenantID})

		wrapped := auth.NewPrincipal(p)
		ctx = menu.WithPrincipal(ctx, wrapped)
		ctx = tables.WithPrincipal(ctx, wrapped)

		next.ServeHTTP(w, r.WithContext(ctx))
	})
}
