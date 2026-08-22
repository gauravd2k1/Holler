import { useNavigate } from "@tanstack/react-router";
import { useCurrentStockQuery } from "../lib/queries";
import { lowStockLines } from "../domain/inventory";

// M4 acceptance criterion 4, verbatim: "An ingredient crossing its reorder
// level is VISIBLE TO A HUMAN ON THE POS, not merely present in a table."
// Follows the `PrintFailureBanner` precedent — a fixed, impossible-to-miss
// banner on every authenticated screen — rather than a badge a cashier must
// navigate to find. Deliberately NOT gated behind `inventory.manage`/
// `inventory.count`: any authenticated principal may see that stock is low,
// the same way any cashier already sees a print failure with no permission
// check (`domain/inventory.ts` module comment).
export function LowStockBanner() {
  const navigate = useNavigate();
  const stockQuery = useCurrentStockQuery();

  const low = lowStockLines(stockQuery.data ?? []);
  if (low.length === 0) return null;

  return (
    <button
      type="button"
      className="low-stock-banner"
      role="alert"
      onClick={() => void navigate({ to: "/inventory/stock" })}
    >
      <span className="low-stock-summary">
        {low.length} item{low.length === 1 ? "" : "s"} low on stock
      </span>
      <span className="low-stock-names">
        {low
          .slice(0, 4)
          .map((l) => l.inventory_item_name)
          .join(", ")}
        {low.length > 4 ? `, +${low.length - 4} more` : ""}
      </span>
      <span className="low-stock-action">View stock</span>
    </button>
  );
}
