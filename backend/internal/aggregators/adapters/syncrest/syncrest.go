// Package syncrest is the SECOND adapter: a conventional SYNCHRONOUS REST
// platform, in the documented Swiggy/Zomato style.
//
// WHY IT EXISTS, AND IT IS NOT FOR COVERAGE. A one-adapter abstraction is
// indistinguishable from no abstraction: the internal contract silently takes
// the shape of whichever platform was written first, nobody notices, and the
// whole thing is redone when the second platform arrives. Same family as a test
// that constructs its own subject.
//
// So this adapter is deliberately shaped NOTHING like Beckn. Flat JSON instead
// of a context/message envelope. Integer paise on the wire instead of decimal
// rupee strings. Its own idea of an order id. A synchronous reply that carries
// the platform's DECISION instead of a bare acknowledgement followed by a
// callback. If `aggregators.Adapter` carries both of those without the core
// branching on platform identity, the abstraction is real — and that is
// discovered now for the price of a fake server, rather than later for the
// price of a rewrite.
//
// It is a WORKING ADAPTER AGAINST A WORKING FAKE, not a stub. A stub would
// prove the interface compiles, which was never in doubt.
package syncrest

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"net/http"
	"strings"
	"time"

	"github.com/holler/backend/internal/aggregators"
)

// Name is DATA, exactly as the Beckn adapter's is. The core stores it and never
// compares it to a literal.
const Name = "syncrest"

type Adapter struct {
	baseURL     string
	client      *http.Client
	resolveItem func(ctx context.Context, externalItemID string) (*string, error)
}

func New(
	baseURL string,
	client *http.Client,
	resolveItem func(ctx context.Context, externalItemID string) (*string, error),
) *Adapter {
	if client == nil {
		client = &http.Client{Timeout: 10 * time.Second}
	}
	return &Adapter{baseURL: strings.TrimSuffix(baseURL, "/"), client: client, resolveItem: resolveItem}
}

func (a *Adapter) Name() string { return Name }

// The inbound shape: FLAT, and paise on the wire. Nothing like Beckn's
// context/message envelope, on purpose — see the package doc.
type inboundOrder struct {
	OrderRef   string `json:"order_ref"`
	EventID    string `json:"event_id"`
	Status     string `json:"status"`
	Revision   int64  `json:"revision"`
	PlacedAt   string `json:"placed_at"`
	TotalPaise *int64 `json:"total_paise"`
	Items      []struct {
		SKU        string `json:"sku"`
		Name       string `json:"name"`
		Qty        int    `json:"qty"`
		PricePaise *int64 `json:"price_paise"`
	} `json:"items"`
}

func (a *Adapter) ParseInbound(ctx context.Context, raw []byte) (aggregators.InboundOrder, error) {
	var in inboundOrder
	if err := json.Unmarshal(raw, &in); err != nil {
		return aggregators.InboundOrder{}, fmt.Errorf("syncrest: decoding order: %w", err)
	}
	if in.EventID == "" {
		// No idempotency key means a retry produces a second document. Same
		// refusal the Beckn adapter makes on a missing message_id, reached
		// independently because both platforms retry.
		return aggregators.InboundOrder{}, fmt.Errorf("syncrest: order carries no event_id")
	}
	if in.OrderRef == "" {
		return aggregators.InboundOrder{}, fmt.Errorf("syncrest: order carries no order_ref")
	}

	var rawMap map[string]any
	if err := json.Unmarshal(raw, &rawMap); err != nil {
		return aggregators.InboundOrder{}, fmt.Errorf("syncrest: retaining raw payload: %w", err)
	}

	lines := make([]aggregators.InboundOrderLine, 0, len(in.Items))
	for i, item := range in.Items {
		// A nil resolution is NORMAL, not an error. Identical policy to the
		// other adapter and for the identical reason (ADR-022 rule 4) --
		// arrived at separately in each adapter rather than enforced by the
		// core, because the core must not know what a platform item is.
		var menuItemID *string
		if a.resolveItem != nil {
			resolved, err := a.resolveItem(ctx, item.SKU)
			if err != nil {
				return aggregators.InboundOrder{}, fmt.Errorf("syncrest: resolving %s: %w", item.SKU, err)
			}
			menuItemID = resolved
		}
		qty := item.Qty
		if qty <= 0 {
			qty = 1
		}
		lines = append(lines, aggregators.InboundOrderLine{
			LineNumber:       i + 1,
			ExternalItemID:   item.SKU,
			ExternalItemName: item.Name,
			MenuItemID:       menuItemID,
			Quantity:         qty,
			// ALREADY PAISE. No conversion, no rounding, nothing to get wrong
			// -- the contrast with Beckn's decimal rupee strings is exactly the
			// kind of difference the two-adapter rule exists to surface.
			StatedUnitPricePaise: item.PricePaise,
		})
	}

	return aggregators.InboundOrder{
		Platform:         Name,
		ExternalOrderID:  in.OrderRef,
		PlatformStatus:   in.Status,
		DocumentVersion:  revisionOrOne(in.Revision),
		MessageID:        in.EventID,
		RawPayload:       rawMap,
		StatedTotalPaise: in.TotalPaise,
		ReceivedAt:       in.PlacedAt,
		BusinessDate:     businessDateOf(in.PlacedAt),
		Lines:            lines,
	}, nil
}

// PushOrderState posts synchronously and reads the platform's decision from the
// RESPONSE.
//
// THE ASYMMETRY WITH BECKN IS THE WHOLE POINT AND IT DOES NOT REACH THE CORE.
// Here the answer is in the reply; on Beckn it arrives minutes later as a
// separate inbound callback. Both satisfy the same signature, because the
// signature promises only that the platform ACCEPTED THE MESSAGE. A contract
// that returned the platform's decision could not carry the async shape, and
// the core would have had to branch on platform identity to know which to
// expect -- which is precisely the failure the drift check is there to catch.
func (a *Adapter) PushOrderState(ctx context.Context, change aggregators.OrderStateChange) error {
	if a.baseURL == "" {
		return aggregators.ErrPlatformNotImplemented
	}
	body, err := json.Marshal(map[string]any{
		"order_ref":  change.ExternalOrderID,
		"status":     localStateToPlatformStatus(change.LocalState),
		"changed_at": change.OccurredAt,
	})
	if err != nil {
		return fmt.Errorf("syncrest: encoding state change: %w", err)
	}

	req, err := http.NewRequestWithContext(ctx, http.MethodPost, a.baseURL+"/orders/state", bytes.NewReader(body))
	if err != nil {
		return fmt.Errorf("syncrest: building request: %w", err)
	}
	req.Header.Set("content-type", "application/json")

	resp, err := a.client.Do(req)
	if err != nil {
		return fmt.Errorf("syncrest: sending state change: %w", err)
	}
	defer func() { _ = resp.Body.Close() }()

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return fmt.Errorf("syncrest: platform refused the state change with status %d", resp.StatusCode)
	}
	return nil
}

func localStateToPlatformStatus(local string) string {
	// Mapped in the ADAPTER, like Beckn's -- and to entirely different strings,
	// which is what makes the pair worth having.
	switch strings.ToUpper(local) {
	case "ACCEPTED", "CONFIRMED":
		return "CONFIRMED"
	case "READY", "PREPARED":
		return "READY_FOR_PICKUP"
	case "CANCELLED":
		return "CANCELLED"
	default:
		return strings.ToUpper(local)
	}
}

func revisionOrOne(r int64) int64 {
	if r <= 0 {
		return 1
	}
	return r
}

// The same known approximation the Beckn adapter documents: the edge recomputes
// the business date against outlet.day_start_time when the document lands.
func businessDateOf(timestamp string) string {
	if len(timestamp) >= 10 {
		return timestamp[:10]
	}
	return timestamp
}
