/**
 * Stage 8 — the two back-office routes contracts 0.7.0 added, driven directly.
 *
 * The admin console is a thin client over these, so exercising them separately
 * tells a reader whether a failing admin screen is a UI problem or a backend
 * one. Reuses the cached JWT — no extra login is spent (see lib.mjs on the
 * five-per-fifteen-minutes budget).
 */
import { BACKEND, OWNER, http, login, record, sql } from "./lib.mjs";

const run = async () => {
  const auth = await login(OWNER);
  if (auth.status !== 200) {
    for (const [id, step, scenario] of [
      ["S-API-01", "2", "PATCH /menu/items/{itemId} changes a price"],
      ["S-API-02", "2", "PATCH /menu/items/{itemId} refuses an identity field"],
      ["S-API-03", "2", "PATCH /menu/items/{itemId} refuses clearing hsn_sac"],
      ["S-API-04", "6", "GET /procurement/goods-receipts serves the replica"],
    ]) {
      record({
        id, demoStep: step, scenario, surface: "backend 8080",
        precondition: "An owner JWT",
        steps: "n/a", expected: "n/a",
        actual: `No JWT available (login returned HTTP ${auth.status}; the login budget may be spent — see S-BE-09)`,
        status: "BLOCKED", evidence: `POST /auth/login -> HTTP ${auth.status}`,
      });
    }
    return;
  }
  const h = { authorization: `Bearer ${auth.body.access_token}`, "content-type": "application/json" };

  const item = sql("select id||'|'||name||'|'||base_price_paise::text||'|'||coalesce(hsn_sac,'') from menu_item order by name limit 1;").trim();
  const [itemId, itemName, basePrice, hsn] = item.split("|");

  // ----------------------------------------------------------------- S-API-01
  const newPrice = Number(basePrice) + 100; // +₹1, integer paise end to end
  const patch = await http(`${BACKEND}/menu/items/${itemId}`, {
    method: "PATCH", headers: h, body: JSON.stringify({ base_price_paise: newPrice }),
  });
  const stored = sql(`select base_price_paise from menu_item where id='${itemId}';`).trim();
  record({
    id: "S-API-01",
    demoStep: "2",
    scenario: "PATCH /menu/items/{itemId} changes a price and it lands in Postgres",
    surface: "backend 8080 -> postgres 5432",
    precondition: `Menu item "${itemName}" at ${basePrice} paise`,
    steps: "PATCH base_price_paise +100, then read the column back from Postgres",
    expected: `2xx and the stored column reading ${newPrice}`,
    actual: `HTTP ${patch.status}; stored base_price_paise=${stored}`,
    status: patch.status >= 200 && patch.status < 300 && stored === String(newPrice) ? "PASS" : "FAIL",
    evidence: `HTTP ${patch.status}; SQL base_price_paise=${stored}`,
    notes: "Read back from the database, not from the response echo — an echo cannot see a write that did not happen.",
  });

  // ----------------------------------------------------------------- S-API-02
  const identity = await http(`${BACKEND}/menu/items/${itemId}`, {
    method: "PATCH", headers: h, body: JSON.stringify({ id: itemId, base_price_paise: newPrice }),
  });
  record({
    id: "S-API-02",
    demoStep: "2",
    scenario: "PATCH /menu/items/{itemId} refuses an identity field rather than ignoring it",
    surface: "backend 8080",
    precondition: "A valid owner JWT and a real menu item",
    steps: "PATCH with an `id` field in the body",
    expected: "422 — id/outlet_id/tenant_id are 422 if present, never a silent ignore (contracts 0.7.0)",
    actual: `HTTP ${identity.status} ${String(identity.body?.code ?? identity.text).slice(0, 80)}`,
    status: identity.status === 422 ? "PASS" : "FAIL",
    evidence: `HTTP ${identity.status}`,
    notes: "A silent ignore and a rejection look identical from the caller unless the status differs.",
  });

  // ----------------------------------------------------------------- S-API-03
  const clearHsn = await http(`${BACKEND}/menu/items/${itemId}`, {
    method: "PATCH", headers: h, body: JSON.stringify({ hsn_sac: "" }),
  });
  const hsnAfter = sql(`select coalesce(hsn_sac,'<null>') from menu_item where id='${itemId}';`).trim();
  record({
    id: "S-API-03",
    demoStep: "2",
    scenario: "PATCH /menu/items/{itemId} may change hsn_sac but never clear it",
    surface: "backend 8080 -> postgres 5432",
    precondition: `Item "${itemName}" carrying hsn_sac "${hsn}"`,
    steps: 'PATCH hsn_sac to an empty string, then read the column back',
    expected: "4xx, and the stored value unchanged — an invoice cannot issue without HSN/SAC (contracts 0.4.5)",
    actual: `HTTP ${clearHsn.status}; stored hsn_sac now "${hsnAfter}" (was "${hsn}")`,
    status: clearHsn.status >= 400 && hsnAfter === hsn ? "PASS" : "FAIL",
    evidence: `HTTP ${clearHsn.status}; SQL hsn_sac=${hsnAfter}`,
    notes: "The read-back is the real assertion: a 4xx with the column cleared anyway would still be a defect.",
  });

  // ----------------------------------------------------------------- S-API-04
  const grn = await http(`${BACKEND}/procurement/goods-receipts?outlet_id=${process.env.HOLLER_OUTLET_ID ?? "0191a000-0000-7000-8000-00000000000a"}`, { headers: h });
  const list = grn.body?.goods_receipts ?? grn.body?.items ?? grn.body?.data ?? [];
  const cloudCount = sql("select count(*) from goods_receipt_note;").trim();
  record({
    id: "S-API-04",
    demoStep: "6",
    scenario: "GET /procurement/goods-receipts serves the cloud replica of edge-authoritative receipts",
    surface: "backend 8080 -> postgres 5432",
    precondition: `Postgres holds ${cloudCount} goods_receipt_note row(s)`,
    steps: "GET /procurement/goods-receipts for this outlet and compare the count with the table",
    expected: `2xx returning ${cloudCount} receipt(s), each carrying all three quantity fields`,
    actual: `HTTP ${grn.status}; ${Array.isArray(list) ? list.length : "?"} receipt(s) returned against ${cloudCount} row(s) in goods_receipt_note. Body keys: ${Object.keys(grn.body ?? {}).join(",") || "<none>"}`,
    status: grn.status === 200 && Array.isArray(list) && String(list.length) === cloudCount ? "PASS" : "FAIL",
    evidence: `HTTP ${grn.status}; API ${Array.isArray(list) ? list.length : "?"} vs SQL ${cloudCount}`,
    notes:
      "A GRN is edge-authoritative and this is a REPLICA — the outlet's own view may legitimately differ and the two " +
      "are shown and labelled, never reconciled (contracts 0.7.0). An empty result here is consistent with gap A7: " +
      "if no receipt has ever replayed, the correct answer IS zero, and the count comparison says which case this is.",
  });
};

run().catch((err) => {
  console.error(err);
  process.exitCode = 1;
});
