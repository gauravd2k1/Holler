# Spec: Menu

Owns: menu/catalog engine, pricing, modifiers.
Source: HOLLER_MASTER_PROMPT.md §9, §10.

## Entities
Menu, Category, Subcategory, MenuItem, Variant, ModifierGroup, Modifier, Combo, TaxProfile, PriceBook, AvailabilityRule, OrderType, Channel, KitchenStation.

## Multi-menu / channel pricing
Multiple menus (breakfast/lunch/dinner, day-of-week, happy hour, outlet-specific, aggregator-specific). Prices vary by channel/outlet/order type via **channel price books** — never duplicate the underlying product.
Example: Butter Chicken — Dine-in ₹410, Takeaway ₹420, Zomato ₹459, Swiggy ₹459.

## Modifiers
Trees support required/optional, min/max selection, repeated, nested modifiers, price deltas, and recipe implications (a modifier can change inventory deduction — see docs/spec/inventory.md).
```
Pizza
├── Size: Regular | Medium | Large
├── Crust: Thin | Cheese Burst
└── Toppings: Paneer | Mushroom | Jalapeño
```

## Conflict policy
Menu description: version-based merge/admin resolution. Availability: latest authorized version wins (cloud is source of truth for catalog — see docs/spec/sync.md §50.1).

## Cross-context dependencies
- Aggregators (docs/spec/aggregators.md) for channel menu sync/overrides.
- Inventory (docs/spec/inventory.md) for stock-out driven item snooze.
- Multi-outlet (docs/spec/multi-outlet.md) for Brand Menu → Outlet Override → Channel Override inheritance.
