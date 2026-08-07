// Table contracts — Milestone 1 (ADR-011). Mirrors src/types/table.ts.
//
// Authority split (§50.1, no split-authority columns):
//   RestaurantTable — pure config, cloud→edge, versioned, replaced wholesale.
//   TableSession    — operational, edge→cloud, append-only replay.
package contracts

import "time"

type RestaurantTable struct {
	ID            string `json:"id"`
	OutletID      string `json:"outlet_id"`
	Section       string `json:"section"`
	Label         string `json:"label"`
	SeatCount     int    `json:"seat_count"`
	IsActive      bool   `json:"is_active"`
	ConfigVersion int    `json:"config_version"`
	SchemaVersion int    `json:"schema_version"`
}

type TableSessionState string

// Stored session states. AVAILABLE is not stored — a table with no open
// session is available (see TableDisplayState).
const (
	TableSessionStateOccupied       TableSessionState = "OCCUPIED"
	TableSessionStateOrdered        TableSessionState = "ORDERED"
	TableSessionStateKotSent        TableSessionState = "KOT_SENT"
	TableSessionStateFoodReady      TableSessionState = "FOOD_READY"
	TableSessionStateBillRequested  TableSessionState = "BILL_REQUESTED"
	TableSessionStatePaymentPending TableSessionState = "PAYMENT_PENDING"
	TableSessionStatePaid           TableSessionState = "PAID"
	TableSessionStateDirty          TableSessionState = "DIRTY"
	TableSessionStateClosed         TableSessionState = "CLOSED"
)

type TableDisplayState string

// Floor-plan states docs/spec/tables.md renders. RESERVED is spec-defined but
// nothing in Milestone 1 produces it (reservations are Milestone 9).
const (
	TableDisplayStateAvailable TableDisplayState = "AVAILABLE"
	TableDisplayStateReserved  TableDisplayState = "RESERVED"
)

type TableSession struct {
	ID              string            `json:"id"`
	OutletID        string            `json:"outlet_id"`
	TableID         string            `json:"table_id"`
	State           TableSessionState `json:"state"`
	CurrentOrderID  *string           `json:"current_order_id"`
	GuestCount      int               `json:"guest_count"`
	OpenedByUserID  *string           `json:"opened_by_user_id"`
	OpenedAt        time.Time         `json:"opened_at"`
	ClosedAt        *time.Time        `json:"closed_at"`
	Version         int               `json:"version"`
	CreatedAt       time.Time         `json:"created_at"`
	UpdatedAt       time.Time         `json:"updated_at"`
	SchemaVersion   int               `json:"schema_version"`
}
