import { useMemo, useState } from "react";
import {
  useBlockedOutboxRowsQuery,
  useMenuItemsQuery,
  useOrdersQuery,
  usePersistentlyFailingOutboxRowsQuery,
} from "../lib/queries";
import type { SyncOutboxBlock } from "../lib/tauri";

// Human-facing label per aggregate type. Never the raw aggregate_type string
// on screen (an internal table name) and never the raw aggregate_id (a UUID)
// — same rule the print template carries for KOTs, applied here.
const AGGREGATE_LABELS: Record<string, string> = {
  order: "Order",
  kot: "Kitchen ticket",
  payment: "Payment",
  invoice: "Invoice",
  cash_shift: "Cash shift",
  stock_count: "Stock count",
};

function aggregateLabel(aggregateType: string): string {
  return (
    AGGREGATE_LABELS[aggregateType] ??
    aggregateType.charAt(0).toUpperCase() + aggregateType.slice(1).replace(/_/g, " ")
  );
}

/** Resolves one blocked/failing outbox row to what a human reads: an order's
 * display number (never its UUID) and a one-line item summary, when the row
 * is an order. Every other aggregate type has no human-facing number today,
 * so it shows a plain label with no id fragment at all — a truncated UUID is
 * still a UUID. */
function useRowDescription(row: SyncOutboxBlock): string {
  const ordersQuery = useOrdersQuery();
  const menuItemsQuery = useMenuItemsQuery();

  return useMemo(() => {
    const label = aggregateLabel(row.aggregate_type);
    if (row.aggregate_type !== "order") {
      return label;
    }
    const order = ordersQuery.data?.find((o) => o.holler_order_id === row.aggregate_id);
    if (!order) {
      return `${label} (details unavailable)`;
    }
    const orderNumber = order.display_number ?? "unnumbered";
    const menuItemName = (menuItemId: string): string =>
      menuItemsQuery.data?.find((m) => m.id === menuItemId)?.name ?? "item";
    let itemSummary = "no items";
    if (order.items.length === 1) {
      itemSummary = menuItemName(order.items[0].menu_item_id);
    } else if (order.items.length > 1) {
      const first = menuItemName(order.items[0].menu_item_id);
      itemSummary = `${first} +${order.items.length - 1} more`;
    }
    return `${label} #${orderNumber} — ${itemSummary}`;
  }, [row.aggregate_type, row.aggregate_id, ordersQuery.data, menuItemsQuery.data]);
}

// M6 A3, and the half of M6 C7 that no wire format can satisfy: the criterion
// asks for a reason the edge RECORDS, which is only true if a person can read
// it back. Follows the `PrintFailureBanner` precedent — a fixed banner on the
// screens a cashier already looks at, not a badge in a corner or a report
// nobody opens.
//
// TWO CONDITIONS, DELIBERATELY DIFFERENT WORDS, because they need different
// actions:
//
//   - GIVEN UP ON (`blocked_at` set). The retry budget is spent and the till
//     will not try again. This part of the trading day will never reach the
//     cloud without someone intervening.
//   - STILL TRYING (`blocked_at` null, attempts high). A transient failure
//     never spends the budget — abandoning good rows because the cloud was
//     down for a day is data loss dressed as resilience — so these rows are
//     retried indefinitely and would otherwise be invisible. A cloud that has
//     been refusing everything since Tuesday must not look like a quiet
//     evening.
//
// Halting sync is survivable. Halting it silently is not, and M5 ended with
// 120 rows pending on a till that reported itself healthy.
//
// COLLAPSIBLE, AND THE SUMMARY NEVER COLLAPSES. Pinned at top:0 with a height
// that grows per row, this banner covered the entire till header at seven rows
// -- the search box, the order-type buttons, the table picker and every
// navigation button -- which is the third instance of a fixed overlay eating
// another region (`.print-failure-banner` and `.stock-banners` had already been
// fixed once for colliding with each other). Observed blocking the M6 C1 run on
// 2026-09-10.
//
// The row list folds away; the count and the "these need someone" line do not,
// because the whole reason this banner exists is that a till must not look
// healthy while records are stranded. Collapsing hides the detail, never the
// fact. The state is per-session and defaults to EXPANDED: a cashier who has
// never seen it should see everything.
function OutboxRowDetail({ row }: { row: SyncOutboxBlock }) {
  const description = useRowDescription(row);
  return (
    <li>
      <span className="sync-blocked-aggregate">{description}</span>
      {" · "}
      {row.attempts} attempt{row.attempts === 1 ? "" : "s"} ·{" "}
      {/* The machine-readable code first: it is stable, and it is
          what the cloud actually said. The prose is a fallback for
          a failure that never reached the wire. */}
      {row.last_code ?? row.last_error}
      {row.last_status !== null && ` (HTTP ${row.last_status})`}
    </li>
  );
}

export function SyncBlockedBanner() {
  const [expanded, setExpanded] = useState(true);
  const [failingExpanded, setFailingExpanded] = useState(true);
  const blockedQuery = useBlockedOutboxRowsQuery();
  const failingQuery = usePersistentlyFailingOutboxRowsQuery();

  const blocked = blockedQuery.data ?? [];
  const failing = failingQuery.data ?? [];
  if (blocked.length === 0 && failing.length === 0) return null;

  return (
    <div className="sync-blocked-banner" role="alert">
      {blocked.length > 0 && (
        <div className="sync-blocked-group">
          <span className="sync-blocked-summary">
            {blocked.length} record{blocked.length === 1 ? "" : "s"} will not reach the cloud
          </span>
          <button
            type="button"
            className="sync-blocked-toggle"
            aria-expanded={expanded}
            onClick={() => setExpanded((v) => !v)}
          >
            {expanded ? "Hide details" : "Show details"}
          </button>
          <ul className="sync-blocked-list" hidden={!expanded}>
            {blocked.map((row) => (
              <OutboxRowDetail key={row.outbox_id} row={row} />
            ))}
          </ul>
          <span className="sync-blocked-action">
            Nothing is lost locally — these need someone to look at them.
          </span>
        </div>
      )}
      {failing.length > 0 && (
        <div className="sync-failing-group">
          <span className="sync-failing-summary">
            {failing.length} record{failing.length === 1 ? "" : "s"} still retrying after repeated
            failures
          </span>
          <button
            type="button"
            className="sync-blocked-toggle"
            aria-expanded={failingExpanded}
            onClick={() => setFailingExpanded((v) => !v)}
          >
            {failingExpanded ? "Hide details" : "Show details"}
          </button>
          <ul className="sync-blocked-list" hidden={!failingExpanded}>
            {failing.map((row) => (
              <OutboxRowDetail key={row.outbox_id} row={row} />
            ))}
          </ul>
        </div>
      )}
    </div>
  );
}
