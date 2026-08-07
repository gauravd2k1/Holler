package auth

import (
	"testing"
	"time"
)

func TestTokenSigner_IssueAndVerify(t *testing.T) {
	signer := NewTokenSigner([]byte("a-test-signing-key"))
	principal := AuthenticatedPrincipal{UserID: "u1", TenantID: "t1", OutletID: "o1", Permissions: []Permission{PermissionOrderCreate}}

	token, err := signer.IssueAccessToken(principal, time.Minute)
	if err != nil {
		t.Fatalf("issue: %v", err)
	}

	got, err := signer.VerifyAccessToken(token)
	if err != nil {
		t.Fatalf("verify: %v", err)
	}
	if got.UserID != principal.UserID || !got.HasPermission(string(PermissionOrderCreate)) {
		t.Fatalf("round-tripped principal mismatch: %+v", got)
	}
}

func TestTokenSigner_RejectsTamperedToken(t *testing.T) {
	signer := NewTokenSigner([]byte("a-test-signing-key"))
	principal := AuthenticatedPrincipal{UserID: "u1"}
	token, _ := signer.IssueAccessToken(principal, time.Minute)

	tampered := token + "x"
	if _, err := signer.VerifyAccessToken(tampered); err != ErrInvalidToken {
		t.Fatalf("expected ErrInvalidToken for tampered token, got %v", err)
	}
}

func TestTokenSigner_RejectsExpiredToken(t *testing.T) {
	signer := NewTokenSigner([]byte("a-test-signing-key"))
	principal := AuthenticatedPrincipal{UserID: "u1"}
	token, _ := signer.IssueAccessToken(principal, -time.Minute)

	if _, err := signer.VerifyAccessToken(token); err != ErrInvalidToken {
		t.Fatalf("expected ErrInvalidToken for expired token, got %v", err)
	}
}

func TestRefreshStore_IssueAndRotate(t *testing.T) {
	store := NewRefreshStore()
	token, err := store.Issue("user-1", "outlet-1", time.Hour)
	if err != nil {
		t.Fatalf("issue: %v", err)
	}

	next, userID, outletID, err := store.Rotate(token, time.Hour)
	if err != nil {
		t.Fatalf("rotate: %v", err)
	}
	if userID != "user-1" || outletID != "outlet-1" {
		t.Fatalf("unexpected identity from rotate: %s %s", userID, outletID)
	}
	if next == token {
		t.Fatal("expected a distinct successor token")
	}
}

func TestRefreshStore_ReuseInvalidatesChain(t *testing.T) {
	store := NewRefreshStore()
	token, _ := store.Issue("user-1", "outlet-1", time.Hour)
	next, _, _, err := store.Rotate(token, time.Hour)
	if err != nil {
		t.Fatalf("rotate: %v", err)
	}

	if _, _, _, err := store.Rotate(token, time.Hour); err != ErrInvalidToken {
		t.Fatalf("expected reuse of rotated token to be rejected, got %v", err)
	}
	if _, _, _, err := store.Rotate(next, time.Hour); err != ErrInvalidToken {
		t.Fatalf("expected reuse to invalidate the whole chain, got %v", err)
	}
}

func TestRefreshStore_RevokeInvalidatesFamily(t *testing.T) {
	store := NewRefreshStore()
	token, _ := store.Issue("user-1", "outlet-1", time.Hour)
	store.Revoke(token)

	if _, _, _, err := store.Rotate(token, time.Hour); err != ErrInvalidToken {
		t.Fatalf("expected rotate after revoke to fail, got %v", err)
	}
}
