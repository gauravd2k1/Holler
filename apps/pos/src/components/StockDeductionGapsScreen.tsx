import { useNavigate } from "@tanstack/react-router";
import { useStockDeductionGapsQuery } from "../lib/queries";
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

  return (
    <main className="stock-deduction-gaps-screen">
      <header>
        <h1>Items Sold With No Recipe</h1>
        <button type="button" onClick={() => void navigate({ to: "/inventory/stock" })}>
          Back to Stock
        </button>
      </header>

      {!canView && <p role="alert">You do not have permission to view this report.</p>}

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
