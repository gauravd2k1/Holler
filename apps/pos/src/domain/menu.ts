import type { MenuItem } from "@holler/contracts";
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
