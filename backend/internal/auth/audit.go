package auth

import (
	"context"
	"time"

	"github.com/holler/backend/internal/platform/id"
)

// AuditRecorder is the interface other bounded contexts depend on to record
// sensitive actions, so they can be developed against a fake without pulling
// in this package's Postgres dependency.
type AuditRecorder interface {
	Audit(ctx context.Context, input AuditInput) error
}

// AuditInput is what a caller supplies; ID/OccurredAt are assigned by the
// helper.
type AuditInput struct {
	TenantID    string
	OutletID    *string
	ActorUserID *string
	DeviceID    *string
	Action      string
	EntityType  string
	EntityID    *string
	OldValue    map[string]interface{}
	NewValue    map[string]interface{}
	Reason      *string
}

// AuditWriter persists a redacted AuditEvent. *Repository satisfies this.
type AuditWriter interface {
	RecordAudit(ctx context.Context, e AuditEvent) error
}

// Auditor is the concrete AuditRecorder. Construct once and share it with any
// context that needs to record sensitive actions.
type Auditor struct {
	writer AuditWriter
	now    func() time.Time
}

func NewAuditor(writer AuditWriter) *Auditor {
	return &Auditor{writer: writer, now: time.Now}
}

// Audit redacts AuditRedactedFields from OldValue/NewValue and persists the
// event. CRITICAL: this is the only path allowed to reach RecordAudit, so no
// caller can accidentally bypass redaction (docs/spec/security-rbac.md
// §Audit, ADR-011).
func (a *Auditor) Audit(ctx context.Context, input AuditInput) error {
	event := AuditEvent{
		ID:          id.New(),
		TenantID:    input.TenantID,
		OutletID:    input.OutletID,
		ActorUserID: input.ActorUserID,
		DeviceID:    input.DeviceID,
		Action:      input.Action,
		EntityType:  input.EntityType,
		EntityID:    input.EntityID,
		OldValue:    redact(input.OldValue),
		NewValue:    redact(input.NewValue),
		Reason:      input.Reason,
		OccurredAt:  a.now().UTC(),
	}
	return a.writer.RecordAudit(ctx, event)
}

// redact returns a shallow copy of v with every key in AuditRedactedFields
// removed. A nil input returns nil.
func redact(v map[string]interface{}) map[string]interface{} {
	if v == nil {
		return nil
	}
	redacted := make(map[string]interface{}, len(v))
	for k, val := range v {
		if isRedactedField(k) {
			continue
		}
		redacted[k] = val
	}
	return redacted
}

func isRedactedField(key string) bool {
	for _, f := range AuditRedactedFields {
		if f == key {
			return true
		}
	}
	return false
}
