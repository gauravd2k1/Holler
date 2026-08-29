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
  MenuItemVariantSchema,
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
  type MenuItemVariant,
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

export async function listMenuItemVariants(): Promise<MenuItemVariant[]> {
  try {
    const raw = await invoke<unknown[]>("list_menu_item_variants");
    return raw.map((v) => MenuItemVariantSchema.parse(v));
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

// ------------------------------------------------------------ inventory (M4) --
// ADR-018, apps/pos/src-tauri/src/dto.rs "inventory (M4)" section. None of
// these shapes has a `packages/contracts` mirror — they are POS-local read
// projections/wire shapes, the `MenuCategory` precedent above — so every
// schema below matches the Rust DTO's field set verbatim rather than
// inventing one. Quantities are integer MICRO-units (`domain/inventory.ts`
// formats them); `StockDeductionGap.quantity` is the one field here that is
// NOT micro — see that module for the distinction.

const CurrentStockLineSchema = z.object({
  inventory_item_id: z.string(),
  inventory_item_name: z.string(),
  dimension: z.string(),
  current_quantity_micro: z.number().int(),
  reorder_level_micro: z.number().int().nullable(),
  par_level_micro: z.number().int().nullable(),
  schema_version: z.literal(1),
});
export type CurrentStockLine = z.infer<typeof CurrentStockLineSchema>;

/** The bounded, outlet-wide current-stock read — what the low-stock signal
 * and every item picker in the wastage/count screens read from
 * (`apps/pos/src-tauri/src/commands/inventory.rs` `list_current_stock`). */
export async function listCurrentStock(): Promise<CurrentStockLine[]> {
  try {
    const raw = await invoke<unknown[]>("list_current_stock");
    return raw.map((l) => CurrentStockLineSchema.parse(l));
  } catch (err) {
    throw toCommandError(err);
  }
}

const StockDeductionGapSchema = z.object({
  id: z.string(),
  outlet_id: z.string(),
  entry_seq: z.number().int(),
  order_id: z.string(),
  order_item_id: z.string(),
  menu_item_id: z.string(),
  menu_item_variant_id: z.string().nullable(),
  menu_item_name: z.string(),
  quantity: z.number().int(),
  reason: z.string(),
  occurred_at: z.string(),
  business_date: z.string(),
  schema_version: z.literal(1),
});
export type StockDeductionGap = z.infer<typeof StockDeductionGapSchema>;

/** One ranged-replay entry this outlet has given up on sending (contracts
 * 0.5.8) — `apps/pos/src-tauri/src/dto.rs` `SyncReplayBlock`. */
const SyncReplayBlockSchema = z.object({
  outlet_id: z.string(),
  stream: z.string(),
  entry_seq: z.number().int(),
  record_id: z.string(),
  attempts: z.number().int(),
  last_status: z.number().int().nullable(),
  last_error: z.string(),
  first_attempt_at: z.string(),
  last_attempt_at: z.string(),
  blocked_at: z.string().nullable(),
});
export type SyncReplayBlock = z.infer<typeof SyncReplayBlockSchema>;

/** Stock history this outlet has stopped trying to send. Empty is the normal
 * answer; anything else is a condition someone has to act on, which is the
 * whole reason the per-entry retry bound writes a row instead of a log line.
 * `apps/pos/src-tauri/src/commands/inventory.rs` `list_blocked_replays`. */
export async function listBlockedReplays(): Promise<SyncReplayBlock[]> {
  try {
    const raw = await invoke<unknown[]>("list_blocked_replays");
    return raw.map((b) => SyncReplayBlockSchema.parse(b));
  } catch (err) {
    throw toCommandError(err);
  }
}

/** The "items sold with no recipe" report (M4 acceptance criterion 5) —
 * `apps/pos/src-tauri/src/commands/inventory.rs` `list_stock_deduction_gaps`. */
export async function listStockDeductionGaps(): Promise<StockDeductionGap[]> {
  try {
    const raw = await invoke<unknown[]>("list_stock_deduction_gaps");
    return raw.map((g) => StockDeductionGapSchema.parse(g));
  } catch (err) {
    throw toCommandError(err);
  }
}

const StockLedgerEntrySchema = z.object({
  id: z.string(),
  outlet_id: z.string(),
  entry_seq: z.number().int(),
  inventory_item_id: z.string(),
  inventory_item_name: z.string(),
  dimension: z.string(),
  entry_type: z.string(),
  origin: z.string(),
  quantity_applied_micro: z.number().int(),
  recipe_id: z.string().nullable(),
  recipe_version: z.number().int().nullable(),
  recipe_name: z.string().nullable(),
  modifier_delta_id: z.string().nullable(),
  modifier_name: z.string().nullable(),
  modifier_delta_version: z.number().int().nullable(),
  source_order_id: z.string().nullable(),
  source_order_item_id: z.string().nullable(),
  reason_code: z.string().nullable(),
  note: z.string().nullable(),
  occurred_at: z.string(),
  business_date: z.string(),
  created_by_user_id: z.string().nullable(),
  unit_cost_paise: z.number().int().nullable(),
  schema_version: z.literal(1),
});
export type StockLedgerEntry = z.infer<typeof StockLedgerEntrySchema>;

/** Records a WASTAGE ledger entry. `quantity` is a WHOLE-NUMBER HUMAN-UNIT
 * quantity (whole grams/millilitres/pieces, matching the item's own
 * dimension) — the Rust side converts to micro-units
 * (`apps/pos/src-tauri/src/commands/inventory.rs` `human_quantity_to_micro`);
 * this function must never multiply by 1e6 itself. Rejects with
 * `WASTAGE_REASON_REQUIRED`/`WASTAGE_QUANTITY_NOT_POSITIVE`/`NOT_FOUND`/
 * `UNKNOWN_DIMENSION` — see `inventoryErrorMessage`. */
export async function recordWastage(args: {
  inventoryItemId: string;
  quantity: number;
  reasonCode: string;
  note: string | null;
  createdByUserId: string;
}): Promise<StockLedgerEntry> {
  try {
    const raw = await invoke("record_wastage", {
      inventoryItemId: args.inventoryItemId,
      quantity: args.quantity,
      reasonCode: args.reasonCode,
      note: args.note,
      createdByUserId: args.createdByUserId,
    });
    return StockLedgerEntrySchema.parse(raw);
  } catch (err) {
    throw toCommandError(err);
  }
}

const StockCountSchema = z.object({
  id: z.string(),
  outlet_id: z.string(),
  business_date: z.string(),
  status: z.enum(["OPEN", "COMPLETED"]),
  started_at: z.string(),
  completed_at: z.string().nullable(),
  counted_by_user_id: z.string().nullable(),
  note: z.string().nullable(),
  schema_version: z.literal(1),
});
export type StockCount = z.infer<typeof StockCountSchema>;

/** Opens a new physical stock count for this outlet
 * (`apps/pos/src-tauri/src/commands/inventory.rs` `open_stock_count`). */
export async function openStockCount(
  countedByUserId: string | null,
  note: string | null,
): Promise<StockCount> {
  try {
    const raw = await invoke("open_stock_count", { countedByUserId, note });
    return StockCountSchema.parse(raw);
  } catch (err) {
    throw toCommandError(err);
  }
}

const StockCountLineSchema = z.object({
  id: z.string(),
  stock_count_id: z.string(),
  inventory_item_id: z.string(),
  inventory_item_name: z.string(),
  dimension: z.string(),
  counted_quantity_micro: z.number().int(),
  expected_quantity_micro: z.number().int(),
  note: z.string().nullable(),
  schema_version: z.literal(1),
});
export type StockCountLine = z.infer<typeof StockCountLineSchema>;

/** Adds or corrects one counted line on a still-OPEN count. `quantity` is a
 * WHOLE-NUMBER HUMAN-UNIT quantity, converted to micro-units on the Rust
 * side exactly as `recordWastage`'s does — never multiply by 1e6 here.
 * Rejects with `STOCK_COUNT_NOT_OPEN` once the count is COMPLETED — a count
 * is mutable only while OPEN. */
export async function addOrUpdateStockCountLine(args: {
  stockCountId: string;
  inventoryItemId: string;
  quantity: number;
  note: string | null;
}): Promise<StockCountLine> {
  try {
    const raw = await invoke("add_or_update_stock_count_line", {
      stockCountId: args.stockCountId,
      inventoryItemId: args.inventoryItemId,
      quantity: args.quantity,
      note: args.note,
    });
    return StockCountLineSchema.parse(raw);
  } catch (err) {
    throw toCommandError(err);
  }
}

export async function listStockCountLines(stockCountId: string): Promise<StockCountLine[]> {
  try {
    const raw = await invoke<unknown[]>("list_stock_count_lines", { stockCountId });
    return raw.map((l) => StockCountLineSchema.parse(l));
  } catch (err) {
    throw toCommandError(err);
  }
}

export async function getStockCount(stockCountId: string): Promise<StockCount | null> {
  try {
    const raw = await invoke("get_stock_count", { stockCountId });
    return raw === null ? null : StockCountSchema.parse(raw);
  } catch (err) {
    throw toCommandError(err);
  }
}

/** OPEN -> COMPLETED. Rejects with `STOCK_COUNT_NOT_OPEN` if the count is
 * already COMPLETED — a completed count cannot be completed twice, and this
 * must surface as a clear message, not a silent no-op. */
export async function completeStockCount(stockCountId: string): Promise<StockCount> {
  try {
    const raw = await invoke("complete_stock_count", { stockCountId });
    return StockCountSchema.parse(raw);
  } catch (err) {
    throw toCommandError(err);
  }
}

const StockCountVarianceLineSchema = z.object({
  inventory_item_id: z.string(),
  inventory_item_name: z.string(),
  dimension: z.string(),
  counted_quantity_micro: z.number().int(),
  expected_quantity_micro: z.number().int(),
  variance_quantity_micro: z.number().int(),
  variance_percentage_bps: z.number().int().nullable(),
  schema_version: z.literal(1),
});
export type StockCountVarianceLine = z.infer<typeof StockCountVarianceLineSchema>;

/** A completed count's variance report. `sales_unaccounted` is the named
 * "N sales unaccounted" term (ADR-018 §10.1) — screens must render it as its
 * own line, never fold it into any line's shrinkage. Every number here is
 * computed by the edge; this function performs no arithmetic of its own
 * (CLAUDE.md: never recompute variance arithmetic in TypeScript). */
const StockCountVarianceReportSchema = z.object({
  stock_count_id: z.string(),
  business_date: z.string(),
  lines: z.array(StockCountVarianceLineSchema),
  sales_unaccounted: z.number().int(),
  schema_version: z.literal(1),
});
export type StockCountVarianceReport = z.infer<typeof StockCountVarianceReportSchema>;

export async function getStockCountVarianceReport(
  stockCountId: string,
): Promise<StockCountVarianceReport> {
  try {
    const raw = await invoke("get_stock_count_variance_report", { stockCountId });
    return StockCountVarianceReportSchema.parse(raw);
  } catch (err) {
    throw toCommandError(err);
  }
}

// ------------------------------------------------ procurement (M5, ADR-019) --
//
// Every shape below is parsed with a Zod schema before the app sees it, the
// same discipline the rest of this file applies. Where `packages/contracts`
// carries the shape (`GrnLineSchema`, `GrnGapSchema`,
// `GoodsReceiptNoteSchema`, `PurchaseReturnSchema`) the local schema mirrors
// its field set verbatim, plus the `schema_version` literal the Tauri DTO
// adds and minus nothing.
//
// Two shapes have NO contract mirror and are reported rather than invented
// around — `GrnEntryIntentEcho` and `PurchaseOrderReceiptProgress` are edge
// READ shapes that cross no sync boundary. See
// `apps/pos/src-tauri/src/dto.rs`'s procurement section.
//
// QUANTITY SCALES DIFFER WITHIN ONE ROW AND THAT IS DELIBERATE:
// `entered_quantity_micro` and `pack_size_micro_applied` count the SUPPLIER's
// purchase unit and the item's base unit respectively. `domain/procurement.ts`
// has a separate formatter for each so a call site cannot pick the wrong one.

const GrnLineSchema = z.object({
  id: z.string(),
  grn_id: z.string(),
  inventory_item_id: z.string(),
  line_number: z.number().int(),
  purchase_order_line_id: z.string().nullable(),
  entered_purchase_unit: z.string(),
  entered_quantity_micro: z.number().int(),
  quantity_dimension: z.string(),
  base_quantity_micro: z.number().int(),
  pack_size_micro_applied: z.number().int(),
  unit_cost_paise: z.number().int(),
  line_total_paise: z.number().int(),
  batch_code: z.string().nullable(),
  expiry_date: z.string().nullable(),
  schema_version: z.literal(1),
});
export type GrnLine = z.infer<typeof GrnLineSchema>;

/** One `grn_gap`. **No `entry_seq`, deliberately** — a GRN gap rides the
 * plain envelope outbox, not a ranged stream (ADR-019 §2). The contrast with
 * `StockDeductionGapSchema` above, which does carry one, is the decision
 * rather than an omission. */
const GrnGapSchema = z.object({
  id: z.string(),
  outlet_id: z.string(),
  grn_id: z.string(),
  grn_line_id: z.string().nullable(),
  inventory_item_id: z.string().nullable(),
  reason: z.string(),
  detail: z.string().nullable(),
  occurred_at: z.string(),
  business_date: z.string(),
  schema_version: z.literal(1),
});
export type GrnGap = z.infer<typeof GrnGapSchema>;

const GoodsReceiptNoteSchema = z.object({
  id: z.string(),
  outlet_id: z.string(),
  purchase_order_id: z.string().nullable(),
  supplier_id: z.string().nullable(),
  grn_number: z.string(),
  delivery_note_ref: z.string().nullable(),
  received_at: z.string(),
  received_by_user_id: z.string(),
  business_date: z.string(),
  notes: z.string().nullable(),
  lines: z.array(GrnLineSchema),
  gaps: z.array(GrnGapSchema),
  schema_version: z.literal(1),
});
export type GoodsReceiptNote = z.infer<typeof GoodsReceiptNoteSchema>;

const GrnEntryIntentEchoSchema = z.object({
  inventory_item_id: z.string(),
  inventory_item_name: z.string(),
  entered_purchase_unit: z.string(),
  entered_quantity_micro: z.number().int(),
  quantity_dimension: z.string(),
  pack_size_micro_applied: z.number().int(),
  base_quantity_micro: z.number().int(),
  item_dimension: z.string(),
  unit_cost_paise: z.number().int(),
  line_total_paise: z.number().int(),
  gap_reasons: z.array(z.string()),
  schema_version: z.literal(1),
});
export type GrnEntryIntentEcho = z.infer<typeof GrnEntryIntentEchoSchema>;

const PurchaseReturnLineSchema = z.object({
  id: z.string(),
  purchase_return_id: z.string(),
  inventory_item_id: z.string(),
  grn_line_id: z.string().nullable(),
  line_number: z.number().int(),
  entered_purchase_unit: z.string(),
  entered_quantity_micro: z.number().int(),
  quantity_dimension: z.string(),
  base_quantity_micro: z.number().int(),
  unit_cost_paise: z.number().int(),
  schema_version: z.literal(1),
});
export type PurchaseReturnLine = z.infer<typeof PurchaseReturnLineSchema>;

const PurchaseReturnSchema = z.object({
  id: z.string(),
  outlet_id: z.string(),
  supplier_id: z.string().nullable(),
  grn_id: z.string().nullable(),
  return_number: z.string(),
  reason: z.string(),
  returned_at: z.string(),
  returned_by_user_id: z.string(),
  business_date: z.string(),
  notes: z.string().nullable(),
  lines: z.array(PurchaseReturnLineSchema),
  schema_version: z.literal(1),
});
export type PurchaseReturn = z.infer<typeof PurchaseReturnSchema>;

/** THIS OUTLET's receipt progress for one PO. The cloud's figure for the same
 * PO will differ and BOTH ARE RIGHT (ADR-019 §4) — never reconcile them. */
const PurchaseOrderReceiptProgressSchema = z.object({
  purchase_order_id: z.string(),
  purchase_order_line_id: z.string(),
  inventory_item_id: z.string(),
  ordered_base_quantity_micro: z.number().int(),
  received_base_quantity_micro_at_this_outlet: z.number().int(),
  schema_version: z.literal(1),
});
export type PurchaseOrderReceiptProgress = z.infer<typeof PurchaseOrderReceiptProgressSchema>;

/** One receiving line as the screen submits it.
 *
 * `enteredQuantity` is the operator's typed DECIMAL STRING ("4", "12.5"),
 * parsed to exact integer micro-units on the Rust side
 * (`parse_purchase_quantity_micro`). It is a string precisely so no float
 * ever touches a receipt quantity in JavaScript.
 *
 * `quantityDimension` IS THE UNIT THE OPERATOR CHOSE. It is required, and no
 * caller may fill it from the selected inventory item's dimension — that
 * would make the edge's `DIMENSION_MISMATCH` comparison `x == x` and the
 * guard could never fire (ADR-019 §6). */
export interface NewGrnLineRequest {
  inventory_item_id: string;
  entered_purchase_unit: string;
  entered_quantity: string;
  quantity_dimension: string;
  purchase_price_paise: number;
  batch_code: string | null;
  expiry_date: string | null;
  purchase_order_line_id: string | null;
}

export interface NewPurchaseReturnLineRequest {
  inventory_item_id: string;
  grn_line_id: string | null;
  entered_purchase_unit: string;
  entered_quantity: string;
  quantity_dimension: string;
  /** `null` values the return at what this outlet actually paid — the edge's
   * weighted average cost. Never coerce it to 0: a blank field and a zero
   * cost are different statements. */
  unit_cost_paise: number | null;
}

/** The `entryIntentEcho` behind M5 acceptance criterion 4. Runs the edge's
 * OWN resolution — the same one the write runs — so the echo cannot disagree
 * with what gets recorded. */
export async function grnEntryIntentEcho(
  supplierId: string | null,
  line: NewGrnLineRequest,
): Promise<GrnEntryIntentEcho> {
  try {
    const raw = await invoke("grn_entry_intent_echo", { supplierId, line });
    return GrnEntryIntentEchoSchema.parse(raw);
  } catch (err) {
    throw toCommandError(err);
  }
}

/** Records a goods receipt, its gaps and its `PURCHASE` ledger entries in one
 * edge transaction.
 *
 * `purchaseOrderId` and `supplierId` may be `null`, and a receipt with both
 * null is ORDINARY: it lands with a `NO_PURCHASE_ORDER` gap attached. No
 * caller may refuse to submit for a missing PO — refusing a delivery standing
 * in the kitchen doorway is the outage, not the protection (ADR-019 §1). */
export async function recordGoodsReceipt(args: {
  purchaseOrderId: string | null;
  supplierId: string | null;
  deliveryNoteRef: string | null;
  notes: string | null;
  receivedByUserId: string;
  lines: NewGrnLineRequest[];
}): Promise<GoodsReceiptNote> {
  try {
    const raw = await invoke("record_goods_receipt", {
      purchaseOrderId: args.purchaseOrderId,
      supplierId: args.supplierId,
      deliveryNoteRef: args.deliveryNoteRef,
      notes: args.notes,
      receivedByUserId: args.receivedByUserId,
      lines: args.lines,
    });
    return GoodsReceiptNoteSchema.parse(raw);
  } catch (err) {
    throw toCommandError(err);
  }
}

/** Records a purchase return and its `RETURN_TO_VENDOR` ledger entries.
 * `returnNumber` is the operator's own paperwork reference — contracts 0.6.0
 * mints a counter for the GRN and none for this document, reported rather
 * than invented. */
export async function recordPurchaseReturn(args: {
  supplierId: string | null;
  grnId: string | null;
  returnNumber: string;
  reason: string;
  notes: string | null;
  returnedByUserId: string;
  lines: NewPurchaseReturnLineRequest[];
}): Promise<PurchaseReturn> {
  try {
    const raw = await invoke("record_purchase_return", {
      supplierId: args.supplierId,
      grnId: args.grnId,
      returnNumber: args.returnNumber,
      reason: args.reason,
      notes: args.notes,
      returnedByUserId: args.returnedByUserId,
      lines: args.lines,
    });
    return PurchaseReturnSchema.parse(raw);
  } catch (err) {
    throw toCommandError(err);
  }
}

/** The GRN gap report behind M5 acceptance criterion 3 — the gap must be
 * VISIBLE TO A HUMAN ON THE POS. Bounded and newest-first at the edge. */
export async function listGrnGaps(): Promise<GrnGap[]> {
  try {
    const raw = await invoke<unknown[]>("list_grn_gaps");
    return raw.map((g) => GrnGapSchema.parse(g));
  } catch (err) {
    throw toCommandError(err);
  }
}

export async function purchaseOrderReceiptProgress(
  purchaseOrderId: string,
): Promise<PurchaseOrderReceiptProgress[]> {
  try {
    const raw = await invoke<unknown[]>("purchase_order_receipt_progress", { purchaseOrderId });
    return raw.map((p) => PurchaseOrderReceiptProgressSchema.parse(p));
  } catch (err) {
    throw toCommandError(err);
  }
}

/** Weighted average cost per BASE unit, in paise. `null` means this outlet
 * has never recorded a costed receipt for the item — **not zero**, and no
 * caller may render it as ₹0.00. */
export async function weightedAverageCostPaise(inventoryItemId: string): Promise<number | null> {
  try {
    const raw = await invoke("weighted_average_cost_paise", { inventoryItemId });
    return z.number().int().nullable().parse(raw);
  } catch (err) {
    throw toCommandError(err);
  }
}
