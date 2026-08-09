// Command devseed prepares a local development database: it applies the
// frozen contract migrations to Postgres and seeds the minimum rows needed to
// exercise Milestone 1 (one tenant, brand, outlet, cashier and a small menu).
//
// DEVELOPMENT ONLY. This never runs at an outlet and is not part of the
// shipped POS. See docs/DEV_SETUP.md.
//
// It exists because cmd/api is still the Milestone 0 health-only entrypoint
// and never calls postgres.Migrate, so nothing else applies the contract
// schema to a developer's Docker Postgres.
//
// The entity ids below are fixed rather than freshly minted so that a re-run
// is idempotent and so the edge seeder (edge/database, bin devseed) can refer
// to the same outlet/device/user without a handshake. They are real UUIDv7
// values reserved for development; production ids are minted per §74.
package main

import (
	"context"
	"flag"
	"fmt"
	"log"
	"os"
	"time"

	"github.com/holler/backend/internal/platform/crypto"
	"github.com/holler/backend/internal/platform/postgres"
)

// Fixed development ids. MUST match the constants in
// edge/database/src/bin/devseed.rs — the two seeders describe the same outlet.
const (
	tenantID    = "0191a000-0000-7000-8000-000000000001"
	brandID     = "0191a000-0000-7000-8000-000000000002"
	outletID    = "0191a000-0000-7000-8000-00000000000a"
	deviceID    = "0191a000-0000-7000-8000-00000000000b"
	cashierID   = "0191a000-0000-7000-8000-00000000000c"
	roleID      = "0191a000-0000-7000-8000-00000000000d"
	userRoleID  = "0191a000-0000-7000-8000-00000000000e"
	categoryID  = "0191a000-0000-7000-8000-000000000010"
	itemChaiID  = "0191a000-0000-7000-8000-000000000011"
	itemThaliID = "0191a000-0000-7000-8000-000000000012"
	variantID   = "0191a000-0000-7000-8000-000000000013"
	modSmallID  = "0191a000-0000-7000-8000-000000000014"
	modLargeID  = "0191a000-0000-7000-8000-000000000015"
)

const (
	cashierEmail = "cashier@holler.test"
	// Development credential only; never a default anywhere else.
	cashierPassword = "holler123"
)

func main() {
	databaseURL := flag.String("database-url", envOr("DATABASE_URL",
		"postgres://holler:holler_dev@localhost:5432/holler?sslmode=disable"),
		"PostgreSQL connection string")
	contractsDir := flag.String("contracts", "../packages/contracts/postgres",
		"directory holding the frozen Postgres migrations")
	flag.Parse()

	ctx := context.Background()

	pool, err := postgres.Open(ctx, *databaseURL)
	if err != nil {
		log.Fatalf("devseed: %v", err)
	}
	defer pool.Close()

	if err := postgres.Migrate(ctx, pool, *contractsDir); err != nil {
		log.Fatalf("devseed: %v", err)
	}
	log.Printf("devseed: migrations applied from %s", *contractsDir)

	// One hash, generated here, used by both Postgres and the edge SQLite.
	// password.go is the single implementation of this format (ADR-011) and
	// edge/database/src/auth.rs verifies exactly what it produces.
	hash, err := crypto.HashPassword(cashierPassword)
	if err != nil {
		log.Fatalf("devseed: hashing password: %v", err)
	}

	if err := seed(ctx, pool, hash); err != nil {
		log.Fatalf("devseed: %v", err)
	}
	log.Printf("devseed: seeded tenant/brand/outlet/cashier/menu")

	// Machine-readable block: scripts/dev-bootstrap.ps1 parses these lines.
	// The hash is printed because the edge seeder needs the identical string;
	// it is a hash, not a credential, and this is a development-only path.
	fmt.Println("---HOLLER-DEVSEED---")
	fmt.Printf("HOLLER_TENANT_ID=%s\n", tenantID)
	fmt.Printf("HOLLER_OUTLET_ID=%s\n", outletID)
	fmt.Printf("HOLLER_DEVICE_ID=%s\n", deviceID)
	fmt.Printf("HOLLER_USER_ID=%s\n", cashierID)
	fmt.Printf("HOLLER_SEED_EMAIL=%s\n", cashierEmail)
	fmt.Printf("HOLLER_SEED_PASSWORD=%s\n", cashierPassword)
	fmt.Printf("HOLLER_SEED_PASSWORD_HASH=%s\n", hash)
	fmt.Println("---END---")
}

func envOr(key, fallback string) string {
	if v := os.Getenv(key); v != "" {
		return v
	}
	return fallback
}

// seed writes the Milestone 1 fixture rows. Every statement is an upsert so
// the command can be re-run against an already-seeded database.
func seed(ctx context.Context, pool postgres.Pool, passwordHash string) error {
	now := time.Now().UTC()

	stmts := []struct {
		label string
		sql   string
		args  []any
	}{
		{"tenant", `INSERT INTO tenant (id, name, created_at, updated_at)
			VALUES ($1, $2, $3, $3)
			ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, updated_at = EXCLUDED.updated_at`,
			[]any{tenantID, "Holler Dev Restaurant", now}},

		{"brand", `INSERT INTO brand (id, tenant_id, name, created_at, updated_at)
			VALUES ($1, $2, $3, $4, $4)
			ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, updated_at = EXCLUDED.updated_at`,
			[]any{brandID, tenantID, "Holler Dev Brand", now}},

		{"outlet", `INSERT INTO outlet (id, brand_id, name, timezone, config_version, created_at, updated_at)
			VALUES ($1, $2, $3, $4, 1, $5, $5)
			ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, updated_at = EXCLUDED.updated_at`,
			[]any{outletID, brandID, "Pune Test Outlet", "Asia/Kolkata", now}},

		{"role", `INSERT INTO role (id, tenant_id, code, name, created_at, updated_at)
			VALUES ($1, $2, 'CASHIER', 'Cashier', $3, $3)
			ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name, updated_at = EXCLUDED.updated_at`,
			[]any{roleID, tenantID, now}},

		{"app_user", `INSERT INTO app_user
			(id, tenant_id, email, full_name, password_hash, is_active, config_version, created_at, updated_at)
			VALUES ($1, $2, $3, $4, $5, TRUE, 1, $6, $6)
			ON CONFLICT (id) DO UPDATE SET
				password_hash = EXCLUDED.password_hash,
				is_active = EXCLUDED.is_active,
				updated_at = EXCLUDED.updated_at`,
			[]any{cashierID, tenantID, cashierEmail, "Dev Cashier", passwordHash, now}},

		{"menu_category", `INSERT INTO menu_category (id, outlet_id, name, sort_order, config_version)
			VALUES ($1, $2, 'Beverages', 1, 1)
			ON CONFLICT (id) DO UPDATE SET name = EXCLUDED.name`,
			[]any{categoryID, outletID}},

		{"menu_item chai", `INSERT INTO menu_item (id, outlet_id, category_id, name, base_price_paise, is_available, config_version)
			VALUES ($1, $2, $3, 'Masala Chai', 4000, TRUE, 1)
			ON CONFLICT (id) DO UPDATE SET base_price_paise = EXCLUDED.base_price_paise`,
			[]any{itemChaiID, outletID, categoryID}},

		{"menu_item thali", `INSERT INTO menu_item (id, outlet_id, category_id, name, base_price_paise, is_available, config_version)
			VALUES ($1, $2, $3, 'Veg Thali', 22000, TRUE, 1)
			ON CONFLICT (id) DO UPDATE SET base_price_paise = EXCLUDED.base_price_paise`,
			[]any{itemThaliID, outletID, categoryID}},

		{"variant", `INSERT INTO menu_item_variant (id, menu_item_id, name, price_delta_paise, config_version)
			VALUES ($1, $2, 'Large', 1500, 1)
			ON CONFLICT (id) DO UPDATE SET price_delta_paise = EXCLUDED.price_delta_paise`,
			[]any{variantID, itemChaiID}},

		// One modifier group ("Sugar") with two options, per the bootstrap
		// requirement of at least one modifier group.
		{"modifier less-sugar", `INSERT INTO menu_item_modifier
			(id, menu_item_id, group_name, option_name, price_delta_paise, min_selection, max_selection, config_version)
			VALUES ($1, $2, 'Sugar', 'Less Sugar', 0, 0, 1, 1)
			ON CONFLICT (id) DO UPDATE SET option_name = EXCLUDED.option_name`,
			[]any{modSmallID, itemChaiID}},

		{"modifier extra-sugar", `INSERT INTO menu_item_modifier
			(id, menu_item_id, group_name, option_name, price_delta_paise, min_selection, max_selection, config_version)
			VALUES ($1, $2, 'Sugar', 'Extra Sugar', 500, 0, 1, 1)
			ON CONFLICT (id) DO UPDATE SET option_name = EXCLUDED.option_name`,
			[]any{modLargeID, itemChaiID}},
	}

	for _, s := range stmts {
		if _, err := pool.Exec(ctx, s.sql, s.args...); err != nil {
			return fmt.Errorf("seeding %s: %w", s.label, err)
		}
	}

	// role_permission is a composite-key table with no single id to conflict on.
	for _, p := range []string{"order.create", "order.modify", "table.manage"} {
		if _, err := pool.Exec(ctx,
			`INSERT INTO role_permission (role_id, permission) VALUES ($1, $2)
			 ON CONFLICT DO NOTHING`, roleID, p); err != nil {
			return fmt.Errorf("seeding role_permission %s: %w", p, err)
		}
	}

	// The cashier's role assignment is scoped to the seeded outlet.
	if _, err := pool.Exec(ctx,
		`INSERT INTO user_role (id, user_id, role_id, outlet_id, created_at)
		 VALUES ($1, $2, $3, $4, $5)
		 ON CONFLICT (id) DO NOTHING`,
		userRoleID, cashierID, roleID, outletID, now); err != nil {
		return fmt.Errorf("seeding user_role: %w", err)
	}

	return nil
}
