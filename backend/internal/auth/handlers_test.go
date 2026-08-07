package auth

import (
	"bytes"
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"strings"
	"testing"
	"time"

	"github.com/go-chi/chi/v5"
	"github.com/holler/backend/internal/platform/id"
)

// TestHandlers_NoResponseEverContainsPasswordHash exercises login, user
// creation, listing and role assignment end-to-end (against fakes) and
// asserts a password/pin hash never appears anywhere in a response body.
func TestHandlers_NoResponseEverContainsPasswordHash(t *testing.T) {
	repo := newFakeRepo()
	auditor := &fakeAuditor{}
	signer := NewTokenSigner([]byte("handler-test-key"))
	refresh := NewInMemoryRefreshStore()
	limiter := NewInMemoryRateLimiter()
	svc := NewService(repo, signer, refresh, limiter, auditor, time.Minute, time.Hour)
	h := NewHandlers(svc, signer)

	tenantID := id.New()
	outletID := id.New()
	adminID := id.New()
	adminHash := mustHash(t, "admin-password-123")
	repo.CreateUser(context.Background(), adminID, tenantID, "admin@example.com", "Admin", adminHash, time.Now())

	role := Role{ID: id.New(), TenantID: tenantID, Code: RoleCodeOrganisationOwner, Name: "Organisation Owner", Permissions: AllM1Permissions}
	repo.addRole(role)
	repo.userRoles[adminID] = []RoleAssignment{{ID: id.New(), RoleID: role.ID, RoleCode: role.Code, OutletID: nil}}

	router := chi.NewRouter()
	h.Mount(router)

	// Login.
	loginBody, _ := json.Marshal(loginRequest{Email: "admin@example.com", Password: "admin-password-123", OutletID: outletID})
	req := httptest.NewRequest(http.MethodPost, "/auth/login", bytes.NewReader(loginBody))
	req.Header.Set("X-Tenant-ID", tenantID)
	rec := httptest.NewRecorder()
	router.ServeHTTP(rec, req)
	if rec.Code != http.StatusOK {
		t.Fatalf("login: expected 200, got %d body=%s", rec.Code, rec.Body.String())
	}
	assertNoHash(t, rec.Body.String())

	var session sessionResponse
	if err := json.Unmarshal(rec.Body.Bytes(), &session); err != nil {
		t.Fatalf("decoding login response: %v", err)
	}

	// Create a new user as the authenticated admin.
	newUserID := id.New()
	createBody, _ := json.Marshal(createUserRequest{ID: newUserID, Email: "cashier@example.com", FullName: "Cash Ier", Password: "cashier-password-456"})
	req = httptest.NewRequest(http.MethodPost, "/users", bytes.NewReader(createBody))
	req.Header.Set("Authorization", "Bearer "+session.AccessToken)
	rec = httptest.NewRecorder()
	router.ServeHTTP(rec, req)
	if rec.Code != http.StatusCreated {
		t.Fatalf("create user: expected 201, got %d body=%s", rec.Code, rec.Body.String())
	}
	assertNoHash(t, rec.Body.String())

	// List users.
	req = httptest.NewRequest(http.MethodGet, "/users", nil)
	req.Header.Set("Authorization", "Bearer "+session.AccessToken)
	rec = httptest.NewRecorder()
	router.ServeHTTP(rec, req)
	if rec.Code != http.StatusOK {
		t.Fatalf("list users: expected 200, got %d body=%s", rec.Code, rec.Body.String())
	}
	assertNoHash(t, rec.Body.String())

	// List roles.
	req = httptest.NewRequest(http.MethodGet, "/roles", nil)
	req.Header.Set("Authorization", "Bearer "+session.AccessToken)
	rec = httptest.NewRecorder()
	router.ServeHTTP(rec, req)
	if rec.Code != http.StatusOK {
		t.Fatalf("list roles: expected 200, got %d body=%s", rec.Code, rec.Body.String())
	}
	assertNoHash(t, rec.Body.String())

	// Refresh.
	refreshBody, _ := json.Marshal(refreshRequest{RefreshToken: session.RefreshToken})
	req = httptest.NewRequest(http.MethodPost, "/auth/refresh", bytes.NewReader(refreshBody))
	rec = httptest.NewRecorder()
	router.ServeHTTP(rec, req)
	if rec.Code != http.StatusOK {
		t.Fatalf("refresh: expected 200, got %d body=%s", rec.Code, rec.Body.String())
	}
	assertNoHash(t, rec.Body.String())
}

func TestLogin_FailureResponseDoesNotDistinguishReasons(t *testing.T) {
	repo := newFakeRepo()
	signer := NewTokenSigner([]byte("handler-test-key"))
	refresh := NewInMemoryRefreshStore()
	limiter := NewInMemoryRateLimiter()
	svc := NewService(repo, signer, refresh, limiter, nil, time.Minute, time.Hour)
	h := NewHandlers(svc, signer)

	tenantID := id.New()
	userID := id.New()
	hash := mustHash(t, "the-real-password")
	repo.CreateUser(context.Background(), userID, tenantID, "exists@example.com", "Exists", hash, time.Now())

	router := chi.NewRouter()
	h.Mount(router)

	wrongPasswordBody, _ := json.Marshal(loginRequest{Email: "exists@example.com", Password: "not-the-password", OutletID: id.New()})
	req := httptest.NewRequest(http.MethodPost, "/auth/login", bytes.NewReader(wrongPasswordBody))
	req.Header.Set("X-Tenant-ID", tenantID)
	rec1 := httptest.NewRecorder()
	router.ServeHTTP(rec1, req)

	noSuchUserBody, _ := json.Marshal(loginRequest{Email: "nobody@example.com", Password: "irrelevant", OutletID: id.New()})
	req = httptest.NewRequest(http.MethodPost, "/auth/login", bytes.NewReader(noSuchUserBody))
	req.Header.Set("X-Tenant-ID", tenantID)
	rec2 := httptest.NewRecorder()
	router.ServeHTTP(rec2, req)

	if rec1.Code != rec2.Code || rec1.Body.String() != rec2.Body.String() {
		t.Fatalf("expected identical failure responses, got (%d,%s) vs (%d,%s)", rec1.Code, rec1.Body.String(), rec2.Code, rec2.Body.String())
	}
}

func assertNoHash(t *testing.T, body string) {
	t.Helper()
	if strings.Contains(body, "argon2id") || strings.Contains(body, "password_hash") || strings.Contains(body, "pin_hash") {
		t.Fatalf("response body must never contain credential material: %s", body)
	}
}
