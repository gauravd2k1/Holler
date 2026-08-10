// Command api is the Holler Cloud backend entrypoint: the composition root
// that constructs every bounded context's repository -> service -> handlers
// from configuration and mounts them on one router (CLAUDE.md "Directory
// ownership").
package main

import (
	"context"
	"log"
	"log/slog"
	"net/http"
	"os"
	"os/signal"
	"syscall"
	"time"

	"github.com/go-chi/chi/v5"

	"github.com/holler/backend/internal/auth"
	"github.com/holler/backend/internal/health"
	"github.com/holler/backend/internal/kitchen"
	"github.com/holler/backend/internal/menu"
	"github.com/holler/backend/internal/ordering"
	"github.com/holler/backend/internal/outlet"
	"github.com/holler/backend/internal/platform/config"
	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/platform/postgres"
	"github.com/holler/backend/internal/tables"
	"github.com/holler/backend/internal/tenant"
)

func main() {
	cfg, err := config.Load()
	if err != nil {
		log.Fatalf("holler api: %v", err)
	}

	ctx := context.Background()

	pool, err := postgres.Open(ctx, cfg.DatabaseURL)
	if err != nil {
		log.Fatalf("holler api: %v", err)
	}
	defer pool.Close()

	// The authoritative schema is packages/contracts/postgres/*.sql
	// (ADR-008); this is the one place that applies it.
	if err := postgres.Migrate(ctx, pool, cfg.ContractsDir); err != nil {
		log.Fatalf("holler api: applying migrations from %s: %v", cfg.ContractsDir, err)
	}

	router := buildRouter(pool, cfg)

	server := &http.Server{
		Addr:              ":" + cfg.Port,
		Handler:           router,
		ReadHeaderTimeout: 10 * time.Second,
	}

	go func() {
		slog.Info("holler backend listening", "port", cfg.Port)
		if err := server.ListenAndServe(); err != nil && err != http.ErrServerClosed {
			log.Fatalf("holler api: server failed: %v", err)
		}
	}()

	stop := make(chan os.Signal, 1)
	signal.Notify(stop, os.Interrupt, syscall.SIGTERM)
	<-stop

	shutdownCtx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	if err := server.Shutdown(shutdownCtx); err != nil {
		slog.Error("holler api: graceful shutdown failed", "error", err)
	}
}

// buildRouter constructs every bounded context (repository -> service ->
// handlers) and mounts it on the shared router. Contexts wired: auth,
// tenant, outlet, menu, tables, ordering, kitchen, health, plus the
// composite GET /sync/config route (T7).
func buildRouter(pool postgres.Pool, cfg config.Config) *chi.Mux {
	// --- auth ---------------------------------------------------------
	authRepo := auth.NewRepository(pool)
	tokens := auth.NewTokenSigner(cfg.TokenSigningKey)
	refreshStore := auth.NewPostgresRefreshStore(pool)
	limiter := auth.NewInMemoryRateLimiter()
	auditor := auth.NewAuditor(authRepo)
	authSvc := auth.NewService(authRepo, tokens, refreshStore, limiter, auditor, cfg.AccessTokenTTL, cfg.RefreshTokenTTL)
	authHandlers := auth.NewHandlers(authSvc, tokens)

	// --- tenant ---------------------------------------------------------
	// tenant.Service is constructed for parity with every other context
	// (CLAUDE.md directory ownership lists it as one of Milestone 1's
	// bounded contexts) but packages/contracts/openapi/openapi.yaml defines
	// no HTTP route for it — organisation/brand creation is an operator-side
	// concern (see backend/cmd/devseed), not an M1 API surface. There is
	// nothing to Mount.
	tenantRepo := tenant.NewPostgresRepository(pool)
	_ = tenant.NewService(tenantRepo)

	// --- outlet ---------------------------------------------------------
	outletRepo := outlet.NewPostgresRepository(pool)
	outletSvc := outlet.NewService(outletRepo)
	outletHandler := outlet.NewHandler(outletSvc)

	// --- kitchen (constructed before menu: menu.NewHandlers takes
	// kitchen.Service as its StationRouter per ADR-014's task split) -------
	kitchenRepo := kitchen.NewRepository(pool)
	kitchenSvc := kitchen.NewService(kitchenRepo, auditor)
	kitchenHandler := kitchen.NewHandler(kitchenSvc)

	// --- menu -------------------------------------------------------------
	menuRepo := menu.NewRepository(pool)
	menuSvc := menu.NewService(menuRepo)
	menuHandlers := menu.NewHandlers(menuSvc, kitchenSvc)

	// --- tables -----------------------------------------------------------
	tablesRepo := tables.NewRepository(pool)
	tablesSvc := tables.NewService(tablesRepo)
	tablesHandlers := tables.NewHandlers(tablesSvc)

	// --- ordering -----------------------------------------------------------
	orderingRepo := ordering.NewPostgresRepository(pool)
	orderingSvc := ordering.NewService(orderingRepo)
	orderingHandler := ordering.NewHandler(orderingSvc)

	// --- composite GET /sync/config ---------------------------------------
	syncConfig := newSyncConfigHandler(outletSvc, menuSvc, tablesSvc, kitchenSvc)

	router := httpx.NewRouter()
	router.Get("/health", health.Handler)

	// POST /auth/login|refresh|logout are unauthenticated by definition; the
	// GET/POST /users and GET /roles routes auth.Handlers.Mount also
	// registers apply auth.Authenticate + RequirePermission themselves
	// (backend/internal/auth/handlers.go Mount), so this package mounts them
	// directly rather than inside the group below.
	authHandlers.Mount(router)

	// Every other context's Mount assumes a principal is already resolved in
	// the request context (auth.PrincipalFromContext /
	// outlet|menu|tables.PrincipalFromContext) — this group is the ONE place
	// that resolves it, per ADR-012's tenant-resolution rule and the auth
	// package's own Authenticate/RequirePermission split.
	router.Group(func(r chi.Router) {
		r.Use(auth.Authenticate(tokens))
		r.Use(bridgeDownstreamPrincipals)

		outletHandler.Mount(r)
		menuHandlers.Mount(r)
		tablesHandlers.Mount(r)
		orderingHandler.Mount(r)
		kitchenHandler.Mount(r)

		// /sync/config serves an enrolled edge node, not a browser session.
		// FINDING (T7): this codebase has no device/edge-enrollment
		// authentication mechanism at all — no device_token, no edge
		// certificate, nothing distinct from a user's bearer access token
		// (grep across backend/ and docs/adr/ turns up nothing named
		// device/enroll/edge auth beyond the SyncEnvelope's plain device_id
		// field, which is unauthenticated metadata, not a credential). This
		// route is therefore gated on the SAME bearer-token principal every
		// other authenticated route uses, requiring user.manage: the closest
		// existing permission, chosen because this route is the one place
		// credential hashes are meant to cross the wire (ADR-011) and
		// /users already requires user.manage to view account data without
		// hashes. A real edge-enrollment credential (e.g. a long-lived
		// per-device certificate or token, checked independently of a human
		// login) is still needed before this route can be honestly described
		// as authenticating "an enrolled edge node" rather than "an already
		// logged-in human's browser session" — see this task's final report.
		r.With(auth.RequirePermission(auth.PermissionUserManage)).Get("/sync/config", syncConfig.ServeHTTP)
	})

	return router
}
