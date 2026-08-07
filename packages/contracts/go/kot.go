package contracts

import "time"

type KotStatus string

const (
	KotStatusNew          KotStatus = "NEW"
	KotStatusAcknowledged KotStatus = "ACKNOWLEDGED"
	KotStatusPreparing    KotStatus = "PREPARING"
	KotStatusReady        KotStatus = "READY"
	KotStatusServed       KotStatus = "SERVED"
	KotStatusCancelled    KotStatus = "CANCELLED"
)

type KotTicketItem struct {
	OrderItemID string   `json:"order_item_id"`
	Name        string   `json:"name"`
	Quantity    int      `json:"quantity"`
	Modifiers   []string `json:"modifiers"`
	Notes       *string  `json:"notes"`
}

// Kot mirrors src/types/kot.ts KotSchema. One row per station ticket, not
// per order — see docs/spec/kitchen.md §12.
type Kot struct {
	ID                 string          `json:"id"`
	OrderID            string          `json:"order_id"`
	Station            string          `json:"station"`
	Sequence           int             `json:"sequence"`
	Status             KotStatus       `json:"status"`
	Items              []KotTicketItem `json:"items"`
	CreatedByDeviceID  string          `json:"created_by_device_id"`
	CreatedAt          time.Time       `json:"created_at"`
	UpdatedAt          time.Time       `json:"updated_at"`
	SchemaVersion      int             `json:"schema_version"`
}
