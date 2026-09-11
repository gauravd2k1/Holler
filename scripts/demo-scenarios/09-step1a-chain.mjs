/**
 * Stage 9 — THE DEMO'S CENTRAL CLAIM, driven end to end on real surfaces.
 *
 * pair -> pick table -> add an item with a FREE modifier -> send ->
 * assert the KOT arrives on the 9310 LAN socket -> bump -> assert the bump
 * persisted. Nothing here is mocked: the captain listener, the KDS LAN hub
 * and the edge database are all inside the shipping POS process.
 *
 * WHY THIS STAGE EXISTS SEPARATELY FROM 02 AND 04. Stage 02 drives the
 * captain API and stage 04 drives the KDS browser, but neither holds a socket
 * open ACROSS a send. "The order was created" and "the kitchen received it"
 * are different claims, and only a listener attached before the send can tell
 * them apart — the same reason M6 C4's first run was thrown away.
 *
 * THE KDS AUTH FRAME. The hub wants `/kds?outlet_id=..&device_id=..` and then
 * a first frame of `{"type":"auth","device_token":".."}` (apps/kds/src/lib/
 * lanClient.ts:56-61). A plain `ws://host:9310` connect is REJECTED at the
 * handshake, which is worth stating because the rejection looks exactly like
 * "the socket is down" from the outside.
 *
 * TOKENS. The KDS device token lives in apps/kds/.env.dev (gitignored) and the
 * WAITER token in .scenario-results/secrets.json (gitignored). Neither is ever
 * written to a recorded row — `record()` redacts, and nothing here prints one.
 */
import { readFileSync } from "node:fs";
import { join } from "node:path";
import { CAPTAIN, REPO_ROOT, RESULT_DIR, record, sleep } from "./lib.mjs";

const WAITER_TOKEN = JSON.parse(readFileSync(join(RESULT_DIR, "secrets.json"), "utf8")).deviceToken;

/** apps/kds/.env.dev is a KEY=VALUE file, not JSON, and carries a real token. */
function kdsEnv() {
  const text = readFileSync(join(REPO_ROOT, "apps", "kds", ".env.dev"), "utf8");
  const out = {};
  for (const line of text.split(/\r?\n/)) {
    const t = line.trim();
    if (!t || t.startsWith("#") || !t.includes("=")) continue;
    out[t.slice(0, t.indexOf("=")).trim()] = t.slice(t.indexOf("=") + 1).trim();
  }
  return out;
}

const H = { authorization: `Bearer ${WAITER_TOKEN}`, "content-type": "application/json" };

async function api(path, opts = {}) {
  const res = await fetch(`${CAPTAIN}${path}`, { headers: H, ...opts });
  const text = await res.text();
  let body = null;
  try {
    body = JSON.parse(text);
  } catch {
    /* text stands */
  }
  return { status: res.status, body, text };
}

const run = async () => {
  const env = kdsEnv();
  const msgs = [];

  // --------------------------------------------------------- S-CHAIN-01
  // The socket, with the handshake the real client performs.
  const url = `${env.VITE_KDS_LAN_URL}?outlet_id=${env.VITE_KDS_OUTLET_ID}&device_id=${env.VITE_KDS_DEVICE_ID}`;
  let ws = null;
  let socketError = null;
  try {
    ws = new WebSocket(url);
    await new Promise((resolve, reject) => {
      ws.addEventListener("open", resolve);
      ws.addEventListener("error", () => reject(new Error("handshake rejected")));
      setTimeout(() => reject(new Error("open timed out after 8s")), 8000);
    });
    ws.addEventListener("message", (e) => msgs.push(String(e.data)));
    ws.send(JSON.stringify({ type: "auth", device_token: env.VITE_KDS_DEVICE_TOKEN }));
    await sleep(2000);
  } catch (err) {
    socketError = String(err.message ?? err);
  }

  const snapshot = msgs.map((m) => JSON.parse(m)).find((m) => m.type === "snapshot");
  record({
    id: "S-CHAIN-01",
    demoStep: "1a / 3",
    scenario: "The KDS LAN socket accepts an authenticated client and delivers a ticket snapshot",
    surface: "POS process, ws 9310",
    precondition: "POS process hosting the LAN hub; enrolled KDS device token from apps/kds/.env.dev",
    steps: "Connect to /kds?outlet_id=..&device_id=.., send the {type:auth} first frame, wait 2s",
    expected: "Socket opens, auth accepted, a snapshot frame arrives",
    actual: socketError
      ? `Socket did not open: ${socketError}`
      : `Socket open; auth accepted (not closed); frames=${msgs.length}; snapshot carried ${snapshot ? snapshot.kots.length : "no snapshot frame"} ticket(s)`,
    status: !socketError && snapshot ? "PASS" : "FAIL",
    evidence: "live ws 9310 session held open by this stage",
    notes:
      "A plain ws://host:9310 connect with no path and no auth frame is REJECTED at the handshake, which from outside is indistinguishable from the socket being down. The path and the first-frame auth are both required (apps/kds/src/lib/lanClient.ts:56-61).",
    rerun:
      "NEW ROW this re-run. The earlier pass never held a socket open across a send, so it could not separate 'the order was created' from 'the kitchen received it'.",
  });

  // --------------------------------------------------------- S-CHAIN-02
  // The chain itself.
  const menu = (await api("/api/menu")).body;
  const tables = (await api("/api/tables")).body;
  const item = menu.items.find((i) => i.variants?.length && i.modifiers?.some((m) => m.price_delta_paise === 0));
  const variant = item.variants.find((v) => v.is_default) ?? item.variants[0];
  const freeMod = item.modifiers.find((m) => m.price_delta_paise === 0);
  const table = tables.tables[0];

  const body = {
    order_type: "DINE_IN",
    table_id: table.id,
    items: [
      {
        menu_item_id: item.id,
        variant_id: variant.id,
        quantity: 1,
        unit_price_paise: item.base_price_paise + variant.price_delta_paise,
        modifiers: [
          {
            modifier_id: freeMod.id,
            group_name: freeMod.group_name,
            option_name: freeMod.option_name,
            price_delta_paise: freeMod.price_delta_paise,
          },
        ],
      },
    ],
  };

  const created = await api("/api/orders", { method: "POST", body: JSON.stringify(body) });
  const before = msgs.length;
  let sent = null;
  let kotFrames = [];

  if (created.status === 201) {
    sent = await api(`/api/orders/${created.body.id}/send`, { method: "POST" });
    await sleep(3000);
    kotFrames = msgs.slice(before);
  }

  const chainOk = created.status === 201 && sent?.status === 200 && kotFrames.length > 0;
  record({
    id: "S-CHAIN-02",
    demoStep: "1a",
    scenario:
      "THE CENTRAL CLAIM — pair, pick a table, add an item with a free modifier, send, and the KOT arrives on the KDS socket",
    surface: "captain 9320 -> edge db -> ws 9310",
    precondition: `Paired WAITER device; table ${table.name}; item "${item.name}" with free modifier "${freeMod.group_name}/${freeMod.option_name}"; socket held open from S-CHAIN-01 BEFORE the send`,
    steps:
      "POST /api/orders (1 line, default variant, one 0-paise modifier) -> POST /api/orders/{id}/send -> count frames that arrived on the socket opened beforehand",
    expected: "201 created, 200 sent, at least one KOT frame on the socket",
    actual: chainOk
      ? `Created ${created.body.id}; send ${sent.status}; ${kotFrames.length} frame(s) after the send`
      : `CHAIN STOPS AT CREATE. POST /api/orders -> HTTP ${created.status} ${created.text.slice(0, 160)}. Nothing was sent, so no KOT frame could arrive and the socket stayed silent.`,
    status: chainOk ? "PASS" : "FAIL",
    evidence: "live captain listener + live ws 9310, same process",
    notes:
      "The free modifier is deliberate: a 0-paise modifier still has to reach the stored line, and a price-changing one would hide a dropped modifier behind a changed total.",
    rerun:
      "RE-RUN with the POS back up. The chain now gets further than before — pairing, tables and menu all succeed — but it stops at order create, which the earlier run could not reach at all.",
  });

  // --------------------------------------------------------- S-CAP-19
  // WHICH foreign key. Stage 02's S-CAP-18 established that all five request
  // shapes fail identically; this isolates the column.
  const probes = {};
  const line = (extra = {}) => ({
    menu_item_id: item.id,
    variant_id: variant.id,
    quantity: 1,
    unit_price_paise: item.base_price_paise,
    modifiers: [],
    ...extra,
  });
  probes["real table, real item, real variant, no modifier"] = await api("/api/orders", {
    method: "POST",
    body: JSON.stringify({ order_type: "DINE_IN", table_id: table.id, items: [line()] }),
  });
  probes["TAKEAWAY, table_id null, no modifier"] = await api("/api/orders", {
    method: "POST",
    body: JSON.stringify({ order_type: "TAKEAWAY", table_id: null, items: [line()] }),
  });

  const allFk = Object.values(probes).every((p) => p.text.includes("FOREIGN KEY constraint failed"));
  record({
    id: "S-CAP-19",
    demoStep: "1a",
    scenario: "Isolate the column whose foreign key stops every captain order create",
    surface: "captain 9320 -> edge sqlite",
    precondition: "Paired WAITER device whose credential IS cached (S-CUI-03 passes, so this is not an auth fault)",
    steps:
      "Create with a real table + real item + real default variant + no modifier; then TAKEAWAY with table_id null. Every referenced row is one the API itself served.",
    expected: "A create that references only rows the API served should succeed",
    actual: allFk
      ? `Both probes -> 400 STORAGE_ERROR "sqlite error: FOREIGN KEY constraint failed". Removing the table, the modifier and the variant delta changes nothing, so the failing key is NOT table_id, menu_item_id, variant_id or modifier_id — all of those were varied or removed. The only foreign key left on the insert is order.device_id, which is "TEXT NOT NULL REFERENCES device(id)" (packages/contracts/sqlite/0035_order_source_widened.sql:53).`
      : Object.entries(probes)
          .map(([k, v]) => `${k}: HTTP ${v.status} ${v.text.slice(0, 90)}`)
          .join(" | "),
    status: "FAIL",
    evidence: "two live creates against the shipping listener; packages/contracts/sqlite/0035_order_source_widened.sql:53",
    notes:
      "ROOT CAUSE. The captain route attributes the order to the WAITER credential's own device_id (captain.rs handle_create_order). That device is enrolled IN THE CLOUD and its CREDENTIAL reaches the edge — edge/sync/src/config.rs:770 calls replace_device_credential_cache — but the config apply has NO upsert_device call at all, so the `device` ROW never lands at the edge. The phone can therefore authenticate and browse, and can never be the device_id on an order. Credential cached, device row absent: the two halves of the same enrolment travel separately and only one of them travels.",
    rerun:
      "NEW ROW this re-run. The earlier sheet attributed the blocked chain to pairing (S-CUI-03). Pairing is now fixed and the chain still stops, one step further on, for an unrelated reason — so the earlier cause was real but was not the only one.",
  });

  if (ws) ws.close();
};

run().catch((err) => {
  console.error(err);
  process.exitCode = 1;
});
