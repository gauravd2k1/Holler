import {
  CanonicalOrderSchema,
  MenuItemSchema,
  MenuCategorySchema,
  ListSuppliersResponseSchema,
  SupplierWithItemsSchema,
  GoodsReceiptPageSchema,
  type MenuItem,
  type MenuCategory,
  type MenuItemPatch,
  type Supplier,
  type SupplierItem,
  type SupplierWithItems,
  type GoodsReceiptPage,
} from "@holler/contracts";
import { ErrorResponseSchema } from "@holler/contracts";
import { z } from "zod";
import { authHeader } from "./session";

/**
 * The cloud API client for the back office.
 *
 * NO HARD-CODED URL (CLAUDE.md). The base comes from the environment, and its
 * absence is a startup error rather than a silent fallback to localhost: a
 * build that quietly points at a developer's machine is one that looks correct
 * in every environment and works in exactly one.
 */
const BASE_URL: string = import.meta.env.VITE_ADMIN_API_BASE_URL ?? "";
const OUTLET_ID: string = import.meta.env.VITE_ADMIN_OUTLET_ID ?? "";
// ADR-012's time-boxed interim: the tenant travels as a header until host-based
// resolution replaces it. Sent on login only.
const TENANT_ID: string = import.meta.env.VITE_ADMIN_TENANT_ID ?? "";

export function configError(): string | null {
  if (BASE_URL === "") return "VITE_ADMIN_API_BASE_URL is not set.";
  if (OUTLET_ID === "") return "VITE_ADMIN_OUTLET_ID is not set.";
  if (TENANT_ID === "") return "VITE_ADMIN_TENANT_ID is not set.";
  return null;
}

export function outletId(): string {
  return OUTLET_ID;
}

export function apiBaseUrl(): string {
  return BASE_URL;
}

export function tenantId(): string {
  return TENANT_ID;
}

export class ApiError extends Error {
  readonly code: string;
  readonly status: number;

  constructor(status: number, code: string, message: string) {
    super(message);
    this.code = code;
    this.status = status;
  }
}

async function request<T>(path: string, schema: z.ZodType<T>, init?: RequestInit): Promise<T> {
  // `fetch` is called free rather than stored on anything. It is
  // receiver-bound in browsers and throws `Illegal invocation` when detached,
  // and no linter and no Node-based test catches that (CLAUDE.md).
  const response = await fetch(`${BASE_URL}${path}`, {
    ...init,
    headers: {
      "content-type": "application/json",
      // Bearer, from the in-memory session. Not a cookie: the backend reads
      // Authorization and nothing else, and a cookie would additionally need
      // CSRF handling this app has no reason to take on.
      ...authHeader(),
      ...(init?.headers ?? {}),
    },
  });

  if (!response.ok) {
    // A 4xx carries a reason the operator can act on; anything else is
    // reported as itself rather than guessed at. M6 A1's whole finding was
    // that a client-data failure reported as a server error tells the caller
    // to retry something that will never succeed.
    const body = await response.json().catch(() => null);
    const parsed = ErrorResponseSchema.safeParse(body);
    throw new ApiError(
      response.status,
      parsed.success ? parsed.data.code : "unknown",
      parsed.success ? parsed.data.message : `request failed with status ${response.status}`,
    );
  }

  // Parsed, not cast. A response that does not match the contract is a defect
  // to surface here, where the shape is known, rather than a crash three
  // components deeper with no clue where it came from.
  return schema.parse(await response.json());
}

// ------------------------------------------------------------------ menu --

export function listMenuItems(): Promise<MenuItem[]> {
  return request(`/menu/items?outlet_id=${OUTLET_ID}`, z.array(MenuItemSchema));
}

export function listMenuCategories(): Promise<MenuCategory[]> {
  return request(`/menu/categories?outlet_id=${OUTLET_ID}`, z.array(MenuCategorySchema));
}

/**
 * PATCH /menu/items/{itemId} — contracts 0.7.0, ADR-024.
 *
 * A SUCCESSFUL RESPONSE MEANS THE CLOUD ROW CHANGED AND NOTHING ELSE. Until the
 * edge's next config pull the till is still selling at the old price, so no
 * caller of this may render "updated" as though the shop floor had changed.
 * The screen says so in as many words.
 */
export function patchMenuItem(itemId: string, patch: MenuItemPatch): Promise<MenuItem> {
  return request(`/menu/items/${itemId}?outlet_id=${OUTLET_ID}`, MenuItemSchema, {
    method: "PATCH",
    body: JSON.stringify(patch),
  });
}

// ------------------------------------------------------------- suppliers --

export async function listSuppliers(): Promise<SupplierWithItems[]> {
  // The route wraps its result in { suppliers: [...] }. Unwrapped here rather
  // than in the component, so exactly one place knows the envelope shape.
  const page = await request(
    `/procurement/suppliers?outlet_id=${OUTLET_ID}`,
    ListSuppliersResponseSchema,
  );
  return page.suppliers;
}

/**
 * POST /procurement/suppliers.
 *
 * TYPED AGAINST THE CONTRACT, NOT `unknown`. An earlier version took `unknown`
 * for convenience, and the form built a payload by hand with `is_active` and
 * `config_version` on the ITEM — neither of which `SupplierItem` has (it
 * carries `is_preferred` and no config version). The backend decodes with
 * DisallowUnknownFields, so it answered a bare `invalid_input` with no detail,
 * and nothing upstream could have caught it: `unknown` disables the one check
 * that would have.
 */
export function createSupplier(body: {
  supplier: Supplier;
  items: SupplierItem[];
}): Promise<SupplierWithItems> {
  return request(`/procurement/suppliers`, SupplierWithItemsSchema, {
    method: "POST",
    body: JSON.stringify(body),
  });
}

// -------------------------------------------------------- goods receipts --

/**
 * GET /procurement/goods-receipts — contracts 0.7.0, ADR-024.
 *
 * THIS IS THE CLOUD'S REPLICA of what an outlet recorded, not the outlet's own
 * view. A GRN is edge-authoritative (ADR-019); the two may legitimately differ,
 * and the screen labels which one it is showing rather than implying a single
 * truth.
 */
export function listGoodsReceipts(cursor?: string): Promise<GoodsReceiptPage> {
  const query = new URLSearchParams({ outlet_id: OUTLET_ID });
  if (cursor !== undefined && cursor !== "") query.set("cursor", cursor);
  return request(`/procurement/goods-receipts?${query.toString()}`, GoodsReceiptPageSchema);
}

// ---------------------------------------------------------------- orders --

/**
 * GET /orders — served since Milestone 1, documented in the OpenAPI spec at
 * contracts 0.8.2. The route was never missing; only its entry in the spec
 * was, which is why a survey of the spec read `/orders` as POST-only.
 *
 * THIS IS THE CLOUD'S REPLICA, exactly as the goods-receipt list is. An order
 * is EDGE-AUTHORITATIVE (§50.1): the till takes it, the cloud replays it, and
 * an order rung while the uplink was down is real and complete at the outlet
 * and simply absent here until its outbox row lands. The screen says so.
 *
 * UNBOUNDED AND OLDEST-FIRST at the route. There is no limit and no cursor —
 * every order this outlet has ever replayed arrives in one response — and the
 * backend orders by created_at ascending. The newest-first ordering the screen
 * wants is applied HERE, over the response, rather than by changing a
 * Milestone 1 query during a demo build; both properties are filed for pilot.
 */
// The return type is INFERRED rather than annotated `CanonicalOrder[]`. Several
// fields on the contract carry `.default()`, so Zod's input and output types
// differ, and `request`'s generic binds to the input side; annotating the
// output type here produces a mismatch on exactly those optional-in/required-out
// fields. The parsed value is the output shape either way.
export async function listOrders() {
  const orders = await request(
    `/orders?outlet_id=${OUTLET_ID}`,
    z.array(CanonicalOrderSchema),
  );
  return [...orders].sort((a, b) =>
    b.timestamps.created_at.localeCompare(a.timestamps.created_at),
  );
}

// ---------------------------------------------------------------- outlet --

/**
 * The outlet's own name, for the header.
 *
 * WHITE-LABEL: HOLLER IS THE PRODUCT, THE RESTAURANT IS THE BUSINESS. The name
 * comes from the `outlet` row — one seeded value — so renaming the restaurant
 * is one change and a re-seed rather than a search across four frontends.
 *
 * `GET /outlets` is tenant-isolated and has existed since Milestone 1; this
 * needed no contract change. It is shaped here rather than imported because
 * `packages/contracts` carries no TS `Outlet` type — only the OpenAPI schema —
 * and inventing one in the admin would be a second definition of a shape the
 * spec already owns.
 */
const OutletSchema = z.object({
  id: z.string().uuid(),
  brand_id: z.string().uuid(),
  name: z.string(),
  timezone: z.string(),
  config_version: z.number().int(),
});

export async function currentOutletName(): Promise<string | null> {
  const outlets = await request(`/outlets`, z.array(OutletSchema));
  // The console serves one outlet per environment (VITE_ADMIN_OUTLET_ID), so
  // this picks that one rather than the first row: a tenant with two outlets
  // would otherwise label the header with whichever sorted first.
  return outlets.find((o) => o.id === OUTLET_ID)?.name ?? null;
}
