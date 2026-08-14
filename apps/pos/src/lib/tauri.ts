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

/** A failed `print_job` joined with the printer name and the KOT's station —
 * the staff-visible failure view (docs/spec/hardware-printing.md: "Print
 * failures must be visible to staff"). `PrintJobSchema` has no wire mirror
 * for the two extra display fields (`print_job` never crosses a sync
 * boundary, ADR-014 §3), so this extends it locally rather than inventing a
 * second, looser schema. */
const FailedPrintJobSchema = PrintJobSchema.extend({
  printer_name: z.string(),
  kot_station: z.string(),
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

/** Issues a GST invoice over every line currently on `orderId`, unsplit
 * (`split_count == 1` — split bills are out of this milestone's POS surface,
 * apps/pos/src-tauri/src/commands/billing.rs). Rejects with
 * `NOTHING_TO_BILL`, `NO_FISCAL_PROFILE_CONFIGURED` or
 * `NO_ACTIVE_INVOICE_SERIES` if outlet config is incomplete — see
 * `billingErrorMessage`. */
export async function issueInvoice(orderId: string, createdByUserId: string): Promise<Invoice> {
  try {
    const raw = await invoke("issue_invoice", { orderId, createdByUserId });
    return InvoiceSchema.parse(raw);
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

/** Records ONE tender against `orderId` — forward (`reversesPaymentId ==
 * null`) or a reversal (void/refund). `cashierUserId` is `created_by_user_id`
 * on the stored row; `cashShiftId` links a CASH tender to the cashier's open
 * shift (so it posts a `cash_movement`) and is `null` for every other method
 * or when no shift is open. Rejects with `FORWARD_PAYMENT_AMOUNT_NOT_POSITIVE`/
 * `REVERSAL_AMOUNT_NOT_NON_POSITIVE`/`REVERSAL_EXCEEDS_REMAINING`/etc — see
 * `billingErrorMessage`. There is no "edit a payment" command: a correction
 * is always a new call here with `reversesPaymentId` set (apps/pos/
 * src-tauri/src/commands/billing.rs `record_payment_impl` — the append-only
 * shape docs/spec/payments.md requires). */
export async function recordPayment(args: {
  orderId: string;
  method: PaymentMethod;
  amountPaise: number;
  tenderedPaise: number | null;
  changePaise: number | null;
  reference: string | null;
  cashShiftId: string | null;
  reversesPaymentId: string | null;
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
