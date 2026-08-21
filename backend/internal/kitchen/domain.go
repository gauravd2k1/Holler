// Package kitchen implements the Milestone 2 kitchen bounded context
// (ADR-014, docs/spec/kitchen.md): KOT ingest, kitchen stations, and printer
// config/routing.
//
// Per docs/spec/sync.md §50.1: the KOT ticket (contracts.Kot) is
// EDGE_TO_CLOUD — edge-authoritative. The cloud replays what the edge sends
// and never originates, renumbers or transitions a ticket on its own
// initiative. Station, Printer, MenuItemStation and StationPrinter are
// CLOUD_TO_EDGE config, the same authority split ADR-011 drew between
// RestaurantTable (config) and TableSession (operational).
package kitchen

import (
	"time"

	contracts "github.com/holler/contracts"
)

// Wire/domain shapes are the contract types, aliased rather than duplicated
// (CLAUDE.md: import contract types, never hand-roll mirrors).
type (
	Station               = contracts.Station
	MenuItemStation       = contracts.MenuItemStation
	Printer               = contracts.Printer
	StationPrinter        = contracts.StationPrinter
	PrinterConnectionKind = contracts.PrinterConnectionKind
	Kot                   = contracts.Kot
	KotStatus             = contracts.KotStatus
	KotTicketItem         = contracts.KotTicketItem
	PrinterRole           = contracts.PrinterRole
	PrinterRoleKind       = contracts.PrinterRoleKind
)

const (
	PrinterRoleKitchen = contracts.PrinterRoleKitchen
	PrinterRoleBill    = contracts.PrinterRoleBill
)

const (
	KotStatusNew          = contracts.KotStatusNew
	KotStatusAcknowledged = contracts.KotStatusAcknowledged
	KotStatusPreparing    = contracts.KotStatusPreparing
	KotStatusReady        = contracts.KotStatusReady
	KotStatusServed       = contracts.KotStatusServed
	KotStatusCancelled    = contracts.KotStatusCancelled
)

const (
	PrinterConnectionNetwork   = contracts.PrinterConnectionNetwork
	PrinterConnectionUSB       = contracts.PrinterConnectionUSB
	PrinterConnectionBluetooth = contracts.PrinterConnectionBluetooth
)

// ConfigBundle is the kitchen context's contribution to GET /sync/config
// (contracts 0.3.0, ADR-014): stations, item→station routing, printers,
// station→printer routing and printer roles, all newer than the caller's
// since_version. The full /sync/config route composes users/tables/
// categories/items from other bounded contexts too — assembling that
// composite response is cross-context wiring owned outside
// backend/internal/kitchen. This type is what kitchen hands to whatever
// composes the full response.
//
// PrinterRoles was added retroactively (M4 T4 delivery-fix task): the
// printer_role table has existed since 0.4.7 in both stores and in Go/TS,
// but this bundle never carried it, so a cloud-synced outlet had zero
// printer roles and print_invoice failed by name at every one.
type ConfigBundle struct {
	Stations        []Station
	ItemStations    []MenuItemStation
	Printers        []Printer
	StationPrinters []StationPrinter
	PrinterRoles    []PrinterRole
}

// NewStationInput is what a caller supplies to create a station. The id is
// caller-supplied (app-generated UUIDv7, §74) per
// packages/contracts/openapi/openapi.yaml POST /stations — the server never
// mints it, unlike backend/internal/menu's categories/items.
type NewStationInput struct {
	ID        string
	OutletID  string
	Code      string
	Name      string
	SortOrder int
	IsActive  bool
}

// NewPrinterInput mirrors POST /printers.
type NewPrinterInput struct {
	ID             string
	OutletID       string
	Name           string
	ConnectionKind PrinterConnectionKind
	Address        string
	PaperWidthMM   int
	IsActive       bool
}

// KotStatusTransition is the payload of a KotStatusEnvelope
// (packages/contracts/openapi/openapi.yaml KotStatusEnvelope), replayed from
// the edge.
type KotStatusTransition struct {
	Status            KotStatus
	ChangedAt         time.Time
	ChangedByDeviceID string
}
