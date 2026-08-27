import { useNavigate } from "@tanstack/react-router";
import { useCurrentStockQuery } from "../lib/queries";
import { formatMicroQuantity, isLowStock, isNegativeStock } from "../domain/inventory";

// The bounded current-stock read (`list_current_stock`) — what
// `LowStockBanner` summarises. Deliberately NOT gated behind
// `inventory.manage`: this screen IS the low-stock detail a cashier is sent
// to from the banner (task requirement: a read a cashier needs during
// service must not be gated behind a permission they lack). Every row a
// banner tap could land on must be visible to whoever tapped it.
export function CurrentStockScreen() {
  const navigate = useNavigate();
  const stockQuery = useCurrentStockQuery();
  const lines = stockQuery.data ?? [];

  return (
    <main className="current-stock-screen">
      <header>
        <h1>Current Stock</h1>
        <nav className="inventory-nav">
          <button type="button" onClick={() => void navigate({ to: "/inventory/wastage" })}>
            Record Wastage
          </button>
          <button type="button" onClick={() => void navigate({ to: "/inventory/counts" })}>
            Stock Counts
          </button>
          <button type="button" onClick={() => void navigate({ to: "/inventory/gaps" })}>
            Items Sold With No Recipe
          </button>
        </nav>
        <button type="button" onClick={() => void navigate({ to: "/" })}>
          Back to POS
        </button>
      </header>
      {stockQuery.isLoading && <p>Loading stock…</p>}
      {stockQuery.isError && <p role="alert">Could not load current stock.</p>}
      <table>
        <thead>
          <tr>
            <th>Item</th>
            <th>Current Quantity</th>
            <th>Reorder Level</th>
          </tr>
        </thead>
        <tbody>
          {lines.map((line) => {
            const negative = isNegativeStock(line);
            // A negative line is not ALSO tagged LOW: below zero is the
            // stronger statement and two tags on one row teach a cashier to
            // skim. Negative is reported with no reorder level configured,
            // which is the whole point (domain/inventory.ts).
            const low = !negative && isLowStock(line);
            return (
              // Rows are visually marked, not merely present — the second
              // half of acceptance criterion 4 alongside `LowStockBanner`.
              <tr
                key={line.inventory_item_id}
                className={
                  negative
                    ? "current-stock-row-negative"
                    : low
                      ? "current-stock-row-low"
                      : undefined
                }
              >
                <td>{line.inventory_item_name}</td>
                <td className={negative ? "current-stock-quantity-negative" : undefined}>
                  {formatMicroQuantity(line.current_quantity_micro, line.dimension)}
                  {negative && (
                    <span className="current-stock-negative-tag" role="alert">
                      BELOW ZERO
                    </span>
                  )}
                </td>
                <td>
                  {line.reorder_level_micro === null
                    ? "—"
                    : formatMicroQuantity(line.reorder_level_micro, line.dimension)}
                  {low && (
                    <span className="current-stock-low-tag" role="alert">
                      LOW
                    </span>
                  )}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </main>
  );
}
