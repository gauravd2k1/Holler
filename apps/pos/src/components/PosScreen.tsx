import { useMemo, useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import type { MenuItem } from "@holler/contracts";
import { useMenuItemsQuery, useTablesQuery, queryKeys } from "../lib/queries";
import { groupItemsByCategory } from "../domain/menu";
import { SUPPORTED_ORDER_TYPES, cartSubtotalPaise, canSendOrder, requiresTable, lineTotal } from "../domain/cart";
import { formatPaiseAsRupees } from "../domain/money";
import { hasPermission } from "../domain/permissions";
import { useAuthStore } from "../store/auth";
import { useCartStore } from "../store/cart";
import { createOrder, isTauriCommandError } from "../lib/tauri";

// docs/spec/ordering.md §POS layout: TOP search + order-type + table,
// LEFT categories, CENTER menu grid, RIGHT cart, BOTTOM subtotal/send.
// Layout is fixed so a trained cashier can work from muscle memory — no
// zone moves depending on state.
export function PosScreen() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const principal = useAuthStore((s) => s.principal);
  const menuItemsQuery = useMenuItemsQuery();
  const tablesQuery = useTablesQuery();

  const orderType = useCartStore((s) => s.orderType);
  const setOrderType = useCartStore((s) => s.setOrderType);
  const tableId = useCartStore((s) => s.tableId);
  const setTableId = useCartStore((s) => s.setTableId);
  const lines = useCartStore((s) => s.lines);
  const addLine = useCartStore((s) => s.addLine);
  const setQuantity = useCartStore((s) => s.setQuantity);
  const clearCart = useCartStore((s) => s.clear);

  const [search, setSearch] = useState("");
  const [activeCategoryId, setActiveCategoryId] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const [sendError, setSendError] = useState<string | null>(null);

  const canCreateOrder = hasPermission(principal, "order.create");

  const menuItems = menuItemsQuery.data ?? [];
  const groups = useMemo(() => groupItemsByCategory(menuItems), [menuItems]);
  const activeGroup = groups.find((g) => g.categoryId === activeCategoryId) ?? groups[0] ?? null;

  const visibleItems: MenuItem[] = useMemo(() => {
    const source = activeGroup?.items ?? [];
    const term = search.trim().toLowerCase();
    if (!term) return source;
    return source.filter((item) => item.name.toLowerCase().includes(term));
  }, [activeGroup, search]);

  const subtotalPaise = cartSubtotalPaise(lines);
  const sendEnabled = canCreateOrder && canSendOrder(orderType, tableId, lines) && !sending;

  async function handleSend() {
    // Permission is enforced here, not just visually: an unauthorized
    // cashier cannot reach `createOrder` at all (task requirement #8).
    if (!canCreateOrder) return;
    if (!canSendOrder(orderType, tableId, lines)) return;
    setSending(true);
    setSendError(null);
    try {
      await createOrder(
        orderType,
        tableId,
        lines.map((l) => ({
          menu_item_id: l.menuItemId,
          variant_id: l.variantId,
          quantity: l.quantity,
          unit_price_paise: l.unitPricePaise,
          notes: l.notes,
        })),
      );
      clearCart();
      await queryClient.invalidateQueries({ queryKey: queryKeys.orders });
    } catch (err) {
      setSendError(isTauriCommandError(err) ? err.message : "Could not send order.");
    } finally {
      setSending(false);
    }
  }

  return (
    <main className="pos-screen">
      <header className="pos-top-bar">
        <input
          className="pos-search"
          placeholder="Search menu…"
          value={search}
          onChange={(e) => setSearch(e.target.value)}
        />
        <div className="pos-order-type">
          {SUPPORTED_ORDER_TYPES.map((type) => (
            <button
              key={type}
              type="button"
              className={type === orderType ? "active" : ""}
              onClick={() => setOrderType(type)}
            >
              {type}
            </button>
          ))}
        </div>
        {requiresTable(orderType) && (
          <select
            className="pos-table-select"
            value={tableId ?? ""}
            onChange={(e) => setTableId(e.target.value || null)}
          >
            <option value="">Select table…</option>
            {(tablesQuery.data ?? []).map((table) => (
              <option key={table.id} value={table.id}>
                {table.section} / {table.label}
              </option>
            ))}
          </select>
        )}
        <button type="button" onClick={() => void navigate({ to: "/orders" })}>
          Orders
        </button>
      </header>

      <nav className="pos-categories">
        {groups.map((group) => (
          <button
            key={group.categoryId}
            type="button"
            className={group.categoryId === activeGroup?.categoryId ? "active" : ""}
            onClick={() => setActiveCategoryId(group.categoryId)}
          >
            {group.categoryId}
          </button>
        ))}
      </nav>

      <section className="pos-menu-grid">
        {menuItemsQuery.isLoading && <p>Loading menu…</p>}
        {visibleItems.map((item) => (
          <button
            key={item.id}
            type="button"
            className="pos-menu-item"
            disabled={!item.is_available || !canCreateOrder}
            onClick={() =>
              addLine({
                menuItemId: item.id,
                menuItemName: item.name,
                variantId: null,
                unitPricePaise: item.base_price_paise,
                quantity: 1,
                notes: null,
              })
            }
          >
            <span className="name">{item.name}</span>
            <span className="price">{formatPaiseAsRupees(item.base_price_paise)}</span>
          </button>
        ))}
      </section>

      <aside className="pos-cart">
        {lines.map((line) => (
          <div className="pos-cart-line" key={line.lineId}>
            <span className="name">{line.menuItemName}</span>
            <div className="qty-controls">
              <button type="button" onClick={() => setQuantity(line.lineId, line.quantity - 1)}>
                −
              </button>
              <span>{line.quantity}</span>
              <button type="button" onClick={() => setQuantity(line.lineId, line.quantity + 1)}>
                +
              </button>
            </div>
            <span className="line-total">{formatPaiseAsRupees(lineTotal(line))}</span>
          </div>
        ))}
        {lines.length === 0 && <p className="pos-cart-empty">Cart is empty.</p>}
      </aside>

      <footer className="pos-bottom-bar">
        <span className="pos-subtotal">Subtotal: {formatPaiseAsRupees(subtotalPaise)}</span>
        {sendError && (
          <span className="pos-send-error" role="alert">
            {sendError}
          </span>
        )}
        <button type="button" className="pos-send" disabled={!sendEnabled} onClick={() => void handleSend()}>
          {sending ? "Sending…" : "Send"}
        </button>
      </footer>
    </main>
  );
}
