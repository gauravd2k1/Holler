// Package adapters is the ONE place that knows which platforms exist.
//
// WHY A REGISTRY RATHER THAN CONSTRUCTING THEM AT THE COMPOSITION ROOT. The
// first version wired the adapters in `cmd/api/main.go`, which meant main.go
// imported each platform package by name — and
// `scripts/check-aggregator-boundary.mjs` failed the build on it. That check
// was right, and not on a technicality: if the composition root names
// platforms, then adding a platform edits the composition root, and the
// "adding a platform is adding a package" property the whole abstraction exists
// for is already gone.
//
// So the root asks this package for whatever it has. Adding a platform is one
// entry below and a new adapter package — nothing else in the repository
// changes.
package adapters

import (
	"context"
	"net/http"

	"github.com/holler/backend/internal/aggregators"
	"github.com/holler/backend/internal/aggregators/adapters/beckn"
	"github.com/holler/backend/internal/aggregators/adapters/syncrest"
)

// Deps are what an adapter needs from the outside world, supplied once rather
// than reached for. Both are optional: an adapter with neither still
// constructs and still refuses to send, loudly, via ErrPlatformNotImplemented.
type Deps struct {
	// ResolveItem maps a platform item id onto a local menu_item, or returns
	// nil. A NIL RESULT IS NORMAL: an unmappable line is recorded, not refused
	// (ADR-022 rule 4).
	ResolveItem func(ctx context.Context, externalItemID string) (*string, error)

	// SyncRESTBaseURL is where the synchronous platform lives. EMPTY BY
	// DEFAULT AND THAT IS DELIBERATE: an adapter with nowhere to send returns
	// ErrPlatformNotImplemented rather than succeeding quietly. An
	// unimplemented adapter that returns a plausible nothing is the same defect
	// as a test suite that runs zero tests and reports success.
	SyncRESTBaseURL string

	HTTPClient *http.Client

	// BecknPush is where an outbound Beckn callback goes. Nil in M6: there is
	// no registry, no signing and no public counterparty until M6.1.
	BecknPush func(ctx context.Context, body []byte) error
}

// All returns every adapter this build knows about.
//
// THE ORDER IS NOT SIGNIFICANT AND NOTHING MAY MAKE IT SO. The service holds
// them in a map keyed by Name(); a caller that depended on position would be
// branching on platform identity by the back door.
func All(deps Deps) []aggregators.Adapter {
	return []aggregators.Adapter{
		beckn.New(deps.ResolveItem, deps.BecknPush),
		syncrest.New(deps.SyncRESTBaseURL, deps.HTTPClient, deps.ResolveItem),
	}
}
