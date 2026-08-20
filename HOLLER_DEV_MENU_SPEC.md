# Holler — Dev Seed Menu Spec (realistic Indian restaurant)

Purpose: replace the 2-item placeholder seed with a realistic menu so demos look real
and the mixed-rate GST engine, per-item tax resolution, HSN/SAC snapshotting, and KOT
station routing are all actually exercised. This is DEV SEED DATA, not a product feature.

All prices are **menu (customer-facing) price in paise**, integer. `tax_profile` names below
map to the tax_profile aggregate; create these three profiles if not already seeded.

## Tax profiles to seed (GST 2.0, post-Sept-2025)

| profile key    | components            | rate      | pricing_mode | applies to                                  |
|----------------|-----------------------|-----------|--------------|---------------------------------------------|
| GST_FOOD_5     | CGST 2.5% + SGST 2.5% | 5%        | INCLUSIVE    | all prepared food & non-packaged drinks     |
| GST_PACKAGED_18| CGST 9% + SGST 9%     | 18%       | INCLUSIVE    | packaged/bottled non-aerated (water, juice) |
| GST_AERATED_40 | CGST 20% + SGST 20%   | 40%       | INCLUSIVE    | aerated/carbonated/sweetened soft drinks    |

Notes:
- Restaurant prepared food is SAC **9963** (996331 dine-in/takeaway/delivery), 5% no ITC.
- Menu prices are tax-INCLUSIVE (Indian restaurant convention) — the engine back-computes
  the tax component from the inclusive price. This exercises the inclusive-mode path.
- HSN codes below are realistic representative codes for seed purposes; a production
  outlet sets its own per its CA. Good enough to prove the field flows end-to-end.

## Stations (seed if missing)
TANDOOR, MAIN (main kitchen / curries), CHAT (cold/snacks), BAR (drinks/beverages), DESSERT

---

## Category: Starters & Chaat  → station CHAT (fried items → MAIN if fryer there)

| item                     | price(paise) | tax_profile | HSN/SAC | station | variants        | modifiers |
|--------------------------|--------------|-------------|---------|---------|-----------------|-----------|
| Samosa (2 pc)            | 6000         | GST_FOOD_5  | 9963    | CHAT    | —               | Extra chutney (+1500) |
| Paneer Tikka             | 32000        | GST_FOOD_5  | 9963    | TANDOOR | Half/Full       | Spice: Mild/Med/Hot |
| Veg Manchurian           | 24000        | GST_FOOD_5  | 9963    | MAIN    | Dry/Gravy       | Spice: Mild/Med/Hot |
| Chicken 65               | 34000        | GST_FOOD_5  | 9963    | MAIN    | Half/Full       | Spice: Mild/Med/Hot |
| Pani Puri (6 pc)         | 8000         | GST_FOOD_5  | 9963    | CHAT    | —               | Extra puri (+3000) |
| Aloo Tikki Chaat         | 12000        | GST_FOOD_5  | 9963    | CHAT    | —               | Extra dahi (+2000) |

## Category: Tandoor & Kebabs  → station TANDOOR

| item                     | price(paise) | tax_profile | HSN/SAC | station | variants   | modifiers |
|--------------------------|--------------|-------------|---------|---------|------------|-----------|
| Tandoori Chicken         | 42000        | GST_FOOD_5  | 9963    | TANDOOR | Half/Full  | Spice: Mild/Med/Hot |
| Seekh Kebab (4 pc)       | 36000        | GST_FOOD_5  | 9963    | TANDOOR | —          | Spice: Mild/Med/Hot |
| Malai Tikka              | 34000        | GST_FOOD_5  | 9963    | TANDOOR | Half/Full  | — |

## Category: Main Course — Veg  → station MAIN

| item                     | price(paise) | tax_profile | HSN/SAC | station | variants   | modifiers |
|--------------------------|--------------|-------------|---------|---------|------------|-----------|
| Paneer Butter Masala     | 32000        | GST_FOOD_5  | 9963    | MAIN    | Half/Full  | Spice: Mild/Med/Hot; Extra gravy (+4000) |
| Dal Makhani              | 26000        | GST_FOOD_5  | 9963    | MAIN    | Half/Full  | Butter (+2000) |
| Palak Paneer             | 30000        | GST_FOOD_5  | 9963    | MAIN    | Half/Full  | Spice: Mild/Med/Hot |
| Chana Masala             | 22000        | GST_FOOD_5  | 9963    | MAIN    | Half/Full  | Spice: Mild/Med/Hot |
| Mixed Veg Curry          | 24000        | GST_FOOD_5  | 9963    | MAIN    | Half/Full  | — |

## Category: Main Course — Non-Veg  → station MAIN

| item                     | price(paise) | tax_profile | HSN/SAC | station | variants   | modifiers |
|--------------------------|--------------|-------------|---------|---------|------------|-----------|
| Butter Chicken           | 38000        | GST_FOOD_5  | 9963    | MAIN    | Half/Full  | Spice: Mild/Med/Hot; Extra gravy (+4000) |
| Chicken Curry            | 34000        | GST_FOOD_5  | 9963    | MAIN    | Half/Full  | Spice: Mild/Med/Hot |
| Mutton Rogan Josh        | 46000        | GST_FOOD_5  | 9963    | MAIN    | Half/Full  | Spice: Mild/Med/Hot |
| Fish Curry               | 40000        | GST_FOOD_5  | 9963    | MAIN    | Half/Full  | Spice: Mild/Med/Hot |
| Egg Bhurji               | 18000        | GST_FOOD_5  | 9963    | MAIN    | —          | — |

## Category: Biryani & Rice  → station MAIN

| item                     | price(paise) | tax_profile | HSN/SAC | station | variants   | modifiers |
|--------------------------|--------------|-------------|---------|---------|------------|-----------|
| Chicken Biryani          | 32000        | GST_FOOD_5  | 9963    | MAIN    | Half/Full  | Spice: Mild/Med/Hot; Extra raita (+3000) |
| Veg Biryani              | 26000        | GST_FOOD_5  | 9963    | MAIN    | Half/Full  | Extra raita (+3000) |
| Mutton Biryani           | 42000        | GST_FOOD_5  | 9963    | MAIN    | Half/Full  | Spice: Mild/Med/Hot |
| Jeera Rice               | 14000        | GST_FOOD_5  | 9963    | MAIN    | —          | — |
| Steamed Rice             | 10000        | GST_FOOD_5  | 9963    | MAIN    | —          | — |

## Category: Breads  → station TANDOOR

| item                     | price(paise) | tax_profile | HSN/SAC | station | variants | modifiers |
|--------------------------|--------------|-------------|---------|---------|----------|-----------|
| Butter Naan              | 6000         | GST_FOOD_5  | 9963    | TANDOOR | —        | Extra butter (+1500) |
| Garlic Naan              | 7000         | GST_FOOD_5  | 9963    | TANDOOR | —        | — |
| Tandoori Roti            | 4000         | GST_FOOD_5  | 9963    | TANDOOR | Plain/Butter | — |
| Laccha Paratha           | 7000         | GST_FOOD_5  | 9963    | TANDOOR | —        | — |

## Category: Beverages  → station BAR
### The tax-rate showcase — three different profiles in one category

| item                     | price(paise) | tax_profile     | HSN/SAC | station | variants        | modifiers |
|--------------------------|--------------|-----------------|---------|---------|-----------------|-----------|
| Masala Chai              | 4000         | GST_FOOD_5      | 9963    | BAR     | —               | Extra strong |
| Filter Coffee            | 5000         | GST_FOOD_5      | 9963    | BAR     | —               | — |
| Fresh Lime Soda          | 8000         | GST_FOOD_5      | 9963    | BAR     | Sweet/Salted    | — |
| Sweet Lassi              | 9000         | GST_FOOD_5      | 9963    | BAR     | Sweet/Mango     | — |
| Bottled Water 1L         | 2000         | GST_PACKAGED_18 | 2201    | BAR     | —               | — |
| Packaged Fruit Juice     | 6000         | GST_PACKAGED_18 | 2202    | BAR     | Mango/Mixed     | — |
| Coca-Cola (can)          | 5000         | GST_AERATED_40  | 2202    | BAR     | —               | — |
| Thums Up (can)           | 5000         | GST_AERATED_40  | 2202    | BAR     | —               | — |

## Category: Desserts  → station DESSERT

| item                     | price(paise) | tax_profile | HSN/SAC | station | variants | modifiers |
|--------------------------|--------------|-------------|---------|---------|----------|-----------|
| Gulab Jamun (2 pc)       | 8000         | GST_FOOD_5  | 9963    | DESSERT | —        | — |
| Gajar Halwa              | 12000        | GST_FOOD_5  | 9963    | DESSERT | —        | Extra dry fruits (+3000) |
| Kulfi                    | 9000         | GST_FOOD_5  | 9963    | DESSERT | Malai/Pista | — |

---

## Why this spec exercises the engine (for the builder)

1. **Three tax profiles in the Beverages category** — a single order of "Butter Chicken +
   Naan + Coke + Bottled Water" spans 5%, 40% and 18% on one bill. This is the exact
   mixed-rate invoice that `menu_item.tax_profile_id` and per-line snapshotting exist for,
   and it forces the per-component conservation property (Σ line CGST = invoice CGST)
   to hold across differing rates, not just within one.
2. **HSN/SAC populated per item** (9963 for food, 2201/2202 for packaged/aerated) — so
   `assemble.rs` reads a real code instead of None, and the no-null-HSN assertion has
   real data to pass against.
3. **All five stations used** — one order can fan to TANDOOR (naan), MAIN (curry),
   BAR (drinks), DESSERT — exercising KOT routing breadth the 2-item seed never could.
4. **Variants and modifiers with price deltas** — Half/Full and "+extra gravy" exercise
   the modifier price-delta path that the money invariant had never seen real data for.
5. **Inclusive pricing** — menu prices include tax, exercising inclusive-mode back-computation.

## Constraints
- Seed data only — no product/menu-management feature.
- Prices, HSN codes and profiles are representative dev values; a production outlet
  configures its own. Leave a comment in devseed saying so.
- Keep the existing seeded outlet/device/user; only expand catalog + stations + profiles.
- devseed must still run start-to-finish and leave a byte-stable snapshot per the
  persistence round-trip discipline.
