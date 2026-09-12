import type { CaptainMenuItem, CaptainModifier, CaptainVariant, NewOrderItem } from "./api";

/**
 * The running cart, kept in this module rather than in a component so it can
 * be tested and reasoned about without a DOM. One line per distinct
 * (item, variant, modifier set) — tapping the same combination again
 * increments its quantity rather than adding a duplicate line, since this
 * scope has no quantity-edit UI and repeated taps are the only way a waiter
 * has to reach "two of these".
 */

export interface CartLine {
  key: string; // menuItemId + variantId + sorted modifier ids, joined
  menuItemId: string;
  itemName: string;
  variantId: string;
  variantName: string;
  unitPricePaise: number; // base_price_paise + variant.price_delta_paise, snapshot at add time
  modifiers: NewOrderItem["modifiers"];
  quantity: number;
  notes: string | null;
}

export function defaultVariant(item: CaptainMenuItem): CaptainVariant | null {
  if (item.variants.length === 0) return null;
  return item.variants.find((v) => v.is_default) ?? item.variants[0];
}

export function freeModifiers(item: CaptainMenuItem): CaptainModifier[] {
  return item.modifiers.filter((m) => m.price_delta_paise === 0);
}

/**
 * A group of an item's FREE modifier options, keyed by group_name. Only free
 * options are selectable in this reduced scope (docs/captain-api.md) — a
 * group whose options are all priced simply does not appear here, which is
 * how "no free options in a group" reduces to a one-tap add with no dialog.
 *
 * `min_selection`/`max_selection` are carried per option row in the API
 * response rather than once per group; every option in one group is assumed
 * to agree, so the first row's values are taken as the group's.
 */
export interface FreeModifierGroup {
  groupName: string;
  minSelection: number;
  maxSelection: number;
  options: CaptainModifier[];
}

export function freeModifierGroups(item: CaptainMenuItem): FreeModifierGroup[] {
  const groups: FreeModifierGroup[] = [];
  const byName = new Map<string, FreeModifierGroup>();
  for (const m of freeModifiers(item)) {
    let group = byName.get(m.group_name);
    if (group === undefined) {
      group = { groupName: m.group_name, minSelection: m.min_selection, maxSelection: m.max_selection, options: [] };
      byName.set(m.group_name, group);
      groups.push(group);
    }
    group.options.push(m);
  }
  return groups;
}

/** A group the waiter must resolve before the line can be added. */
export function groupRequiresSelection(group: FreeModifierGroup): boolean {
  return group.minSelection >= 1;
}

/** All required groups have at least one selected option. */
export function modifierSelectionSatisfied(
  groups: FreeModifierGroup[],
  selected: Map<string, CaptainModifier[]>,
): boolean {
  return groups.every((g) => {
    if (!groupRequiresSelection(g)) return true;
    return (selected.get(g.groupName) ?? []).length > 0;
  });
}

function lineKey(menuItemId: string, variantId: string, modifierIds: string[]): string {
  return [menuItemId, variantId, ...[...modifierIds].sort()].join("|");
}

export function addToCart(
  lines: CartLine[],
  item: CaptainMenuItem,
  variant: CaptainVariant,
  selectedModifiers: CaptainModifier[],
): CartLine[] {
  const key = lineKey(
    item.id,
    variant.id,
    selectedModifiers.map((m) => m.id),
  );
  const existing = lines.find((l) => l.key === key);
  if (existing !== undefined) {
    return lines.map((l) => (l.key === key ? { ...l, quantity: l.quantity + 1 } : l));
  }
  const newLine: CartLine = {
    key,
    menuItemId: item.id,
    itemName: item.name,
    variantId: variant.id,
    variantName: variant.name,
    unitPricePaise: item.base_price_paise + variant.price_delta_paise,
    modifiers: selectedModifiers.map((m) => ({
      modifier_id: m.id,
      group_name: m.group_name,
      option_name: m.option_name,
      price_delta_paise: m.price_delta_paise,
    })),
    quantity: 1,
    notes: null,
  };
  return [...lines, newLine];
}

export function cartTotalPaise(lines: CartLine[]): number {
  return lines.reduce((sum, l) => sum + l.unitPricePaise * l.quantity, 0);
}

export function cartToOrderItems(lines: CartLine[]): NewOrderItem[] {
  return lines.map((l) => ({
    menu_item_id: l.menuItemId,
    variant_id: l.variantId,
    quantity: l.quantity,
    unit_price_paise: l.unitPricePaise,
    notes: l.notes,
    modifiers: l.modifiers,
  }));
}

/**
 * Set a line's quantity, removing the line at zero.
 *
 * ONE FUNCTION, NOT remove-then-add. Quantity is a single edit to a single
 * line — the same rule contracts/src/types/order.ts states for
 * SET_ORDER_ITEM_QUANTITY, and for the same reason: two operations with a gap
 * between them is a state nobody meant to be in, here a half-second where the
 * waiter's cart is missing the line he is editing.
 *
 * Identified by `key`, never by index: the list re-sorts on nothing today, but
 * an index is a position, and a position is only accidentally an identity.
 */
export function setLineQuantity(lines: CartLine[], key: string, quantity: number): CartLine[] {
  if (quantity <= 0) return lines.filter((l) => l.key !== key);
  return lines.map((l) => (l.key === key ? { ...l, quantity } : l));
}

/** Total units across every line — what "3 items" on the cart bar counts. */
export function cartUnitCount(lines: CartLine[]): number {
  return lines.reduce((sum, l) => sum + l.quantity, 0);
}
