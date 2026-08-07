import type { MenuItem } from "@holler/contracts";

/**
 * CONTRACT/API GAP (reported, not worked around): `list_menu_items` is the
 * only menu-read Tauri command this app has (apps/pos/src-tauri/src/commands/menu.rs
 * doc comment: `edge/database`'s `repo` module exposes no
 * `list_menu_categories_for_outlet`, and category names are not embedded on
 * `MenuItem` — only `category_id`). There is therefore no category *name* to
 * show anywhere in this app; the LEFT panel groups items by the real
 * `category_id` values returned by the one command that exists and labels
 * each group with that id, rather than fabricating a name. This should be
 * revisited once a `list_menu_categories` command exists.
 */
export interface MenuCategoryGroup {
  categoryId: string;
  items: MenuItem[];
}

export function groupItemsByCategory(items: readonly MenuItem[]): MenuCategoryGroup[] {
  const byCategory = new Map<string, MenuItem[]>();
  for (const item of items) {
    const group = byCategory.get(item.category_id);
    if (group) {
      group.push(item);
    } else {
      byCategory.set(item.category_id, [item]);
    }
  }
  return Array.from(byCategory.entries()).map(([categoryId, categoryItems]) => ({
    categoryId,
    items: categoryItems,
  }));
}
