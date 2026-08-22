import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import type { MenuItem } from "@holler/contracts";
import { useMenuCategoriesQuery, useMenuItemsQuery, useTablesQuery, queryKeys } from "../lib/queries";
import { groupItemsByCategory } from "../domain/menu";
import { SUPPORTED_ORDER_TYPES, cartSubtotalPaise, canSendOrder, requiresTable, lineTotal } from "../domain/cart";
import { formatPaiseAsRupees, parseRupeesToPaise } from "../domain/money";
import { hasPermission } from "../domain/permissions";
import { useAuthStore } from "../store/auth";
import { useCartStore } from "../store/cart";
import { PrintFailureBanner } from "./PrintFailureBanner";
import { LowStockBanner } from "./LowStockBanner";

// docs/spec/ordering.md §POS layout: TOP search + order-type + table,
// LEFT categories, CENTER menu grid, RIGHT cart, BOTTOM subtotal/send.
// Layout is fixed so a trained cashier can work from muscle memory — no
// zone moves depending on state.
//
// docs/backlog-m2.md "POS cart persistence" (reopened 2026-08-10): every
// line in `lines` below already exists in SQLite by the time it renders —
// see `store/cart.ts`. There is no local-only cart state left to lose.
export function PosScreen() {
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const principal = useAuthStore((s) => s.principal);
  const menuItemsQuery = useMenuItemsQuery();
  const menuCategoriesQuery = useMenuCategoriesQuery();
  const tablesQuery = useTablesQuery();

  const orderId = useCartStore((s) => s.orderId);
  const orderStatus = useCartStore((s) => s.orderStatus);
  const orderType = useCartStore((s) => s.orderType);
  const setOrderType = useCartStore((s) => s.setOrderType);
  const tableId = useCartStore((s) => s.tableId);
  const setTableId = useCartStore((s) => s.setTableId);
  const lines = useCartStore((s) => s.lines);
  const hydrated = useCartStore((s) => s.hydrated);
  const cartPending = useCartStore((s) => s.pending);
  const cartError = useCartStore((s) => s.error);
  const hydrate = useCartStore((s) => s.hydrate);
  const addItem = useCartStore((s) => s.addItem);
  const setLineQuantity = useCartStore((s) => s.setLineQuantity);
  const removeItem = useCartStore((s) => s.removeItem);
  const clearAfterHandoff = useCartStore((s) => s.clearAfterHandoff);

  const [search, setSearch] = useState("");
  const [activeCategoryId, setActiveCategoryId] = useState<string | null>(null);
  // Which menu item's modifier form is currently open, if any — functional-
  // only per docs/m3-planning.md §3 (UI polish deferred): one plain text
  // field for the option name, one for the price delta in rupees, no
  // catalog picker (no Tauri command yet exposes `menu_item_modifier` reads
  // to this app — see this task's report). Attaching a modifier always adds
  // a brand-new line (modifiers cannot be appended to an existing line —
  // there is no command for that either), so this deliberately does not
  // reuse `addItem`'s quantity-merge path.
  const [modifierFormItemId, setModifierFormItemId] = useState<string | null>(null);
  const [modifierGroupName, setModifierGroupName] = useState("");
  const [modifierOptionName, setModifierOptionName] = useState("");
  const [modifierPriceRupees, setModifierPriceRupees] = useState("");
  const [modifierFormError, setModifierFormError] = useState<string | null>(null);

  const canCreateOrder = hasPermission(principal, "order.create");

  const menuItems = menuItemsQuery.data ?? [];
  const menuCategories = menuCategoriesQuery.data ?? [];

  // Recover this device's in-progress order exactly once, as soon as the
  // menu is available to resolve line item names against (task requirement
  // 2: "On startup, recover any DRAFT order for this outlet/device and
  // restore it as the active cart"). `hydrate` itself is idempotent, so a
  // re-render with a fresh `menuItems` array is harmless.
  useEffect(() => {
    if (menuItemsQuery.isSuccess) {
      void hydrate(menuItems);
    }
    // Intentionally depends on isSuccess only — see the comment above: this
    // must run exactly once when the menu becomes available, not on every
    // `menuItems` array identity change.
  }, [menuItemsQuery.isSuccess]);

  const groups = useMemo(
    () => groupItemsByCategory(menuItems, menuCategories),
    [menuItems, menuCategories],
  );
  const activeGroup = groups.find((g) => g.categoryId === activeCategoryId) ?? groups[0] ?? null;

  const visibleItems: MenuItem[] = useMemo(() => {
    const source = activeGroup?.items ?? [];
    const term = search.trim().toLowerCase();
    if (!term) return source;
    return source.filter((item) => item.name.toLowerCase().includes(term));
  }, [activeGroup, search]);

  const subtotalPaise = cartSubtotalPaise(lines);
  const sendEnabled = canCreateOrder && canSendOrder(orderType, tableId, lines) && !cartPending;
  // Order type/table stay editable for the order's entire DRAFT lifetime —
  // not merely before it existed (docs/retro.md P0 regression, task T14).
  // `orderId === null` means no order has been persisted yet (also
  // editable); once one exists, `orderStatus` is the actual gate, since a
  // DRAFT order created on the first tapped item must not lock the cashier
  // out of correcting its type/table before Send.
  const canEditOrderShape = orderId === null || orderStatus === "DRAFT";

  function openModifierForm(itemId: string) {
    setModifierFormItemId(itemId);
    setModifierGroupName("");
    setModifierOptionName("");
    setModifierPriceRupees("");
    setModifierFormError(null);
  }

  function closeModifierForm() {
    setModifierFormItemId(null);
    setModifierFormError(null);
  }

  function handleAddWithModifier(item: MenuItem) {
    if (!modifierOptionName.trim()) {
      setModifierFormError("Enter what the modifier is (e.g. \"Extra cheese\").");
      return;
    }
    const priceDeltaPaise = parseRupeesToPaise(modifierPriceRupees);
    if (priceDeltaPaise === null) {
      setModifierFormError("Price must be a number like 25 or 12.50.");
      return;
    }
    void addItem(
      {
        menuItemId: item.id,
        variantId: null,
        unitPricePaise: item.base_price_paise,
        quantity: 1,
        notes: null,
        modifiers: [
          {
            groupName: modifierGroupName.trim() || "Modifier",
            optionName: modifierOptionName.trim(),
            priceDeltaPaise,
          },
        ],
      },
      menuItems,
    );
    closeModifierForm();
  }

  function handleSend() {
    if (!canSendOrder(orderType, tableId, lines)) return;
    // The order is already fully durable (every line landed on add) —
    // nothing to write here. This only frees the cart for the next order
    // and refreshes the Orders list, where it is confirmed/sent to kitchen.
    clearAfterHandoff();
    void queryClient.invalidateQueries({ queryKey: queryKeys.orders });
  }

  return (
    <main className="pos-screen">
      <PrintFailureBanner />
      <LowStockBanner />
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
              disabled={!canEditOrderShape || cartPending}
              onClick={() => void setOrderType(type)}
            >
              {type}
            </button>
          ))}
        </div>
        {requiresTable(orderType) && (
          <select
            className="pos-table-select"
            value={tableId ?? ""}
            disabled={!canEditOrderShape || cartPending}
            onChange={(e) => void setTableId(e.target.value || null)}
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
        <button type="button" onClick={() => void navigate({ to: "/inventory/stock" })}>
          Stock
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
            {group.categoryName}
          </button>
        ))}
      </nav>

      <section className="pos-menu-grid">
        {(menuItemsQuery.isLoading || !hydrated) && <p>Loading menu…</p>}
        {visibleItems.map((item) => (
          <div key={item.id} className="pos-menu-item-cell">
            <button
              type="button"
              className="pos-menu-item"
              disabled={!item.is_available || !canCreateOrder || cartPending || !hydrated}
              onClick={() =>
                // A plain tap: no modifiers. If a plain line for this item
                // already exists, the store raises its quantity instead of
                // adding a second line (docs/backlog-m2.md "No quantity
                // control on a cart line").
                void addItem(
                  {
                    menuItemId: item.id,
                    variantId: null,
                    unitPricePaise: item.base_price_paise,
                    quantity: 1,
                    notes: null,
                  },
                  menuItems,
                )
              }
            >
              <span className="name">{item.name}</span>
              <span className="price">{formatPaiseAsRupees(item.base_price_paise)}</span>
            </button>
            <button
              type="button"
              className="pos-menu-item-modifier-toggle"
              disabled={!item.is_available || !canCreateOrder || cartPending || !hydrated}
              onClick={() =>
                modifierFormItemId === item.id ? closeModifierForm() : openModifierForm(item.id)
              }
            >
              + Modifier
            </button>
            {modifierFormItemId === item.id && (
              <div className="pos-modifier-form">
                <input
                  placeholder="Group (e.g. Spice level) — optional"
                  value={modifierGroupName}
                  onChange={(e) => setModifierGroupName(e.target.value)}
                />
                <input
                  placeholder="Modifier (e.g. Extra cheese)"
                  value={modifierOptionName}
                  onChange={(e) => setModifierOptionName(e.target.value)}
                />
                <input
                  placeholder="Price ₹ (e.g. 25 or -10)"
                  inputMode="decimal"
                  value={modifierPriceRupees}
                  onChange={(e) => setModifierPriceRupees(e.target.value)}
                />
                {modifierFormError && (
                  <p className="pos-modifier-form-error" role="alert">
                    {modifierFormError}
                  </p>
                )}
                <button type="button" onClick={() => handleAddWithModifier(item)}>
                  Add to cart
                </button>
                <button type="button" onClick={closeModifierForm}>
                  Cancel
                </button>
              </div>
            )}
          </div>
        ))}
      </section>

      <aside className="pos-cart">
        {lines.map((line) => (
          <div className="pos-cart-line" key={line.lineId}>
            <span className="name">{line.menuItemName}</span>
            {line.modifiers.length > 0 && (
              <ul className="pos-cart-line-modifiers">
                {line.modifiers.map((m, i) => (
                  <li key={`${line.lineId}-${m.modifierId}-${i}`}>
                    {m.optionName} ({formatPaiseAsRupees(m.priceDeltaPaise)})
                  </li>
                ))}
              </ul>
            )}
            <span className="qty-controls">
              <button
                type="button"
                aria-label={`Decrease quantity of ${line.menuItemName}`}
                disabled={!canCreateOrder || cartPending || line.quantity <= 1}
                onClick={() => void setLineQuantity(line.lineId, line.quantity - 1, menuItems)}
              >
                −
              </button>
              <span className="qty">{line.quantity}×</span>
              <button
                type="button"
                aria-label={`Increase quantity of ${line.menuItemName}`}
                disabled={!canCreateOrder || cartPending}
                onClick={() => void setLineQuantity(line.lineId, line.quantity + 1, menuItems)}
              >
                +
              </button>
            </span>
            <span className="line-total">{formatPaiseAsRupees(lineTotal(line))}</span>
            <button
              type="button"
              disabled={!canCreateOrder || cartPending}
              onClick={() => void removeItem(line.lineId, menuItems)}
            >
              Remove
            </button>
          </div>
        ))}
        {lines.length === 0 && <p className="pos-cart-empty">Cart is empty.</p>}
      </aside>

      <footer className="pos-bottom-bar">
        <span className="pos-subtotal">Subtotal: {formatPaiseAsRupees(subtotalPaise)}</span>
        {cartError && (
          <span className="pos-send-error" role="alert">
            {cartError}
          </span>
        )}
        <button type="button" className="pos-send" disabled={!sendEnabled} onClick={handleSend}>
          Send
        </button>
      </footer>
    </main>
  );
}
