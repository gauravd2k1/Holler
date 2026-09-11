import { z } from "zod";
import { CanonicalOrderSchema, type CanonicalOrder } from "@holler/contracts";
import { authHeader } from "./session";

/**
 * The captain JSON API client, against docs/captain-api.md — the binding spec
 * agreed with the parallel Rust listener track before either side wrote code.
 *
 * BASE URL: empty by default, i.e. same-origin. In production this page is
 * served BY the till's own HOLLER_CAPTAIN_BIND_ADDR listener at `/`, so `/api/…`
 * is same-origin and no base is needed. VITE_CAPTAIN_API_BASE_URL exists only
 * so `pnpm dev` (Vite on 5176) can point at a POS listening on a LAN address —
 * it is never required in the shipped build.
 */
const BASE_URL: string = import.meta.env.VITE_CAPTAIN_API_BASE_URL ?? "";

/**
 * The error body every captain route returns. docs/captain-api.md gives
 * `UNAUTHORIZED` as an example code and says "the codes the POS already
 * uses" without enumerating them — this is a different route surface from
 * the cloud sync API's `ErrorCodeSchema` (packages/contracts), so the code is
 * read as a string and branched on by value, never coerced into that enum.
 */
export const CaptainErrorSchema = z.object({
  code: z.string(),
  message: z.string(),
});
export type CaptainError = z.infer<typeof CaptainErrorSchema>;

export class ApiError extends Error {
  readonly code: string;
  readonly status: number;

  constructor(status: number, code: string, message: string) {
    super(message);
    this.code = code;
    this.status = status;
  }
}

// ---------------------------------------------------------------- session --

export const SessionSchema = z.object({
  device_id: z.string().uuid(),
  outlet_id: z.string().uuid(),
  outlet_name: z.string(),
  device_kind: z.string(),
});
export type Session = z.infer<typeof SessionSchema>;

// ----------------------------------------------------------------- tables --

export const CaptainTableSchema = z.object({
  id: z.string().uuid(),
  name: z.string(),
  seats: z.number().int(),
  open_session_id: z.string().uuid().nullable(),
  open_order_id: z.string().uuid().nullable(),
});
export type CaptainTable = z.infer<typeof CaptainTableSchema>;

const TablesResponseSchema = z.object({
  tables: z.array(CaptainTableSchema),
});

// ------------------------------------------------------------------- menu --

export const CaptainVariantSchema = z.object({
  id: z.string().uuid(),
  name: z.string(),
  price_delta_paise: z.number().int(),
  is_default: z.boolean(),
});
export type CaptainVariant = z.infer<typeof CaptainVariantSchema>;

export const CaptainModifierSchema = z.object({
  id: z.string().uuid(),
  group_name: z.string(),
  option_name: z.string(),
  price_delta_paise: z.number().int(),
  min_selection: z.number().int(),
  max_selection: z.number().int(),
});
export type CaptainModifier = z.infer<typeof CaptainModifierSchema>;

export const CaptainMenuItemSchema = z.object({
  id: z.string().uuid(),
  category_id: z.string().uuid(),
  name: z.string(),
  base_price_paise: z.number().int(),
  is_available: z.boolean(),
  variants: z.array(CaptainVariantSchema),
  modifiers: z.array(CaptainModifierSchema),
});
export type CaptainMenuItem = z.infer<typeof CaptainMenuItemSchema>;

export const CaptainCategorySchema = z.object({
  id: z.string().uuid(),
  name: z.string(),
  sort_order: z.number().int(),
});
export type CaptainCategory = z.infer<typeof CaptainCategorySchema>;

const MenuResponseSchema = z.object({
  categories: z.array(CaptainCategorySchema),
  items: z.array(CaptainMenuItemSchema),
});
export type MenuResponse = z.infer<typeof MenuResponseSchema>;

// ----------------------------------------------------------------- orders --

export const NewOrderItemModifierSchema = z.object({
  modifier_id: z.string().uuid(),
  group_name: z.string(),
  option_name: z.string(),
  price_delta_paise: z.number().int(),
});
export type NewOrderItemModifier = z.infer<typeof NewOrderItemModifierSchema>;

// A variant is mandatory on every line — never `variant_id: null`
// (docs/captain-api.md, CLAUDE.md M4 criterion-1 lesson).
export const NewOrderItemSchema = z.object({
  menu_item_id: z.string().uuid(),
  variant_id: z.string().uuid(),
  quantity: z.number().int().positive(),
  unit_price_paise: z.number().int(),
  notes: z.string().nullable(),
  modifiers: z.array(NewOrderItemModifierSchema),
});
export type NewOrderItem = z.infer<typeof NewOrderItemSchema>;

export interface CreateOrderRequest {
  order_type: "DINE_IN";
  table_id: string;
  items: NewOrderItem[];
}

const KotSummarySchema = z.object({
  id: z.string().uuid(),
  station: z.string(),
  sequence: z.number().int(),
  status: z.string(),
});
export type KotSummary = z.infer<typeof KotSummarySchema>;

const SendResponseSchema = z.object({
  order: CanonicalOrderSchema,
  kots: z.array(KotSummarySchema),
});
export type SendResponse = z.infer<typeof SendResponseSchema>;

// -------------------------------------------------------------- transport --

async function request<T>(
  token: string,
  path: string,
  parse: (data: unknown) => T,
  init?: RequestInit,
): Promise<T> {
  // `fetch` is called free, never stored on an object field — it is
  // receiver-bound in browsers and throws `Illegal invocation` when detached
  // (CLAUDE.md; no linter and no Node-based test catches this).
  const response = await fetch(`${BASE_URL}${path}`, {
    ...init,
    headers: {
      "content-type": "application/json",
      ...authHeader(token),
      ...(init?.headers ?? {}),
    },
  });

  if (!response.ok) {
    const body = await response.json().catch(() => null);
    const parsed = CaptainErrorSchema.safeParse(body);
    throw new ApiError(
      response.status,
      parsed.success ? parsed.data.code : "unknown",
      parsed.success ? parsed.data.message : `request failed with status ${response.status}`,
    );
  }

  return parse(await response.json());
}

export function fetchSession(token: string): Promise<Session> {
  return request(token, "/api/session", (d) => SessionSchema.parse(d));
}

export function fetchTables(token: string): Promise<CaptainTable[]> {
  return request(token, "/api/tables", (d) => TablesResponseSchema.parse(d)).then(
    (r) => r.tables,
  );
}

export function fetchMenu(token: string): Promise<MenuResponse> {
  return request(token, "/api/menu", (d) => MenuResponseSchema.parse(d));
}

export function createOrder(token: string, body: CreateOrderRequest): Promise<CanonicalOrder> {
  return request(token, "/api/orders", (d) => CanonicalOrderSchema.parse(d), {
    method: "POST",
    body: JSON.stringify(body),
  });
}

export function appendOrderItems(
  token: string,
  orderId: string,
  item: NewOrderItem,
): Promise<CanonicalOrder> {
  return request(token, `/api/orders/${orderId}/items`, (d) => CanonicalOrderSchema.parse(d), {
    method: "POST",
    body: JSON.stringify(item),
  });
}

export function sendOrder(token: string, orderId: string): Promise<SendResponse> {
  return request(token, `/api/orders/${orderId}/send`, (d) => SendResponseSchema.parse(d), {
    method: "POST",
  });
}
