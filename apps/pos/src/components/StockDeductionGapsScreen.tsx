import { useNavigate } from "@tanstack/react-router";
import { useBlockedReplaysQuery, useStockDeductionGapsQuery } from "../lib/queries";
import { canManageInventory, formatGapQuantity } from "../domain/inventory";
import { useAuthStore } from "../store/auth";

// The "items sold with no recipe" report (M4 acceptance criterion 5). A
// back-office report, not something a cashier needs mid-service — gated
// behind `inventory.manage` (this task's judgement call: the low-stock
// signal and its detail screen are ungated because a cashier needs them
// during service; this report is not).
export function StockDeductionGapsScreen() {
  const navigate = useNavigate();
  const principal = useAuthStore((s) => s.principal);
  const canView = canManageInventory(principal);
  const gapsQuery = useStockDeductionGapsQuery();
  const blockedQuery = useBlockedReplaysQuery();
  const blocked = blockedQuery.data ?? [];

  return (
    <main className="stock-deduction-gaps-screen">
      <header>
        <h1>Items Sold With No Recipe</h1>
        <button type="button" onClick={() => void navigate({ to: "/inventory/stock" })}>
          Back to Stock
        </button>
      </header>

      {!canView && <p role="alert">You do not have permission to view this report.</p>}

      {/* Stock history this outlet has stopped trying to send (contracts
          0.5.8). The per-entry retry bound exists so one row the cloud will
          not accept cannot hold back every row behind it — but an outlet
          quietly not replaying part of its stock history is exactly the
          failure that must not surface months later in a variance report.
          Halting sync is survivable; halting it silently is not, which is
          why this is on a screen and not only in a table. */}
      {canView && blocked.length > 0 && (
        <section className="sync-replay-blocked" role="alert">
          <h2>
            {blocked.length} stock {blocked.length === 1 ? "record has" : "records have"} stopped
            syncing
          </h2>
          <p>
            These were rejected by the cloud repeatedly and are no longer being retried. The rest of
            this outlet&rsquo;s stock history is still syncing normally.
          </p>
          <table>
            <thead>
              <tr>
                <th>Stream</th>
                <th>Entry</th>
                <th>Record</th>
                <th>Attempts</th>
                <th>Last error</th>
                <th>Blocked since</th>
              </tr>
            </thead>
            <tbody>
              {blocked.map((b) => (
                <tr key={`${b.stream}-${b.entry_seq}`}>
                  <td>{b.stream}</td>
                  <td>{b.entry_seq}</td>
                  <td>{b.record_id}</td>
                  <td>{b.attempts}</td>
                  <td>{b.last_error}</td>
                  <td>{b.blocked_at ?? ""}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </section>
      )}

      {canView && (
        <>
          {gapsQuery.isLoading && <p>Loading…</p>}
          {gapsQuery.isError && <p role="alert">Could not load this report.</p>}
          <table>
            <thead>
              <tr>
                <th>Item</th>
                <th>Quantity</th>
                <th>Reason</th>
                <th>When</th>
              </tr>
            </thead>
            <tbody>
              {(gapsQuery.data ?? []).map((gap) => (
                <tr key={gap.id}>
                  <td>{gap.menu_item_name}</td>
                  {/* NOT a micro-quantity — `formatGapQuantity`, never
                      `formatMicroQuantity`, per the task's explicit
                      distinction. */}
                  <td>{formatGapQuantity(gap.quantity)}</td>
                  <td>{gap.reason}</td>
                  <td>{gap.occurred_at}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </>
      )}
    </main>
  );
}
