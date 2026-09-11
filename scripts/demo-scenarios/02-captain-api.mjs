/**
 * Stage 2 — the captain LAN JSON API on 9320, driven directly, plus the
 * central claim of demo step 1a: the KOT a waiter sends reaches the KDS.
 *
 * The KDS assertion subscribes to the REAL 9310 WebSocket the KDS page uses,
 * as a second client, BEFORE the send — so a ticket observed arriving cannot
 * be a ticket that was already there. Nothing here is mocked; every response
 * recorded is a response the shipping POS binary produced.
 */
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import {
  CAPTAIN, KDS_WS, REPO_ROOT, RESULT_DIR,
  fingerprint, http, loadState, record, saveState, sleep,
} from "./lib.mjs";

const secrets = JSON.parse(readFileSync(join(RESULT_DIR, "secrets.json"), "utf8"));
const TOKEN = secrets.deviceToken;
const state = loadState();
const bearer = (t) => ({ authorization: `Bearer ${t}`, "content-type": "application/json" });

/** Read the operator's KDS device identity from apps/kds/.env.dev — read-only, never printed. */
function kdsEnv() {
  const p = join(REPO_ROOT, "apps", "kds", ".env.dev");
  if (!existsSync(p)) return null;
  const out = {};
  for (const line of readFileSync(p, "utf8").split("\n")) {
    const m = line.match(/^(VITE_[A-Z_]+)=(.*)$/);
    if (m) out[m[1]] = m[2].trim();
  }
  return out;
}

/**
 * Subscribe to the KDS LAN socket as a second client and collect frames.
 * Returns a handle whose `frames` array fills in the background.
 */
async function openKdsSocket(env) {
  const url = `${KDS_WS}/kds?outlet_id=${encodeURIComponent(env.VITE_KDS_OUTLET_ID)}&device_id=${encodeURIComponent(env.VITE_KDS_DEVICE_ID)}`;
  const ws = new WebSocket(url);
  const frames = [];
  const handle = { frames, ws, closed: false, error: null };
  await new Promise((resolve, reject) => {
    const t = setTimeout(() => reject(new Error("KDS socket did not open within 8s")), 8000);
    ws.onopen = () => {
      clearTimeout(t);
      // First frame must be a valid auth message (ADR-017 hole 3) — a browser
      // WebSocket cannot set an Authorization header on the handshake at all.
      ws.send(JSON.stringify({ type: "auth", device_token: env.VITE_KDS_DEVICE_TOKEN }));
      resolve();
    };
    ws.onerror = (e) => { clearTimeout(t); reject(new Error(`KDS socket error: ${e?.message ?? "unknown"}`)); };
  });
  ws.onmessage = (ev) => {
    try { frames.push(JSON.parse(ev.data)); } catch { frames.push({ type: "<unparseable>", raw: String(ev.data).slice(0, 200) }); }
  };
  ws.onclose = (ev) => { handle.closed = true; handle.closeCode = ev.code; };
  return handle;
}

async function waitForFrame(handle, predicate, ms) {
  const deadline = Date.now() + ms;
  while (Date.now() < deadline) {
    const hit = handle.frames.find(predicate);
    if (hit) return hit;
    if (handle.closed) return null;
    await sleep(200);
  }
  return null;
}

const run = async () => {
  // ---------------------------------------------------------------- S-CAP-01
  const noAuth = await http(`${CAPTAIN}/api/session`);
  record({
    id: "S-CAP-01",
    demoStep: "1a",
    scenario: "Captain API refuses a request with no Authorization header",
    surface: "captain 9320",
    precondition: "POS process up, captain listener bound on 9320",
    steps: "GET /api/session with no Authorization header",
    expected: "401 UNAUTHORIZED",
    actual: `HTTP ${noAuth.status} ${noAuth.text.slice(0, 100)}`,
    status: noAuth.status === 401 ? "PASS" : "FAIL",
    evidence: `HTTP ${noAuth.status}`,
  });

  // ---------------------------------------------------------------- S-CAP-02
  const garbage = await http(`${CAPTAIN}/api/session`, { headers: bearer("00000000-0000-0000-0000-000000000000.notarealsecretvalue") });
  record({
    id: "S-CAP-02",
    demoStep: "1a",
    scenario: "Pairing with a garbage device token is refused",
    surface: "captain 9320",
    precondition: "A syntactically plausible but unenrolled credential",
    steps: "GET /api/session with a fabricated credential_id.secret",
    expected: "401 — absence is unknown, never allow (docs/captain-api.md)",
    actual: `HTTP ${garbage.status} ${garbage.text.slice(0, 120)}`,
    status: garbage.status === 401 ? "PASS" : "FAIL",
    evidence: `HTTP ${garbage.status}`,
    notes: "Falsifier for S-CAP-03: a listener that accepts anything would pass S-CAP-03 too.",
  });

  // ---------------------------------------------------------------- S-CAP-03
  const session = await http(`${CAPTAIN}/api/session`, { headers: bearer(TOKEN) });
  const sessionOk = session.status === 200 && session.body?.device_kind === "WAITER";
  record({
    id: "S-CAP-03",
    demoStep: "1a",
    scenario: "Pairing with the real WAITER token returns this device's identity",
    surface: "captain 9320",
    precondition: `WAITER enrolled in S-BE-06 (token ${fingerprint(TOKEN)}); credential cached at the edge`,
    steps: "GET /api/session with the enrolled WAITER token",
    expected: "200 {device_id, outlet_id, outlet_name, device_kind:'WAITER'}",
    actual: `HTTP ${session.status}; device_kind=${session.body?.device_kind ?? "n/a"}; outlet_name=${session.body?.outlet_name ?? "n/a"}; device_id=${session.body?.device_id ?? "n/a"}`,
    status: sessionOk ? "PASS" : "FAIL",
    evidence: `HTTP ${session.status}, device_kind ${session.body?.device_kind ?? "n/a"}`,
    notes:
      session.status === 401
        ? "401 here means the cloud credential has not yet reached device_credential_cache at the edge — the A5 config pull has not run or has not included it."
        : "The device_id comes from the resolved credential, never the request body.",
  });

  if (!sessionOk) {
    record({
      id: "S-CAP-BLOCKED",
      demoStep: "1a",
      scenario: "Remaining captain scenarios (tables, menu, order, send, KDS)",
      surface: "captain 9320",
      precondition: "A paired WAITER session",
      steps: "n/a",
      expected: "n/a",
      actual: "Pairing failed at S-CAP-03, so no authenticated captain call can be made",
      status: "BLOCKED",
      evidence: `GET /api/session returned HTTP ${session.status}`,
    });
    return;
  }
  const waiterDeviceId = session.body.device_id;
  saveState({ waiterDeviceIdFromSession: waiterDeviceId });

  // ---------------------------------------------------------------- S-CAP-04
  const tables = await http(`${CAPTAIN}/api/tables`, { headers: bearer(TOKEN) });
  const tableList = tables.body?.tables ?? [];
  const tableShapeOk =
    Array.isArray(tableList) &&
    tableList.length > 0 &&
    tableList.every((t) => typeof t.id === "string" && typeof t.name === "string" && "open_session_id" in t && "open_order_id" in t);
  record({
    id: "S-CAP-04",
    demoStep: "1a",
    scenario: "GET /api/tables returns the outlet's tables with live session state",
    surface: "captain 9320",
    precondition: "Paired WAITER session",
    steps: "GET /api/tables",
    expected: "200 {tables:[{id,name,seats,open_session_id,open_order_id}]}, non-empty",
    actual: `HTTP ${tables.status}; ${tableList.length} tables; keys=${Object.keys(tableList[0] ?? {}).join(",")}`,
    status: tables.status === 200 && tableShapeOk ? "PASS" : "FAIL",
    evidence: `HTTP ${tables.status}, ${tableList.length} rows`,
  });

  // ---------------------------------------------------------------- S-CAP-05
  const menu = await http(`${CAPTAIN}/api/menu`, { headers: bearer(TOKEN) });
  const items = menu.body?.items ?? [];
  const cats = menu.body?.categories ?? [];
  const withVariants = items.filter((i) => (i.variants ?? []).length > 0);
  record({
    id: "S-CAP-05",
    demoStep: "1a",
    scenario: "GET /api/menu returns the whole catalogue in one call",
    surface: "captain 9320",
    precondition: "Paired WAITER session",
    steps: "GET /api/menu",
    expected: "200 with categories[], items[] each carrying variants[] and modifiers[]",
    actual: `HTTP ${menu.status}; ${cats.length} categories, ${items.length} items, ${withVariants.length} items with at least one variant`,
    status: menu.status === 200 && cats.length > 0 && items.length > 0 ? "PASS" : "FAIL",
    evidence: `HTTP ${menu.status}, ${cats.length} categories / ${items.length} items`,
  });

  // ---------------------------------------------------------------- S-CAP-06
  // A variant is mandatory on every line (docs/captain-api.md). An item with
  // no variant at all is a seed defect to REPORT, not a null to send.
  const noVariant = items.filter((i) => (i.variants ?? []).length === 0);
  record({
    id: "S-CAP-06",
    demoStep: "1a",
    scenario: "Every menu item carries at least one variant, so no line needs a null variant_id",
    surface: "captain 9320",
    precondition: "GET /api/menu returned the catalogue",
    steps: "Count items whose variants[] is empty",
    expected: "0 — a variant is mandatory on every line",
    actual: `${noVariant.length} of ${items.length} items have no variant${noVariant.length ? `: ${noVariant.slice(0, 5).map((i) => i.name).join(", ")}${noVariant.length > 5 ? ", …" : ""}` : ""}`,
    status: noVariant.length === 0 ? "PASS" : "FAIL",
    evidence: `${noVariant.length}/${items.length} items with empty variants[]`,
    notes:
      "docs/captain-api.md: 'if an item has no variant at all, that is a seed defect to report, not a null to send.' " +
      "The till hardcoding variantId:null for a whole milestone is why this is checked at all (M4 criterion 1).",
  });

  // ---------------------------------------------------------------- S-CAP-07
  const unavailable = items.filter((i) => i.is_available === false);
  const availableField = items.every((i) => typeof i.is_available === "boolean");
  record({
    id: "S-CAP-07",
    demoStep: "1a",
    scenario: "is_available is served on every menu item so the page can respect a stock-out",
    surface: "captain 9320",
    precondition: "GET /api/menu returned the catalogue",
    steps: "Check is_available is a boolean on every item; count snoozed items",
    expected: "Boolean on every item; the field is served as stored",
    actual: `is_available boolean on all ${items.length} items = ${availableField}; ${unavailable.length} items currently snoozed`,
    status: availableField ? "PASS" : "FAIL",
    evidence: `${unavailable.length} of ${items.length} items have is_available=false`,
    notes:
      unavailable.length === 0
        ? "NOTHING IS SNOOZED IN THIS SEED, so this run proves the field is SERVED, not that the page HONOURS it. " +
          "S-CAP-UI-06 drives the page against a snoozed item to close that gap; without one, 'the page respects it' is untested."
        : "At least one snoozed item exists, so the page-side check in S-CAP-UI-06 is meaningful.",
  });

  // Pick a real item with a default variant for the order scenarios.
  const orderable = withVariants.find((i) => i.is_available !== false);
  if (!orderable) {
    record({
      id: "S-CAP-08",
      demoStep: "1a",
      scenario: "Create an order from the captain API",
      surface: "captain 9320",
      precondition: "An available menu item with a variant",
      steps: "n/a",
      expected: "n/a",
      actual: "No available item with a variant exists in the catalogue",
      status: "BLOCKED",
      evidence: `${items.length} items, ${withVariants.length} with variants`,
    });
    return;
  }
  const variant = orderable.variants.find((v) => v.is_default) ?? orderable.variants[0];
  const freeItem = withVariants.find((i) => (i.modifiers ?? []).some((m) => m.price_delta_paise === 0));
  const table = tableList.find((t) => t.open_order_id == null) ?? tableList[0];

  const line = (extra = {}) => ({
    menu_item_id: orderable.id,
    variant_id: variant.id,
    quantity: 1,
    unit_price_paise: orderable.base_price_paise + (variant.price_delta_paise ?? 0),
    notes: null,
    modifiers: [],
    ...extra,
  });

  // ---------------------------------------------------------------- S-CAP-08
  const emptyOrder = await http(`${CAPTAIN}/api/orders`, {
    method: "POST",
    headers: bearer(TOKEN),
    body: JSON.stringify({ order_type: "DINE_IN", table_id: table.id, items: [] }),
  });
  record({
    id: "S-CAP-08",
    demoStep: "1a",
    scenario: "Creating an order with no lines is refused",
    surface: "captain 9320",
    precondition: "Paired WAITER session",
    steps: "POST /api/orders with items: []",
    expected: "400 EMPTY_ORDER",
    actual: `HTTP ${emptyOrder.status} code=${emptyOrder.body?.code ?? "n/a"}`,
    status: emptyOrder.status === 400 && emptyOrder.body?.code === "EMPTY_ORDER" ? "PASS" : "FAIL",
    evidence: `HTTP ${emptyOrder.status} ${emptyOrder.body?.code ?? ""}`,
  });

  // ---------------------------------------------------------------- S-CAP-09
  const nullVariant = await http(`${CAPTAIN}/api/orders`, {
    method: "POST",
    headers: bearer(TOKEN),
    body: JSON.stringify({ order_type: "DINE_IN", table_id: table.id, items: [line({ variant_id: null })] }),
  });
  record({
    id: "S-CAP-09",
    demoStep: "1a",
    scenario: "A line with a null variant_id is refused at the API, not silently accepted",
    surface: "captain 9320",
    precondition: "Paired WAITER session",
    steps: "POST /api/orders with items[0].variant_id = null",
    expected: "400 — a variant is mandatory on every line",
    actual: `HTTP ${nullVariant.status} code=${nullVariant.body?.code ?? "n/a"} ${String(nullVariant.body?.message ?? "").slice(0, 90)}`,
    status: nullVariant.status === 400 ? "PASS" : "FAIL",
    evidence: `HTTP ${nullVariant.status} ${nullVariant.body?.code ?? ""}`,
    notes:
      "This is the M4 criterion-1 defect as a guard: a null variant satisfied order_item_variant_id_fkey trivially " +
      "and meant no sale wrote a stock ledger row, and nothing noticed for a milestone.",
  });

  // ---------------------------------------------------------------- S-CAP-10
  const created = await http(`${CAPTAIN}/api/orders`, {
    method: "POST",
    headers: bearer(TOKEN),
    body: JSON.stringify({ order_type: "DINE_IN", table_id: table.id, items: [line()] }),
  });
  const order = created.body;
  record({
    id: "S-CAP-10",
    demoStep: "1a",
    scenario: "A waiter creates an order on a table from the captain API",
    surface: "captain 9320",
    precondition: `Paired WAITER session; table ${table.name}; item ${orderable.name}`,
    steps: "POST /api/orders {order_type:DINE_IN, table_id, items:[one line with a real variant]}",
    expected: "201 CanonicalOrder in DRAFT with one line",
    actual: `HTTP ${created.status}; order_id=${order?.id ?? "n/a"}; status=${order?.status ?? "n/a"}; lines=${order?.items?.length ?? 0}`,
    status: created.status === 201 && order?.id ? "PASS" : "FAIL",
    evidence: `HTTP ${created.status}, order ${order?.id ?? "n/a"}, ${order?.items?.length ?? 0} line(s)`,
  });
  if (!order?.id) return;
  saveState({ captainOrderId: order.id, captainOrderNumber: order.order_number ?? order.orderNumber ?? null });

  // ---------------------------------------------------------------- S-CAP-11
  // THE KNOWN TRAP: create_order_impl takes device_id from state.device_id,
  // which is the TILL. If attribution were not overridden the order would read
  // as till-authored on every screen — and it would look correct in review.
  const attributed = order.device_id ?? order.deviceId ?? null;
  record({
    id: "S-CAP-11",
    demoStep: "1a",
    scenario: "order.device_id is the WAITER's device, not the till's",
    surface: "captain 9320",
    precondition: `Waiter device ${waiterDeviceId}; till device is a different row in the device table`,
    steps: "Read device_id off the CanonicalOrder returned by POST /api/orders",
    expected: `device_id == ${waiterDeviceId} (the resolved credential's device)`,
    actual: `order.device_id=${attributed ?? "<absent from CanonicalOrder>"}`,
    status: attributed === waiterDeviceId ? "PASS" : "FAIL",
    evidence: `order ${order.id} device_id ${attributed ?? "absent"}`,
    notes:
      attributed === null
        ? "CanonicalOrder does not expose device_id, so this is asserted against Postgres in S-SYNC-03 instead."
        : "Attribution comes from the resolved credential, never the request body (ADR-014 §6).",
  });

  // ---------------------------------------------------------------- S-CAP-12
  const appended = await http(`${CAPTAIN}/api/orders/${order.id}/items`, {
    method: "POST",
    headers: bearer(TOKEN),
    body: JSON.stringify(line({ quantity: 2, notes: "scenario suite append" })),
  });
  record({
    id: "S-CAP-12",
    demoStep: "1a",
    scenario: "A waiter appends a line to an order that already exists",
    surface: "captain 9320",
    precondition: `Order ${order.id} in DRAFT with one line`,
    steps: "POST /api/orders/{orderId}/items with one more line",
    expected: "201 CanonicalOrder now carrying two lines",
    actual: `HTTP ${appended.status}; lines=${appended.body?.items?.length ?? 0}`,
    status: appended.status === 201 && (appended.body?.items?.length ?? 0) === 2 ? "PASS" : "FAIL",
    evidence: `HTTP ${appended.status}, ${appended.body?.items?.length ?? 0} lines`,
  });

  // ---------------------------------------------------------------- S-CAP-13
  let modifierStatus = "NOT TESTABLE";
  let modifierActual = "No menu item in this catalogue carries a free modifier (price_delta_paise == 0), so the reduced-scope selectable set is empty";
  let modifierEvidence = `${items.reduce((n, i) => n + (i.modifiers ?? []).length, 0)} modifier rows across ${items.length} items, 0 of them free`;
  if (freeItem) {
    const mod = freeItem.modifiers.find((m) => m.price_delta_paise === 0);
    const fv = freeItem.variants.find((v) => v.is_default) ?? freeItem.variants[0];
    const withMod = await http(`${CAPTAIN}/api/orders/${order.id}/items`, {
      method: "POST",
      headers: bearer(TOKEN),
      body: JSON.stringify({
        menu_item_id: freeItem.id,
        variant_id: fv.id,
        quantity: 1,
        unit_price_paise: freeItem.base_price_paise + (fv.price_delta_paise ?? 0),
        notes: null,
        modifiers: [
          { modifier_id: mod.id, group_name: mod.group_name, option_name: mod.option_name, price_delta_paise: 0 },
        ],
      }),
    });
    const lines = withMod.body?.items ?? [];
    const landed = lines.some((l) => (l.modifiers ?? []).some((m) => m.option_name === mod.option_name || m.optionName === mod.option_name));
    modifierStatus = withMod.status === 201 && landed ? "PASS" : "FAIL";
    modifierActual = `HTTP ${withMod.status}; free modifier "${mod.group_name}/${mod.option_name}" (0 paise) sent; present on the returned line = ${landed}`;
    modifierEvidence = `HTTP ${withMod.status}, order ${order.id}, modifier ${mod.option_name}`;
  }
  record({
    id: "S-CAP-13",
    demoStep: "1a",
    scenario: "A free-modifier selection lands in the request body and on the stored line",
    surface: "captain 9320",
    precondition: "An item carrying at least one modifier with price_delta_paise == 0",
    steps: "POST /api/orders/{orderId}/items with modifiers:[{price_delta_paise:0}] and read the echoed line",
    expected: "201 and the modifier present on the returned CanonicalOrder line",
    actual: modifierActual,
    status: modifierStatus,
    evidence: modifierEvidence,
    notes: "Only free modifiers are selectable in the reduced scope (docs/captain-api.md).",
  });

  // ------------------------------------------- S-CAP-14 / S-KDS-01 the claim
  const env = kdsEnv();
  let kdsHandle = null;
  let snapshot = null;
  if (env?.VITE_KDS_DEVICE_TOKEN) {
    try {
      kdsHandle = await openKdsSocket(env);
      snapshot = await waitForFrame(kdsHandle, (f) => f.type === "snapshot", 8000);
    } catch (err) {
      kdsHandle = { frames: [], error: String(err), closed: true };
    }
  }
  record({
    id: "S-KDS-01",
    demoStep: "1a",
    scenario: "A second client can subscribe to the real KDS LAN socket and receives a snapshot",
    surface: "KDS LAN ws 9310",
    precondition: "POS process hosts the LAN server; KDS device credential from apps/kds/.env.dev (never printed)",
    steps: "Open ws://localhost:9310/kds?outlet_id&device_id, send the first-frame auth message, await a snapshot frame",
    expected: "Socket opens, auth accepted, a snapshot frame arrives",
    actual: env?.VITE_KDS_DEVICE_TOKEN
      ? kdsHandle?.error
        ? `Socket failed: ${kdsHandle.error}`
        : `Socket open; snapshot received = ${Boolean(snapshot)}; snapshot tickets = ${snapshot?.kots?.length ?? snapshot?.payload?.length ?? "n/a"}; frames seen = ${kdsHandle.frames.length}`
      : "apps/kds/.env.dev carries no VITE_KDS_DEVICE_TOKEN, so no client identity is available",
    status: snapshot ? "PASS" : env?.VITE_KDS_DEVICE_TOKEN ? "FAIL" : "NOT TESTABLE",
    evidence: snapshot ? `snapshot frame with ${snapshot.kots?.length ?? snapshot.payload?.length ?? "?"} tickets` : `frames=${kdsHandle?.frames?.length ?? 0}`,
    notes: "Subscribed BEFORE the send, so a ticket observed in S-KDS-02 cannot be one that was already on the hub.",
  });

  const framesBefore = kdsHandle?.frames?.length ?? 0;

  // ---------------------------------------------------------------- S-CAP-14
  const sent = await http(`${CAPTAIN}/api/orders/${order.id}/send`, { method: "POST", headers: bearer(TOKEN) });
  const kots = sent.body?.kots ?? [];
  record({
    id: "S-CAP-14",
    demoStep: "1a",
    scenario: "Sending the order confirms it and cuts KOTs",
    surface: "captain 9320",
    precondition: `Order ${order.id} with lines`,
    steps: "POST /api/orders/{orderId}/send",
    expected: "200 {order, kots:[{id,station,sequence,status:'NEW'}]} with at least one KOT",
    actual: `HTTP ${sent.status}; order.status=${sent.body?.order?.status ?? "n/a"}; kots=${kots.length} (${kots.map((k) => `${k.station}#${k.sequence}/${k.status}`).join(", ")})`,
    status: sent.status === 200 && kots.length > 0 ? "PASS" : "FAIL",
    evidence: `HTTP ${sent.status}, ${kots.length} KOT(s), order status ${sent.body?.order?.status ?? "n/a"}`,
  });
  saveState({ captainKotIds: kots.map((k) => k.id) });

  // ---------------------------------------------------------------- S-KDS-02
  // THE DEMO'S CENTRAL CLAIM.
  let arrived = null;
  if (kdsHandle && !kdsHandle.error) {
    arrived = await waitForFrame(
      kdsHandle,
      (f) => f.type === "kot_upserted" && kots.some((k) => k.id === (f.kot?.id ?? f.payload?.id)),
      12000,
    );
  }
  record({
    id: "S-KDS-02",
    demoStep: "1a",
    scenario: "THE CENTRAL CLAIM — a KOT sent from the waiter's phone reaches the KDS hub",
    surface: "captain 9320 → KDS LAN ws 9310",
    precondition: `Socket subscribed and idle at ${framesBefore} frames BEFORE the send; order ${order.id} not yet sent`,
    steps: "POST /api/orders/{orderId}/send, then await a kot_upserted frame carrying one of the returned KOT ids",
    expected: "A kot_upserted frame naming a KOT id from the send response",
    actual: kdsHandle?.error
      ? `Could not subscribe to 9310: ${kdsHandle.error}`
      : arrived
        ? `kot_upserted received for KOT ${arrived.kot?.id ?? arrived.payload?.id}; frames grew ${framesBefore} → ${kdsHandle.frames.length}`
        : `No kot_upserted naming any of ${kots.map((k) => k.id).join(", ")} within 12s; frames grew ${framesBefore} → ${kdsHandle?.frames?.length ?? 0}; types seen = ${[...new Set((kdsHandle?.frames ?? []).map((f) => f.type))].join(",")}`,
    status: arrived ? "PASS" : kdsHandle?.error ? "BLOCKED" : "FAIL",
    evidence: arrived
      ? `kot_upserted frame, KOT ${arrived.kot?.id ?? arrived.payload?.id}, order ${order.id}`
      : `frames before=${framesBefore} after=${kdsHandle?.frames?.length ?? 0}`,
    notes:
      "The frame count before the send is recorded so an arriving ticket cannot be confused with a ticket already on the hub.",
  });

  // ---------------------------------------------------------------- S-CAP-15
  const appendAfterSend = await http(`${CAPTAIN}/api/orders/${order.id}/items`, {
    method: "POST",
    headers: bearer(TOKEN),
    body: JSON.stringify(line({ quantity: 1, notes: "append after send" })),
  });
  record({
    id: "S-CAP-15",
    demoStep: "1a",
    scenario: "A waiter appends to an order the kitchen already has",
    surface: "captain 9320",
    precondition: `Order ${order.id} already sent (status ${sent.body?.order?.status ?? "n/a"})`,
    steps: "POST /api/orders/{orderId}/items after the send",
    expected: "201 — append-after-send is legal through DRAFT/CONFIRMED/SENT_TO_KITCHEN/PREPARING",
    actual: `HTTP ${appendAfterSend.status}; lines now ${appendAfterSend.body?.items?.length ?? 0}; code=${appendAfterSend.body?.code ?? ""}`,
    status: appendAfterSend.status === 201 ? "PASS" : "FAIL",
    evidence: `HTTP ${appendAfterSend.status}, ${appendAfterSend.body?.items?.length ?? 0} lines`,
    notes: "This is the whole point of append-only: the waiter walks back and adds two more.",
  });

  // ---------------------------------------------------------------- S-CAP-16
  const unknownOrder = await http(`${CAPTAIN}/api/orders/0191a000-0000-7000-8000-ffffffffffff/send`, {
    method: "POST",
    headers: bearer(TOKEN),
  });
  record({
    id: "S-CAP-16",
    demoStep: "1a",
    scenario: "Sending an order that does not exist is a 404, not a 500",
    surface: "captain 9320",
    precondition: "Paired WAITER session",
    steps: "POST /api/orders/{unknown uuid}/send",
    expected: "404 ORDER_NOT_FOUND",
    actual: `HTTP ${unknownOrder.status} code=${unknownOrder.body?.code ?? "n/a"}`,
    status: unknownOrder.status === 404 ? "PASS" : "FAIL",
    evidence: `HTTP ${unknownOrder.status} ${unknownOrder.body?.code ?? ""}`,
  });

  // ---------------------------------------------------------------- S-CAP-17
  const badRoute = await http(`${CAPTAIN}/api/orders/${order.id}/cancel`, { method: "POST", headers: bearer(TOKEN) });
  record({
    id: "S-CAP-17",
    demoStep: "1a",
    scenario: "An out-of-scope captain route is refused rather than half-implemented",
    surface: "captain 9320",
    precondition: "Paired WAITER session",
    steps: "POST /api/orders/{orderId}/cancel — explicitly out of scope in docs/captain-api.md",
    expected: "404, with no order state change",
    actual: `HTTP ${badRoute.status} code=${badRoute.body?.code ?? "n/a"}`,
    status: badRoute.status === 404 ? "PASS" : "FAIL",
    evidence: `HTTP ${badRoute.status}`,
    notes: "No cancel, no line removal, no quantity edit, no payment — out of scope, explicitly.",
  });

  kdsHandle?.ws?.close?.();
  await sleep(300);
};

run().catch((err) => {
  console.error(err);
  process.exitCode = 1;
});
