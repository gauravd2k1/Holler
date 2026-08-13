package main

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"testing"
	"time"

	"github.com/holler/backend/internal/auth"
	"github.com/holler/backend/internal/outlet"
	"github.com/holler/backend/internal/platform/config"
	"github.com/holler/backend/internal/platform/crypto"
	"github.com/holler/backend/internal/platform/id"
	"github.com/holler/backend/internal/tenant"
)

// TestIngestRoute_DeviceCredentialGate is the composed seam test the T8
// pilot blocker gates on (ADR-017's 0.4.3 amendment). It exercises ONE
// route, the table_session envelope-ingest path
// (POST /outlets/{outletId}/table-sessions), from BOTH directions in a
// single test:
//
//  1. An enrolled device's credential pushes a real envelope and is
//     accepted (200/201) with the row actually stored.
//  2. The SAME route, presented a valid, generously-permissioned HUMAN
//     bearer token instead, is rejected (401).
//
// Two separately-passing halves are exactly what let ADR-017 hole 1 ship
// undetected the first time (docs backlog: "a correctly enrolled edge node
// could pull config and then have every envelope push rejected" — found
// only by tracing across the seam, not by either side's own test suite).
// This test exists specifically so that shape of miss cannot recur: a
// regression that reverted the ingest routes back onto auth.Authenticate
// would make the device push in step 1 fail — see this package's own
// scratch-copy falsification, recorded in this task's final report, which
// confirms exactly that.
func TestIngestRoute_DeviceCredentialGate(t *testing.T) {
	pool := setupIntegrationPool(t)
	ctx := context.Background()

	tenantSvc := tenant.NewService(tenant.NewPostgresRepository(pool))
	org, err := tenantSvc.CreateOrganisation(ctx, "Ingest Gate Integration Org")
	if err != nil {
		t.Fatalf("CreateOrganisation: %v", err)
	}
	t.Cleanup(func() {
		pool.Exec(ctx, `DELETE FROM table_session WHERE outlet_id IN (SELECT id FROM outlet WHERE brand_id IN (SELECT id FROM brand WHERE tenant_id = $1))`, org.ID)
		pool.Exec(ctx, `DELETE FROM device_credential WHERE tenant_id = $1`, org.ID)
		pool.Exec(ctx, `DELETE FROM device WHERE outlet_id IN (SELECT id FROM outlet WHERE brand_id IN (SELECT id FROM brand WHERE tenant_id = $1))`, org.ID)
		pool.Exec(ctx, `DELETE FROM refresh_token WHERE user_id IN (SELECT id FROM app_user WHERE tenant_id = $1)`, org.ID)
		pool.Exec(ctx, `DELETE FROM user_role WHERE user_id IN (SELECT id FROM app_user WHERE tenant_id = $1)`, org.ID)
		pool.Exec(ctx, `DELETE FROM app_user WHERE tenant_id = $1`, org.ID)
		pool.Exec(ctx, `DELETE FROM role_permission WHERE role_id IN (SELECT id FROM role WHERE tenant_id = $1)`, org.ID)
		pool.Exec(ctx, `DELETE FROM role WHERE tenant_id = $1`, org.ID)
		pool.Exec(ctx, `DELETE FROM restaurant_table WHERE outlet_id IN (SELECT id FROM outlet WHERE brand_id IN (SELECT id FROM brand WHERE tenant_id = $1))`, org.ID)
		pool.Exec(ctx, `DELETE FROM outlet WHERE brand_id IN (SELECT id FROM brand WHERE tenant_id = $1)`, org.ID)
		pool.Exec(ctx, `DELETE FROM brand WHERE tenant_id = $1`, org.ID)
		pool.Exec(ctx, `DELETE FROM tenant WHERE id = $1`, org.ID)
	})
	brand, err := tenantSvc.CreateBrand(ctx, org.ID, "Ingest Gate Integration Brand")
	if err != nil {
		t.Fatalf("CreateBrand: %v", err)
	}
	outletSvc := outlet.NewService(outlet.NewPostgresRepository(pool))
	out, err := outletSvc.CreateOutlet(ctx, outlet.Principal{TenantID: org.ID}, brand.ID, "Ingest Gate Integration Outlet", "")
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
	hash, err := crypto.HashPassword("ingest-gate-test-password")
	if err != nil {
		t.Fatalf("HashPassword: %v", err)
	}
	// id.New() rather than a fixed literal (unlike
	// TestBuildRouter_SyncConfigEndToEnd's fixture): this suite has no
	// cross-package Postgres sharing concern of its own, but a fixed literal
	// here still risks colliding with a leftover row from an interrupted
	// prior run (app_user's primary key is not tenant-scoped), which is
	// exactly the failure a scratch-copy falsification run hit while
	// developing this test.
	userID := id.New()
	userEmail := "ingest-gate-" + userID + "@holler.test"
	if err := authRepo.CreateUser(ctx, userID, org.ID, userEmail, "Ingest Gate Owner", hash, time.Now().UTC()); err != nil {
		t.Fatalf("CreateUser: %v", err)
	}
	if err := authRepo.ReplaceUserRoles(ctx, userID, []auth.RoleAssignment{{ID: id.New(), RoleID: ownerRoleID}}, time.Now().UTC()); err != nil {
		t.Fatalf("ReplaceUserRoles: %v", err)
	}

	cfg := config.Config{
		Port: "0", DatabaseURL: "unused-in-test",
		AccessTokenTTL: 15 * time.Minute, RefreshTokenTTL: 720 * time.Hour,
		TokenSigningKey: []byte("ingest-gate-test-signing-key-not-for-prod"),
	}
	router := buildRouter(pool, cfg)
	server := httptest.NewServer(router)
	defer server.Close()

	// --- log in as the fully-privileged organisation owner: "generous
	// permissions" per this track's brief, so a rejection can only be
	// attributed to the credential TYPE, never a missing permission. -------
	loginBody, _ := json.Marshal(map[string]string{
		"email": userEmail, "password": "ingest-gate-test-password", "outlet_id": out.ID,
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

	// --- create a table (config, human-authenticated) so the ingest
	// envelope below references a real table_id. -------------------------
	tableResp := authedPost("/outlets/"+out.ID+"/tables", map[string]any{
		"section": "Main", "label": "IG1", "seat_count": 4,
	})
	if tableResp.StatusCode != http.StatusCreated {
		t.Fatalf("create table: expected 201, got %d", tableResp.StatusCode)
	}
	var createdTable struct {
		ID string `json:"id"`
	}
	if err := json.NewDecoder(tableResp.Body).Decode(&createdTable); err != nil {
		t.Fatalf("decoding table: %v", err)
	}
	tableResp.Body.Close()

	// --- enrol a device (human-privileged action, same as
	// TestBuildRouter_SyncConfigEndToEnd) and capture its one-time token. --
	enrollResp := authedPost("/devices/enroll", map[string]any{
		"outlet_id": out.ID, "kind": "POS", "name": "Ingest Gate Integration POS", "label": "integration test",
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

	// --- build ONE envelope and post it down BOTH paths, so the only
	// variable between the two requests is the credential presented. -------
	recordID := "14141414-1414-7414-8414-141414141414"
	now := time.Now().UTC().Format(time.RFC3339Nano)
	envelope := map[string]any{
		"record_id": recordID, "tenant_id": org.ID, "outlet_id": out.ID,
		"device_id": enrolled.DeviceID, "aggregate_type": "table_session", "direction": "EDGE_TO_CLOUD",
		"created_at": now, "updated_at": now, "version": 1, "sync_status": "PENDING",
		"payload": map[string]any{
			"id": recordID, "outlet_id": out.ID, "table_id": createdTable.ID,
			"state": "OCCUPIED", "guest_count": 2, "opened_at": now,
			"created_at": now, "updated_at": now, "version": 1, "schema_version": 1,
		},
	}
	rawEnvelope, _ := json.Marshal(envelope)
	ingestPath := "/outlets/" + out.ID + "/table-sessions"

	// --- direction 1: enrolled device -> envelope push -> 200/201, row
	// actually stored. ------------------------------------------------------
	deviceReq, _ := http.NewRequest(http.MethodPost, server.URL+ingestPath, bytes.NewReader(rawEnvelope))
	deviceReq.Header.Set("Authorization", "Bearer "+enrolled.Token)
	deviceReq.Header.Set("Content-Type", "application/json")
	deviceResp, err := http.DefaultClient.Do(deviceReq)
	if err != nil {
		t.Fatalf("device POST %s: %v", ingestPath, err)
	}
	defer deviceResp.Body.Close()
	if deviceResp.StatusCode != http.StatusCreated {
		t.Fatalf("expected the enrolled device's envelope push to be ACCEPTED (201), got %d", deviceResp.StatusCode)
	}
	var count int
	if err := pool.QueryRow(ctx, `SELECT count(*) FROM table_session WHERE id = $1 AND outlet_id = $2`, recordID, out.ID).Scan(&count); err != nil {
		t.Fatalf("counting table_session rows: %v", err)
	}
	if count != 1 {
		t.Fatalf("expected the device's envelope to be actually stored, found %d rows", count)
	}

	// --- direction 2: the SAME route, the SAME envelope shape, presented a
	// valid, fully-privileged HUMAN bearer token instead of a device
	// credential -> REJECTED (401). A different record_id so this can never
	// be confused with an idempotent replay of direction 1. -----------------
	humanEnvelope := map[string]any{}
	for k, v := range envelope {
		humanEnvelope[k] = v
	}
	humanRecordID := "15151515-1515-7515-8515-151515151515"
	humanEnvelope["record_id"] = humanRecordID
	humanPayload := map[string]any{}
	for k, v := range envelope["payload"].(map[string]any) {
		humanPayload[k] = v
	}
	humanPayload["id"] = humanRecordID
	humanEnvelope["payload"] = humanPayload
	rawHumanEnvelope, _ := json.Marshal(humanEnvelope)

	humanReq, _ := http.NewRequest(http.MethodPost, server.URL+ingestPath, bytes.NewReader(rawHumanEnvelope))
	humanReq.Header.Set("Authorization", "Bearer "+loginRespBody.AccessToken)
	humanReq.Header.Set("Content-Type", "application/json")
	humanResp, err := http.DefaultClient.Do(humanReq)
	if err != nil {
		t.Fatalf("human POST %s: %v", ingestPath, err)
	}
	defer humanResp.Body.Close()
	if humanResp.StatusCode != http.StatusUnauthorized {
		t.Fatalf("expected a fully-privileged HUMAN bearer token to be REJECTED (401) on an ingest route, got %d", humanResp.StatusCode)
	}
	var humanCount int
	if err := pool.QueryRow(ctx, `SELECT count(*) FROM table_session WHERE id = $1`, humanRecordID).Scan(&humanCount); err != nil {
		t.Fatalf("counting table_session rows for the rejected human push: %v", err)
	}
	if humanCount != 0 {
		t.Fatalf("expected the rejected human push to have stored NOTHING, found %d rows", humanCount)
	}
}
