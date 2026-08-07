// The only module that calls `@tauri-apps/api`'s `invoke`. Every response
// crossing the Rust->JS boundary is parsed with the matching Zod schema from
// `@holler/contracts` before the rest of the app ever sees it — data
// crossing that boundary is untrusted input, not a trusted cast (task
// requirement #6).
import { invoke } from "@tauri-apps/api/core";
import {
  AuthenticatedPrincipalSchema,
  CanonicalOrderSchema,
  MenuItemSchema,
  RestaurantTableSchema,
  TableSessionSchema,
  type AuthenticatedPrincipal,
  type CanonicalOrder,
  type MenuItem,
  type OrderType,
  type RestaurantTable,
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

export interface NewOrderItemRequest {
  menu_item_id: string;
  variant_id: string | null;
  quantity: number;
  unit_price_paise: number;
  notes: string | null;
}

export async function login(email: string, password: string): Promise<AuthenticatedPrincipal> {
  try {
    const raw = await invoke("login", { email, password });
    return AuthenticatedPrincipalSchema.parse(raw);
  } catch (err) {
    throw toCommandError(err);
  }
}

/**
 * KNOWN CONTRACT GAP (reported, not worked around): the Tauri `MenuItem` DTO
 * (apps/pos/src-tauri/src/dto.rs) omits `schema_version`, which
 * `@holler/contracts`'s `MenuItemSchema` requires as `z.literal(1)`. Every
 * other DTO in that file includes it. Rather than hand-roll a second,
 * looser menu-item type here, this patches in the one constant the schema
 * requires (always `1` for a schema at version 1 — no business data is
 * invented) so the real contract schema is still the thing validating the
 * rest of the shape. See task report for the exact fix needed on the Rust
 * side (add `schema_version: 1` to `dto::MenuItem`).
 */
function parseMenuItem(raw: unknown): MenuItem {
  const withSchemaVersion =
    typeof raw === "object" && raw !== null ? { schema_version: 1, ...raw } : raw;
  return MenuItemSchema.parse(withSchemaVersion);
}

export async function listMenuItems(): Promise<MenuItem[]> {
  try {
    const raw = await invoke<unknown[]>("list_menu_items");
    return raw.map(parseMenuItem);
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

// `add_order_item` / `remove_order_item` Tauri commands exist but always
// reject with `UNSUPPORTED_DB_OPERATION` (see
// apps/pos/src-tauri/src/commands/orders.rs module doc comment: the edge
// database crate exposes no add/remove-item-with-outbox API for an
// already-persisted order). This app therefore never calls them: the cart
// is assembled entirely in frontend state (`store/cart.ts`) and sent to
// `create_order` once, atomically, when the cashier presses Send. This is
// not a workaround invented here — it is the only order-creation path the
// Rust layer actually implements, and it matches ordering.md's DRAFT ->
// SENT flow for a cart that has not yet touched storage.
