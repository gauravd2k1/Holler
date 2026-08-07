// Package contracts mirrors packages/contracts/src/types/*.ts. Frozen at
// Milestone 0.5 (ADR-008); round-trip-tested against packages/contracts/fixtures
// by CI's contract-drift check. Money fields are integer paise (CLAUDE.md).
package contracts

import "time"

type OrderType string

const (
	OrderTypeDineIn      OrderType = "DINE_IN"
	OrderTypeTakeaway    OrderType = "TAKEAWAY"
	OrderTypeDelivery    OrderType = "DELIVERY"
	OrderTypeAggregator  OrderType = "AGGREGATOR"
	OrderTypeQR          OrderType = "QR"
	OrderTypeRoomService OrderType = "ROOM_SERVICE"
	OrderTypeCatering    OrderType = "CATERING"
)

type OrderSource string

const (
	OrderSourcePOS              OrderSource = "POS"
	OrderSourceQR               OrderSource = "QR"
	OrderSourceAggregatorZomato OrderSource = "AGGREGATOR_ZOMATO"
	OrderSourceAggregatorSwiggy OrderSource = "AGGREGATOR_SWIGGY"
	OrderSourceDirect           OrderSource = "DIRECT"
)

// OrderStatus mirrors docs/domain/ORDER_STATE_MACHINE.md. Do not add states
// here without updating that document and bumping SchemaVersion.
type OrderStatus string

const (
	OrderStatusDraft          OrderStatus = "DRAFT"
	OrderStatusConfirmed      OrderStatus = "CONFIRMED"
	OrderStatusSentToKitchen  OrderStatus = "SENT_TO_KITCHEN"
	OrderStatusPreparing      OrderStatus = "PREPARING"
	OrderStatusReady          OrderStatus = "READY"
	OrderStatusServed         OrderStatus = "SERVED"
	OrderStatusBilled         OrderStatus = "BILLED"
	OrderStatusPaid           OrderStatus = "PAID"
	OrderStatusClosed         OrderStatus = "CLOSED"
	OrderStatusCancelled      OrderStatus = "CANCELLED"
)

type PaymentStatus string

const (
	PaymentStatusUnpaid         PaymentStatus = "UNPAID"
	PaymentStatusPartiallyPaid  PaymentStatus = "PARTIALLY_PAID"
	PaymentStatusPaid           PaymentStatus = "PAID"
	PaymentStatusRefunded       PaymentStatus = "REFUNDED"
)

type OrderItemModifier struct {
	ModifierID      string `json:"modifier_id"`
	GroupName       string `json:"group_name"`
	OptionName      string `json:"option_name"`
	PriceDeltaPaise int64  `json:"price_delta_paise"`
}

type OrderItem struct {
	ID              string              `json:"id"`
	MenuItemID      string              `json:"menu_item_id"`
	VariantID       *string             `json:"variant_id"`
	Quantity        int                 `json:"quantity"`
	UnitPricePaise  int64               `json:"unit_price_paise"`
	LineTotalPaise  int64               `json:"line_total_paise"`
	Modifiers       []OrderItemModifier `json:"modifiers"`
	Notes           *string             `json:"notes"`
}

type OrderCustomer struct {
	Name  *string `json:"name"`
	Phone *string `json:"phone"`
}

type OrderRider struct {
	Name   string `json:"name"`
	Phone  string `json:"phone"`
	Status string `json:"status"`
}

type OrderTimestamps struct {
	CreatedAt   time.Time  `json:"created_at"`
	ConfirmedAt *time.Time `json:"confirmed_at"`
	UpdatedAt   time.Time  `json:"updated_at"`
}

// CanonicalOrder mirrors src/types/order.ts CanonicalOrderSchema. Every
// order-producing channel normalizes into this shape (docs/spec/ordering.md).
type CanonicalOrder struct {
	HollerOrderID    string      `json:"holler_order_id"`
	ExternalOrderID  *string     `json:"external_order_id"`
	Source           OrderSource `json:"source"`
	OutletID         string      `json:"outlet_id"`

	OrderType OrderType   `json:"order_type"`
	Status    OrderStatus `json:"status"`
	TableID   *string     `json:"table_id"`

	Customer          *OrderCustomer `json:"customer"`
	DeliveryAddress   *string        `json:"delivery_address"`

	Items []OrderItem `json:"items"`

	SubtotalPaise           int64 `json:"subtotal_paise"`
	DiscountPaise           int64 `json:"discount_paise"`
	PackagingPaise          int64 `json:"packaging_paise"`
	DeliveryChargePaise     int64 `json:"delivery_charge_paise"`
	TaxesPaise              int64 `json:"taxes_paise"`
	AggregatorDiscountPaise int64 `json:"aggregator_discount_paise"`
	MerchantDiscountPaise   int64 `json:"merchant_discount_paise"`
	TotalPaise              int64 `json:"total_paise"`

	PaymentStatus PaymentStatus `json:"payment_status"`
	PaymentSource *string       `json:"payment_source"`

	PreparationTimeMinutes *int        `json:"preparation_time_minutes"`
	Rider                  *OrderRider `json:"rider"`

	Timestamps    OrderTimestamps        `json:"timestamps"`
	SourcePayload map[string]interface{} `json:"source_payload"` // raw external payload, audit only

	SchemaVersion int `json:"schema_version"`
}

// OrderCommand mirrors the discriminated union in src/types/order.ts. Go
// lacks native discriminated unions, so Type selects which optional fields
// apply; callers switch on Type.
type OrderCommand struct {
	Type    string  `json:"type"` // CONFIRM_ORDER | SEND_TO_KITCHEN | MARK_READY | MARK_SERVED | BILL_ORDER | CANCEL_ORDER
	OrderID string  `json:"order_id"`
	Reason  *string `json:"reason,omitempty"` // CANCEL_ORDER only
}
