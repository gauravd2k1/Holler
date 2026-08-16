// Printer contracts — added at 0.3.0 (ADR-014, Milestone 2).
// Mirrors src/types/printer.ts.
//
// Printer and StationPrinter are CONFIG (cloud→edge, versioned by
// config_version). PrintJob is EDGE-LOCAL and crosses no boundary — see the
// note on the type. Hardware specifics live in edge/printer behind an adapter
// interface and never leak into a domain service
// (docs/spec/hardware-printing.md §Hardware abstraction).
package contracts

import "time"

type PrinterConnectionKind string

// Transport, not vendor. A new printer brand is an edge/printer adapter detail;
// a new transport is a contract change. Label printers are excluded from
// Milestone 2 (§81) and get no variant here.
const (
	PrinterConnectionNetwork   PrinterConnectionKind = "ESCPOS_NETWORK"
	PrinterConnectionUSB       PrinterConnectionKind = "ESCPOS_USB"
	PrinterConnectionBluetooth PrinterConnectionKind = "ESCPOS_BLUETOOTH"
)

type Printer struct {
	ID       string `json:"id"`
	OutletID string `json:"outlet_id"`
	// Unique per outlet, not globally.
	Name           string                `json:"name"`
	ConnectionKind PrinterConnectionKind `json:"connection_kind"`
	// Transport-dependent, interpreted only by the matching adapter:
	// "192.168.1.50:9100" for network, a USB path for USB, a MAC for Bluetooth.
	Address       string `json:"address"`
	PaperWidthMM  int    `json:"paper_width_mm"` // 58 or 80
	IsActive      bool   `json:"is_active"`
	ConfigVersion int    `json:"config_version"`
	SchemaVersion int    `json:"schema_version"`
}

// StationPrinter is station → printer routing
// (docs/spec/hardware-printing.md §Printing: Tandoor → Printer A, Bar →
// Printer B). Many-to-many in both directions.
type StationPrinter struct {
	StationID     string `json:"station_id"`
	PrinterID     string `json:"printer_id"`
	ConfigVersion int    `json:"config_version"`
	SchemaVersion int    `json:"schema_version"`
}

// PrinterRoleKind is what a printer is eligible to print (0.4.7).
//
// KOTs route station -> station_printer; a bill has no station, so nothing in
// the contract could answer "which printer prints the bill" until this landed.
// KITCHEN does not replace station_printer routing — it classifies the device.
type PrinterRoleKind string

const (
	PrinterRoleKitchen PrinterRoleKind = "KITCHEN"
	PrinterRoleBill    PrinterRoleKind = "BILL"
)

// PrinterRole is a join row, not a column on Printer, deliberately: `printer`
// is built by struct literal in eight-plus places across three Rust crates
// plus these mirrors, so widening it breaks all of them at once — the cascade
// contracts 0.4.5 caused (docs/retro.md, 2026-08-15). Two rows also model a
// shared printer honestly, with no BOTH member for every reader to
// special-case.
//
// A printer with no row here has no role. Absence is never permission: an
// outlet with no BILL printer fails loudly at issue time.
type PrinterRole struct {
	PrinterID     string          `json:"printer_id"`
	Role          PrinterRoleKind `json:"role"`
	ConfigVersion int             `json:"config_version"`
	SchemaVersion int             `json:"schema_version"`
}

type PrintJobStatus string

const (
	PrintJobQueued   PrintJobStatus = "QUEUED"
	PrintJobPrinting PrintJobStatus = "PRINTING"
	PrintJobPrinted  PrintJobStatus = "PRINTED"
	PrintJobFailed   PrintJobStatus = "FAILED"
)

// PrintJob is EDGE-LOCAL and deliberately absent from the AggregateType list.
//
// This mirrors the refresh_token precedent (0.2.1): that table is cloud-only
// and was deliberately not made an AggregateType, because listing it would
// promise a sync direction for something that never syncs. PrintJob is the same
// case from the other side — a spool entry is a fact about one outlet's paper
// and one printer's socket. Giving it a direction would invite a replay path
// that must not exist.
//
// It is typed here for symmetry with the TypeScript binding, which the POS uses
// to render print failures to staff (docs/spec/hardware-printing.md: "Print
// failures must be visible to staff"). No cloud handler should construct one.
type PrintJob struct {
	ID            string         `json:"id"`
	KotID         string         `json:"kot_id"`
	PrinterID     string         `json:"printer_id"`
	Status        PrintJobStatus `json:"status"`
	AttemptCount  int            `json:"attempt_count"`
	LastError     *string        `json:"last_error"`
	CreatedAt     time.Time      `json:"created_at"`
	UpdatedAt     time.Time      `json:"updated_at"`
	SchemaVersion int            `json:"schema_version"`
}
