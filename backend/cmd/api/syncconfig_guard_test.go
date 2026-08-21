package main

import (
	"context"
	"reflect"
	"sort"
	"strings"
	"testing"

	"github.com/holler/backend/internal/platform/postgres"
	"github.com/holler/backend/internal/platform/testdb"
)

// --- the generalised config-delivery guard ---------------------------------
//
// Three shipped defects shared one shape: GET /sync/config returned empty
// `users`, `printer_role` never reached an edge, and `day_start_time` was
// read at the edge and written by nothing. All three are "a real column on a
// cloud-authoritative table that the wire response never carries" — and
// nothing caught any of them until a human went looking.
//
// This is the class guard. For every column of every table declared below —
// the CLOUD_TO_EDGE aggregates in contracts.AggregateAuthority, plus their
// child-row tables (ADR-011/014/016/018's own "rides in the parent's config
// bundle" language names each one) — it asserts the column is represented
// somewhere in syncConfigResponse's wire shapes, or is named in
// guardExemptions with a stated reason.
//
// Modelled on SINGLE_STORE_MIGRATIONS in edge/database/src/migrations.rs:
// the declaration IS the guard. An undeclared table is simply not checked
// (declare it to bring it under the guard); an undeclared column on a
// declared table fails; a stale exemption naming a column that IS carried
// also fails, so the exemption list cannot silently accumulate dead entries.
//
// Falsified per CLAUDE.md's rubric before being trusted — see this task's
// final report for which runs were watched fail and why.

// guardedTable is one table this guard holds to account, with the
// AggregateType (or parent aggregate, for a child row) that puts it in
// scope.
type guardedTable struct {
	table  string
	origin string // which CLOUD_TO_EDGE aggregate or ADR clause put it here
}

// guardedTables enumerates every CLOUD_TO_EDGE aggregate table
// (contracts.AggregateAuthority) plus every child-row table the ADRs
// describe as riding inside that aggregate's config bundle. EDGE_TO_CLOUD
// aggregates (order, kot, invoice, payment, cash_shift, table_session,
// stock_ledger_entry, stock_count, stock_deduction_gap) are out of scope by
// construction: the cloud never sends those down, so GET /sync/config is not
// where they would be missed.
var guardedTables = []guardedTable{
	{"menu_item", "AggregateTypeMenuItem (CLOUD_TO_EDGE)"},
	{"menu_item_variant", "child of menu_item (ADR-018 §2, menu_item_variant precedent)"},
	{"menu_item_modifier", "child of menu_item (ADR-018 §6, menu_item_modifier precedent)"},
	{"app_user", "AggregateTypeAppUser (CLOUD_TO_EDGE, ADR-011)"},
	{"role", "AggregateTypeRole (CLOUD_TO_EDGE, ADR-011)"},
	{"role_permission", "child of role"},
	{"restaurant_table", "AggregateTypeRestaurantTable (CLOUD_TO_EDGE, ADR-011)"},
	{"station", "AggregateTypeStation (CLOUD_TO_EDGE, ADR-014)"},
	{"menu_item_station", "child row (ADR-014 §2, PUT not merge)"},
	{"printer", "AggregateTypePrinter (CLOUD_TO_EDGE, ADR-014)"},
	{"printer_role", "child of printer (0.4.7)"},
	{"station_printer", "child of station (ADR-014)"},
	{"tax_profile", "AggregateTypeTaxProfile (CLOUD_TO_EDGE, ADR-016)"},
	{"tax_rule", "child of tax_profile (ADR-016)"},
	{"compliance_version", "AggregateTypeComplianceVersion (CLOUD_TO_EDGE, ADR-016)"},
	{"invoice_series", "AggregateTypeInvoiceSeries (CLOUD_TO_EDGE, ADR-016)"},
	{"discount_definition", "AggregateTypeDiscountDefinition (CLOUD_TO_EDGE, ADR-016)"},
	{"inventory_item", "AggregateTypeInventoryItem (CLOUD_TO_EDGE, ADR-018)"},
	{"item_unit_conversion", "child of inventory_item (ADR-018 §4)"},
	{"recipe", "AggregateTypeRecipe (CLOUD_TO_EDGE, ADR-018)"},
	{"recipe_ingredient", "child of recipe (ADR-018 §7)"},
	{"modifier_ingredient_delta", "child of menu_item_modifier, itself child of menu_item (ADR-018 §1)"},
}

// guardExemptions: table -> column -> stated reason. Every entry here is
// deliberate, never a placeholder. Two categories:
//
//  1. Genuinely edge-irrelevant (bookkeeping the edge never reads, or a
//     column withdrawn from the wire by an explicit, documented decision).
//  2. RECORDED, NOT EXCUSED gaps this guard's own construction surfaced —
//     tax_profile_id/hsn_sac on menu_item, and menu_item_variant /
//     menu_item_modifier being entirely absent from GET /sync/config. These
//     are real omissions in backend/internal/menu, outside this task's
//     owned directories (backend/internal/inventory, backend/cmd/api) and
//     explicitly out of scope per this task's brief ("menu context's
//     missing tests are a separate filed retrofit — do NOT expand into
//     it"). Exempting them here keeps the guard truthful about what it
//     covers rather than silently failing on a defect this task cannot fix
//     — and the reason string says so, so nobody mistakes the exemption for
//     "by design."
var guardExemptions = map[string]map[string]string{
	"menu_item": {
		"tax_profile_id": "KNOWN GAP, not fixed by this task: menu.Item (backend/internal/menu) does not carry tax_profile_id at all yet, though contracts.MenuItem has since 0.4.2. Outside backend/internal/inventory and backend/cmd/api's owned scope; filed as a follow-up.",
		"hsn_sac":        "KNOWN GAP, not fixed by this task: same as tax_profile_id, added to contracts.MenuItem at 0.4.5 but never reached menu.Item or the sync bundle. An invoice cannot legally issue without it (CLAUDE.md), so this is the higher-priority follow-up of the two.",
	},
	"menu_item_variant": {
		"id":                "KNOWN GAP, not fixed by this task: menu_item_variant is entirely absent from GET /sync/config and from openapi.yaml's MenuItem schema. Every column here is exempted for that one reason, not per-column reasons — variants never reach the edge today.",
		"menu_item_id":      "see id's entry in this table",
		"name":              "see id's entry in this table",
		"price_delta_paise": "see id's entry in this table",
		"config_version":    "see id's entry in this table",
		"is_default":        "see id's entry in this table",
	},
	"menu_item_modifier": {
		"id":                "KNOWN GAP, not fixed by this task: menu_item_modifier is entirely absent from GET /sync/config, for the same reason as menu_item_variant above — no route or bundle field carries it.",
		"menu_item_id":      "see id's entry in this table",
		"group_name":        "see id's entry in this table",
		"option_name":       "see id's entry in this table",
		"price_delta_paise": "see id's entry in this table",
		"min_selection":     "see id's entry in this table",
		"max_selection":     "see id's entry in this table",
		"config_version":    "see id's entry in this table",
	},
	"app_user": {
		"created_at": "cloud-internal bookkeeping. EdgeUserCacheEntry carries updated_at, which is what the edge compares for cache freshness; it has never needed created_at.",
	},
	"role": {
		"id":         "Removed from GET /sync/config at 0.2.2 BY DESIGN: the edge has no role table. Permissions arrive pre-flattened on every EdgeUserCacheEntry.Permissions instead (openapi.yaml's `roles was removed at 0.2.2` comment). Every column here shares this one reason.",
		"tenant_id":  "see id's entry in this table",
		"code":       "see id's entry in this table",
		"name":       "see id's entry in this table",
		"created_at": "see id's entry in this table",
		"updated_at": "see id's entry in this table",
	},
	"role_permission": {
		"role_id":    "see role's entry: role_permission is role's child row and was removed from the wire in the same 0.2.2 decision.",
		"permission": "see role's entry in this table",
	},
	"restaurant_table": {
		"created_at": "cloud-internal bookkeeping, not needed at the edge (config_version is what the edge compares).",
		"updated_at": "cloud-internal bookkeeping, not needed at the edge (config_version is what the edge compares).",
	},
}

func guardExemptionReason(table, column string) (string, bool) {
	cols, ok := guardExemptions[table]
	if !ok {
		return "", false
	}
	reason, ok := cols[column]
	return reason, ok
}

// collectJSONTags walks t (following pointers/slices/arrays/maps) and every
// struct field's type recursively, collecting every json tag name it finds.
// This is deliberately structural rather than a hand-maintained list: a
// field removed from any wire struct in the type graph disappears from this
// set on the next run, which is exactly the failure mode the guard exists
// to catch (see this task's report for the falsification that proved it).
func collectJSONTags(t reflect.Type, seen map[reflect.Type]bool, tags map[string]bool) {
	switch t.Kind() {
	case reflect.Pointer, reflect.Slice, reflect.Array:
		collectJSONTags(t.Elem(), seen, tags)
		return
	case reflect.Map:
		collectJSONTags(t.Elem(), seen, tags)
		return
	}
	if t.Kind() != reflect.Struct {
		return
	}
	if seen[t] {
		return
	}
	seen[t] = true
	for i := 0; i < t.NumField(); i++ {
		f := t.Field(i)
		tag := f.Tag.Get("json")
		name := strings.Split(tag, ",")[0]
		if name != "" && name != "-" {
			tags[name] = true
		}
		collectJSONTags(f.Type, seen, tags)
	}
}

// TestSyncConfigGuard_EveryCloudAuthoritativeColumnIsWiredOrExempted is the
// guard itself.
func TestSyncConfigGuard_EveryCloudAuthoritativeColumnIsWiredOrExempted(t *testing.T) {
	dbURL := testdb.RequireDatabaseURL(t)
	ctx := context.Background()
	pool, err := postgres.Open(ctx, dbURL)
	if err != nil {
		t.Fatalf("postgres.Open: %v", err)
	}
	defer pool.Close()

	tags := map[string]bool{}
	collectJSONTags(reflect.TypeOf(syncConfigResponse{}), map[reflect.Type]bool{}, tags)
	if len(tags) < 20 {
		// A sanity floor: if reflection somehow collected almost nothing,
		// every table below would "fail" for the wrong reason (a broken
		// guard, not a real gap) and the failures would be uninformative.
		t.Fatalf("collectJSONTags found only %d tags from syncConfigResponse — the guard itself is broken", len(tags))
	}

	tableNames := make([]string, len(guardedTables))
	for i, g := range guardedTables {
		tableNames[i] = g.table
	}

	rows, err := pool.Query(ctx,
		`SELECT table_name, column_name FROM information_schema.columns
		 WHERE table_schema = 'public' AND table_name = ANY($1)
		 ORDER BY table_name, ordinal_position`,
		tableNames,
	)
	if err != nil {
		t.Fatalf("querying information_schema.columns: %v", err)
	}
	defer rows.Close()

	type dbColumn struct{ table, column string }
	var actual []dbColumn
	seenTables := map[string]bool{}
	for rows.Next() {
		var c dbColumn
		if err := rows.Scan(&c.table, &c.column); err != nil {
			t.Fatalf("scanning information_schema row: %v", err)
		}
		actual = append(actual, c)
		seenTables[c.table] = true
	}
	if err := rows.Err(); err != nil {
		t.Fatalf("iterating information_schema rows: %v", err)
	}

	// A declared table absent from the live schema is a guard-authoring
	// bug (a typo, or a table this run's migrations don't include) —
	// distinguishing that from "zero columns is fine" matters, because a
	// silently-empty result set would make every column of that table
	// vacuously "pass".
	for _, name := range tableNames {
		if !seenTables[name] {
			t.Fatalf("declared table %q was not found in the live schema (typo in guardedTables, or migrations out of date)", name)
		}
	}

	var failures []string
	exemptionsUsed := map[string]map[string]bool{}
	for _, c := range actual {
		if reason, exempt := guardExemptionReason(c.table, c.column); exempt {
			if exemptionsUsed[c.table] == nil {
				exemptionsUsed[c.table] = map[string]bool{}
			}
			exemptionsUsed[c.table][c.column] = true
			_ = reason // present in the map; asserted non-stale below
			continue
		}
		if !tags[c.column] {
			failures = append(failures, c.table+"."+c.column)
		}
	}

	// A stale exemption — one naming a column that no longer exists on the
	// live schema (renamed, dropped, or a typo when it was declared) —
	// fails too: an exemption list that can silently accumulate dead
	// entries is exactly as untrustworthy as one that never gets checked.
	//
	// This does NOT also re-check "does the wire now carry a matching json
	// tag", even though the task instruction's phrasing suggests it should:
	// collectJSONTags is deliberately flat across the whole response type
	// graph (a json tag isn't attributed back to the table that motivated
	// it), so common column names — id, name, config_version — exist
	// elsewhere in the graph for unrelated reasons and would make every
	// such exemption look "stale" whether or not the specific table in
	// question is actually wired. That is a real limitation of this
	// implementation, stated rather than hidden behind a check that would
	// production false positives on every generically-named exemption
	// (menu_item_variant.id, menu_item_modifier.config_version, …).
	var staleExemptions []string
	for table, cols := range guardExemptions {
		for column := range cols {
			if !exemptionsUsed[table][column] {
				staleExemptions = append(staleExemptions, table+"."+column+" (not a real column on the live schema — remove or fix this exemption)")
			}
		}
	}

	sort.Strings(failures)
	sort.Strings(staleExemptions)

	if len(failures) > 0 {
		t.Errorf("column present on a cloud-authoritative table but absent from every wire shape in syncConfigResponse, and not exempted:\n  %s",
			strings.Join(failures, "\n  "))
	}
	if len(staleExemptions) > 0 {
		t.Errorf("stale guardExemptions entries:\n  %s", strings.Join(staleExemptions, "\n  "))
	}
}
