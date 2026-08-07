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

// Migrate applies every .sql file in contractsDir that has not been applied
// yet, each in its own transaction, in lexical filename order.
func Migrate(ctx context.Context, pool Pool, contractsDir string) error {
	if _, err := pool.Exec(ctx, migrationsTable); err != nil {
		return fmt.Errorf("postgres: creating migration table: %w", err)
	}

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

	for _, name := range files {
		var applied bool
		err := pool.QueryRow(ctx,
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

		tx, err := pool.Begin(ctx)
		if err != nil {
			return fmt.Errorf("postgres: begin for %s: %w", name, err)
		}
		if _, err := tx.Exec(ctx, string(body)); err != nil {
			_ = tx.Rollback(ctx)
			return fmt.Errorf("postgres: applying %s: %w", name, err)
		}
		if _, err := tx.Exec(ctx,
			`INSERT INTO schema_migration (filename) VALUES ($1)`, name,
		); err != nil {
			_ = tx.Rollback(ctx)
			return fmt.Errorf("postgres: recording %s: %w", name, err)
		}
		if err := tx.Commit(ctx); err != nil {
			return fmt.Errorf("postgres: commit for %s: %w", name, err)
		}
	}

	return nil
}
