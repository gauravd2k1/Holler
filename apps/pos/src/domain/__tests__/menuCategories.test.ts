import { describe, expect, it } from "vitest";
import type { MenuItem } from "@holler/contracts";

import type { MenuCategory } from "../../lib/tauri";
import { groupItemsByCategory } from "../menu";

// The rail is the first thing the client looks at, so what it must NOT show is
// as load-bearing as what it must. The seed's two internal categories carry
// only `is_available: false` items and were reaching the screen as tabs
// reading "Test fixtures (internal -- not sold)".

function category(id: string, name: string, sortOrder: number): MenuCategory {
  return {
    id,
    outlet_id: "0191a000-0000-7000-8000-00000000000a",
    name,
    sort_order: sortOrder,
    config_version: 1,
  };
}

function item(id: string, categoryId: string, name: string, isAvailable: boolean): MenuItem {
  return {
    id,
    outlet_id: "0191a000-0000-7000-8000-00000000000a",
    category_id: categoryId,
    name,
    base_price_paise: 18000,
    is_available: isAvailable,
    tax_profile_id: null,
    hsn_sac: "9963",
    config_version: 1,
    schema_version: 1,
  };
}

const SALAD = category("0191e850-0000-7000-8000-0000000000c1", "Salad", 2);
const SUSHI = category("0191e850-0000-7000-8000-0000000000c2", "Sushi Roll (4pc)", 5);
// The two real seed strings, verbatim, including the sort orders the seed
// gives them (`seed/demo-outlet.json`).
const FIXTURES = category(
  "0191e850-0000-7000-8000-0000000000c3",
  "Test fixtures (internal -- not sold)",
  98,
);
const PREP = category(
  "0191e850-0000-7000-8000-0000000000c4",
  "Kitchen Prep (internal -- not sold)",
  99,
);

describe("groupItemsByCategory", () => {
  it("keeps every category out of the rail whose items are all unavailable", () => {
    // Both internal categories hold exactly the seed's shape: two items, none
    // available. Neither may appear, and the food categories are untouched.
    const groups = groupItemsByCategory(
      [
        item("0191e850-0000-7000-8000-0000000000a1", SALAD.id, "Wakame", true),
        item("0191e850-0000-7000-8000-0000000000a2", SUSHI.id, "Spicy Tuna", true),
        item("0191e850-0000-7000-8000-0000000000a3", FIXTURES.id, "Masala Chai", false),
        item("0191e850-0000-7000-8000-0000000000a4", FIXTURES.id, "Veg Thali", false),
        item("0191e850-0000-7000-8000-0000000000a5", PREP.id, "Dashi Base", false),
        item("0191e850-0000-7000-8000-0000000000a6", PREP.id, "Ponzu", false),
      ],
      [SALAD, SUSHI, FIXTURES, PREP],
    );

    expect(groups.map((g) => g.categoryName)).toEqual(["Salad", "Sushi Roll (4pc)"]);
  });

  it("keeps an unavailable item visible inside a category that has others", () => {
    // Per-item behaviour is unchanged: staff must still see that an 86'd dish
    // exists and is off. `PosScreen` disables it; the rail keeps the tab.
    const groups = groupItemsByCategory(
      [
        item("0191e850-0000-7000-8000-0000000000a1", SALAD.id, "Wakame", true),
        item("0191e850-0000-7000-8000-0000000000a2", SALAD.id, "Sunomono", false),
      ],
      [SALAD],
    );

    expect(groups).toHaveLength(1);
    expect(groups[0]!.items.map((i) => i.name)).toEqual(["Wakame", "Sunomono"]);
  });

  it("drops a category by orderability and never by its name", () => {
    // The naming rule stated in `menu.ts`, pinned: a category whose name reads
    // internal but which HAS an orderable item stays, and one with an ordinary
    // name but nothing orderable goes. A name match would get both backwards.
    const groups = groupItemsByCategory(
      [
        item("0191e850-0000-7000-8000-0000000000a1", FIXTURES.id, "Masala Chai", true),
        item("0191e850-0000-7000-8000-0000000000a2", SALAD.id, "Wakame", false),
      ],
      [SALAD, FIXTURES],
    );

    expect(groups.map((g) => g.categoryName)).toEqual(["Test fixtures (internal -- not sold)"]);
  });

  it("still sorts the surviving categories by sort_order, then name", () => {
    const groups = groupItemsByCategory(
      [
        item("0191e850-0000-7000-8000-0000000000a1", SUSHI.id, "Spicy Tuna", true),
        item("0191e850-0000-7000-8000-0000000000a2", SALAD.id, "Wakame", true),
      ],
      [SALAD, SUSHI],
    );

    expect(groups.map((g) => g.sortOrder)).toEqual([2, 5]);
  });

  it("keeps an orderable item whose category row has not arrived yet", () => {
    // The existing fallback: an unknown `category_id` shows under the raw id
    // rather than hiding the items. Orderability, not category knowledge, is
    // what the new filter tests -- so this must survive it.
    const groups = groupItemsByCategory(
      [item("0191e850-0000-7000-8000-0000000000a1", "0191e850-0000-7000-8000-0000000000cf", "New", true)],
      [],
    );

    expect(groups).toHaveLength(1);
    expect(groups[0]!.categoryName).toBe("0191e850-0000-7000-8000-0000000000cf");
  });
});
