// Package postgres owns the connection pool and the migration runner. The
// authoritative schema is packages/contracts/postgres/*.sql (ADR-008) — the
// runner applies those files in lexical order and records what it applied.
// Backend code never defines schema of its own.
package postgres

import (
	"context"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/jackc/pgx/v5/pgxpool"
)

// Pool is the interface database code depends on, so repositories can be
// tested against a fake without a live server.
type Pool = *pgxpool.Pool

func Open(ctx context.Context, databaseURL string) (Pool, error) {
	pool, err := pgxpool.New(ctx, databaseURL)
	if err != nil {
		return nil, fmt.Errorf("postgres: opening pool: %w", err)
	}
	if err := pool.Ping(ctx); err != nil {
		pool.Close()
		return nil, fmt.Errorf("postgres: ping: %w", err)
	}
	return pool, nil
}

const migrationsTable = `
CREATE TABLE IF NOT EXISTS schema_migration (
    filename    TEXT PRIMARY KEY,
    applied_at  TIMESTAMPTZ NOT NULL DEFAULT now()
)`

// migrationLockKey is the fixed advisory-lock key Migrate holds while it
// checks and applies contract migrations, so concurrent callers (every
// Postgres-backed package's test suite, in parallel, against the same
// database) serialize instead of racing to create the same tables. The
// value is arbitrary but must stay stable and unique to this lock's
// purpose within the database.
const migrationLockKey = 891_427_001

// Migrate applies every .sql file in contractsDir that has not been applied
// yet, in lexical filename order, inside a single transaction guarded by a
// Postgres advisory lock (pg_advisory_xact_lock). The lock is
// transaction-scoped: it is released automatically on commit or rollback,
// including on every error path, so a failed migration cannot wedge later
// callers. Concurrent callers block on the lock rather than racing the
// schema_migration ledger; against an already-migrated database the lock
// is held only long enough to run the (now all-skip) check loop, so the
// common case stays effectively free.
func Migrate(ctx context.Context, pool Pool, contractsDir string) error {
	entries, err := os.ReadDir(contractsDir)
	if err != nil {
		return fmt.Errorf("postgres: reading %s: %w", contractsDir, err)
	}

	var files []string
	for _, e := range entries {
		if !e.IsDir() && strings.HasSuffix(e.Name(), ".sql") {
			files = append(files, e.Name())
		}
	}
	sort.Strings(files)

	conn, err := pool.Acquire(ctx)
	if err != nil {
		return fmt.Errorf("postgres: acquiring connection for migrate: %w", err)
	}
	defer conn.Release()

	tx, err := conn.Begin(ctx)
	if err != nil {
		return fmt.Errorf("postgres: begin migrate transaction: %w", err)
	}
	defer func() { _ = tx.Rollback(ctx) }()

	if _, err := tx.Exec(ctx, `SELECT pg_advisory_xact_lock($1)`, migrationLockKey); err != nil {
		return fmt.Errorf("postgres: acquiring migration advisory lock: %w", err)
	}

	if _, err := tx.Exec(ctx, migrationsTable); err != nil {
		return fmt.Errorf("postgres: creating migration table: %w", err)
	}

	for _, name := range files {
		var applied bool
		err := tx.QueryRow(ctx,
			`SELECT EXISTS (SELECT 1 FROM schema_migration WHERE filename = $1)`, name,
		).Scan(&applied)
		if err != nil {
			return fmt.Errorf("postgres: checking migration %s: %w", name, err)
		}
		if applied {
			continue
		}

		body, err := os.ReadFile(filepath.Join(contractsDir, name))
		if err != nil {
			return fmt.Errorf("postgres: reading migration %s: %w", name, err)
		}

		if _, err := tx.Exec(ctx, string(body)); err != nil {
			return fmt.Errorf("postgres: applying %s: %w", name, err)
		}
		if _, err := tx.Exec(ctx,
			`INSERT INTO schema_migration (filename) VALUES ($1)`, name,
		); err != nil {
			return fmt.Errorf("postgres: recording %s: %w", name, err)
		}
	}

	if err := tx.Commit(ctx); err != nil {
		return fmt.Errorf("postgres: commit migrate transaction: %w", err)
	}

	return nil
}
