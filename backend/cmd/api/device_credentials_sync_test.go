package main

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/holler/backend/internal/auth"
	"github.com/holler/backend/internal/outlet"
	"github.com/holler/backend/internal/platform/config"
	"github.com/holler/backend/internal/platform/crypto"
	"github.com/holler/backend/internal/platform/id"
	"github.com/holler/backend/internal/tenant"
)

// TestSyncConfig_DeviceCredentialsFlowThroughRealPostgres is T13's follow-up
// evidence: GET /sync/config's device_credentials array populated from real
// Postgres device_credential rows, not a fake. It proves, against
// HOLLER_TEST_DATABASE_URL:
//   - a credential minted by POST /devices/enroll appears in a subsequent
//     GET /sync/config pull for its outlet, credential_hash intact
//     (Argon2id, never the plaintext token from the enroll response);
//   - a since_version watermark taken BEFORE the enrollment excludes the
//     credential correctly filters, and a pull with a value below the
//     outlet's current config_version returns it — the same config_version
//     contract tables/categories/items/users all obey;
//   - a REVOKED credential is still present afterward, with revoked_at
//     populated, never dropped from the array (ADR-017 0.4.3 amendment: "the
//     edge learns a credential is dead by syncing it, not by its absence").
func TestSyncConfig_DeviceCredentialsFlowThroughRealPostgres(t *testing.T) {
	pool := setupIntegrationPool(t)
	ctx := context.Background()

	tenantSvc := tenant.NewService(tenant.NewPostgresRepository(pool))
	org, err := tenantSvc.CreateOrganisation(ctx, "Device Credential Sync Org "+id.New())
	if err != nil {
		t.Fatalf("CreateOrganisation: %v", err)
	}
	t.Cleanup(func() {
		pool.Exec(ctx, `DELETE FROM device_credential WHERE tenant_id = $1`, org.ID)
		pool.Exec(ctx, `DELETE FROM device WHERE outlet_id IN (SELECT id FROM outlet WHERE brand_id IN (SELECT id FROM brand WHERE tenant_id = $1))`, org.ID)
		pool.Exec(ctx, `DELETE FROM refresh_token WHERE user_id IN (SELECT id FROM app_user WHERE tenant_id = $1)`, org.ID)
		pool.Exec(ctx, `DELETE FROM user_role WHERE user_id IN (SELECT id FROM app_user WHERE tenant_id = $1)`, org.ID)
		pool.Exec(ctx, `DELETE FROM app_user WHERE tenant_id = $1`, org.ID)
		pool.Exec(ctx, `DELETE FROM role_permission WHERE role_id IN (SELECT id FROM role WHERE tenant_id = $1)`, org.ID)
		pool.Exec(ctx, `DELETE FROM role WHERE tenant_id = $1`, org.ID)
		pool.Exec(ctx, `DELETE FROM outlet WHERE brand_id IN (SELECT id FROM brand WHERE tenant_id = $1)`, org.ID)
		pool.Exec(ctx, `DELETE FROM brand WHERE tenant_id = $1`, org.ID)
		pool.Exec(ctx, `DELETE FROM tenant WHERE id = $1`, org.ID)
	})

	brand, err := tenantSvc.CreateBrand(ctx, org.ID, "Device Credential Sync Brand")
	if err != nil {
		t.Fatalf("CreateBrand: %v", err)
	}
	outletSvc := outlet.NewService(outlet.NewPostgresRepository(pool))
	out, err := outletSvc.CreateOutlet(ctx, outlet.Principal{TenantID: org.ID}, brand.ID, "Device Credential Sync Outlet", "")
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
	hash, err := crypto.HashPassword("device-credential-sync-test-password")
	if err != nil {
		t.Fatalf("HashPassword: %v", err)
	}
	userID := id.New()
	userEmail := "device-credential-sync-" + userID + "@holler.test"
	if err := authRepo.CreateUser(ctx, userID, org.ID, userEmail, "Device Credential Sync Owner", hash, time.Now().UTC()); err != nil {
		t.Fatalf("CreateUser: %v", err)
	}
	if err := authRepo.ReplaceUserRoles(ctx, userID, []auth.RoleAssignment{{ID: id.New(), RoleID: ownerRoleID}}, time.Now().UTC()); err != nil {
		t.Fatalf("ReplaceUserRoles: %v", err)
	}

	cfg := config.Config{
		Port: "0", DatabaseURL: "unused-in-test",
		AccessTokenTTL: 15 * time.Minute, RefreshTokenTTL: 720 * time.Hour,
		TokenSigningKey: []byte("device-credential-sync-test-signing-key-not-for-prod"),
	}
	router := buildRouter(pool, cfg)
	server := httptest.NewServer(router)
	defer server.Close()

	loginBody, _ := json.Marshal(map[string]string{
		"email": userEmail, "password": "device-credential-sync-test-password", "outlet_id": out.ID,
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

	// --- enroll the READER device: its token authenticates the
	// GET /sync/config pulls below, exactly like
	// TestBuildRouter_SyncConfigEndToEnd. ------------------------------------
	readerResp := authedPost("/devices/enroll", map[string]any{
		"outlet_id": out.ID, "kind": "POS", "name": "Reader POS " + id.New(), "label": "sync reader",
	})
	if readerResp.StatusCode != http.StatusCreated {
		t.Fatalf("enroll reader device: expected 201, got %d", readerResp.StatusCode)
	}
	var reader struct {
		Token string `json:"token"`
	}
	if err := json.NewDecoder(readerResp.Body).Decode(&reader); err != nil {
		t.Fatalf("decoding reader enroll response: %v", err)
	}
	readerResp.Body.Close()

	deviceGet := func(query string) *http.Response {
		req, _ := http.NewRequest(http.MethodGet, server.URL+"/sync/config?"+query, nil)
		req.Header.Set("Authorization", "Bearer "+reader.Token)
		resp, err := http.DefaultClient.Do(req)
		if err != nil {
			t.Fatalf("GET /sync/config: %v", err)
		}
		return resp
	}

	// Watermark BEFORE the credential under test is enrolled.
	baselineResp := deviceGet("outlet_id=" + out.ID + "&since_version=0")
	if baselineResp.StatusCode != http.StatusOK {
		t.Fatalf("baseline GET /sync/config: expected 200, got %d", baselineResp.StatusCode)
	}
	var baseline syncConfigResponse
	if err := json.NewDecoder(baselineResp.Body).Decode(&baseline); err != nil {
		t.Fatalf("decoding baseline response: %v", err)
	}
	baselineResp.Body.Close()
	watermark := baseline.ConfigVersion

	// --- enroll the device UNDER TEST (a KDS) -------------------------------
	kdsName := "KDS Under Test " + id.New()
	kdsResp := authedPost("/devices/enroll", map[string]any{
		"outlet_id": out.ID, "kind": "KDS", "name": kdsName, "label": "credential sync target",
	})
	if kdsResp.StatusCode != http.StatusCreated {
		t.Fatalf("enroll KDS device: expected 201, got %d", kdsResp.StatusCode)
	}
	var kds struct {
		DeviceID string `json:"device_id"`
		Token    string `json:"token"`
	}
	if err := json.NewDecoder(kdsResp.Body).Decode(&kds); err != nil {
		t.Fatalf("decoding KDS enroll response: %v", err)
	}
	kdsResp.Body.Close()
	if kds.DeviceID == "" || kds.Token == "" {
		t.Fatal("expected a non-empty device_id and token from KDS enrollment")
	}

	// --- pull #1: the new credential must appear, above the watermark,
	// credential_hash intact and never equal to the plaintext token. --------
	afterEnrollResp := deviceGet("outlet_id=" + out.ID + "&since_version=" + strconv.Itoa(watermark))
	if afterEnrollResp.StatusCode != http.StatusOK {
		t.Fatalf("GET /sync/config after enroll: expected 200, got %d", afterEnrollResp.StatusCode)
	}
	var afterEnroll syncConfigResponse
	if err := json.NewDecoder(afterEnrollResp.Body).Decode(&afterEnroll); err != nil {
		t.Fatalf("decoding post-enroll response: %v", err)
	}
	afterEnrollResp.Body.Close()

	var kdsCred *struct {
		CredentialID   string
		CredentialHash string
		RevokedAt      *string
	}
	for _, c := range afterEnroll.DeviceCredentials {
		if c.DeviceID == kds.DeviceID {
			cCopy := c
			kdsCred = &struct {
				CredentialID   string
				CredentialHash string
				RevokedAt      *string
			}{CredentialID: cCopy.CredentialID, CredentialHash: cCopy.CredentialHash, RevokedAt: cCopy.RevokedAt}
		}
	}
	if kdsCred == nil {
		t.Fatalf("expected the newly enrolled KDS credential in device_credentials, got %+v", afterEnroll.DeviceCredentials)
	}
	if kdsCred.RevokedAt != nil {
		t.Fatalf("expected a freshly enrolled credential to have revoked_at nil, got %v", *kdsCred.RevokedAt)
	}
	if !strings.HasPrefix(kdsCred.CredentialHash, "$argon2id$") {
		t.Fatalf("expected credential_hash to be an Argon2id verifier, got %q", kdsCred.CredentialHash)
	}
	if strings.Contains(kds.Token, kdsCred.CredentialHash) || kdsCred.CredentialHash == kds.Token {
		t.Fatalf("credential_hash must never equal or embed the plaintext enrollment token")
	}

	// A pull at exactly the post-enroll config_version excludes it again —
	// the collection behaves like every other config aggregate's
	// since_version contract.
	atCurrentResp := deviceGet("outlet_id=" + out.ID + "&since_version=" + strconv.Itoa(afterEnroll.ConfigVersion))
	var atCurrent syncConfigResponse
	if err := json.NewDecoder(atCurrentResp.Body).Decode(&atCurrent); err != nil {
		t.Fatalf("decoding at-current response: %v", err)
	}
	atCurrentResp.Body.Close()
	for _, c := range atCurrent.DeviceCredentials {
		if c.DeviceID == kds.DeviceID {
			t.Fatalf("expected since_version at the current config_version to exclude the credential, got %+v", atCurrent.DeviceCredentials)
		}
	}

	// --- revoke it, then pull again: it must still be present, marked
	// revoked, never dropped. ------------------------------------------------
	revokeResp := authedPost("/devices/"+kds.DeviceID+"/credentials/revoke", map[string]any{})
	if revokeResp.StatusCode != http.StatusNoContent && revokeResp.StatusCode != http.StatusOK {
		t.Fatalf("revoke credential: expected 200/204, got %d", revokeResp.StatusCode)
	}
	revokeResp.Body.Close()

	afterRevokeResp := deviceGet("outlet_id=" + out.ID + "&since_version=0")
	if afterRevokeResp.StatusCode != http.StatusOK {
		t.Fatalf("GET /sync/config after revoke: expected 200, got %d", afterRevokeResp.StatusCode)
	}
	var afterRevoke syncConfigResponse
	if err := json.NewDecoder(afterRevokeResp.Body).Decode(&afterRevoke); err != nil {
		t.Fatalf("decoding post-revoke response: %v", err)
	}
	afterRevokeResp.Body.Close()

	found := false
	for _, c := range afterRevoke.DeviceCredentials {
		if c.CredentialID == kdsCred.CredentialID {
			found = true
			if c.RevokedAt == nil {
				t.Fatalf("expected the revoked credential to carry a non-nil revoked_at, got %+v", c)
			}
			if !strings.HasPrefix(c.CredentialHash, "$argon2id$") {
				t.Fatalf("expected the revoked credential to still carry its hash, got %q", c.CredentialHash)
			}
		}
	}
	if !found {
		t.Fatalf("expected the REVOKED credential to remain present in device_credentials (never dropped), got %+v", afterRevoke.DeviceCredentials)
	}
	if afterRevoke.ConfigVersion <= afterEnroll.ConfigVersion {
		t.Fatalf("expected config_version to advance again on revoke, before=%d after=%d", afterEnroll.ConfigVersion, afterRevoke.ConfigVersion)
	}
}
