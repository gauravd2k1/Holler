// The scenario executor: generates a randomized (seeded) action sequence,
// drives it against the real Rust bridge (POS commands + DB introspection)
// and the real KDS client (kdsDriver.ts), and checks every invariant.
import { Rng } from "./rng";
import { HarnessBridge, type ScenarioInfo } from "./bridge";
import { KdsDriver } from "./kdsDriver";
import type { ActionLogEntry, InvariantId, InvariantOutcome, ScenarioResult } from "./types";
import { ALL_INVARIANTS } from "./types";

interface ItemFixture {
  key: "single" | "single2" | "multi" | "no_station";
  id: string;
  price: number;
  stationCount: number;
}

function itemPool(info: ScenarioInfo): ItemFixture[] {
  return [
    { key: "single", id: info.items.single_station.id, price: info.items.single_station.unit_price_paise, stationCount: 1 },
    { key: "single2", id: info.items.single_station_2.id, price: info.items.single_station_2.unit_price_paise, stationCount: 1 },
    { key: "multi", id: info.items.multi_station.id, price: info.items.multi_station.unit_price_paise, stationCount: 2 },
    { key: "no_station", id: info.items.no_station.id, price: info.items.no_station.unit_price_paise, stationCount: 0 },
  ];
}

/** Every paise amount this scenario has observed must be a real integer —
 * a float comparison in a money assertion is itself a defect (T11b brief).
 * `signed` allows negative (round_off_paise, a reversal payment's
 * amount_paise); everything else must be >= 0. */
function isIntegerPaise(n: unknown, signed = false): boolean {
  if (typeof n !== "number" || !Number.isInteger(n)) return false;
  return signed || n >= 0;
}

interface InvoiceLineLike {
  taxable_value_paise: number;
  cgst_paise: number;
  sgst_paise: number;
  igst_paise: number;
  cess_paise: number;
  total_paise: number;
  hsn_sac: string | null;
}

interface InvoiceLike {
  id: string;
  taxable_value_paise: number;
  cgst_paise: number;
  sgst_paise: number;
  igst_paise: number;
  cess_paise: number;
  round_off_paise: number;
  grand_total_paise: number;
  lines: InvoiceLineLike[];
}

/** Invariant 9: every paise field on the invoice reconciles — per line,
 * across lines, and up to the grand total (ADR-016's tax engine formula:
 * `grand_total = taxable_value + Σtax components + round_off`, exactly,
 * never as a float). */
function checkTaxReconciliation(
  inv: InvariantsRecord,
  invoice: InvoiceLike,
  where: string,
): void {
  for (const field of ["taxable_value_paise", "cgst_paise", "sgst_paise", "igst_paise", "cess_paise", "grand_total_paise"] as const) {
    if (!isIntegerPaise(invoice[field])) {
      mark(inv, "9_tax_reconciliation", false, `${where}: ${field}=${invoice[field]} is not a non-negative integer`);
    }
  }
  if (!isIntegerPaise(invoice.round_off_paise, true)) {
    mark(inv, "9_tax_reconciliation", false, `${where}: round_off_paise=${invoice.round_off_paise} is not an integer`);
  }
  for (const line of invoice.lines) {
    const lineTotal = line.taxable_value_paise + line.cgst_paise + line.sgst_paise + line.igst_paise + line.cess_paise;
    if (lineTotal !== line.total_paise) {
      mark(inv, "9_tax_reconciliation", false, `${where}: line total_paise=${line.total_paise} but taxable+tax components sum to ${lineTotal}`);
    }
    // ADR-016 0.4.5 §3 / the HSN/SAC track: a GST tax invoice is not a
    // compliant document if any line prints with no HSN (goods) or SAC
    // (services) code. `edge/database` rejects issuance outright when the
    // resolved code is NULL or blank, so if a line ever reaches this
    // orchestrator with one missing, the rejection did not fire —
    // seed-side and issuance-side are checked by the same invariant.
    if (line.hsn_sac === null || line.hsn_sac === undefined || line.hsn_sac.trim() === "") {
      mark(inv, "9_tax_reconciliation", false, `${where}: invoice line has a NULL or blank hsn_sac`);
    }
  }
  const sumTaxable = invoice.lines.reduce((a, l) => a + l.taxable_value_paise, 0);
  const sumCgst = invoice.lines.reduce((a, l) => a + l.cgst_paise, 0);
  const sumSgst = invoice.lines.reduce((a, l) => a + l.sgst_paise, 0);
  const sumIgst = invoice.lines.reduce((a, l) => a + l.igst_paise, 0);
  const sumCess = invoice.lines.reduce((a, l) => a + l.cess_paise, 0);
  if (sumTaxable !== invoice.taxable_value_paise) {
    mark(inv, "9_tax_reconciliation", false, `${where}: Σline taxable_value_paise=${sumTaxable} but invoice.taxable_value_paise=${invoice.taxable_value_paise}`);
  }
  if (sumCgst !== invoice.cgst_paise || sumSgst !== invoice.sgst_paise || sumIgst !== invoice.igst_paise || sumCess !== invoice.cess_paise) {
    mark(inv, "9_tax_reconciliation", false, `${where}: per-line tax components do not sum to the invoice's own totals (cgst ${sumCgst}/${invoice.cgst_paise}, sgst ${sumSgst}/${invoice.sgst_paise}, igst ${sumIgst}/${invoice.igst_paise}, cess ${sumCess}/${invoice.cess_paise})`);
  }
  const preRound = invoice.taxable_value_paise + invoice.cgst_paise + invoice.sgst_paise + invoice.igst_paise + invoice.cess_paise;
  if (preRound + invoice.round_off_paise !== invoice.grand_total_paise) {
    mark(inv, "9_tax_reconciliation", false, `${where}: taxable+tax+round_off=${preRound + invoice.round_off_paise} but grand_total_paise=${invoice.grand_total_paise}`);
  } else {
    mark(inv, "9_tax_reconciliation", true);
  }
}

type InvariantsRecord = Record<InvariantId, InvariantOutcome>;

function freshInvariants(): Record<InvariantId, InvariantOutcome> {
  const out = {} as Record<InvariantId, InvariantOutcome>;
  for (const id of ALL_INVARIANTS) out[id] = { checked: false, passed: true };
  return out;
}

function mark(inv: Record<InvariantId, InvariantOutcome>, id: InvariantId, passed: boolean, detail?: string) {
  const cur = inv[id];
  cur.checked = true;
  if (!passed) {
    cur.passed = false;
    cur.detail = cur.detail ? `${cur.detail}; ${detail ?? ""}` : detail;
  }
}

interface ExpectedItem {
  orderItemId: string;
  menuItemId: string;
  quantity: number;
  unitPricePaise: number;
  lineTotalPaise: number;
}

export interface ScenarioOptions {
  name: string;
  seed: number;
  /** Forces a crash+recover step into the sequence (named regressions use
   * this; randomized scenarios roll it probabilistically). */
  forceCrash?: boolean | "post-send" | "mid-draft";
  /** Caps how many cart-edit steps happen before confirm (keeps named
   * regressions short and deterministic). */
  maxCartSteps?: number;
}

export async function runScenario(
  bridge: HarnessBridge,
  opts: ScenarioOptions,
): Promise<ScenarioResult> {
  const rng = new Rng(opts.seed);
  const result: ScenarioResult = {
    name: opts.name,
    seed: opts.seed,
    actions: [],
    invariants: freshInvariants(),
    findings: [],
    latencySamples: [],
    crashed: false,
  };

  let seq = 0;
  const log = (entry: Omit<ActionLogEntry, "seq">) => {
    seq += 1;
    result.actions.push({ seq, ...entry });
  };

  const info = await bridge.newScenario();
  const pool = itemPool(info);
  const kds = new KdsDriver(info);

  result.findings.push(
    "COVERAGE GAP: cancel_kitchen_items_with_outbox exists in edge/database but has no Tauri command — " +
    "'#132-C' cancellation of items already sent to the kitchen is unreachable from the shipped surface. " +
    "Not faked and not added here per track rules; recorded as a finding only.",
  );
  result.findings.push(
    "COVERAGE GAP: split-bill invoicing is unreachable from the shipped surface. " +
    "holler_edge_database::Db::issue_split_invoices_with_outbox exists but " +
    "apps/pos/src-tauri/src/commands/billing.rs deliberately excludes it from M3's command surface " +
    "(docs/m3-planning.md) — issue_invoice always bills the whole order at split_count == 1. The " +
    "'a split bill's parts sum to the whole' money invariant this track was asked to add therefore " +
    "cannot be exercised end to end; not faked and not added here per track rules.",
  );
  result.findings.push(
    "COVERAGE GAP: a per-line discount is unreachable from the shipped surface. " +
    "apps/pos/src-tauri/src/commands/billing.rs's build_invoice_lines hard-codes " +
    "discount_per_unit_paise: 0 for every invoice line — no command surface lets a cashier apply one. " +
    "The 'discounts' money invariant this track was asked to add therefore cannot be exercised end to " +
    "end; not faked and not added here per track rules.",
  );

  let orderId: string | null = null;
  let phase: "none" | "draft" | "confirmed" | "sent" = "none";
  const expectedItems = new Map<string, ExpectedItem>();
  let expectedOutboxEvents = 0;
  const kotIds: string[] = [];
  const kotStatuses = new Map<string, string>();
  const orderHasNoStationItem = () =>
    [...expectedItems.values()].some((i) => i.menuItemId === info.items.no_station.id);
  const orderHasRoutableItem = () =>
    [...expectedItems.values()].some((i) => i.menuItemId !== info.items.no_station.id);

  interface OrderItemLike {
    line_total_paise: number;
    unit_price_paise: number;
    quantity: number;
    modifiers?: { price_delta_paise: number }[];
  }

  const checkMoney = (order: { subtotal_paise: number; total_paise: number; items: OrderItemLike[] } | undefined, where: string) => {
    if (!order) return;
    const sum = order.items.reduce((a, i) => a + i.line_total_paise, 0);
    if (sum !== order.subtotal_paise) {
      mark(result.invariants, "5_money", false, `${where}: subtotal_paise=${order.subtotal_paise} but sum(line_total_paise)=${sum}`);
    } else {
      mark(result.invariants, "5_money", true);
    }
    if (order.total_paise !== order.subtotal_paise) {
      // M1/M2 scope: no tax/discount computation yet — total must equal subtotal.
      mark(result.invariants, "5_money", false, `${where}: total_paise (${order.total_paise}) != subtotal_paise (${order.subtotal_paise})`);
    }
    // Per-line reconciliation: quantity AND every modifier price delta must
    // both be reflected in line_total_paise — the exact shape the money
    // invariant "passed 204/204 without ever exercising a modifier price
    // delta" gap (docs/m3-planning.md Track B) left unseen.
    for (const item of order.items) {
      if (!isIntegerPaise(item.line_total_paise) || !isIntegerPaise(item.unit_price_paise)) {
        mark(result.invariants, "5_money", false, `${where}: non-integer or negative paise on a line (line_total_paise=${item.line_total_paise}, unit_price_paise=${item.unit_price_paise})`);
        continue;
      }
      const modifierDeltaSum = (item.modifiers ?? []).reduce((a, m) => a + m.price_delta_paise, 0);
      const expectedLineTotal = (item.unit_price_paise + modifierDeltaSum) * item.quantity;
      if (expectedLineTotal !== item.line_total_paise) {
        mark(result.invariants, "5_money", false,
          `${where}: line_total_paise=${item.line_total_paise} but (unit_price_paise=${item.unit_price_paise} + Σmodifier_deltas=${modifierDeltaSum}) * quantity=${item.quantity} = ${expectedLineTotal}`);
      }
    }
  };

  // A modifier price delta only exists on the "single" fixture (Masala
  // Chai's "Sugar" group, devseed.rs) — picks zero or one modifier for that
  // item so the money invariant actually sees a delta land in a line total
  // (docs/m3-planning.md Track B: "the money invariant has passed 204/204
  // scenarios without ever exercising a modifier price delta").
  const modifiersFor = (item: ItemFixture): { modifier_id: string; group_name: string; option_name: string; price_delta_paise: number }[] => {
    if (item.key !== "single" || !rng.bool(0.6)) return [];
    const picked = rng.pick(info.items.single_station.modifiers);
    return [{
      modifier_id: picked.id,
      group_name: picked.group_name,
      option_name: picked.option_name,
      price_delta_paise: picked.price_delta_paise,
    }];
  };

  try {
    await kds.connect();

    // ---- 1. create draft (always the first action) ----
    const first = rng.pick(pool);
    const qty = 1 + rng.int(3);
    const wantTable = rng.bool(0.5);
    const orderType = rng.pick(["DINE_IN", "TAKEAWAY", "DELIVERY"] as const);
    const tableId = wantTable ? rng.pick(info.tables) : null;
    const firstModifiers = modifiersFor(first);
    const createResp = await bridge.request<{ ok: boolean; order?: any; error?: any }>({
      op: "create_draft",
      order_type: orderType,
      table_id: tableId,
      menu_item_id: first.id,
      variant_id: null,
      unit_price_paise: first.price,
      quantity: qty,
      notes: null,
      modifiers: firstModifiers,
    });
    log({ action: "create_draft", request: { item: first.key, qty, orderType, tableId }, ok: createResp.ok, error: createResp.error });
    if (!createResp.ok || !createResp.order) {
      mark(result.invariants, "1_state_machine", false, "create_draft (legal, first action) was rejected");
      result.fatalError = "create_draft failed; aborting scenario";
      return finish();
    }
    orderId = createResp.order.holler_order_id;
    phase = "draft";
    expectedOutboxEvents += 1;
    for (const it of createResp.order.items) {
      expectedItems.set(it.id, {
        orderItemId: it.id,
        menuItemId: it.menu_item_id,
        quantity: it.quantity,
        unitPricePaise: it.unit_price_paise,
        lineTotalPaise: it.line_total_paise,
      });
    }
    checkMoney(createResp.order, "create_draft");

    // ---- 2. cart-editing steps while DRAFT ----
    const cartSteps = opts.maxCartSteps ?? rng.int(5);
    for (let i = 0; i < cartSteps && phase === "draft"; i++) {
      const choice = rng.int(4);
      if (choice === 0) {
        // add item (same or different — covers "add same item repeatedly")
        const pick = rng.pick(pool);
        const q = 1 + rng.int(3);
        const resp = await bridge.request<{ ok: boolean; order?: any; error?: any }>({
          op: "add_item",
          order_id: orderId,
          menu_item_id: pick.id,
          variant_id: null,
          unit_price_paise: pick.price,
          quantity: q,
          notes: rng.bool(0.3) ? "extra spicy" : null,
          modifiers: modifiersFor(pick),
        });
        log({ action: "add_item", request: { item: pick.key, q }, ok: resp.ok, error: resp.error });
        if (!resp.ok) {
          mark(result.invariants, "1_state_machine", false, `add_item rejected while DRAFT: ${JSON.stringify(resp.error)}`);
        } else {
          expectedItems.clear();
          for (const it of resp.order.items) {
            expectedItems.set(it.id, {
              orderItemId: it.id, menuItemId: it.menu_item_id, quantity: it.quantity,
              unitPricePaise: it.unit_price_paise, lineTotalPaise: it.line_total_paise,
            });
          }
          checkMoney(resp.order, "add_item");
        }
      } else if (choice === 1 && expectedItems.size > 1) {
        // remove item — never remove the last line (not a supported/expected UI action to empty the cart here)
        const ids = [...expectedItems.keys()];
        const victim = rng.pick(ids);
        const resp = await bridge.request<{ ok: boolean; order?: any; error?: any }>({
          op: "remove_item", order_id: orderId, order_item_id: victim,
        });
        log({ action: "remove_item", request: { victim }, ok: resp.ok, error: resp.error });
        if (!resp.ok) {
          mark(result.invariants, "1_state_machine", false, `remove_item rejected while DRAFT: ${JSON.stringify(resp.error)}`);
        } else {
          expectedItems.clear();
          for (const it of resp.order.items) {
            expectedItems.set(it.id, {
              orderItemId: it.id, menuItemId: it.menu_item_id, quantity: it.quantity,
              unitPricePaise: it.unit_price_paise, lineTotalPaise: it.line_total_paise,
            });
          }
          checkMoney(resp.order, "remove_item");
        }
      } else if (choice === 2) {
        // change order type / table (T14 fix under test — must succeed)
        const newType = rng.pick(["DINE_IN", "TAKEAWAY", "DELIVERY"] as const);
        const newTable = newType === "DINE_IN" && rng.bool(0.6) ? rng.pick(info.tables) : null;
        const resp = await bridge.request<{ ok: boolean; order?: any; error?: any }>({
          op: "update_shape", order_id: orderId, order_type: newType, table_id: newTable,
        });
        log({ action: "update_shape", request: { newType, newTable }, ok: resp.ok, error: resp.error });
        if (!resp.ok) {
          mark(result.invariants, "1_state_machine", false, `update_shape rejected while DRAFT (shape-lock-after-first-tap regression): ${JSON.stringify(resp.error)}`);
        } else {
          if (resp.order.order_type !== newType || resp.order.table_id !== newTable) {
            mark(result.invariants, "1_state_machine", false, "update_shape accepted but did not persist the requested shape");
          }
        }
      } else {
        // no-op filler step (keeps the distribution from over-weighting shape
        // changes when the cart only has one line) — attempt an add instead.
        const pick = rng.pick(pool);
        const resp = await bridge.request<{ ok: boolean; order?: any; error?: any }>({
          op: "add_item", order_id: orderId, menu_item_id: pick.id, variant_id: null,
          unit_price_paise: pick.price, quantity: 1, notes: null,
        });
        log({ action: "add_item(filler)", request: { item: pick.key }, ok: resp.ok, error: resp.error });
        if (resp.ok) {
          expectedItems.clear();
          for (const it of resp.order.items) {
            expectedItems.set(it.id, {
              orderItemId: it.id, menuItemId: it.menu_item_id, quantity: it.quantity,
              unitPricePaise: it.unit_price_paise, lineTotalPaise: it.line_total_paise,
            });
          }
        }
      }
    }

    // ---- crash mid-DRAFT, before confirm (probabilistic / forced) ----
    if (opts.forceCrash === true || opts.forceCrash === "mid-draft" || (opts.forceCrash === undefined && rng.bool(0.08))) {
      await crashAndRecover("mid-draft");
    }

    // ---- 3. confirm ----
    const confirmResp = await bridge.request<{ ok: boolean; order?: any; error?: any }>({
      op: "confirm", order_id: orderId,
    });
    log({ action: "confirm", ok: confirmResp.ok, error: confirmResp.error });
    if (!confirmResp.ok) {
      mark(result.invariants, "1_state_machine", false, `confirm rejected on a DRAFT order: ${JSON.stringify(confirmResp.error)}`);
    } else {
      phase = "confirmed";
      expectedOutboxEvents += 1;
    }

    // ---- amendment probe: add-item after CONFIRMED (the #132-A path) ----
    // add_order_item_impl was widened past DRAFT-only during M3 Track B
    // (apps/pos/src-tauri/src/commands/orders.rs's own module doc: legal
    // through DRAFT/CONFIRMED/SENT_TO_KITCHEN/PREPARING, rejected with
    // ORDER_NOT_DRAFT only once the order is terminal). This scenario is
    // CONFIRMED, not terminal, so the correct outcome here is now success —
    // an EARLIER version of this probe asserted the opposite (a harness bug
    // that bit-rotted against the shipped surface, not a product defect;
    // T11b fixed it after the CI job went red on every single scenario).
    if (phase === "confirmed") {
      const pick = rng.pick(pool);
      const resp = await bridge.request<{ ok: boolean; order?: any; error?: any }>({
        op: "add_item", order_id: orderId, menu_item_id: pick.id, variant_id: null,
        unit_price_paise: pick.price, quantity: 1, notes: null,
        modifiers: modifiersFor(pick),
      });
      log({ action: "add_item_after_confirm(#132-A amendment)", ok: resp.ok, error: resp.error });
      if (!resp.ok) {
        mark(result.invariants, "1_state_machine", false, `#132-A amendment (add_item on a CONFIRMED, non-terminal order) was rejected: ${JSON.stringify(resp.error)}`);
      } else {
        expectedItems.clear();
        for (const it of resp.order.items) {
          expectedItems.set(it.id, {
            orderItemId: it.id, menuItemId: it.menu_item_id, quantity: it.quantity,
            unitPricePaise: it.unit_price_paise, lineTotalPaise: it.line_total_paise,
          });
        }
        checkMoney(resp.order, "add_item_after_confirm");
      }
    }

    // ---- 4. send to kitchen ----
    if (phase === "confirmed") {
      const mixedNoStation = orderHasNoStationItem() && orderHasRoutableItem();
      const onlyNoStation = orderHasNoStationItem() && !orderHasRoutableItem();

      const sendResp = await bridge.request<{ ok: boolean; kots?: any[]; error?: any }>({
        op: "send_to_kitchen", order_id: orderId,
      });
      log({ action: "send_to_kitchen", ok: sendResp.ok, error: sendResp.error });

      if (onlyNoStation) {
        if (sendResp.ok || sendResp.error?.code !== "NOTHING_TO_SEND_TO_KITCHEN") {
          mark(result.invariants, "4_no_station_explicit", false, `all-items-unrouted send did not produce the expected explicit NOTHING_TO_SEND_TO_KITCHEN outcome (got ${JSON.stringify(sendResp)})`);
        } else {
          mark(result.invariants, "4_no_station_explicit", true);
        }
      } else if (mixedNoStation) {
        // M3 Track A fix (docs/backlog-m2.md / docs/m3-planning.md §2 Track
        // A): a mixed order — some routable items, one or more with no
        // station route — is now rejected outright rather than sent with
        // the unrouted line silently dropped. Db::send_order_to_kitchen_
        // with_outbox_inner writes zero `kot` rows for EITHER line and
        // returns DbError::UnroutedKitchenItems, surfaced to this bridge as
        // `{ error: { code: "UNROUTED_KITCHEN_ITEMS", message: "<N> item(s)
        // ... not sent: <names>" } }`. Nothing was sent to any kitchen, so
        // `phase` stays "confirmed" — no KOTs, no KDS fidelity checks, no
        // send-again probe for this order.
        if (sendResp.ok || sendResp.error?.code !== "UNROUTED_KITCHEN_ITEMS") {
          mark(result.invariants, "4_no_station_explicit", false,
            `mixed routable + no-station order did not produce the expected explicit ` +
            `UNROUTED_KITCHEN_ITEMS rejection (got ${JSON.stringify(sendResp)})`);
        } else if (typeof sendResp.error?.message !== "string" || sendResp.error.message.length === 0) {
          mark(result.invariants, "4_no_station_explicit", false,
            "UNROUTED_KITCHEN_ITEMS rejection carried no cashier-facing message naming the unrouted item(s)");
        } else {
          mark(result.invariants, "4_no_station_explicit", true);
        }
      } else if (sendResp.ok) {
        phase = "sent";
        const kots = sendResp.kots ?? [];
        expectedOutboxEvents += kots.length;
        const routedOrderItemIds = new Set<string>();
        for (const k of kots) {
          kotIds.push(k.id);
          kotStatuses.set(k.id, k.status);
          for (const it of k.items) routedOrderItemIds.add(it.order_item_id);
        }

        // KOT conservation (invariant 2): every routable item id appears on
        // exactly one KOT PER STATION it is routed to, never twice on the
        // same station, never on a station it is not routed to. A
        // multi-station item legitimately appears on more than one KOT (one
        // per station) BY DESIGN (send_order_to_kitchen_with_outbox_inner
        // groups by station and pushes a ticket line into every station
        // group an item routes to) — "exactly one KOT exactly once" is
        // therefore checked per (order_item_id, station), not per
        // order_item_id alone.
        const stationCountByMenuItem = new Map(pool.map((p) => [p.id, p.stationCount]));
        const perItemStations = new Map<string, Set<string>>();
        for (const k of kots) {
          for (const it of k.items) {
            if (!perItemStations.has(it.order_item_id)) perItemStations.set(it.order_item_id, new Set());
            const stations = perItemStations.get(it.order_item_id) as Set<string>;
            if (stations.has(k.station)) {
              mark(result.invariants, "2_kot_conservation", false, `order item ${it.order_item_id} appears twice on station ${k.station}`);
            }
            stations.add(k.station);
          }
        }
        let conservationOk = true;
        for (const item of expectedItems.values()) {
          if (item.menuItemId === info.items.no_station.id) continue;
          const expectedStationCount = stationCountByMenuItem.get(item.menuItemId) ?? 1;
          const actualStationCount = perItemStations.get(item.orderItemId)?.size ?? 0;
          if (actualStationCount !== expectedStationCount) {
            conservationOk = false;
            mark(result.invariants, "2_kot_conservation", false,
              `order item ${item.orderItemId} is on ${actualStationCount} station's KOT(s), expected ${expectedStationCount}`);
          }
        }
        for (const [id] of perItemStations) {
          if (!expectedItems.has(id)) {
            conservationOk = false;
            mark(result.invariants, "2_kot_conservation", false, `KOT references unknown order item ${id}`);
          }
        }
        if (conservationOk) mark(result.invariants, "2_kot_conservation", true);

        // KDS fidelity (invariant 3): every kot must reach the connected
        // KDS client within 2s.
        for (const k of kots) {
          try {
            const ms = await kds.waitForKot(k.id, 2_000);
            result.latencySamples.push({ invariant: "3_kds_fidelity", ms });
            mark(result.invariants, "3_kds_fidelity", true);
          } catch {
            mark(result.invariants, "3_kds_fidelity", false, `KOT ${k.id} did not reach the subscribed KDS client within 2s`);
          }
        }
      } else {
        mark(result.invariants, "1_state_machine", false, `send_to_kitchen rejected unexpectedly: ${JSON.stringify(sendResp.error)}`);
      }
    }

    // ---- 5. send again (idempotency / double-send-already-sent) ----
    if (phase === "sent") {
      const before = await bridge.request<{ ok: boolean; kots: any[] }>({ op: "list_kots", order_id: orderId });
      const secondSend = await bridge.request<{ ok: boolean; kots?: any[]; error?: any }>({ op: "send_to_kitchen", order_id: orderId });
      log({ action: "send_to_kitchen(again)", ok: secondSend.ok, error: secondSend.error });
      const after = await bridge.request<{ ok: boolean; kots: any[] }>({ op: "list_kots", order_id: orderId });
      if (before.ok && after.ok && before.kots.length !== after.kots.length) {
        mark(result.invariants, "2_kot_conservation", false, `re-send changed KOT count (${before.kots.length} -> ${after.kots.length}); re-send must never duplicate`);
      } else if (secondSend.ok && (secondSend.kots?.length ?? 0) > 0) {
        mark(result.invariants, "2_kot_conservation", false, "re-send produced new KOTs for already-ticketed items");
      } else if (!secondSend.ok && secondSend.error?.code !== "NOTHING_TO_SEND_TO_KITCHEN") {
        mark(result.invariants, "1_state_machine", false, `re-send rejected with unexpected code ${secondSend.error?.code}`);
      }
    }

    // ---- 6. KDS-side actions ----
    if (phase === "sent" && kotIds.length > 0) {
      for (const kotId of kotIds) {
        // out-of-order probe from the ticket's true starting state (NEW):
        // must be caught before any of the walk below moves it elsewhere —
        // testing the same skip later, once the ticket has already left
        // NEW, only re-proves a DIFFERENT (already-legal-from-there)
        // transition is illegal, not this one.
        if (rng.bool(0.4)) {
          const resp = await bridge.request<{ ok: boolean; error?: any }>({
            op: "transition_kot", order_id: orderId, kot_id: kotId, status: "SERVED",
          });
          log({ action: "illegal_transition_probe(NEW->SERVED)", request: { kotId }, ok: resp.ok, error: resp.error });
          if (resp.ok) {
            mark(result.invariants, "1_state_machine", false, `illegal transition NEW -> SERVED was accepted for KOT ${kotId}`);
          }
        }

        const walk = rng.int(4); // how far to progress this ticket
        const path = ["ACKNOWLEDGED", "PREPARING", "READY", "SERVED"].slice(0, walk + 1);
        for (const status of path) {
          const viaKds = rng.bool(0.5);
          if (viaKds) {
            kds.requestStatusChange(kotId, status as any);
            try {
              const ms = await kds.waitForStatusOnBridge(bridge, orderId as string, kotId, status);
              result.latencySamples.push({ invariant: "8_status_echo", ms });
              expectedOutboxEvents += 1; // KotStatusChanged, edge-authoritative regardless of who drove it
            } catch {
              mark(result.invariants, "8_status_echo", false, `KOT ${kotId} -> ${status} (KDS-driven) not reflected POS-side within 2s`);
            }
          } else {
            const resp = await bridge.request<{ ok: boolean; kots?: any[]; error?: any }>({
              op: "transition_kot", order_id: orderId, kot_id: kotId, status,
            });
            log({ action: "transition_kot(pos)", request: { kotId, status }, ok: resp.ok, error: resp.error });
            if (!resp.ok) {
              mark(result.invariants, "1_state_machine", false, `legal POS transition to ${status} rejected: ${JSON.stringify(resp.error)}`);
            } else {
              expectedOutboxEvents += 1;
              try {
                const ms = status === "SERVED"
                  ? await kds.waitForRemoved(kotId, 2_000)
                  : await kds.waitForStatus(kotId, status, 2_000);
                result.latencySamples.push({ invariant: "3_kds_fidelity", ms });
              } catch {
                mark(result.invariants, "3_kds_fidelity", false, `KOT ${kotId} -> ${status} (POS-driven) not echoed to KDS within 2s`);
              }
            }
          }
          kotStatuses.set(kotId, status);
        }

        // out-of-order / illegal transition attempt, occasionally
        if (rng.bool(0.25)) {
          const illegal = "SERVED"; // NEW/ACKNOWLEDGED -> SERVED directly skips required states unless already there
          const current = kotStatuses.get(kotId);
          if (current !== "SERVED" && current !== "CANCELLED" && current !== "READY") {
            const resp = await bridge.request<{ ok: boolean; error?: any }>({
              op: "transition_kot", order_id: orderId, kot_id: kotId, status: illegal,
            });
            log({ action: "illegal_transition_probe", request: { kotId, from: current, to: illegal }, ok: resp.ok, error: resp.error });
            if (resp.ok) {
              mark(result.invariants, "1_state_machine", false, `illegal transition ${current} -> ${illegal} was accepted`);
            }
          }
        }
      }

      // ack an unknown/stale KOT id over the LAN
      if (rng.bool(0.3)) {
        const bogus = "01900000-0000-7000-8000-00000000ffff";
        kds.requestStatusChange(bogus, "ACKNOWLEDGED" as any);
        // No confirming frame can ever arrive for an id the edge does not
        // have — the correct, non-silent outcome is the pending marker
        // timing out (mirrors tests/integration/kds-lan test 4).
        try {
          // transitionTimeoutMs is 2s and the controller only re-checks
          // pending transitions on a 1s tick, so the marker can legitimately
          // land up to ~1s after the 2s mark — allow real margin above that
          // before treating a still-pending marker as a silent failure.
          await KdsDriver.waitForPendingTimeout(kds, bogus, 4_000);
        } catch {
          mark(result.invariants, "1_state_machine", false, "ack on an unknown KOT id neither confirmed nor timed out (silent)");
        }
      }

      // disconnect / reconnect mid-sequence
      if (rng.bool(0.3)) {
        kds.disconnect();
        await new Promise((r) => setTimeout(r, 50));
        await kds.reconnect();
        // After reconnect, every still-active KOT must reappear via the
        // fresh snapshot.
        for (const [kotId, status] of kotStatuses) {
          if (status === "SERVED" || status === "CANCELLED") continue;
          try {
            await kds.waitForKot(kotId, 2_000);
          } catch {
            mark(result.invariants, "3_kds_fidelity", false, `KOT ${kotId} missing from snapshot after KDS reconnect`);
          }
        }
      }
    }

    // ---- crash after send (probabilistic / forced) ----
    if (phase === "sent" && (opts.forceCrash === "post-send" || (opts.forceCrash === undefined && rng.bool(0.08)))) {
      await crashAndRecover("post-send");
    }

    // ---- 7. billing: issue a GST invoice, then a split tender (T11b) ----
    // Reachable regardless of kitchen phase — issue_invoice_impl only
    // requires the order to carry at least one line, per
    // apps/pos/src-tauri/src/commands/billing.rs; billing is not gated on
    // send-to-kitchen. Runs whenever the order left DRAFT with at least one
    // line, so this exercises tax/payment invariants on every scenario that
    // reaches CONFIRMED, not only the ones that also reach the kitchen.
    if (phase !== "draft" && phase !== "none" && orderId !== null && expectedItems.size > 0) {
      await billOrder(orderId);
    } else {
      result.findings.push(
        "COVERAGE GAP: this scenario never reached CONFIRMED with at least one line, so the billing " +
        "surface (issue_invoice/record_payment) was not exercised this run — not a defect, a run-to-run " +
        "coverage note.",
      );
    }

    await finalChecks();
    return finish();
  } catch (e) {
    result.fatalError = e instanceof Error ? `${e.message}\n${e.stack}` : String(e);
    return finish();
  } finally {
    kds.disconnect();
    try {
      await bridge.closeScenario();
    } catch {
      // best-effort cleanup
    }
  }

  /** Issues a GST invoice for `order_id`, checks invariant 9 against it, then
   * records a payment sequence checked against invariant 10: most runs take
   * a genuine two-tender split (§35 shape: CASH + UPI covering the exact
   * grand total, mirroring apps/pos/src-tauri/tests/billing_flow.rs's own
   * proven split-tender case) so "settled payments never exceed the invoice
   * total" is actually exercised against >1 payment row, not just one. A
   * minority of runs additionally record a reversal (non-positive
   * amount_paise, `reverses_payment_id` set) to exercise the reversal half
   * of invariant 10. */
  async function billOrder(orderIdForInvoice: string): Promise<void> {
    const invoiceResp = await bridge.request<{ ok: boolean; invoice?: InvoiceLike & { grand_total_paise: number }; error?: any }>({
      op: "issue_invoice",
      order_id: orderIdForInvoice,
      created_by_user_id: info.cashier_user_id,
    });
    log({ action: "issue_invoice", ok: invoiceResp.ok, error: invoiceResp.error });
    if (!invoiceResp.ok || !invoiceResp.invoice) {
      mark(result.invariants, "9_tax_reconciliation", false, `issue_invoice rejected on a CONFIRMED-or-later order with lines: ${JSON.stringify(invoiceResp.error)}`);
      return;
    }
    const invoice = invoiceResp.invoice;
    checkTaxReconciliation(result.invariants, invoice, "issue_invoice");

    const total = invoice.grand_total_paise;
    const payments: { amount_paise: number; reverses?: boolean }[] = [];
    if (rng.bool(0.7) && total > 1) {
      // Split tender across two methods, summing exactly to the total —
      // largest_remainder rounding means an arbitrary split could be off by
      // a paise, so this picks a cash share that leaves an exact remainder.
      const cashShare = 1 + rng.int(Math.max(1, total - 1));
      payments.push({ amount_paise: cashShare });
      payments.push({ amount_paise: total - cashShare });
    } else {
      payments.push({ amount_paise: total });
    }

    let settled = 0;
    let lastPaymentId: string | null = null;
    let lastPaymentAmount = 0;
    for (const [idx, p] of payments.entries()) {
      const method = idx === 0 ? "CASH" : "UPI";
      const resp = await bridge.request<{ ok: boolean; payment?: { id: string; amount_paise: number }; error?: any }>({
        op: "record_payment",
        order_id: orderIdForInvoice,
        method,
        amount_paise: p.amount_paise,
        tendered_paise: method === "CASH" ? p.amount_paise : null,
        change_paise: method === "CASH" ? 0 : null,
        reference: null,
        cash_shift_id: null,
        reverses_payment_id: null,
        invoice_id: invoice.id,
        created_by_user_id: info.cashier_user_id,
      });
      log({ action: "record_payment", request: { method, amount_paise: p.amount_paise }, ok: resp.ok, error: resp.error });
      if (!resp.ok || !resp.payment) {
        mark(result.invariants, "10_payment_settlement", false, `record_payment rejected for a legal tender: ${JSON.stringify(resp.error)}`);
        continue;
      }
      if (!isIntegerPaise(resp.payment.amount_paise)) {
        mark(result.invariants, "10_payment_settlement", false, `record_payment returned a non-integer or negative forward amount_paise: ${resp.payment.amount_paise}`);
      }
      settled += resp.payment.amount_paise;
      lastPaymentId = resp.payment.id;
      lastPaymentAmount = resp.payment.amount_paise;
    }

    // A minority of runs also record a reversal — append-only correction
    // (T7c), never a mutation of the forward row: amount_paise must be
    // non-positive and reverses_payment_id must name the row it reverses.
    // Capped at the reversed payment's OWN amount, never the order's total
    // settled sum — a real per-payment guard (record_payment_impl's
    // REVERSAL_EXCEEDS_REMAINING) rejects a reversal larger than the
    // specific payment row it names, found by this harness's first run.
    if (lastPaymentId && lastPaymentAmount > 0 && rng.bool(0.25)) {
      const reversalAmount = -(1 + rng.int(lastPaymentAmount));
      const resp = await bridge.request<{ ok: boolean; payment?: { id: string; amount_paise: number; reverses_payment_id: string | null }; error?: any }>({
        op: "record_payment",
        order_id: orderIdForInvoice,
        method: "CASH",
        amount_paise: reversalAmount,
        tendered_paise: null,
        change_paise: null,
        reference: "e2e-harness reversal",
        cash_shift_id: null,
        reverses_payment_id: lastPaymentId,
        created_by_user_id: info.cashier_user_id,
      });
      log({ action: "record_payment(reversal)", request: { amount_paise: reversalAmount }, ok: resp.ok, error: resp.error });
      if (!resp.ok || !resp.payment) {
        mark(result.invariants, "10_payment_settlement", false, `a non-positive reversal against a real payment was rejected: ${JSON.stringify(resp.error)}`);
      } else {
        if (!isIntegerPaise(resp.payment.amount_paise, true) || resp.payment.amount_paise > 0) {
          mark(result.invariants, "10_payment_settlement", false, `reversal amount_paise=${resp.payment.amount_paise} must be a non-positive integer`);
        }
        if (resp.payment.reverses_payment_id !== lastPaymentId) {
          mark(result.invariants, "10_payment_settlement", false, "reversal payment did not carry the reverses_payment_id it was submitted with");
        }
        settled += resp.payment.amount_paise;
      }
    }

    // The core settlement invariant: forward tenders plus any (non-positive)
    // reversals must never exceed the invoice total.
    if (settled > total) {
      mark(result.invariants, "10_payment_settlement", false, `settled payments (${settled} paise) exceed the invoice grand_total_paise (${total})`);
    } else {
      mark(result.invariants, "10_payment_settlement", true);
    }

    const listResp = await bridge.request<{ ok: boolean; payments?: { amount_paise: number; reverses_payment_id: string | null }[] }>({
      op: "list_payments_for_order",
      order_id: orderIdForInvoice,
    });
    if (listResp.ok && listResp.payments) {
      const persistedSum = listResp.payments.reduce((a, p) => a + p.amount_paise, 0);
      if (persistedSum !== settled) {
        mark(result.invariants, "10_payment_settlement", false, `list_payments_for_order sum (${persistedSum}) disagrees with the amounts this scenario recorded (${settled})`);
      }
      for (const p of listResp.payments) {
        const isReversal = p.reverses_payment_id !== null;
        if (!isIntegerPaise(p.amount_paise, isReversal)) {
          mark(result.invariants, "10_payment_settlement", false, `persisted payment amount_paise=${p.amount_paise} (reversal=${isReversal}) is not a valid paise value`);
        }
        if (isReversal && p.amount_paise > 0) {
          mark(result.invariants, "10_payment_settlement", false, `persisted reversal has a positive amount_paise=${p.amount_paise}`);
        }
      }
      if (persistedSum > total) {
        mark(result.invariants, "10_payment_settlement", false, `persisted payment rows sum to ${persistedSum} paise, exceeding invoice total ${total}`);
      }
    }
  }

  async function crashAndRecover(where: string) {
    const pre = await bridge.request<{ orders: any[]; kots: any[]; outbox_unpublished: any[] }>({ op: "introspect" });
    kds.disconnect(); // the whole server process is about to die with it
    let resumed: ScenarioInfo | null = null;
    let error: string | undefined;
    try {
      resumed = await bridge.crashAndResume(info.scenario_dir);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
    log({ action: `crash_and_recover(${where})`, ok: resumed !== null, error: resumed ? undefined : { code: "RECOVER_FAILED", message: error ?? "" } });
    result.crashed = true;
    if (!resumed) {
      mark(result.invariants, "6_durability", false, `recovery failed at ${where}: ${error}`);
      return;
    }
    // The resumed process bound a fresh ephemeral port — every subsequent
    // bridge/KDS call in this scenario must use it. Mutated in place (not
    // reassigned) so `kds`'s already-captured reference sees the update.
    Object.assign(info, resumed);
    await kds.connect();

    const post = await bridge.request<{ orders: any[]; kots: any[]; outbox_unpublished: any[] }>({ op: "introspect" });
    const preKey = normalizeIntrospect(pre);
    const postKey = normalizeIntrospect(post);
    if (preKey !== postKey) {
      mark(result.invariants, "6_durability", false, `committed state differs after crash+recover at ${where}`);
    } else {
      mark(result.invariants, "6_durability", true);
    }
  }

  function normalizeIntrospect(dump: { orders: any[]; kots: any[] }): string {
    // Excludes outbox (attempt_count/timestamps are allowed to differ across
    // a crash boundary in principle) — durability here is judged against the
    // orders/items/kots the cashier and kitchen actually see.
    const orders = [...dump.orders].sort((a, b) => a.id.localeCompare(b.id));
    const kots = [...dump.kots].sort((a, b) => a.id.localeCompare(b.id));
    return JSON.stringify({ orders, kots });
  }

  async function finalChecks(): Promise<void> {
    if (orderId === null) return;
    const dump = await bridge.request<{ orders: any[]; kots: any[]; outbox_unpublished: any[] }>({ op: "introspect" });

    // Invariant 1: every persisted status value is a member of the legal
    // state set (never a value the state machine does not know about).
    const legalOrderStatuses = new Set(["DRAFT", "CONFIRMED", "SENT_TO_KITCHEN", "PREPARING", "READY", "SERVED", "BILLED", "PAID", "CLOSED", "CANCELLED"]);
    const legalKotStatuses = new Set(["NEW", "ACKNOWLEDGED", "PREPARING", "READY", "SERVED", "CANCELLED"]);
    for (const o of dump.orders) {
      if (!legalOrderStatuses.has(o.status)) mark(result.invariants, "1_state_machine", false, `order ${o.id} has unknown status ${o.status}`);
    }
    for (const k of dump.kots) {
      if (!legalKotStatuses.has(k.status)) mark(result.invariants, "1_state_machine", false, `kot ${k.id} has unknown status ${k.status}`);
    }
    // Every scenario exercises at least create_draft + confirm, both legal
    // transitions this function's own checks above (and the many inline
    // `mark(..., false, ...)` calls throughout this scenario) would have
    // already flagged if violated — record the positive result explicitly
    // so "checked" reflects that every scenario really did exercise this
    // invariant, not just the ones that happened to break it.
    mark(result.invariants, "1_state_machine", true);
    if (result.latencySamples.some((s) => s.invariant === "8_status_echo")) {
      mark(result.invariants, "8_status_echo", true);
    }

    // Invariant 5 (money), against the durable row this scenario's order
    // actually finished at.
    const finalOrder = dump.orders.find((o) => o.id === orderId);
    if (finalOrder) {
      const sum = finalOrder.items.reduce((a: number, i: any) => a + i.line_total_paise, 0);
      if (sum !== finalOrder.subtotal_paise) {
        mark(result.invariants, "5_money", false, `final DB row: subtotal_paise=${finalOrder.subtotal_paise} but sum(items)=${sum}`);
      } else {
        mark(result.invariants, "5_money", true);
      }
    }

    // Invariant 7 (outbox): no duplicate ids, nothing marked published (no
    // publisher ever runs in this harness, so "published_at IS NULL" has
    // nothing to violate), and at least the minimum set of events this
    // scenario is known to have caused directly is present. Not an exact
    // count: `transition_kot_status_with_outbox` also emits an internal
    // `OrderReady` outbox row once every KOT on an order independently
    // reaches READY-or-beyond, which this model does not re-derive — an
    // under-count here would still be a real finding (a state change with
    // no outbox row at all), so the >= check still has teeth.
    const thisOrderOutbox = dump.outbox_unpublished.filter((e) => e.aggregate_id === orderId || dump.kots.some((k) => k.order_id === orderId && k.id === e.aggregate_id));
    const ids = new Set<string>();
    let duplicate = false;
    for (const e of dump.outbox_unpublished) {
      if (ids.has(e.id)) duplicate = true;
      ids.add(e.id);
      if (e.published_at !== null && e.published_at !== undefined) {
        mark(result.invariants, "7_outbox", false, `outbox row ${e.id} has published_at set even though no publisher ran in this harness`);
      }
    }
    if (duplicate) mark(result.invariants, "7_outbox", false, "duplicate outbox row id");
    if (thisOrderOutbox.length < expectedOutboxEvents) {
      mark(result.invariants, "7_outbox", false, `expected at least ${expectedOutboxEvents} outbox rows for this order's lifecycle, found only ${thisOrderOutbox.length}`);
    } else if (!duplicate) {
      mark(result.invariants, "7_outbox", true);
    }
  }

  function finish(): ScenarioResult {
    return result;
  }
}
