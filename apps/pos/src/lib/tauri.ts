// The only module that calls `@tauri-apps/api`'s `invoke`. Every response
// crossing the Rust->JS boundary is parsed with the matching Zod schema from
// `@holler/contracts` before the rest of the app ever sees it — data
// crossing that boundary is untrusted input, not a trusted cast (task
// requirement #6).
import { invoke } from "@tauri-apps/api/core";
import {
  AuthenticatedPrincipalSchema,
  CanonicalOrderSchema,
  CashMovementSchema,
  CashShiftSchema,
  DiscountDefinitionSchema,
  InvoiceSchema,
  KotSchema,
  MenuItemSchema,
  PaymentSchema,
  PrintJobSchema,
  RestaurantTableSchema,
  StationSchema,
  TableSessionSchema,
  type AuthenticatedPrincipal,
  type CanonicalOrder,
  type CashMovement,
  type CashMovementKind,
  type CashShift,
  type DiscountDefinition,
  type Invoice,
  type Kot,
  type MenuItem,
  type OrderType,
  type Payment,
  type PaymentMethod,
  type RestaurantTable,
  type Station,
  type TableSession,
} from "@holler/contracts";
import { z } from "zod";

/** The `{code, message}` shape every Tauri command in this app rejects with
 * (apps/pos/src-tauri/src/error.rs `AppError`). Never contains credential
 * material. */
export interface TauriCommandError {
  code: string;
  message: string;
}

const TauriCommandErrorSchema = z.object({
  code: z.string(),
  message: z.string(),
});

export function isTauriCommandError(err: unknown): err is TauriCommandError {
  return TauriCommandErrorSchema.safeParse(err).success;
}

/** Normalizes anything `invoke()` can reject with into `TauriCommandError`. */
function toCommandError(err: unknown): TauriCommandError {
  if (isTauriCommandError(err)) return err;
  return { code: "UNKNOWN_ERROR", message: String(err) };
}

/** One modifier selection submitted alongside a cart line — mirrors
 * `apps/pos/src-tauri/src/commands/orders.rs` `NewOrderItemModifierRequest`.
 * `modifier_id` is a snapshot (see `packages/contracts/sqlite/
 * 0003_order_item_modifiers.sql`: deliberately not a foreign key), minted by
 * the caller. */
export interface NewOrderItemModifierRequest {
  modifier_id: string;
  group_name: string;
  option_name: string;
  price_delta_paise: number;
}

export interface NewOrderItemRequest {
  menu_item_id: string;
  variant_id: string | null;
  quantity: number;
  unit_price_paise: number;
  notes: string | null;
  /** Optional — mirrors the Rust side's `#[serde(default)]`. Omitting this
   * is equivalent to sending an empty array; either way, no modifier delta
   * reaches the line total unless this is actually populated. */
  modifiers?: NewOrderItemModifierRequest[];
}

export async function login(email: string, password: string): Promise<AuthenticatedPrincipal> {
  try {
    const raw = await invoke("login", { email, password });
    return AuthenticatedPrincipalSchema.parse(raw);
  } catch (err) {
    throw toCommandError(err);
  }
}

export async function listMenuItems(): Promise<MenuItem[]> {
  try {
    const raw = await invoke<unknown[]>("list_menu_items");
    return raw.map((i) => MenuItemSchema.parse(i));
  } catch (err) {
    throw toCommandError(err);
  }
}

/**
 * `packages/contracts` has no TS+Zod mirror for `menu_category` yet (see
 * `apps/pos/src-tauri/src/dto.rs` module doc comment — a contract gap, not
 * worked around here). This local schema matches the Rust DTO's field set
 * verbatim rather than inventing a wire shape, so it is a trivial
 * rename-free swap once a contract mirror exists.
 */
const MenuCategorySchema = z.object({
  id: z.string(),
  outlet_id: z.string(),
  name: z.string(),
  sort_order: z.number().int(),
  config_version: z.number().int(),
});
export type MenuCategory = z.infer<typeof MenuCategorySchema>;

export async function listMenuCategories(): Promise<MenuCategory[]> {
  try {
    const raw = await invoke<unknown[]>("list_menu_categories");
    return raw.map((c) => MenuCategorySchema.parse(c));
  } catch (err) {
    throw toCommandError(err);
  }
}

export async function listTables(): Promise<RestaurantTable[]> {
  try {
    const raw = await invoke<unknown[]>("list_tables");
    return raw.map((t) => RestaurantTableSchema.parse(t));
  } catch (err) {
    throw toCommandError(err);
  }
}

export async function getOpenTableSession(tableId: string): Promise<TableSession | null> {
  try {
    const raw = await invoke("get_open_table_session", { tableId });
    return raw === null ? null : TableSessionSchema.parse(raw);
  } catch (err) {
    throw toCommandError(err);
  }
}

export async function createOrder(
  orderType: OrderType,
  tableId: string | null,
  items: NewOrderItemRequest[],
): Promise<CanonicalOrder> {
  try {
    const raw = await invoke("create_order", { orderType, tableId, items });
    return CanonicalOrderSchema.parse(raw);
  } catch (err) {
    throw toCommandError(err);
  }
}

export async function getOrder(orderId: string): Promise<CanonicalOrder | null> {
  try {
    const raw = await invoke("get_order", { orderId });
    return raw === null ? null : CanonicalOrderSchema.parse(raw);
  } catch (err) {
    throw toCommandError(err);
  }
}

export async function listOrders(): Promise<CanonicalOrder[]> {
  try {
    const raw = await invoke<unknown[]>("list_orders");
    return raw.map((o) => CanonicalOrderSchema.parse(o));
  } catch (err) {
    throw toCommandError(err);
  }
}

/** Recovers this device's active (DRAFT) in-progress order, if any
 * (apps/pos/src-tauri/src/commands/orders.rs `get_active_draft_order`).
 * Called once at startup so a crash mid-order restores the cashier's cart
 * from whatever is actually durable in SQLite (docs/backlog-m2.md "POS
 * cart persistence") rather than starting empty. Returns `null` when there
 * is nothing to recover — not an error. */
export async function getActiveDraftOrder(): Promise<CanonicalOrder | null> {
  try {
    const raw = await invoke("get_active_draft_order");
    return raw === null ? null : CanonicalOrderSchema.parse(raw);
  } catch (err) {
    throw toCommandError(err);
  }
}

/** The cashier's DRAFT -> CONFIRMED transition (apps/pos/src-tauri/src/commands/orders.rs
 * `confirm_order`). Rejects with `ORDER_NOT_CONFIRMABLE` if the order is not
 * currently DRAFT — the edge, not this layer, is authoritative for that
 * check (sync.md §50.1). */
export async function confirmOrder(orderId: string): Promise<CanonicalOrder> {
  try {
    const raw = await invoke("confirm_order", { orderId });
    return CanonicalOrderSchema.parse(raw);
  } catch (err) {
    throw toCommandError(err);
  }
}

/** Adds one line to an already-persisted `DRAFT` order
 * (apps/pos/src-tauri/src/commands/orders.rs `add_order_item`). Rejects with
 * `ORDER_NOT_DRAFT` once the order has moved past DRAFT. */
export async function addOrderItem(
  orderId: string,
  item: NewOrderItemRequest,
): Promise<CanonicalOrder> {
  try {
    const raw = await invoke("add_order_item", { orderId, item });
    return CanonicalOrderSchema.parse(raw);
  } catch (err) {
    throw toCommandError(err);
  }
}

/** Sets an existing order line's `quantity` in place — the frozen
 * `SET_ORDER_ITEM_QUANTITY` command (contracts 0.4.0, ADR-016; apps/pos/
 * src-tauri/src/commands/orders.rs `update_order_item_quantity`). A single
 * durable write, deliberately not remove-then-add (docs/backlog-m2.md P1,
 * docs/m3-planning.md Track B). Rejects with `ORDER_ITEM_ALREADY_TICKETED`
 * if some `kot` row has already frozen a snapshot of this line — the kitchen
 * copy and the cashier's copy must never silently diverge. */
export async function updateOrderItemQuantity(
  orderId: string,
  orderItemId: string,
  quantity: number,
): Promise<CanonicalOrder> {
  try {
    const raw = await invoke("update_order_item_quantity", { orderId, orderItemId, quantity });
    return CanonicalOrderSchema.parse(raw);
  } catch (err) {
    throw toCommandError(err);
  }
}

/** Removes one line from an already-persisted `DRAFT` order
 * (apps/pos/src-tauri/src/commands/orders.rs `remove_order_item`). */
export async function removeOrderItem(
  orderId: string,
  orderItemId: string,
): Promise<CanonicalOrder> {
  try {
    const raw = await invoke("remove_order_item", { orderId, orderItemId });
    return CanonicalOrderSchema.parse(raw);
  } catch (err) {
    throw toCommandError(err);
  }
}

/** Sets `order_type`/`table_id` on an already-persisted `DRAFT` order
 * (apps/pos/src-tauri/src/commands/orders.rs `update_order_shape`). This is
 * the fix for the M2 P0 order-shape lock (docs/retro.md, task T14): a DRAFT
 * order is created on the first cart line for crash durability, and its
 * shape must stay editable for the order's whole DRAFT lifetime, not just
 * before it existed. Rejects with `ORDER_NOT_DRAFT` once the order has left
 * DRAFT — the cart store must not pretend a shape edit succeeded locally
 * when it did not persist. */
export async function updateOrderShape(
  orderId: string,
  orderType: OrderType,
  tableId: string | null,
): Promise<CanonicalOrder> {
  try {
    const raw = await invoke("update_order_shape", { orderId, orderType, tableId });
    return CanonicalOrderSchema.parse(raw);
  } catch (err) {
    throw toCommandError(err);
  }
}

// ------------------------------------------------------------- kitchen (M2) --
// ADR-014, docs/spec/kitchen.md, docs/spec/hardware-printing.md.

/** Generates and returns the station tickets for an order's send-to-kitchen
 * moment (apps/pos/src-tauri/src/commands/kitchen.rs `send_order_to_kitchen`).
 * Rejects with `ORDER_NOT_SENDABLE_TO_KITCHEN` (order not CONFIRMED/
 * SENT_TO_KITCHEN/PREPARING) or `NOTHING_TO_SEND_TO_KITCHEN` (nothing new to
 * ticket). Also best-effort queues and attempts to print every ticket — a
 * print failure there never fails this call; see `listFailedPrintJobs`. */
export async function sendOrderToKitchen(orderId: string): Promise<Kot[]> {
  try {
    const raw = await invoke<unknown[]>("send_order_to_kitchen", { orderId });
    return raw.map((k) => KotSchema.parse(k));
  } catch (err) {
    throw toCommandError(err);
  }
}

export async function listKotsForOrder(orderId: string): Promise<Kot[]> {
  try {
    const raw = await invoke<unknown[]>("list_kots_for_order", { orderId });
    return raw.map((k) => KotSchema.parse(k));
  } catch (err) {
    throw toCommandError(err);
  }
}

/** Transitions one KOT's status (NEW -> ACKNOWLEDGED -> PREPARING -> READY ->
 * SERVED, or CANCELLED from any non-terminal status) and returns the order's
 * refreshed ticket list. Rejects illegal transitions with
 * `ILLEGAL_KOT_STATUS_TRANSITION` rather than silently no-op'ing. */
export async function transitionKotStatus(
  orderId: string,
  kotId: string,
  newStatus: Kot["status"],
): Promise<Kot[]> {
  try {
    const raw = await invoke<unknown[]>("transition_kot_status", {
      orderId,
      kotId,
      newStatus,
    });
    return raw.map((k) => KotSchema.parse(k));
  } catch (err) {
    throw toCommandError(err);
  }
}

export async function listStations(): Promise<Station[]> {
  try {
    const raw = await invoke<unknown[]>("list_stations");
    return raw.map((s) => StationSchema.parse(s));
  } catch (err) {
    throw toCommandError(err);
  }
}

/** A failed `print_job` joined with the printer name and, depending on
 * `target`, the KOT's station or the invoice's number — the staff-visible
 * failure view (docs/spec/hardware-printing.md: "Print failures must be
 * visible to staff"). A cook needs the station; a cashier needs the invoice
 * number, and a bill that silently exhausted its print retries is the same
 * failure one layer up from a dropped KOT (§64).
 *
 * `print_job` never crosses a sync boundary (ADR-014 §3), so it has no wire
 * mirror in `packages/contracts` to extend — and `PrintJobSchema.kot_id` is
 * a required uuid, which can no longer describe an invoice-linked row, so
 * this is a standalone local schema rather than `PrintJobSchema.extend(...)`.
 * `target` is what callers should branch on — never infer the kind from
 * which of `kot_id`/`invoice_id` happens to be present. */
const FailedPrintJobSchema = z.object({
  id: z.string().uuid(),
  target: z.enum(["KOT", "INVOICE"]),
  kot_id: z.string().uuid().nullable(),
  kot_station: z.string().nullable(),
  invoice_id: z.string().uuid().nullable(),
  invoice_number: z.string().nullable(),
  printer_id: z.string().uuid(),
  status: PrintJobSchema.shape.status,
  attempt_count: z.number().int().nonnegative(),
  last_error: z.string().nullable(),
  created_at: z.string().datetime(),
  updated_at: z.string().datetime(),
  printer_name: z.string(),
  schema_version: z.literal(1),
});
export type FailedPrintJob = z.infer<typeof FailedPrintJobSchema>;

export async function listFailedPrintJobs(): Promise<FailedPrintJob[]> {
  try {
    const raw = await invoke<unknown[]>("list_failed_print_jobs");
    return raw.map((j) => FailedPrintJobSchema.parse(j));
  } catch (err) {
    throw toCommandError(err);
  }
}

/** Manually re-attempts every print job currently due (queued, or failed and
 * past its backoff window) and returns the still-failing set — the
 * staff-facing "retry" action next to the failure banner. */
export async function retryFailedPrintJobs(): Promise<FailedPrintJob[]> {
  try {
    const raw = await invoke<unknown[]>("retry_failed_print_jobs");
    return raw.map((j) => FailedPrintJobSchema.parse(j));
  } catch (err) {
    throw toCommandError(err);
  }
}

// ------------------------------------------------------------- billing (M3) --
// ADR-016, docs/spec/payments.md, docs/spec/compliance.md. Every money value
// returned here is already computed by the edge (`edge/database`'s tax
// engine and append-only payment/cash-shift writers) — this module only
// invokes the command and validates the wire shape; it never derives a
// paise amount of its own (CLAUDE.md §Money).

/** One cashier-chosen discount application, submitted alongside `issueInvoice`
 * — mirrors `apps/pos/src-tauri/src/commands/billing.rs` `LineDiscountInput`.
 * `reason` is required only when the named `discount_definition.requires_reason`
 * is `true`; the edge (not this layer) is what actually enforces that,
 * rejecting with `DISCOUNT_REASON_REQUIRED` if it is missing. */
export interface LineDiscountRequest {
  orderItemId: string;
  discountDefinitionId: string;
  reason: string | null;
}

/** Issues a GST invoice over every line currently on `orderId`, unsplit
 * (`split_count == 1` — see `issueSplitInvoices` for N ways).
 * `discounts` is optional and
 * defaults to none applied — omitting it bills every line at full price,
 * unchanged from before this discount surface existed. Rejects with
 * `NOTHING_TO_BILL`, `NO_FISCAL_PROFILE_CONFIGURED`, `NO_ACTIVE_INVOICE_SERIES`,
 * `DISCOUNT_REASON_REQUIRED`, `DISCOUNT_PERMISSION_DENIED`,
 * `DISCOUNT_SCOPE_NOT_SUPPORTED`, `DISCOUNT_DEFINITION_NOT_FOUND`,
 * `DISCOUNT_NOT_ACTIVE` or `INVALID_INPUT` (the edge tax engine's own guard
 * on a malformed discount) — see `billingErrorMessage`. */
export async function issueInvoice(
  orderId: string,
  createdByUserId: string,
  discounts: readonly LineDiscountRequest[] = [],
): Promise<Invoice> {
  try {
    const raw = await invoke("issue_invoice", {
      orderId,
      createdByUserId,
      discounts: discounts.map((d) => ({
        order_item_id: d.orderItemId,
        discount_definition_id: d.discountDefinitionId,
        reason: d.reason,
      })),
    });
    return InvoiceSchema.parse(raw);
  } catch (err) {
    throw toCommandError(err);
  }
}

/** Queues an issued bill for print at every printer this outlet has given
 * the `BILL` role (`printer_role`, contracts 0.4.7), and makes one immediate
 * attempt. Returns the queued `print_job` ids.
 *
 * Separate from `issueInvoice` on purpose: issuing a bill and printing it
 * are distinct cashier actions (a bill may be issued and shown on screen,
 * then printed once, then reprinted), and issuing must never fail because a
 * printer is unplugged. Idempotent per (invoice, printer) — a second tap
 * returns the existing job rather than spooling a duplicate bill.
 *
 * Rejects with `NO_PRINTER_ROUTED` when the outlet has configured no active
 * BILL printer: unlike a KOT — which still reaches the kitchen on a KDS
 * screen when its print fails — a bill has no second channel, so this one
 * surfaces rather than being logged and swallowed. A print that fails AFTER
 * queueing (a dead printer) does not reject here; it appears in
 * `listFailedPrintJobs` and stays retryable. */
export async function printInvoice(invoiceId: string): Promise<string[]> {
  try {
    return await invoke<string[]>("print_invoice", { invoiceId });
  } catch (err) {
    throw toCommandError(err);
  }
}

export async function listInvoicesForOrder(orderId: string): Promise<Invoice[]> {
  try {
    const raw = await invoke<unknown[]>("list_invoices_for_order", { orderId });
    return raw.map((i) => InvoiceSchema.parse(i));
  } catch (err) {
    throw toCommandError(err);
  }
}

/** One invoice-to-be within a split — "this part bills `quantity` of
 * `orderItemId`". Mirrors
 * `apps/pos/src-tauri/src/commands/billing.rs` `SplitLineInput`. */
export interface SplitLineRequest {
  orderItemId: string;
  quantity: number;
}

/** One part of a split — becomes one independently-numbered invoice.
 * Mirrors `SplitPartInput`. */
export interface SplitPartRequest {
  lines: readonly SplitLineRequest[];
}

/** Issues N invoices over every line on `orderId`, sharing one
 * `split_group_id` (ADR-016 §4) — `parts` names, for each invoice-to-be,
 * which order lines and quantities it bills. `discounts` resolves exactly
 * as it does for `issueInvoice`, once across the whole order, not per part.
 *
 * The edge (`Db::issue_split_invoices_with_outbox`) is the SOLE authority on
 * whether `parts` reconstructs the order's lines exactly (§66) — this
 * function performs no conservation check of its own and never will; a
 * mismatched split is rejected as a whole with `INVALID_INPUT`, naming the
 * offending order_item and the quantity mismatch, and consumes NO invoice
 * number for any part (all issued together in one transaction, or none).
 * Also rejects with the same `NOTHING_TO_BILL`/`NO_FISCAL_PROFILE_CONFIGURED`/
 * `NO_ACTIVE_INVOICE_SERIES`/discount codes `issueInvoice` does, plus
 * `SPLIT_REQUIRES_AT_LEAST_TWO_PARTS` (fewer than two parts supplied — a
 * one-part "split" is just a normal bill and must go through `issueInvoice`
 * instead, so `invoice.split_group_id` never means "one invoice") and
 * `EMPTY_SPLIT_PART` (a part with no lines) — see `billingErrorMessage`. */
export async function issueSplitInvoices(
  orderId: string,
  createdByUserId: string,
  parts: readonly SplitPartRequest[],
  discounts: readonly LineDiscountRequest[] = [],
): Promise<Invoice[]> {
  try {
    const raw = await invoke<unknown[]>("issue_split_invoices", {
      orderId,
      createdByUserId,
      parts: parts.map((p) => ({
        lines: p.lines.map((l) => ({
          order_item_id: l.orderItemId,
          quantity: l.quantity,
        })),
      })),
      discounts: discounts.map((d) => ({
        order_item_id: d.orderItemId,
        discount_definition_id: d.discountDefinitionId,
        reason: d.reason,
      })),
    });
    return raw.map((i) => InvoiceSchema.parse(i));
  } catch (err) {
    throw toCommandError(err);
  }
}

/** Every invoice sharing one `split_group_id` (ADR-016 §4) — lets the POS
 * tell a cashier which parts of a split remain unpaid. */
export async function listInvoicesForSplitGroup(splitGroupId: string): Promise<Invoice[]> {
  try {
    const raw = await invoke<unknown[]>("list_invoices_for_split_group", { splitGroupId });
    return raw.map((i) => InvoiceSchema.parse(i));
  } catch (err) {
    throw toCommandError(err);
  }
}

/** This outlet's discount catalogue (`discount_definition`, CLOUD_TO_EDGE
 * config, ADR-016 §1) — apps/pos/src-tauri/src/commands/billing.rs
 * `list_discount_definitions`. Includes inactive/not-yet-effective rows;
 * `domain/billing.ts` filters what is actually offerable right now. */
export async function listDiscountDefinitions(): Promise<DiscountDefinition[]> {
  try {
    const raw = await invoke<unknown[]>("list_discount_definitions");
    return raw.map((d) => DiscountDefinitionSchema.parse(d));
  } catch (err) {
    throw toCommandError(err);
  }
}

/** Records ONE tender against `orderId` — forward (`reversesPaymentId ==
 * null`) or a reversal (void/refund). `cashierUserId` is `created_by_user_id`
 * on the stored row; `cashShiftId` links a CASH tender to the cashier's open
 * shift (so it posts a `cash_movement`) and is `null` for every other method
 * or when no shift is open. `invoiceId` names which invoice a FORWARD tender
 * settles — required once a bill has been issued, so the edge can reject a
 * tender that would exceed the invoice's remaining due (T9 retry: the
 * double-settlement defect, `FORWARD_PAYMENT_EXCEEDS_REMAINING_DUE`); a
 * reversal ignores it and derives its own target automatically. Rejects with
 * `FORWARD_PAYMENT_AMOUNT_NOT_POSITIVE`/`REVERSAL_AMOUNT_NOT_NON_POSITIVE`/
 * `REVERSAL_EXCEEDS_REMAINING`/`FORWARD_PAYMENT_EXCEEDS_REMAINING_DUE`/etc —
 * see `billingErrorMessage`. There is no "edit a payment" command: a
 * correction is always a new call here with `reversesPaymentId` set
 * (apps/pos/src-tauri/src/commands/billing.rs `record_payment_impl` — the
 * append-only shape docs/spec/payments.md requires). */
export async function recordPayment(args: {
  orderId: string;
  method: PaymentMethod;
  amountPaise: number;
  tenderedPaise: number | null;
  changePaise: number | null;
  reference: string | null;
  cashShiftId: string | null;
  reversesPaymentId: string | null;
  invoiceId: string | null;
  createdByUserId: string;
}): Promise<Payment> {
  try {
    const raw = await invoke("record_payment", {
      orderId: args.orderId,
      method: args.method,
      amountPaise: args.amountPaise,
      tenderedPaise: args.tenderedPaise,
      changePaise: args.changePaise,
      reference: args.reference,
      cashShiftId: args.cashShiftId,
      reversesPaymentId: args.reversesPaymentId,
      invoiceId: args.invoiceId,
      createdByUserId: args.createdByUserId,
    });
    return PaymentSchema.parse(raw);
  } catch (err) {
    throw toCommandError(err);
  }
}

export async function listPaymentsForOrder(orderId: string): Promise<Payment[]> {
  try {
    const raw = await invoke<unknown[]>("list_payments_for_order", { orderId });
    return raw.map((p) => PaymentSchema.parse(p));
  } catch (err) {
    throw toCommandError(err);
  }
}

/** Opens a new cash shift (§39) for `cashierUserId` on this device. Rejects
 * with `CASH_SHIFT_ALREADY_OPEN` if the cashier already has one open here. */
export async function openCashShift(
  cashierUserId: string,
  openingCashPaise: number,
): Promise<CashShift> {
  try {
    const raw = await invoke("open_cash_shift", { cashierUserId, openingCashPaise });
    return CashShiftSchema.parse(raw);
  } catch (err) {
    throw toCommandError(err);
  }
}

/** Closes an open shift. `expected_cash_paise` is derived by the edge from
 * its own posted movements — this call NEVER supplies it. If the derived
 * variance is non-zero and `varianceReason` is `null`/blank, the edge
 * rejects with `CASH_VARIANCE_REASON_REQUIRED` (§39, binding) and the shift
 * stays open, unmutated — the caller must collect a reason and retry, never
 * silently drop the close. */
export async function closeCashShift(
  cashShiftId: string,
  actualCashPaise: number,
  varianceReason: string | null,
): Promise<CashShift> {
  try {
    const raw = await invoke("close_cash_shift", {
      cashShiftId,
      actualCashPaise,
      varianceReason,
    });
    return CashShiftSchema.parse(raw);
  } catch (err) {
    throw toCommandError(err);
  }
}

/** Posts a PAID_IN/PAID_OUT cash movement against an open shift. `reason` is
 * mandatory — the edge rejects a blank one with
 * `CASH_MOVEMENT_REASON_REQUIRED` before writing anything. */
export async function recordPaidInOut(args: {
  cashShiftId: string;
  kind: Extract<CashMovementKind, "PAID_IN" | "PAID_OUT">;
  amountPaise: number;
  reason: string;
  createdByUserId: string;
}): Promise<CashMovement> {
  try {
    const raw = await invoke("record_paid_in_out", {
      cashShiftId: args.cashShiftId,
      kind: args.kind,
      amountPaise: args.amountPaise,
      reason: args.reason,
      createdByUserId: args.createdByUserId,
    });
    return CashMovementSchema.parse(raw);
  } catch (err) {
    throw toCommandError(err);
  }
}

export async function getCashShift(cashShiftId: string): Promise<CashShift | null> {
  try {
    const raw = await invoke("get_cash_shift", { cashShiftId });
    return raw === null ? null : CashShiftSchema.parse(raw);
  } catch (err) {
    throw toCommandError(err);
  }
}

/** Recovers `cashierUserId`'s currently OPEN shift on this device, if any
 * (T9 retry, Defect 2: "cash shift restart is an operational dead end").
 * Unlike `getCashShift`, this needs no shift id — the POS calls it on
 * startup, once a cashier is known, to recover a shift orphaned by a
 * restart automatically rather than leaving it permanently unclosable
 * (`apps/pos/src-tauri/src/commands/billing.rs` `find_open_cash_shift_impl`,
 * `holler_edge_database::Db::find_open_cash_shift`). */
export async function findOpenCashShift(cashierUserId: string): Promise<CashShift | null> {
  try {
    const raw = await invoke("find_open_cash_shift", { cashierUserId });
    return raw === null ? null : CashShiftSchema.parse(raw);
  } catch (err) {
    throw toCommandError(err);
  }
}
