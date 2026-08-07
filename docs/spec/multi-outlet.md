# Spec: Multi-Outlet

Owns: org/brand/outlet hierarchy, central admin, inheritance.
Source: HOLLER_MASTER_PROMPT.md §5, §46, §90.

## Domain hierarchy
```
Organisation
└── Brand
    └── Outlet
        ├── Revenue Centers
        ├── Floors
        ├── Tables
        ├── Kitchens
        ├── Stations
        ├── Registers
        └── Devices
```
Revenue center examples: restaurant, bar, bakery, room service, takeaway, delivery, banquet, food-court counter. Never assume one restaurant = one outlet.

## Central admin
Manages menus, recipes, pricing, tax rules, staff, suppliers, inventory templates, promotions, analytics from one place. Inheritance: **Brand Menu → Outlet Override → Channel Override** — never manually duplicate data per outlet.

## Control plane (long term)
Holler Cloud lets the owner see all outlets, change menu/prices, publish recipes, review purchases, monitor stock/KDS performance/anomalies, push configuration, disable compromised terminals, monitor sync status. Config changes propagate safely; outlets keep operating locally during cloud outages.

## Milestone note
Milestone 8 delivers brand management, central menu/recipes, outlet overrides, executive dashboard, cost analytics.
