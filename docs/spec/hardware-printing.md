# Spec: Hardware & Printing

Owns: Device Service, printer abstraction, templates.
Source: HOLLER_MASTER_PROMPT.md §53–§54.

## Hardware abstraction
Device Service. Printer adapters: ESC/POS, Network, USB, Bluetooth where supported. Sizes: 80mm/58mm receipt, KOT printers, label printers. Future: weighing scales, barcode scanners, customer displays, cash drawer, payment terminals. Hardware code must never leak into domain services.

## Printing
Template engine covering: GST invoice, KOT, proforma bill, receipt, settlement summary, purchase order, GRN. Routing example: Tandoor → Printer A, Bar → Printer B, Main Kitchen → Printer C. Automatic retry + print spool. Print failures must be visible to staff. A late printer ack must never cause a duplicate KOT.

## Cross-context dependencies
- Kitchen (docs/spec/kitchen.md) — KOT print routing per station.
- Compliance (docs/spec/compliance.md) — GST invoice template.
