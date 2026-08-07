package auth

import (
	"net/http"
	"net/http/httptest"
	"testing"
	"time"
)

func TestAuthenticate_MissingHeaderRejected(t *testing.T) {
	signer := NewTokenSigner([]byte("k"))
	handler := Authenticate(signer)(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		t.Fatal("handler must not run without a valid token")
	}))

	req := httptest.NewRequest(http.MethodGet, "/whatever", nil)
	rec := httptest.NewRecorder()
	handler.ServeHTTP(rec, req)

	if rec.Code != http.StatusUnauthorized {
		t.Fatalf("expected 401, got %d", rec.Code)
	}
}

func TestAuthenticate_ValidTokenSetsPrincipal(t *testing.T) {
	signer := NewTokenSigner([]byte("k"))
	principal := AuthenticatedPrincipal{UserID: "u1", Permissions: []Permission{PermissionOrderCreate}}
	token, err := signer.IssueAccessToken(principal, time.Minute)
	if err != nil {
		t.Fatalf("issue: %v", err)
	}

	var seen AuthenticatedPrincipal
	handler := Authenticate(signer)(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		p, ok := PrincipalFromContext(r.Context())
		if !ok {
			t.Fatal("expected principal in context")
		}
		seen = p
		w.WriteHeader(http.StatusOK)
	}))

	req := httptest.NewRequest(http.MethodGet, "/whatever", nil)
	req.Header.Set("Authorization", "Bearer "+token)
	rec := httptest.NewRecorder()
	handler.ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d", rec.Code)
	}
	if seen.UserID != "u1" {
		t.Fatalf("expected principal user id u1, got %s", seen.UserID)
	}
}

func TestRequirePermission_ForbidsMissingPermission(t *testing.T) {
	handler := RequirePermission(PermissionUserManage)(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		t.Fatal("handler must not run without required permission")
	}))

	principal := AuthenticatedPrincipal{UserID: "u1", Permissions: []Permission{PermissionOrderCreate}}
	req := httptest.NewRequest(http.MethodGet, "/users", nil)
	req = req.WithContext(WithPrincipal(req.Context(), principal))
	rec := httptest.NewRecorder()
	handler.ServeHTTP(rec, req)

	if rec.Code != http.StatusForbidden {
		t.Fatalf("expected 403, got %d", rec.Code)
	}
}

func TestRequirePermission_AllowsGrantedPermission(t *testing.T) {
	called := false
	handler := RequirePermission(PermissionUserManage)(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		called = true
		w.WriteHeader(http.StatusOK)
	}))

	principal := AuthenticatedPrincipal{UserID: "u1", Permissions: []Permission{PermissionUserManage}}
	req := httptest.NewRequest(http.MethodGet, "/users", nil)
	req = req.WithContext(WithPrincipal(req.Context(), principal))
	rec := httptest.NewRecorder()
	handler.ServeHTTP(rec, req)

	if !called || rec.Code != http.StatusOK {
		t.Fatalf("expected handler to run and return 200, got called=%v code=%d", called, rec.Code)
	}
}
