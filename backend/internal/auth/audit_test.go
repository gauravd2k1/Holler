package auth

import (
	"context"
	"testing"
)

// fakeAuditWriter captures the AuditEvent that would have hit Postgres.
type fakeAuditWriter struct {
	last AuditEvent
}

func (f *fakeAuditWriter) RecordAudit(ctx context.Context, e AuditEvent) error {
	f.last = e
	return nil
}

// TestAudit_RedactsPasswordHash proves that auditing a user update whose old
// value contains password_hash writes a row with that key absent
// (docs/spec/security-rbac.md §Audit, ADR-011).
func TestAudit_RedactsPasswordHash(t *testing.T) {
	writer := &fakeAuditWriter{}
	auditor := NewAuditor(writer)

	err := auditor.Audit(context.Background(), AuditInput{
		TenantID:   "tenant-1",
		Action:     "user.update",
		EntityType: "app_user",
		OldValue: map[string]interface{}{
			"email":         "old@example.com",
			"password_hash": "$argon2id$v=19$m=65536,t=2,p=4$salt$hash",
		},
		NewValue: map[string]interface{}{
			"email":    "new@example.com",
			"pin_hash": "$argon2id$v=19$m=65536,t=2,p=4$salt2$hash2",
		},
	})
	if err != nil {
		t.Fatalf("audit: %v", err)
	}

	if _, present := writer.last.OldValue["password_hash"]; present {
		t.Fatal("expected password_hash to be redacted from OldValue")
	}
	if _, present := writer.last.NewValue["pin_hash"]; present {
		t.Fatal("expected pin_hash to be redacted from NewValue")
	}
	if writer.last.OldValue["email"] != "old@example.com" {
		t.Error("expected non-redacted fields to survive")
	}
	if writer.last.NewValue["email"] != "new@example.com" {
		t.Error("expected non-redacted fields to survive")
	}
}

func TestRedact_NilMapStaysNil(t *testing.T) {
	if got := redact(nil); got != nil {
		t.Fatalf("expected nil in, nil out, got %v", got)
	}
}
