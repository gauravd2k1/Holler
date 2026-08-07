package crypto

import (
	"strings"
	"testing"
)

func TestHashPasswordVerifies(t *testing.T) {
	encoded, err := HashPassword("correct horse battery staple")
	if err != nil {
		t.Fatalf("hashing: %v", err)
	}
	if err := VerifyPassword("correct horse battery staple", encoded); err != nil {
		t.Fatalf("verifying correct password: %v", err)
	}
}

func TestVerifyPasswordRejectsWrongPassword(t *testing.T) {
	encoded, err := HashPassword("correct horse battery staple")
	if err != nil {
		t.Fatalf("hashing: %v", err)
	}
	if err := VerifyPassword("wrong password", encoded); err != ErrMismatch {
		t.Fatalf("want ErrMismatch, got %v", err)
	}
}

func TestHashPasswordSaltsEachHash(t *testing.T) {
	a, _ := HashPassword("same")
	b, _ := HashPassword("same")
	if a == b {
		t.Fatal("two hashes of the same password must differ (per-hash salt)")
	}
}

func TestEncodedHashHasArgon2idFormat(t *testing.T) {
	encoded, err := HashPassword("pw")
	if err != nil {
		t.Fatalf("hashing: %v", err)
	}
	if !strings.HasPrefix(encoded, "$argon2id$v=") {
		t.Fatalf("unexpected encoded format: %s", encoded)
	}
	if strings.Contains(encoded, "pw") {
		t.Fatal("encoded hash must not contain the plaintext")
	}
}

func TestVerifyPasswordRejectsMalformedHash(t *testing.T) {
	for _, bad := range []string{"", "not-a-hash", "$argon2i$v=19$m=1,t=1,p=1$c2FsdA$a2V5"} {
		if err := VerifyPassword("pw", bad); err != ErrMalformedHash {
			t.Fatalf("hash %q: want ErrMalformedHash, got %v", bad, err)
		}
	}
}
