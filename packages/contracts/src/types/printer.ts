// Printer contracts — added at 0.3.0 (ADR-014, Milestone 2).
//
// Two different things live here, on opposite sides of the §50.1 authority
// line, and the split is deliberate:
//
//   Printer, StationPrinter  — CONFIG, cloud→edge. Which printers an outlet has
//                              and which station prints where is a management
//                              decision, versioned by config_version.
//   PrintJob                 — EDGE-LOCAL. The spool. It never crosses a
//                              boundary in either direction (see below).
//
// Hardware specifics (ESC/POS byte sequences, USB descriptors, socket dialling)
// live in edge/printer behind an adapter interface and must never leak into a
// domain service — docs/spec/hardware-printing.md §Hardware abstraction.

import { z } from "zod";

// Transport, not vendor. A new printer brand is an edge/printer adapter detail;
// a new *transport* is a contract change, which is why only this axis is frozen.
// Label printers are excluded from Milestone 2 (§81) and get no variant here.
export const PrinterConnectionKindSchema = z.enum([
  "ESCPOS_NETWORK",
  "ESCPOS_USB",
  "ESCPOS_BLUETOOTH",
]);
export type PrinterConnectionKind = z.infer<typeof PrinterConnectionKindSchema>;

export const PrinterPaperWidthSchema = z.union([z.literal(58), z.literal(80)]);
export type PrinterPaperWidth = z.infer<typeof PrinterPaperWidthSchema>;

export const PrinterSchema = z.object({
  id: z.string().uuid(),
  outlet_id: z.string().uuid(),
  name: z.string().min(1), // unique per outlet, not globally
  connection_kind: PrinterConnectionKindSchema,
  // Transport-dependent and interpreted only by the matching adapter:
  // "192.168.1.50:9100" for network, a USB path for USB, a MAC for Bluetooth.
  address: z.string().min(1),
  paper_width_mm: PrinterPaperWidthSchema,
  is_active: z.boolean(),
  config_version: z.number().int(),
  schema_version: z.literal(1),
});
export type Printer = z.infer<typeof PrinterSchema>;

// Station → printer routing (docs/spec/hardware-printing.md §Printing:
// Tandoor → Printer A, Bar → Printer B). Many-to-many: one printer can serve
// two stations, and one station can fan out to two printers.
export const StationPrinterSchema = z.object({
  station_id: z.string().uuid(),
  printer_id: z.string().uuid(),
  config_version: z.number().int(),
  schema_version: z.literal(1),
});
export type StationPrinter = z.infer<typeof StationPrinterSchema>;

export const PrintJobStatusSchema = z.enum([
  "QUEUED",
  "PRINTING",
  "PRINTED",
  "FAILED",
]);
export type PrintJobStatus = z.infer<typeof PrintJobStatusSchema>;

// PrintJob is EDGE-LOCAL and deliberately absent from AggregateTypeSchema.
//
// This mirrors the refresh_token precedent (0.2.1): that table is cloud-only
// and was deliberately not made an AggregateType, because listing it would
// have promised a sync direction for something that never syncs. PrintJob is
// the same case from the other side. A spool entry is a fact about one
// outlet's paper and one printer's socket; the cloud has no use for it and no
// authority over it, and giving it a direction would invite a replay path that
// must not exist.
//
// It is typed here anyway — not for the wire, but because the POS renders print
// failures to staff (docs/spec/hardware-printing.md: "Print failures must be
// visible to staff"), and that read crosses the Tauri boundary into TypeScript.
export const PrintJobSchema = z.object({
  id: z.string().uuid(),
  kot_id: z.string().uuid(),
  printer_id: z.string().uuid(),
  status: PrintJobStatusSchema,
  attempt_count: z.number().int().nonnegative(),
  last_error: z.string().nullable(),
  created_at: z.string().datetime(),
  updated_at: z.string().datetime(),
  schema_version: z.literal(1),
});
export type PrintJob = z.infer<typeof PrintJobSchema>;
