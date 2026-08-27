import type { MenuItem, MenuItemVariant } from "@holler/contracts";
import type { MenuCategory } from "../lib/tauri";

// `list_menu_categories` (apps/pos/src-tauri/src/commands/menu.rs) closed the
// M1 backlog item that used to leave this LEFT panel labelling groups by raw
// `category_id` UUID; groups are now named from the real `menu_category` rows,
// sorted the same way the edge stores them (sort_order, then name).
export interface MenuCategoryGroup {
  categoryId: string;
  categoryName: string;
  sortOrder: number;
  items: MenuItem[];
}

export function groupItemsByCategory(
  items: readonly MenuItem[],
  categories: readonly MenuCategory[],
): MenuCategoryGroup[] {
  const nameById = new Map(categories.map((c) => [c.id, c] as const));
  const byCategory = new Map<string, MenuItem[]>();
  for (const item of items) {
    const group = byCategory.get(item.category_id);
    if (group) {
      group.push(item);
    } else {
      byCategory.set(item.category_id, [item]);
    }
  }
  return Array.from(byCategory.entries())
    .map(([categoryId, categoryItems]) => {
      const category = nameById.get(categoryId);
      return {
        categoryId,
        // A category id with no matching row (e.g. sync has not delivered
        // it yet) falls back to the raw id rather than hiding the items.
        categoryName: category?.name ?? categoryId,
        sortOrder: category?.sort_order ?? Number.MAX_SAFE_INTEGER,
        items: categoryItems,
      };
    })
    .sort((a, b) => a.sortOrder - b.sortOrder || a.categoryName.localeCompare(b.categoryName));
}


// ------------------------------------------------------------- variants --
// How a tap on the menu grid becomes a cart line.
//
// A recipe binds to `menu_item_variant_id` (NOT NULL, sqlite 0015), so a line
// carrying a null variant deducts NOTHING: resolution returns
// `GapReason::NoVariant` and the sale completes with no ledger rows. Before
// 2026-08-27 `variantId` was hardcoded null at every call site, so that was
// every sale the POS had ever taken.
//
// THE FIX HAS A TRAP. The obvious repair — fall back to the variant marked
// `is_default` — turns a stock defect into a REVENUE defect. Half at 18000
// paise and Full at 32000 are a price decision; resolving silently to the
// default sells Full whenever nobody chose, and prints a wrong bill. A wrong
// bill is worse than a missing deduction.
//
// So the rule is by CARDINALITY, not by default:
//   0 variants -> resolve with null. The item genuinely has none; deduction
//                 records NO_VARIANT, which is the honest answer (T0b seeds 11
//                 such items on purpose).
//   1 variant  -> resolve silently. There is nothing to choose between, so a
//                 tap cannot be ambiguous. This is what T0b's six "Regular"
//                 variants are for.
//   2+         -> PICKER MANDATORY. Never resolvable without a human choice.
//
// `is_default` PRESELECTS inside that picker and never RESOLVES. A default is
// only safe where there is nothing to choose between.

export type VariantResolution =
  | { kind: "RESOLVED"; variantId: string | null; pricePaise: number }
  | { kind: "MUST_CHOOSE"; options: MenuItemVariant[]; preselectedId: string | null };

export function variantsForItem(
  item: MenuItem,
  variants: readonly MenuItemVariant[],
): MenuItemVariant[] {
  return variants
    .filter((v) => v.menu_item_id === item.id)
    .slice()
    .sort((a, b) => a.price_delta_paise - b.price_delta_paise || a.name.localeCompare(b.name));
}

export function variantPricePaise(item: MenuItem, variant: MenuItemVariant | null): number {
  return item.base_price_paise + (variant?.price_delta_paise ?? 0);
}

/**
 * Decide whether a tap on `item` can become a cart line without asking.
 *
 * Never returns RESOLVED for a multi-variant item, whatever `is_default` says.
 */
export function resolveVariantForTap(
  item: MenuItem,
  variants: readonly MenuItemVariant[],
): VariantResolution {
  const options = variantsForItem(item, variants);

  if (options.length === 0) {
    return { kind: "RESOLVED", variantId: null, pricePaise: variantPricePaise(item, null) };
  }
  if (options.length === 1) {
    const only = options[0]!;
    return { kind: "RESOLVED", variantId: only.id, pricePaise: variantPricePaise(item, only) };
  }
  // `is_default` reaches the picker as a PRESELECTION and goes no further. If
  // two rows claim the default (a config defect the contract does not forbid),
  // preselect neither rather than guess at a price.
  const defaults = options.filter((v) => v.is_default);
  return {
    kind: "MUST_CHOOSE",
    options,
    preselectedId: defaults.length === 1 ? defaults[0]!.id : null,
  };
}
