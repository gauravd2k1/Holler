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
	"github.com/holler/backend/internal/compliance"
	"github.com/holler/backend/internal/health"
	"github.com/holler/backend/internal/kitchen"
	"github.com/holler/backend/internal/menu"
	"github.com/holler/backend/internal/ordering"
	"github.com/holler/backend/internal/outlet"
	"github.com/holler/backend/internal/payments"
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

	// --- device enrollment (ADR-017, T1) ---------------------------------
	// outletRepo also implements outlet.DeviceRepository (device_postgres.go)
	// — one Postgres pool, two concerns sharing a type, exactly like
	// outlet.Repository/DeviceRepository split by file.
	deviceSvc := outlet.NewDeviceService(outletRepo, outletRepo, auditor)
	deviceHandler := outlet.NewDeviceHandler(deviceSvc)

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

	// --- payments (ADR-016; T8) ---------------------------------------------
	// invoice, payment and cash_shift are edge-authoritative replay-only
	// aggregates with no human-authored write path at all (unlike
	// ordering/kitchen/tables, there is nothing here to split into a
	// human-auth Mount — every route paymentsHandler.Mount registers belongs
	// in the device-authenticated group below).
	paymentsRepo := payments.NewPostgresRepository(pool)
	paymentsSvc := payments.NewService(paymentsRepo)
	paymentsHandler := payments.NewHandler(paymentsSvc)

	// --- compliance (ADR-016; T13) ------------------------------------------
	// compliance_version, tax_profile (+ its tax_rule children),
	// invoice_series, discount_definition and outlet_fiscal_profile are
	// CLOUD_TO_EDGE config: the cloud is where these get written, unlike
	// payments' aggregates above. Every route complianceHandler.Mount
	// registers is HUMAN-authenticated (management decisions), so it belongs
	// in the human-auth group, not the device-authenticated one.
	complianceRepo := compliance.NewRepository(pool)
	complianceSvc := compliance.NewService(complianceRepo)
	complianceHandler := compliance.NewHandler(complianceSvc)

	// --- composite GET /sync/config ---------------------------------------
	syncConfig := newSyncConfigHandler(outletSvc, menuSvc, tablesSvc, kitchenSvc, complianceSvc, authSvc, deviceSvc)

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
	//
	// This group carries HUMAN-authenticated routes only: config
	// (tables/stations/printers/menu/outlet) and reads (GET orders,
	// GET table-sessions). It deliberately excludes every edge->cloud
	// ingest write — those are mounted below under
	// outlet.DeviceAuthenticate (ADR-017's 0.4.3 amendment).
	router.Group(func(r chi.Router) {
		r.Use(auth.Authenticate(tokens))
		r.Use(bridgeDownstreamPrincipals)

		outletHandler.Mount(r)
		menuHandlers.Mount(r)
		tablesHandlers.Mount(r)
		orderingHandler.Mount(r)
		kitchenHandler.Mount(r)
		complianceHandler.Mount(r)

		// Device enrollment/rotation/revocation are human-privileged
		// management actions (a technician or manager acting through an
		// authenticated session), gated on outlet.manage — the closest
		// existing permission to "may register hardware at this outlet".
		r.With(auth.RequirePermission(auth.PermissionOutletManage)).Group(func(r chi.Router) {
			deviceHandler.Mount(r)
		})
	})

	// /sync/config, and every edge->cloud INGEST route, serve an enrolled
	// edge node, not a browser session (ADR-017 §2, and the 0.4.3
	// amendment extending that rule to order/table_session/kot/invoice/
	// payment/cash_shift ingest). outlet.DeviceAuthenticate is the ONLY
	// middleware guarding this group, resolving tenant_id/outlet_id from
	// the verified device_credential row rather than from anything the
	// caller supplied — an envelope's own tenant_id/outlet_id claims are
	// checked against that resolved identity downstream, never trusted on
	// their own. A request carrying a valid human access token but no
	// device credential gets the same 401 as one carrying nothing at all —
	// that break is intentional (ADR-017 "Consequences": "Any existing
	// caller relying on [a human bearer token here] is broken deliberately;
	// it was the hole.").
	router.Group(func(r chi.Router) {
		r.Use(outlet.DeviceAuthenticate(deviceSvc))

		r.Get("/sync/config", syncConfig.ServeHTTP)

		orderingHandler.MountIngest(r)
		kitchenHandler.MountIngest(r)
		tablesHandlers.MountEnvelopeIngest(r)
		paymentsHandler.Mount(r)
	})

	return router
}
