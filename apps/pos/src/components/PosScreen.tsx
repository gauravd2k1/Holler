import { useEffect, useMemo, useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { useQueryClient } from "@tanstack/react-query";
import type { MenuItem, MenuItemVariant } from "@holler/contracts";
import {
  useMenuCategoriesQuery,
  useMenuItemsQuery,
  useMenuItemVariantsQuery,
  useTablesQuery,
  queryKeys,
} from "../lib/queries";
import { groupItemsByCategory, resolveVariantForTap, variantPricePaise } from "../domain/menu";
import { SUPPORTED_ORDER_TYPES, cartSubtotalPaise, canSendOrder, requiresTable, lineTotal } from "../domain/cart";
import { formatPaiseAsRupees, parseRupeesToPaise } from "../domain/money";
import { hasPermission } from "../domain/permissions";
import { useAuthStore } from "../store/auth";
import { useCartStore } from "../store/cart";
import { PrintFailureBanner } from "./PrintFailureBanner";
import { SyncBlockedBanner } from "./SyncBlockedBanner";
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
  const menuItemVariantsQuery = useMenuItemVariantsQuery();
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

  const menuItemVariants = menuItemVariantsQuery.data ?? [];
  // Which item is showing its variant picker, and what is selected in it.
  // `null` = no picker open. Opening one preselects `is_default` if exactly
  // one row claims it -- a preselection the cashier can change, never a
  // resolution taken on their behalf (domain/menu.ts explains why).
  const [variantPickerItemId, setVariantPickerItemId] = useState<string | null>(null);
  const [chosenVariantId, setChosenVariantId] = useState<string | null>(null);

  function addResolvedLine(item: MenuItem, variantId: string | null, pricePaise: number) {
    void addItem(
      {
        menuItemId: item.id,
        variantId,
        unitPricePaise: pricePaise,
        quantity: 1,
        notes: null,
      },
      menuItems,
    );
  }

  /**
   * A plain tap on the grid. Resolves without asking ONLY when the item has
   * zero or one variant; anything else opens the picker. Before 2026-08-27
   * this passed `variantId: null` unconditionally, so no sale the POS ever
   * took deducted any stock (docs/RESUME.md §2a).
   */
  function handleTapItem(item: MenuItem) {
    const resolution = resolveVariantForTap(item, menuItemVariants);
    if (resolution.kind === "RESOLVED") {
      addResolvedLine(item, resolution.variantId, resolution.pricePaise);
      return;
    }
    setVariantPickerItemId(item.id);
    setChosenVariantId(resolution.preselectedId);
  }

  function resolveVariantForTapOptions(item: MenuItem): MenuItemVariant[] {
    const r = resolveVariantForTap(item, menuItemVariants);
    return r.kind === "MUST_CHOOSE" ? r.options : [];
  }

  function confirmVariantChoice(item: MenuItem) {
    const chosen = menuItemVariants.find((v) => v.id === chosenVariantId);
    // Defensive: the Add button is disabled until something is chosen, so
    // this cannot normally fire. Adding at the base price would be a silent
    // wrong bill, which is the failure this whole path exists to prevent.
    if (!chosen) return;
    addResolvedLine(item, chosen.id, variantPricePaise(item, chosen));
    setVariantPickerItemId(null);
    setChosenVariantId(null);
  }

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
    // A modifier line resolves its variant by the same rule as a plain tap.
    // A multi-variant item must be chosen through the picker first, so the
    // modifier form refuses rather than silently billing the base price.
    const resolution = resolveVariantForTap(item, menuItemVariants);
    if (resolution.kind !== "RESOLVED") {
      setModifierFormError("Choose a size for this item first, then add the modifier.");
      return;
    }
    void addItem(
      {
        menuItemId: item.id,
        variantId: resolution.variantId,
        unitPricePaise: resolution.pricePaise,
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
      <SyncBlockedBanner />
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
                // control on a cart line"). Multi-variant items open the
                // picker instead of resolving -- see handleTapItem.
                handleTapItem(item)
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
            {variantPickerItemId === item.id && (
              <div className="pos-variant-picker" role="group" aria-label={`Choose a size for ${item.name}`}>
                <p className="pos-variant-picker-prompt">Choose a size</p>
                {resolveVariantForTapOptions(item).map((v) => (
                  <label key={v.id} className="pos-variant-option">
                    <input
                      type="radio"
                      name={`variant-${item.id}`}
                      value={v.id}
                      checked={chosenVariantId === v.id}
                      onChange={() => setChosenVariantId(v.id)}
                    />
                    <span className="name">{v.name}</span>
                    <span className="price">{formatPaiseAsRupees(variantPricePaise(item, v))}</span>
                  </label>
                ))}
                <div className="pos-variant-picker-actions">
                  <button
                    type="button"
                    // Disabled until a size is chosen. There is deliberately
                    // no "just add it" escape: an unchosen size is an unpriced
                    // line, and billing one at the base price is a wrong bill.
                    disabled={chosenVariantId === null || cartPending}
                    onClick={() => confirmVariantChoice(item)}
                  >
                    Add
                  </button>
                  <button
                    type="button"
                    onClick={() => {
                      setVariantPickerItemId(null);
                      setChosenVariantId(null);
                    }}
                  >
                    Cancel
                  </button>
                </div>
              </div>
            )}
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
