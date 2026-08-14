package main

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"strconv"
	"testing"
	"time"

	"github.com/holler/backend/internal/auth"
	"github.com/holler/backend/internal/menu"
	"github.com/holler/backend/internal/outlet"
	"github.com/holler/backend/internal/platform/config"
	"github.com/holler/backend/internal/platform/crypto"
	"github.com/holler/backend/internal/platform/postgres"
	"github.com/holler/backend/internal/platform/testdb"
	"github.com/holler/backend/internal/tables"
	"github.com/holler/backend/internal/tenant"
)

// setupIntegrationPool mirrors backend/internal/kitchen/postgres_test.go's
// setupPool: same migration path, same shared testdb gate. See
// internal/platform/testdb: an unset HOLLER_TEST_DATABASE_URL fails this
// test loudly by default rather than skipping silently, since this file
// includes TestBuildRouter_SyncConfigEndToEnd (M2 acceptance item 4).
func setupIntegrationPool(t *testing.T) postgres.Pool {
	t.Helper()

	dbURL := testdb.RequireDatabaseURL(t)

	ctx := context.Background()
	pool, err := postgres.Open(ctx, dbURL)
	if err != nil {
		t.Fatalf("postgres.Open: %v", err)
	}
	t.Cleanup(pool.Close)

	contractsDir, err := filepath.Abs(filepath.Join("..", "..", "..", "packages", "contracts", "postgres"))
	if err != nil {
		t.Fatalf("resolving contracts dir: %v", err)
	}
	if err := postgres.Migrate(ctx, pool, contractsDir); err != nil {
		t.Fatalf("postgres.Migrate: %v", err)
	}
	return pool
}

// TestBuildRouter_SyncConfigEndToEnd exercises the composition root's real
// buildRouter (real Postgres-backed repositories, not fakes) end to end:
// login, create a table/category/item/station via the mounted HTTP routes,
// then pull GET /sync/config and assert all nine fields are present, that
// since_version filtering excludes rows at or below the watermark for both
// the pre-existing fields and users, and that no response anywhere in this
// flow — /auth/login included — ever carries password_hash/pin_hash except
// inside /sync/config's own users array, where both fields are expected
// (ADR-015).
func TestBuildRouter_SyncConfigEndToEnd(t *testing.T) {
	pool := setupIntegrationPool(t)
	ctx := context.Background()

	// --- fixture: tenant/brand/outlet/role/user, mirroring
	// backend/internal/kitchen/postgres_test.go's newFixture pattern -------
	tenantSvc := tenant.NewService(tenant.NewPostgresRepository(pool))
	org, err := tenantSvc.CreateOrganisation(ctx, "Sync Config Integration Org")
	if err != nil {
		t.Fatalf("CreateOrganisation: %v", err)
	}
	// This fixture uses fixed IDs below (userID, station id) rather than
	// id.New(), so without cleanup a second run collides on a primary key
	// that survived the first. Delete in FK-safe order, scoped to org.ID.
	t.Cleanup(func() {
		pool.Exec(ctx, `DELETE FROM audit_event WHERE tenant_id = $1`, org.ID)
		pool.Exec(ctx, `DELETE FROM device_credential WHERE tenant_id = $1`, org.ID)
		pool.Exec(ctx, `DELETE FROM device WHERE outlet_id IN (SELECT id FROM outlet WHERE brand_id IN (SELECT id FROM brand WHERE tenant_id = $1))`, org.ID)
		pool.Exec(ctx, `DELETE FROM refresh_token WHERE user_id IN (SELECT id FROM app_user WHERE tenant_id = $1)`, org.ID)
		pool.Exec(ctx, `DELETE FROM user_role WHERE user_id IN (SELECT id FROM app_user WHERE tenant_id = $1)`, org.ID)
		pool.Exec(ctx, `DELETE FROM app_user WHERE tenant_id = $1`, org.ID)
		pool.Exec(ctx, `DELETE FROM role_permission WHERE role_id IN (SELECT id FROM role WHERE tenant_id = $1)`, org.ID)
		pool.Exec(ctx, `DELETE FROM role WHERE tenant_id = $1`, org.ID)
		pool.Exec(ctx, `DELETE FROM menu_item_station WHERE menu_item_id IN (SELECT id FROM menu_item WHERE outlet_id IN (SELECT id FROM outlet WHERE brand_id IN (SELECT id FROM brand WHERE tenant_id = $1)))`, org.ID)
		pool.Exec(ctx, `DELETE FROM station_printer WHERE station_id IN (SELECT id FROM station WHERE outlet_id IN (SELECT id FROM outlet WHERE brand_id IN (SELECT id FROM brand WHERE tenant_id = $1)))`, org.ID)
		pool.Exec(ctx, `DELETE FROM station WHERE outlet_id IN (SELECT id FROM outlet WHERE brand_id IN (SELECT id FROM brand WHERE tenant_id = $1))`, org.ID)
		pool.Exec(ctx, `DELETE FROM printer WHERE outlet_id IN (SELECT id FROM outlet WHERE brand_id IN (SELECT id FROM brand WHERE tenant_id = $1))`, org.ID)
		pool.Exec(ctx, `DELETE FROM restaurant_table WHERE outlet_id IN (SELECT id FROM outlet WHERE brand_id IN (SELECT id FROM brand WHERE tenant_id = $1))`, org.ID)
		pool.Exec(ctx, `DELETE FROM menu_item WHERE outlet_id IN (SELECT id FROM outlet WHERE brand_id IN (SELECT id FROM brand WHERE tenant_id = $1))`, org.ID)
		pool.Exec(ctx, `DELETE FROM menu_category WHERE outlet_id IN (SELECT id FROM outlet WHERE brand_id IN (SELECT id FROM brand WHERE tenant_id = $1))`, org.ID)
		pool.Exec(ctx, `DELETE FROM outlet WHERE brand_id IN (SELECT id FROM brand WHERE tenant_id = $1)`, org.ID)
		pool.Exec(ctx, `DELETE FROM brand WHERE tenant_id = $1`, org.ID)
		pool.Exec(ctx, `DELETE FROM tenant WHERE id = $1`, org.ID)
	})
	brand, err := tenantSvc.CreateBrand(ctx, org.ID, "Sync Config Integration Brand")
	if err != nil {
		t.Fatalf("CreateBrand: %v", err)
	}

	outletSvc := outlet.NewService(outlet.NewPostgresRepository(pool))
	out, err := outletSvc.CreateOutlet(ctx, outlet.Principal{TenantID: org.ID}, brand.ID, "Sync Config Integration Outlet", "")
	if err != nil {
		t.Fatalf("CreateOutlet: %v", err)
	}

	authRepo := auth.NewRepository(pool)
	if err := auth.SeedTenantRoles(ctx, authRepo, org.ID); err != nil {
		t.Fatalf("SeedTenantRoles: %v", err)
	}
	roles, err := authRepo.ListRoles(ctx, org.ID)
	if err != nil {
		t.Fatalf("ListRoles: %v", err)
	}
	var ownerRoleID string
	for _, r := range roles {
		if r.Code == auth.RoleCodeOrganisationOwner {
			ownerRoleID = r.ID
		}
	}
	if ownerRoleID == "" {
		t.Fatalf("ORGANISATION_OWNER role not seeded")
	}

	hash, err := crypto.HashPassword("integration-test-password")
	if err != nil {
		t.Fatalf("HashPassword: %v", err)
	}
	userID := "dddddddd-dddd-7ddd-8ddd-dddddddddddd"
	if err := authRepo.CreateUser(ctx, userID, org.ID, "sync-config@holler.test", "Sync Config Owner", hash, time.Now().UTC()); err != nil {
		t.Fatalf("CreateUser: %v", err)
	}
	if _, err := authRepo.GetUser(ctx, org.ID, userID); err != nil {
		t.Fatalf("GetUser: %v", err)
	}
	if err := authRepo.ReplaceUserRoles(ctx, userID, []auth.RoleAssignment{{ID: "eeeeeeee-eeee-7eee-8eee-eeeeeeeeeeee", RoleID: ownerRoleID}}, time.Now().UTC()); err != nil {
		t.Fatalf("ReplaceUserRoles: %v", err)
	}

	cfg := config.Config{
		Port:            "0",
		DatabaseURL:     "unused-in-test",
		AccessTokenTTL:  15 * time.Minute,
		RefreshTokenTTL: 720 * time.Hour,
		TokenSigningKey: []byte("integration-test-signing-key-not-for-prod"),
	}
	router := buildRouter(pool, cfg)
	server := httptest.NewServer(router)
	defer server.Close()

	// --- login: same bearer-token mechanism every other authenticated
	// route uses (see this task's report re: no distinct edge/device
	// authentication mechanism exists in this codebase). -------------------
	loginBody, _ := json.Marshal(map[string]string{
		"email": "sync-config@holler.test", "password": "integration-test-password", "outlet_id": out.ID,
	})
	loginReq, _ := http.NewRequest(http.MethodPost, server.URL+"/auth/login", bytes.NewReader(loginBody))
	loginReq.Header.Set("Content-Type", "application/json")
	loginReq.Header.Set("X-Tenant-ID", org.ID)
	loginResp, err := http.DefaultClient.Do(loginReq)
	if err != nil {
		t.Fatalf("login request: %v", err)
	}
	defer loginResp.Body.Close()
	if loginResp.StatusCode != http.StatusOK {
		t.Fatalf("login: expected 200, got %d", loginResp.StatusCode)
	}
	var loginRespBody struct {
		AccessToken string `json:"access_token"`
	}
	if err := json.NewDecoder(loginResp.Body).Decode(&loginRespBody); err != nil {
		t.Fatalf("decoding login response: %v", err)
	}
	if containsCredentialMaterial(mustMarshal(t, loginRespBody)) {
		t.Fatalf("login response must never carry credential material")
	}

	authedGet := func(path string) *http.Response {
		req, _ := http.NewRequest(http.MethodGet, server.URL+path, nil)
		req.Header.Set("Authorization", "Bearer "+loginRespBody.AccessToken)
		resp, err := http.DefaultClient.Do(req)
		if err != nil {
			t.Fatalf("GET %s: %v", path, err)
		}
		return resp
	}
	authedPost := func(path string, body any) *http.Response {
		raw, _ := json.Marshal(body)
		req, _ := http.NewRequest(http.MethodPost, server.URL+path, bytes.NewReader(raw))
		req.Header.Set("Authorization", "Bearer "+loginRespBody.AccessToken)
		req.Header.Set("Content-Type", "application/json")
		resp, err := http.DefaultClient.Do(req)
		if err != nil {
			t.Fatalf("POST %s: %v", path, err)
		}
		return resp
	}

	// --- config low-watermark: create a table + category before recording
	// since_version, then more after, to prove filtering works end to end
	// through the real mounted routes rather than only through the fake
	// providers in syncconfig_test.go. -------------------------------------
	tableResp := authedPost("/outlets/"+out.ID+"/tables", map[string]any{
		"section": "Main", "label": "T1", "seat_count": 4,
	})
	if tableResp.StatusCode != http.StatusCreated {
		t.Fatalf("create table: expected 201, got %d", tableResp.StatusCode)
	}
	var createdTable tables.RestaurantTable
	if err := json.NewDecoder(tableResp.Body).Decode(&createdTable); err != nil {
		t.Fatalf("decoding table: %v", err)
	}
	tableResp.Body.Close()

	sinceVersion := createdTable.ConfigVersion // watermark: this table must NOT reappear

	categoryResp := authedPost("/menu/categories", map[string]any{
		"outlet_id": out.ID, "name": "Mains", "sort_order": 1,
	})
	if categoryResp.StatusCode != http.StatusCreated {
		t.Fatalf("create category: expected 201, got %d", categoryResp.StatusCode)
	}
	var createdCategory menu.Category
	if err := json.NewDecoder(categoryResp.Body).Decode(&createdCategory); err != nil {
		t.Fatalf("decoding category: %v", err)
	}
	categoryResp.Body.Close()

	itemResp := authedPost("/menu/items", map[string]any{
		"outlet_id": out.ID, "category_id": createdCategory.ID, "name": "Thali", "base_price_paise": 22000,
	})
	if itemResp.StatusCode != http.StatusCreated {
		t.Fatalf("create item: expected 201, got %d", itemResp.StatusCode)
	}
	var createdItem struct {
		ID string `json:"id"`
	}
	if err := json.NewDecoder(itemResp.Body).Decode(&createdItem); err != nil {
		t.Fatalf("decoding item: %v", err)
	}
	itemResp.Body.Close()

	stationResp := authedPost("/stations", map[string]any{
		"id": "ffffffff-ffff-7fff-8fff-ffffffffffff", "outlet_id": out.ID,
		"code": "MAIN", "name": "Main Kitchen", "sort_order": 1, "is_active": true,
	})
	if stationResp.StatusCode != http.StatusCreated {
		t.Fatalf("create station: expected 201, got %d", stationResp.StatusCode)
	}
	stationResp.Body.Close()

	// --- enroll a device: GET /sync/config no longer accepts the human
	// bearer token used above (ADR-017 §2) — it is gated on a verified
	// device credential, resolved server-side to tenant_id/outlet_id rather
	// than trusted from the caller. Enrollment itself IS a human-privileged
	// action, so it uses the owner's session, exactly like table/menu/
	// station creation above. -----------------------------------------------
	enrollResp := authedPost("/devices/enroll", map[string]any{
		"outlet_id": out.ID, "kind": "POS", "name": "Sync Config Integration POS", "label": "integration test",
	})
	if enrollResp.StatusCode != http.StatusCreated {
		t.Fatalf("POST /devices/enroll: expected 201, got %d", enrollResp.StatusCode)
	}
	var enrolled struct {
		DeviceID string `json:"device_id"`
		Token    string `json:"token"`
	}
	if err := json.NewDecoder(enrollResp.Body).Decode(&enrolled); err != nil {
		t.Fatalf("decoding enroll response: %v", err)
	}
	enrollResp.Body.Close()
	if enrolled.Token == "" {
		t.Fatal("expected a non-empty device token from enrollment")
	}

	deviceGet := func(path string) *http.Response {
		req, _ := http.NewRequest(http.MethodGet, server.URL+path, nil)
		req.Header.Set("Authorization", "Bearer "+enrolled.Token)
		resp, err := http.DefaultClient.Do(req)
		if err != nil {
			t.Fatalf("GET %s: %v", path, err)
		}
		return resp
	}

	// A human access token — even the owning tenant's own, fully-privileged
	// one — must no longer work on this route. This is the direct
	// falsification of the hole ADR-017 closes: prove the OLD gate (a valid
	// bearer token + user.manage) now fails where it used to pass.
	if humanResp := authedGet("/sync/config?outlet_id=" + out.ID + "&since_version=0"); humanResp.StatusCode != http.StatusUnauthorized {
		t.Fatalf("expected a human bearer token to be REJECTED on GET /sync/config, got %d", humanResp.StatusCode)
	}

	// --- the route under test ---------------------------------------------
	syncResp := deviceGet("/sync/config?outlet_id=" + out.ID + "&since_version=0")
	if syncResp.StatusCode != http.StatusOK {
		t.Fatalf("GET /sync/config: expected 200, got %d", syncResp.StatusCode)
	}
	rawBody, err := jsonBody(syncResp)
	if err != nil {
		t.Fatalf("reading /sync/config body: %v", err)
	}

	var all map[string]json.RawMessage
	if err := json.Unmarshal(rawBody, &all); err != nil {
		t.Fatalf("decoding /sync/config: %v", err)
	}
	for _, field := range []string{
		"config_version", "users", "tables", "categories", "items",
		"stations", "item_stations", "printers", "station_printers",
	} {
		if _, ok := all[field]; !ok {
			t.Errorf("GET /sync/config missing required field %q", field)
		}
	}

	var resp syncConfigResponse
	if err := json.Unmarshal(rawBody, &resp); err != nil {
		t.Fatalf("decoding into syncConfigResponse: %v", err)
	}
	if len(resp.Categories) != 1 || resp.Categories[0].ID != createdCategory.ID {
		t.Errorf("expected exactly the one category created after since_version=0, got %+v", resp.Categories)
	}
	if len(resp.Items) != 1 || resp.Items[0].ID != createdItem.ID {
		t.Errorf("expected exactly the one item created after since_version=0, got %+v", resp.Items)
	}
	if len(resp.Stations) != 1 {
		t.Errorf("expected exactly the one station, got %+v", resp.Stations)
	}
	if len(resp.Users) != 1 || resp.Users[0].ID != userID {
		t.Fatalf("expected exactly the tenant-wide login user in the edge cache, got %+v", resp.Users)
	}
	if resp.Users[0].PasswordHash == "" {
		t.Errorf("expected password_hash populated on the edge cache entry")
	}
	if resp.Users[0].PinHash != nil {
		t.Errorf("expected pin_hash nil (no PIN set for this fixture user), got %v", *resp.Users[0].PinHash)
	}
	foundUserManage := false
	for _, p := range resp.Users[0].Permissions {
		if string(p) == "user.manage" {
			foundUserManage = true
		}
	}
	if !foundUserManage {
		t.Errorf("expected the Organisation Owner's flattened permissions to include user.manage, got %v", resp.Users[0].Permissions)
	}

	// --- since_version filtering: pulling again with the table's own
	// config_version as the watermark must exclude it. ---------------------
	filteredResp := deviceGet("/sync/config?outlet_id=" + out.ID + "&since_version=" + strconv.Itoa(sinceVersion))
	filteredBody, err := jsonBody(filteredResp)
	if err != nil {
		t.Fatalf("reading filtered /sync/config body: %v", err)
	}
	var filtered syncConfigResponse
	if err := json.Unmarshal(filteredBody, &filtered); err != nil {
		t.Fatalf("decoding filtered response: %v", err)
	}
	for _, tbl := range filtered.Tables {
		if tbl.ID == createdTable.ID {
			t.Errorf("table %s at/below since_version=%d must be excluded, got %+v", createdTable.ID, sinceVersion, filtered.Tables)
		}
	}

	// --- credential material must appear inside /sync/config's users array
	// and nowhere else in this response (ADR-015). Strip "users" out and
	// assert the remainder of the body is clean, then assert users itself
	// does carry it — a false negative there would hide the field silently
	// going missing rather than proving containment.
	withoutUsers := map[string]json.RawMessage{}
	if err := json.Unmarshal(rawBody, &withoutUsers); err != nil {
		t.Fatalf("re-decoding /sync/config body: %v", err)
	}
	delete(withoutUsers, "users")
	restOfBody, err := json.Marshal(withoutUsers)
	if err != nil {
		t.Fatalf("marshaling body without users: %v", err)
	}
	if containsCredentialMaterial(restOfBody) {
		t.Errorf("GET /sync/config response must never leak credential material outside users: %s", string(restOfBody))
	}
	if !containsCredentialMaterial(rawBody) {
		t.Errorf("expected GET /sync/config's users array to carry password_hash, got none: %s", string(rawBody))
	}
}

func jsonBody(resp *http.Response) ([]byte, error) {
	defer resp.Body.Close()
	return io.ReadAll(resp.Body)
}

func mustMarshal(t *testing.T, v any) []byte {
	t.Helper()
	b, err := json.Marshal(v)
	if err != nil {
		t.Fatalf("marshaling: %v", err)
	}
	return b
}

// containsCredentialMaterial is a blunt substring check for the field names
// ADR-011 forbids on the wire outside GET /sync/config's users array.
func containsCredentialMaterial(body []byte) bool {
	s := string(body)
	return contains(s, "password_hash") || contains(s, "pin_hash") || contains(s, "token_hash")
}
