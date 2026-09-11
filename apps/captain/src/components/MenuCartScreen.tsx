import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { fetchMenu, type CaptainMenuItem } from "../lib/api";
import { addToCart, cartTotalPaise, defaultVariant, type CartLine } from "../lib/cart";
import { formatPaise } from "../lib/money";

interface Props {
  token: string;
  tableName: string;
  cart: CartLine[];
  onCartChange: (lines: CartLine[]) => void;
  onSend: () => void;
  sending: boolean;
  sendError: string | null;
}

/**
 * Menu + cart. One fetch (docs/captain-api.md) — categories, items, variants
 * and modifiers all arrive together, so a phone on a restaurant hotspot never
 * makes four round trips for one screen. The running cart and its total stay
 * visible without scrolling: the cart bar is pinned outside the scrolling list.
 */
export function MenuCartScreen({
  token,
  tableName,
  cart,
  onCartChange,
  onSend,
  sending,
  sendError,
}: Props) {
  const query = useQuery({ queryKey: ["captain", "menu"], queryFn: () => fetchMenu(token) });
  const [activeCategory, setActiveCategory] = useState<string | null>(null);

  const categories = query.data?.categories ?? [];
  const items = query.data?.items ?? [];

  const sortedCategories = useMemo(
    () => [...categories].sort((a, b) => a.sort_order - b.sort_order),
    [categories],
  );
  const currentCategoryId = activeCategory ?? sortedCategories[0]?.id ?? null;
  const visibleItems = items.filter((i) => i.category_id === currentCategoryId);

  const total = cartTotalPaise(cart);
  const count = cart.reduce((n, l) => n + l.quantity, 0);

  function handleAdd(item: CaptainMenuItem) {
    if (!item.is_available) return; // a snoozed item must not be orderable
    const variant = defaultVariant(item);
    if (variant === null) {
      window.alert(
        `"${item.name}" has no variant configured. This is a seed defect — it cannot be ordered until it is fixed.`,
      );
      return;
    }
    // Only free modifiers are selectable in this reduced scope
    // (docs/captain-api.md), and this reduced flow offers no modifier picker
    // at all — tap-to-add sends the line with none selected.
    onCartChange(addToCart(cart, item, variant, []));
  }

  if (query.isLoading) return <p className="screen">Loading menu…</p>;
  if (query.isError) {
    return <p className="screen error">Could not load the menu: {String(query.error)}</p>;
  }

  return (
    <div className="screen" style={{ padding: 0 }}>
      <div style={{ padding: 12, display: "flex", flexDirection: "column", gap: 12, flex: 1, minHeight: 0 }}>
        <h2 style={{ margin: 0 }}>{tableName}</h2>
        <div className="category-tabs">
          {sortedCategories.map((c) => (
            <button
              key={c.id}
              type="button"
              className={c.id === currentCategoryId ? "active" : ""}
              onClick={() => setActiveCategory(c.id)}
            >
              {c.name}
            </button>
          ))}
        </div>
        <div className="menu-list">
          {visibleItems.map((item) => (
            <button
              key={item.id}
              type="button"
              className="menu-row"
              disabled={!item.is_available}
              onClick={() => handleAdd(item)}
            >
              <span>
                <span className="name">{item.name}</span>
                {!item.is_available && <div className="unavailable-tag">Not available</div>}
              </span>
              <span className="price">{formatPaise(item.base_price_paise)}</span>
            </button>
          ))}
        </div>
      </div>
      {sendError !== null && <p className="error" style={{ padding: "0 12px" }}>{sendError}</p>}
      <div className="cart-bar">
        <span className="count">{count} item{count === 1 ? "" : "s"}</span>
        <span className="total">{formatPaise(total)}</span>
        <button
          type="button"
          className="big-button"
          style={{ width: "auto", padding: "14px 20px" }}
          disabled={cart.length === 0 || sending}
          onClick={onSend}
        >
          {sending ? "Sending…" : "Send"}
        </button>
      </div>
    </div>
  );
}
