// Package aggregators is the platform-agnostic core of M6 Phase C.
//
// NOTHING IN THIS PACKAGE MAY NAME A PLATFORM. Not a platform's name, not one
// of its callback actions, not one of its signing headers. That is not a style
// rule: `scripts/check-aggregator-boundary.mjs` holds the list and fails the
// build on any of them outside `internal/aggregators/adapters/`, and the check
// is watched going RED before it is trusted (M6 C8).
//
// THE LIST LIVES IN THE CHECK AND NOT IN THIS COMMENT, DELIBERATELY. Writing
// the forbidden words here would put them in the core, which is the thing being
// forbidden — and it is how the next person learns that naming a platform in
// this package is normal. Read the script.
//
// WHY THE BOUNDARY IS STRUCTURAL RATHER THAN A CONVENTION. A one-adapter
// abstraction is indistinguishable from no abstraction: the internal contract
// silently takes the shape of whichever platform was written first, and nobody
// notices until the second one arrives and the whole thing is redone. Same
// family as a test that constructs its own subject. So two adapters are built
// at once — one asynchronous callback platform, one conventional synchronous
// REST platform — and if the contract below carries both WITHOUT a branch on
// platform identity anywhere in this package, it is right.
package aggregators

import (
	"context"
	"errors"

	contracts "github.com/holler/contracts"
)

// ErrPlatformNotImplemented is what an unimplemented adapter returns from its
// FIRST LINE.
//
// IT NEVER RETURNS SUCCESS, NEVER RETURNS EMPTY, AND IS NEVER SELECTABLE BY
// DEFAULT. An unimplemented adapter that returns a plausible-looking nothing is
// the same defect as a test that runs zero tests and reports success: an empty
// order list from a platform that was never called is indistinguishable from a
// quiet Tuesday, and the first person to notice is a restaurant wondering where
// its delivery orders went.
var ErrPlatformNotImplemented = errors.New("aggregators: platform adapter not implemented")

// InboundOrder is what an adapter produces from whatever its platform sent.
//
// THE ADAPTER OWNS THE TRANSLATION; THE CORE NEVER SEES A PLATFORM PAYLOAD.
// Every field here is in this product's vocabulary — integer paise, our own
// idea of a line — except the two that are deliberately verbatim:
// PlatformStatus and RawPayload. Those stay in the platform's own words because
// coercing an unrecognised status into the nearest local one is how a state we
// have never seen becomes a state we think we understand.
type InboundOrder struct {
	// Platform is DATA, not a discriminator. The core stores it, scopes
	// uniqueness by it, and never branches on it. A `switch` on this value
	// anywhere in this package is the abstraction failing.
	Platform string

	ExternalOrderID string
	PlatformStatus  string
	DocumentVersion int64

	// The platform's own message identifier, used for idempotency. Platforms
	// retry; a retried callback must produce no second document.
	MessageID string

	RawPayload       map[string]any
	StatedTotalPaise *int64
	ReceivedAt       string
	BusinessDate     string

	Lines []InboundOrderLine
}

// InboundOrderLine carries what the platform said AND what we resolved it to,
// both, always.
//
// MenuItemID IS NULLABLE AND THE NULL IS LOAD-BEARING (ADR-022 rule 4). A line
// that cannot be mapped is RECORDED, NOT REFUSED — the grn_gap precedent from
// ADR-019, which is itself "stock never blocks a sale" pointed at the inbound
// side. Refusing a delivery order that is already cooking is the outage, not
// the protection.
type InboundOrderLine struct {
	LineNumber           int
	ExternalItemID       string
	ExternalItemName     string
	MenuItemID           *string
	Quantity             int
	StatedUnitPricePaise *int64
}

// OrderStateChange is a state transition ORIGINATING AT THE TILL, travelling
// outward to a platform.
//
// The direction matters and is the whole of ADR-022's state round-trip: accept
// / ready / picked-up originate locally and ride the edge→cloud→platform path
// on the edge-authoritative `order`. Platform-originated changes arrive on the
// cloud-authoritative document instead, and are SURFACED to the till rather
// than silently applied.
type OrderStateChange struct {
	ExternalOrderID string
	// LocalState is OUR vocabulary. Mapping it onto a platform's states is the
	// adapter's job, precisely so that adding a platform with a different state
	// machine cannot reach into the core.
	LocalState string
	OccurredAt string
}

// Adapter is the whole contract between the core and a platform.
//
// IT MUST CARRY BOTH SHAPES WITHOUT SPECIAL-CASING EITHER: an asynchronous
// callback platform, where PushOrderState returns as soon as the request is
// accepted and the real answer arrives later on the callback path, and a
// synchronous REST platform, where the answer is in the response.
//
// The seam that makes that possible is that NOTHING HERE RETURNS A PLATFORM'S
// REPLY. PushOrderState reports only whether the platform accepted the message.
// A sync adapter has its answer immediately and reports it; an async adapter
// reports acceptance and the eventual on_* callback arrives through
// ParseInbound like any other inbound message. The core cannot tell the two
// apart, which is the point — and it is why this method returns an error rather
// than a status.
type Adapter interface {
	// Name identifies the adapter for logging and for the `platform` column. It
	// is data, and the core never compares it to a literal.
	Name() string

	// ParseInbound turns a platform's raw message into this product's
	// vocabulary. Returning an error means the message was unintelligible;
	// returning an InboundOrder with unmapped lines is NORMAL and must not be
	// treated as failure.
	ParseInbound(ctx context.Context, raw []byte) (InboundOrder, error)

	// PushOrderState sends a till-originated state change outward. It reports
	// whether the PLATFORM ACCEPTED THE MESSAGE, never what the platform
	// decided — see the interface doc for why that distinction is what lets one
	// contract carry both transport shapes.
	PushOrderState(ctx context.Context, change OrderStateChange) error
}

// ToAggregatorOrder maps an adapter's output onto the contract shape.
//
// THE CORE'S ONLY JOB IN THE INBOUND PATH, and it is deliberately dull: no
// validation that would refuse a document, no defaulting that would invent a
// value, no branch on Platform. The one piece of policy is that an unmapped
// line survives.
func ToAggregatorOrder(id, tenantID, outletID string, in InboundOrder, lineIDs []string) contracts.AggregatorOrder {
	lines := make([]contracts.AggregatorOrderLine, 0, len(in.Lines))
	for i, l := range in.Lines {
		lines = append(lines, contracts.AggregatorOrderLine{
			ID:                   lineIDs[i],
			AggregatorOrderID:    id,
			LineNumber:           l.LineNumber,
			ExternalItemID:       l.ExternalItemID,
			ExternalItemName:     l.ExternalItemName,
			MenuItemID:           l.MenuItemID,
			Quantity:             l.Quantity,
			StatedUnitPricePaise: l.StatedUnitPricePaise,
			SchemaVersion:        1,
		})
	}
	return contracts.AggregatorOrder{
		ID:               id,
		TenantID:         tenantID,
		OutletID:         outletID,
		Platform:         in.Platform,
		ExternalOrderID:  in.ExternalOrderID,
		PlatformStatus:   in.PlatformStatus,
		DocumentVersion:  in.DocumentVersion,
		RawPayload:       in.RawPayload,
		StatedTotalPaise: in.StatedTotalPaise,
		ReceivedAt:       in.ReceivedAt,
		BusinessDate:     in.BusinessDate,
		// Both nil on arrival, and that is the operational state "arrived, not
		// yet accepted". Creation of the local order is OPERATOR-CONFIRMED
		// (ADR-022 addendum §2), so nothing here may fill these in.
		AcceptedAt:    nil,
		LocalOrderID:  nil,
		Lines:         lines,
		SchemaVersion: 1,
	}
}
