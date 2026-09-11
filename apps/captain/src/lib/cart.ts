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
