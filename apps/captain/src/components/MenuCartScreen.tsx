import { useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { fetchMenu, type CaptainMenuItem, type CaptainModifier } from "../lib/api";
import { addToCart, cartTotalPaise, defaultVariant, freeModifierGroups, type CartLine } from "../lib/cart";
import { formatPaise } from "../lib/money";
import { ModifierSheet } from "./ModifierSheet";
import { Empty, Waiting, errorText } from "./Waiting";

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
  // The item currently awaiting a free-modifier choice, plus the resolved
  // variant it will be added with once the sheet confirms.
  const [pending, setPending] = useState<{ item: CaptainMenuItem; variantId: string } | null>(
    null,
  );

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
    // A group with no free option, or an item with no groups at all, adds in
    // one tap — no dialog in front of the common case. A group with at least
    // one free option (required or not) is offered, because an optional free
    // choice is exactly what a waiter needs to reach for (docs/captain-api.md).
    if (freeModifierGroups(item).length === 0) {
      onCartChange(addToCart(cart, item, variant, []));
      return;
    }
    setPending({ item, variantId: variant.id });
  }

  function handleConfirmModifiers(selected: CaptainModifier[]) {
    if (pending === null) return;
    const variant = pending.item.variants.find((v) => v.id === pending.variantId);
    if (variant === undefined) {
      setPending(null);
      return;
    }
    onCartChange(addToCart(cart, pending.item, variant, selected));
    setPending(null);
  }

  if (query.isLoading) return <Waiting label="Loading menu…" />;
  if (query.isError) {
    return <p className="screen error">Could not load the menu. {errorText(query.error)}</p>;
  }

  return (
    <div className="screen screen--flush">
      <div className="menu-cart-body">
        <h2 className="menu-cart-title">{tableName}</h2>
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
          {visibleItems.length === 0 && (
            <Empty
              title="Nothing in this category."
              detail="Pick another category above, or ask the till whether these items are switched off."
            />
          )}
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
      {sendError !== null && <p className="error menu-cart-send-error">{sendError}</p>}
      <div className="cart-bar">
        <span className="count">{count} item{count === 1 ? "" : "s"}</span>
        <span className="total">{formatPaise(total)}</span>
        <button
          type="button"
          className="btn btn--primary btn--lg"
          disabled={cart.length === 0 || sending}
          onClick={onSend}
        >
          {sending ? (
            <>
              <span className="spinner spinner--on-dark" aria-hidden="true" />
              Sending…
            </>
          ) : (
            "Send"
          )}
        </button>
      </div>
      {pending !== null && (
        <ModifierSheet
          item={pending.item}
          onCancel={() => setPending(null)}
          onConfirm={handleConfirmModifiers}
        />
      )}
    </div>
  );
}
