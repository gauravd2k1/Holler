// Package beckn is the ONDC/Beckn adapter: an ASYNCHRONOUS CALLBACK platform.
//
// THIS IS ONE OF EXACTLY TWO PLACES IN THE REPOSITORY WHERE BECKN VOCABULARY
// MAY APPEAR. `scripts/check-aggregator-boundary.mjs` fails the build if
// `beckn`, `ondc`, `on_confirm`, `on_status`, `bpp_id`, `X-Gateway-Authorization`
// or their siblings appear outside `internal/aggregators/adapters/`. That check
// is watched going RED before it is trusted — a boundary nobody has seen fail is
// not a boundary (M6 C8).
//
// WHAT MAKES THIS ADAPTER THE INTERESTING HALF OF THE PAIR. Beckn is
// asynchronous: a request is acknowledged immediately and the real answer
// arrives later as a separate inbound `on_*` callback. The other adapter speaks
// conventional synchronous REST, where the answer is in the response body. If
// `aggregators.Adapter` carries both WITHOUT the core branching on platform
// identity, the abstraction is real. That is the whole reason two adapters are
// built at once rather than one.
//
// SIGNING IS DELIBERATELY ABSENT. Ed25519 signing, the registry subscribe /
// on_subscribe challenge and the public HTTPS ingress are M6.1. In M6 the
// callback receive path is built and reachable LOCALLY ONLY: nothing here is
// exposed publicly, and a self-authored counterparty cannot falsify a signature
// scheme anyway — it can only agree with it.
package beckn

import (
	"context"
	"encoding/json"
	"fmt"
	"strings"

	"github.com/holler/backend/internal/aggregators"
)

// Name is DATA. The core stores it on the document and scopes uniqueness by it,
// and never compares it to a literal.
const Name = "ondc"

// Adapter implements aggregators.Adapter over Beckn's async callback model.
//
// resolveItem maps a platform item id onto a local menu_item, or returns nil.
// A NIL RESULT IS NORMAL, NOT AN ERROR: an unmappable line is recorded, not
// refused (ADR-022 rule 4). Injected rather than reached for, so this adapter
// stays testable against fixtures with no database.
type Adapter struct {
	resolveItem func(ctx context.Context, externalItemID string) (*string, error)
	// push is where an outbound message would go. In M6 it is a local sink:
	// there is no registry, no signing and no public counterparty.
	push func(ctx context.Context, body []byte) error
}

func New(
	resolveItem func(ctx context.Context, externalItemID string) (*string, error),
	push func(ctx context.Context, body []byte) error,
) *Adapter {
	return &Adapter{resolveItem: resolveItem, push: push}
}

func (a *Adapter) Name() string { return Name }

// The envelope, mirroring ONDC-RET-Specifications release-2.0.2. Field names are
// theirs; see fixtures/SOURCES.md for the artefacts and the commit they were
// pinned at.
type envelope struct {
	Context struct {
		Action        string `json:"action"`
		Version       string `json:"version"`
		TransactionID string `json:"transaction_id"`
		MessageID     string `json:"message_id"`
		Timestamp     string `json:"timestamp"`
		BppID         string `json:"bpp_id"`
	} `json:"context"`
	Message struct {
		Order struct {
			ID    string `json:"id"`
			State string `json:"state"`
			Quote struct {
				Price   price          `json:"price"`
				Breakup []breakupEntry `json:"breakup"`
			} `json:"quote"`
		} `json:"order"`
	} `json:"message"`
}

type price struct {
	Currency string `json:"currency"`
	Value    string `json:"value"`
}

type breakupEntry struct {
	ItemID   string `json:"@ondc/org/item_id"`
	Quantity struct {
		Count int `json:"count"`
	} `json:"@ondc/org/item_quantity"`
	Title     string `json:"title"`
	TitleType string `json:"@ondc/org/title_type"`
	Price     price  `json:"price"`
	Item      struct {
		Price price `json:"price"`
	} `json:"item"`
}

// ParseInbound turns an ONDC callback into this product's vocabulary.
func (a *Adapter) ParseInbound(ctx context.Context, raw []byte) (aggregators.InboundOrder, error) {
	var env envelope
	if err := json.Unmarshal(raw, &env); err != nil {
		return aggregators.InboundOrder{}, fmt.Errorf("beckn: decoding callback: %w", err)
	}
	if env.Context.MessageID == "" {
		// Without it there is no idempotency key, and a platform retry would
		// produce a second document. Refusing here is correct: this is a
		// malformed message, not an unmappable one.
		return aggregators.InboundOrder{}, fmt.Errorf("beckn: callback carries no message_id")
	}
	if env.Message.Order.ID == "" {
		return aggregators.InboundOrder{}, fmt.Errorf("beckn: callback carries no order id")
	}

	var rawMap map[string]any
	if err := json.Unmarshal(raw, &rawMap); err != nil {
		return aggregators.InboundOrder{}, fmt.Errorf("beckn: retaining raw payload: %w", err)
	}

	lines := make([]aggregators.InboundOrderLine, 0, len(env.Message.Order.Quote.Breakup))
	lineNumber := 0
	for _, b := range env.Message.Order.Quote.Breakup {
		// Only the item rows are order lines. delivery, packing, tax and
		// discount rows are charges against the order, not things the kitchen
		// makes, and turning them into lines would put "Delivery charges" on a
		// KOT.
		if b.TitleType != "item" {
			continue
		}
		lineNumber++

		unit, err := rupeeStringToPaise(b.Item.Price.Value)
		if err != nil {
			return aggregators.InboundOrder{}, fmt.Errorf("beckn: line %s: %w", b.ItemID, err)
		}

		// A nil resolution is NORMAL. The line is recorded with the platform's
		// own name so a human can see what was ordered even when nothing local
		// matches it.
		var menuItemID *string
		if a.resolveItem != nil {
			resolved, err := a.resolveItem(ctx, b.ItemID)
			if err != nil {
				return aggregators.InboundOrder{}, fmt.Errorf("beckn: resolving %s: %w", b.ItemID, err)
			}
			menuItemID = resolved
		}

		count := b.Quantity.Count
		if count <= 0 {
			count = 1
		}

		lines = append(lines, aggregators.InboundOrderLine{
			LineNumber:           lineNumber,
			ExternalItemID:       b.ItemID,
			ExternalItemName:     b.Title,
			MenuItemID:           menuItemID,
			Quantity:             count,
			StatedUnitPricePaise: unit,
		})
	}

	total, err := rupeeStringToPaise(env.Message.Order.Quote.Price.Value)
	if err != nil {
		return aggregators.InboundOrder{}, fmt.Errorf("beckn: order total: %w", err)
	}

	return aggregators.InboundOrder{
		Platform:        Name,
		ExternalOrderID: env.Message.Order.ID,
		// VERBATIM AND UNMAPPED. "Accepted", "In-progress", "Completed" are
		// ONDC's words and they stay ONDC's words: coercing an unrecognised
		// state into the nearest local one is how a state we have never seen
		// becomes a state we think we understand. A platform status never
		// writes order.status.
		PlatformStatus:   env.Message.Order.State,
		DocumentVersion:  1,
		MessageID:        env.Context.MessageID,
		RawPayload:       rawMap,
		StatedTotalPaise: total,
		ReceivedAt:       env.Context.Timestamp,
		BusinessDate:     businessDateOf(env.Context.Timestamp),
		Lines:            lines,
	}, nil
}

// PushOrderState sends a till-originated state change outward.
//
// IT REPORTS ACCEPTANCE, NOT THE PLATFORM'S DECISION, and that is what lets one
// interface carry both transport shapes. On Beckn the real answer arrives later
// as a separate inbound callback, which reaches the core through ParseInbound
// like any other message — so the core cannot tell an async platform from a
// sync one, which is the point.
func (a *Adapter) PushOrderState(ctx context.Context, change aggregators.OrderStateChange) error {
	body, err := json.Marshal(map[string]any{
		"context": map[string]any{
			"action":    "status",
			"version":   "2.0.2",
			"timestamp": change.OccurredAt,
		},
		"message": map[string]any{
			"order_id": change.ExternalOrderID,
			// Mapped HERE, in the adapter, precisely so a platform with a
			// different state machine cannot reach into the core.
			"state": localStateToBecknState(change.LocalState),
		},
	})
	if err != nil {
		return fmt.Errorf("beckn: encoding state change: %w", err)
	}
	if a.push == nil {
		return aggregators.ErrPlatformNotImplemented
	}
	return a.push(ctx, body)
}

func localStateToBecknState(local string) string {
	switch strings.ToUpper(local) {
	case "ACCEPTED", "CONFIRMED":
		return "Accepted"
	case "READY", "PREPARED":
		return "Order-picked-up"
	case "CANCELLED":
		return "Cancelled"
	default:
		// Unknown local states are passed through rather than guessed at. A
		// wrong state on a delivery platform sends a rider to a kitchen that
		// has not cooked anything.
		return local
	}
}

// rupeeStringToPaise converts ONDC's decimal STRING in rupees to integer paise.
//
// INTEGER ARITHMETIC ONLY, NEVER A FLOAT PARSE. ONDC prices are strings like
// "536.50", and `strconv.ParseFloat * 100` gives 53649.999999999993 for
// perfectly ordinary values. Money is integer paise end to end in this product
// (CLAUDE.md), and the one place a float could creep in is exactly here, at the
// boundary where somebody else's format meets ours.
func rupeeStringToPaise(v string) (*int64, error) {
	trimmed := strings.TrimSpace(v)
	if trimmed == "" {
		// Absent, not zero. A missing price is not a free item.
		return nil, nil
	}
	negative := strings.HasPrefix(trimmed, "-")
	trimmed = strings.TrimPrefix(trimmed, "-")

	whole, frac, found := strings.Cut(trimmed, ".")
	if !found {
		frac = "0"
	}
	if len(frac) > 2 {
		// More precision than paise. Refused rather than rounded: silently
		// dropping a digit off somebody else's money is how a rounding defect
		// enters at a boundary nobody re-reads.
		return nil, fmt.Errorf("price %q has more precision than paise", v)
	}
	for len(frac) < 2 {
		frac += "0"
	}

	var rupees, paise int64
	if _, err := fmt.Sscanf(whole, "%d", &rupees); err != nil {
		return nil, fmt.Errorf("price %q is not a number", v)
	}
	if _, err := fmt.Sscanf(frac, "%d", &paise); err != nil {
		return nil, fmt.Errorf("price %q has a bad fractional part", v)
	}
	total := rupees*100 + paise
	if negative {
		total = -total
	}
	return &total, nil
}

// businessDateOf takes the date portion of an RFC3339 timestamp.
//
// A KNOWN APPROXIMATION, NAMED RATHER THAN HIDDEN. The correct function is
// `compute_business_date` at the edge, which honours outlet.day_start_time so a
// trading night that crosses midnight stays one business date. This is the
// cloud ingest path and the outlet's day-start is not in hand here; the edge
// recomputes it when the document lands. Filed as a follow-up rather than
// silently shipped as if it were right.
func businessDateOf(timestamp string) string {
	if len(timestamp) >= 10 {
		return timestamp[:10]
	}
	return timestamp
}
