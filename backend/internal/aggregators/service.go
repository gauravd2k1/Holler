package aggregators

import (
	"context"
	"fmt"

	"github.com/jackc/pgx/v5"

	"github.com/holler/backend/internal/platform/httpx"
	"github.com/holler/backend/internal/platform/id"
	contracts "github.com/holler/contracts"
)

// Service receives inbound platform messages and turns them into documents.
//
// IT HOLDS ADAPTERS BY NAME AND NEVER BRANCHES ON WHICH ONE IT HAS. The
// registry below is a map lookup, not a switch: adding a platform is adding a
// map entry and an adapter package, and if it ever requires a change in this
// file the abstraction has failed. `scripts/check-aggregator-boundary.mjs`
// enforces the half of that it can see.
type Service struct {
	repo     Repository
	adapters map[string]Adapter
}

func NewService(repo Repository, adapters ...Adapter) *Service {
	byName := make(map[string]Adapter, len(adapters))
	for _, a := range adapters {
		byName[a.Name()] = a
	}
	return &Service{repo: repo, adapters: byName}
}

// Scope is the tenant, outlet and platform a callback is being processed for.
//
// IT TRAVELS ON THE CONTEXT BECAUSE AN ADAPTER MUST NOT LEARN THESE AS
// PARAMETERS. `Adapter.ParseInbound` takes raw bytes and nothing else, which is
// what keeps a platform adapter from reaching into tenancy; but the item
// resolver an adapter is handed DOES need the scope, because
// `aggregator_item_map` is keyed by tenant, outlet and platform and a lookup
// that ignored any of them would cross a tenancy boundary -- the uniqueness rule
// in the contract rubric, reached from the other side.
//
// So the scope is put here by the service, which legitimately knows it, and read
// by the composition root's resolver closure. Nothing in between can see it.
type Scope struct {
	TenantID string
	OutletID string
	Platform string
}

type scopeKey struct{}

// WithScope attaches the scope a callback is being processed under.
func WithScope(ctx context.Context, scope Scope) context.Context {
	return context.WithValue(ctx, scopeKey{}, scope)
}

// ScopeFromContext returns the scope, if a service put one there.
//
// A MISSING SCOPE MUST NOT BE READ AS "NO TENANT". A resolver that got no scope
// has to refuse to resolve rather than query without one: resolving across every
// tenant's mappings is worse than resolving nothing, and an unmapped line is
// already a normal, recorded outcome.
func ScopeFromContext(ctx context.Context) (Scope, bool) {
	scope, ok := ctx.Value(scopeKey{}).(Scope)
	return scope, ok
}

// ReceiveCallback is the inbound path, and in M6 it is REACHABLE LOCALLY ONLY.
//
// No public endpoint, no registry, no signature verification. Ed25519 signing,
// the registry subscription handshake and the public HTTPS ingress with its
// security gate are M6.1 in full -- deferred rather than deleted so they are
// not rediscovered, and deferred because a self-authored counterparty cannot
// falsify a signature scheme, it can only agree with it.
//
// WHAT THAT MEANS FOR ANYONE MOUNTING THIS: it must not be exposed publicly. An
// unauthenticated, unsigned endpoint that writes documents is exactly what the
// M6.1 gate exists for.
func (s *Service) ReceiveCallback(ctx context.Context, tenantID, outletID, platform string, raw []byte) (Receipt, error) {
	adapter, ok := s.adapters[platform]
	if !ok {
		// Named, not guessed. An unknown platform is a configuration error the
		// caller can act on, and it is NEVER silently accepted -- a document
		// stored under a platform nothing can push back to is a delivery order
		// nobody will ever mark ready.
		return Receipt{}, fmt.Errorf("%w: no adapter registered for %q", httpx.ErrInvalidInput, platform)
	}

	// The scope the adapter's item resolver needs, and the ONLY way it gets it.
	ctx = WithScope(ctx, Scope{TenantID: tenantID, OutletID: outletID, Platform: platform})

	inbound, err := adapter.ParseInbound(ctx, raw)
	if err != nil {
		// The message was unintelligible. Distinct from a message whose lines
		// could not be MAPPED, which is normal and reaches the store.
		return Receipt{}, fmt.Errorf("%w: %s", httpx.ErrInvalidInput, err.Error())
	}

	var receipt Receipt
	err = s.repo.WithTx(ctx, func(tx pgx.Tx) error {
		fresh, err := s.repo.RecordCallback(ctx, tx, id.New(), tenantID, inbound.Platform, inbound.MessageID, "inbound_order")
		if err != nil {
			return err
		}
		if !fresh {
			// A RETRY, AND IT IS A SUCCESS. Platforms retry on any doubt; the
			// correct answer to a duplicate is "yes, I have it", not an error
			// that provokes another retry.
			receipt = Receipt{Duplicate: true, ExternalOrderID: inbound.ExternalOrderID}
			return nil
		}

		lineIDs := make([]string, len(inbound.Lines))
		for i := range lineIDs {
			lineIDs[i] = id.New()
		}
		doc := ToAggregatorOrder(id.New(), tenantID, outletID, inbound, lineIDs)

		applied, err := s.repo.UpsertDocument(ctx, tx, doc)
		if err != nil {
			return err
		}
		receipt = Receipt{
			ExternalOrderID: doc.ExternalOrderID,
			Applied:         applied,
			UnmappedLines:   countUnmapped(doc.Lines),
		}
		return nil
	})
	if err != nil {
		return Receipt{}, err
	}
	return receipt, nil
}

// Receipt is what the caller learns. It reports UnmappedLines because that is
// operationally interesting and NOT an error: the document is stored either
// way, and somebody has to know a dish arrived that nothing local matches.
type Receipt struct {
	ExternalOrderID string `json:"external_order_id"`
	Applied         bool   `json:"applied"`
	Duplicate       bool   `json:"duplicate"`
	UnmappedLines   int    `json:"unmapped_lines"`
}

// countUnmapped reports how many lines nothing local matched. A count, not a
// failure: the document is stored either way (ADR-022 rule 4), and this is the
// number a buyer or a manager acts on.
func countUnmapped(lines []contracts.AggregatorOrderLine) int {
	n := 0
	for _, l := range lines {
		if l.MenuItemID == nil {
			n++
		}
	}
	return n
}
