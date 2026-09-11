#!/usr/bin/env python3
"""Generate the Gong menu half of the demo seed from the client's own workbook.

`menu_imgs_gong/gong_menu.xlsx`, sheet "Menu", is the AUTHORING SOURCE for the
catalogue: it is what the client sent and what a human corrects. This script
reads every row with `include = Y` and writes two generated artefacts:

  edge/database/src/bin/devseed/gong_menu.rs  the categories/items/variants and
                                       modifiers as a Rust literal, consumed by devseed
  seed/gong-menu-manifest.json         counts plus a digest of both sides

Neither is hand-edited. `edge/database/src/bin/devseed.rs` then emits
`seed/demo-outlet.json` from the Rust data as before, so the chain is

    gong_menu.xlsx -> devseed/gong_menu.rs -> demo-outlet.json -> {SQLite, Postgres}

and every link but the first is checked by something that fails when it drifts:
`devseed`'s own `gong_menu_matches_the_generated_manifest` test covers the
first arrow, `scripts/check-seed-drift.mjs` the second.

WHY A MANIFEST AND NOT A COMMENT. The seed used to be bound to
`HOLLER_DEV_MENU_SPEC.md` by a test that parsed that document and compared
counts, because the two were independently hand-maintained. The generated file is
GENERATED, so the drift that test guarded against is now a different one: a
hand-edit of the generated file. The manifest carries a digest of the
generated data alongside a digest of the workbook it came from, so an edit to
either side without a regeneration fails the build.

Run from the repository root:

    python scripts/gong-menu-to-seed.py

then re-emit and commit the seed:

    cd edge/database && cargo run --bin devseed -- --emit-json ../../seed/demo-outlet.json
"""

from __future__ import annotations

import hashlib
import html
import json
import re
import sys
import zipfile
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
WORKBOOK = REPO_ROOT / "menu_imgs_gong" / "gong_menu.xlsx"
RUST_OUT = REPO_ROOT / "edge" / "database" / "src" / "bin" / "devseed" / "gong_menu.rs"
MANIFEST_OUT = REPO_ROOT / "seed" / "gong-menu-manifest.json"

# NOTE on the output path: a .rs file directly under src/bin/ is auto-discovered
# by cargo as its own binary target, which would fail to build for want of a
# main(). src/bin/devseed/ is a module directory of the devseed bin instead.

# Sheet 2 is "Menu" (sheet 1 is "Read me", sheet 3 the alternative beverage
# price levels the Read me says are NOT current).
MENU_SHEET_INDEX = 2

# The workbook's `station` column holds human labels; the seed stores a
# station CODE on the item (`kot.station` stores the code, never the id, so a
# ticket survives a rename -- contracts 0.3.0).
STATION_CODES = {
    "Kitchen": "KITCHEN",
    "Wok": "WOK",
    "Sushi Bar": "SUSHI_BAR",
    "Dimsum": "DIMSUM",
    "Dessert": "DESSERT",
    "Beverage": "BEVERAGE",
    "Bar": "BAR",
}

# The workbook's `tax` column maps to a seeded tax_profile constant.
# "VAT (alcohol)" has no expressible component under contracts 0.8.1 --
# tax_rule.component is CHECKed to CGST/SGST/IGST/CESS -- so alcohol carries a
# dedicated ZERO-RATE profile rather than a wrong rate under a wrong label.
# See seed/README.md and docs/demo-status.md; the VAT component itself is a
# pilot-readiness contracts item.
TAX_PROFILES = {
    "GST 5%": "TAX_PROFILE_FOOD5_ID",
    "VAT (alcohol)": "TAX_PROFILE_ALCOHOL_VAT_ID",
}

# Restaurant/catering service. Every line on this card is served at table, so
# one code covers it; an invoice cannot issue with a blank HSN/SAC on any line
# (contracts 0.4.5).
HSN_SAC = "9963"

# The Read me says these three rows are add-ons, not items: they are priced
# supplements to the Staple dishes and must be seeded as modifiers on those
# dishes. Keyed by (section, item_name) so a future "Add Chicken" elsewhere on
# the card is not swept up by accident.
ADDON_SECTION = "Staple"
ADDON_GROUP_NAME = "Add-on"
ADDON_ROWS = ("Add Chicken", "Add Prawns", "Add Mixed Meat")


# ---------------------------------------------------------------------------
# xlsx reading. Deliberately dependency-free: this repository installs no
# Python packages, and the sheet is a flat table of strings and numbers.
# ---------------------------------------------------------------------------
def read_sheet(path: Path, sheet_index: int) -> list[dict[str, str]]:
    with zipfile.ZipFile(path) as z:
        shared: list[str] = []
        if "xl/sharedStrings.xml" in z.namelist():
            raw = z.read("xl/sharedStrings.xml").decode("utf8")
            for si in re.findall(r"<si>(.*?)</si>", raw, re.S):
                shared.append(
                    html.unescape("".join(re.findall(r"<t[^>]*>(.*?)</t>", si, re.S)))
                )
        raw = z.read(f"xl/worksheets/sheet{sheet_index}.xml").decode("utf8")

    rows: list[tuple[int, dict[str, str]]] = []
    for row_number, body in re.findall(r'<row[^>]*r="(\d+)"[^>]*>(.*?)</row>', raw, re.S):
        cells: dict[str, str] = {}
        # Self-closing cells (`<c r="E2" s="5"/>`) are blanks and MUST be
        # matched: missing them shifts every later column by one, silently.
        for ref, attrs, inner in re.findall(
            r'<c r="([A-Z]+)\d+"([^>]*?)(?:/>|>(.*?)</c>)', body, re.S
        ):
            inner = inner or ""
            kind = re.search(r't="([^"]+)"', attrs)
            value = re.search(r"<v>(.*?)</v>", inner, re.S)
            if kind and kind.group(1) == "s" and value:
                text = shared[int(value.group(1))]
            elif kind and kind.group(1) == "inlineStr":
                text = html.unescape("".join(re.findall(r"<t[^>]*>(.*?)</t>", inner, re.S)))
            elif value:
                text = html.unescape(value.group(1))
            else:
                text = ""
            cells[ref] = text.strip()
        rows.append((int(row_number), cells))

    rows.sort(key=lambda r: r[0])
    header = rows[0][1]
    out = []
    for row_number, cells in rows[1:]:
        record = {header[ref]: value for ref, value in cells.items() if ref in header}
        record["_row"] = str(row_number)
        out.append(record)
    return out


def fnv1a64(text: str) -> int:
    """FNV-1a, 64-bit, over the UTF-8 bytes of the canonical projection.

    NOT a cryptographic digest and not trying to be: the thing it guards
    against is an accidental hand-edit of a generated file, and `devseed` has
    no hashing crate among its dependencies (argon2 pulls blake2 in
    transitively, which is not the same as this crate being allowed to import
    it). Twelve lines of arithmetic reproduced identically on both sides beats
    adding a dependency to the edge binary for a dev-only guard.
    """
    h = 0xCBF29CE484222325
    for byte in text.encode("utf8"):
        h = ((h ^ byte) * 0x100000001B3) & 0xFFFFFFFFFFFFFFFF
    return h


def rust_str(value: str) -> str:
    return '"' + value.replace("\\", "\\\\").replace('"', '\\"') + '"'


def main() -> int:
    rows = read_sheet(WORKBOOK, MENU_SHEET_INDEX)

    included = []
    rejected: list[tuple[str, str, str]] = []
    for row in rows:
        where = f"row {row['_row']} {row.get('menu', '?')}/{row.get('section', '?')}/{row.get('item_name', '?')}"
        if row.get("include") != "Y":
            rejected.append((where, "include is not Y", row.get("include", "")))
            continue
        if not row.get("item_name"):
            rejected.append((where, "no item_name", ""))
            continue
        if not row.get("price_paise"):
            rejected.append((where, "no price_paise", row.get("price_inr", "")))
            continue
        if row.get("station") not in STATION_CODES:
            rejected.append((where, "unmapped station", row.get("station", "")))
            continue
        if row.get("tax") not in TAX_PROFILES:
            rejected.append((where, "unmapped tax", row.get("tax", "")))
            continue
        try:
            int(row["price_paise"])
        except ValueError:
            rejected.append((where, "price_paise is not an integer", row["price_paise"]))
            continue
        included.append(row)

    # Add-on rows leave the item stream and become one modifier group, applied
    # to every other item in their section.
    addons = [
        r
        for r in included
        if r["section"] == ADDON_SECTION and r["item_name"] in ADDON_ROWS
    ]
    missing_addons = set(ADDON_ROWS) - {r["item_name"] for r in addons}
    if missing_addons:
        print(
            f"gong-menu-to-seed: the Read me names add-on rows that are not in the "
            f"sheet: {sorted(missing_addons)}",
            file=sys.stderr,
        )
        return 1
    addon_options = [(r["item_name"], int(r["price_paise"])) for r in addons]
    included = [r for r in included if r not in addons]

    # Group into categories (sheet order) and items (sheet order within a
    # category). An item is identified by (menu, section, item_name): the same
    # name appears in two sections -- "Thai Exotica" is both a Mocktail and a
    # Zero Proof pour at different prices -- and those are two items.
    categories: dict[tuple[str, str], dict[str, list[dict[str, str]]]] = {}
    for row in included:
        key = (row["menu"], row["section"])
        categories.setdefault(key, {}).setdefault(row["item_name"], []).append(row)

    lines: list[str] = []
    manifest_items = []
    item_count = 0
    variant_count = 0
    modifier_count = 0

    lines.append("// @generated by scripts/gong-menu-to-seed.py -- DO NOT EDIT BY HAND.")
    lines.append("//")
    lines.append("// Source: menu_imgs_gong/gong_menu.xlsx, sheet \"Menu\", rows with")
    lines.append("// include = Y. Edit the workbook and re-run the generator; a hand-edit")
    lines.append("// here fails `gong_menu_matches_the_generated_manifest` in devseed.")
    lines.append("//")
    lines.append("// PRICES. The card prints an ABSOLUTE price per variant (Laksa Veg 425,")
    lines.append("// Chicken 485, Prawn 495), while the contract stores one")
    lines.append("// `menu_item.base_price_paise` plus a `price_delta_paise` per variant.")
    lines.append("// The base is the CHEAPEST printed variant and every delta is that")
    lines.append("// variant's printed price minus the base, so each variant still rings up")
    lines.append("// at exactly its printed price and no delta is ever negative.")
    lines.append("//")
    lines.append("// A single-price item carries one \"Regular\" variant at delta 0: a recipe")
    lines.append("// binds to a menu_item_variant_id (ADR-018 2.1), and apps/captain refuses")
    lines.append("// to order an item with no variant at all.")
    lines.append("")
    lines.append("use crate::{SeedItem, TAX_PROFILE_ALCOHOL_VAT_ID, TAX_PROFILE_FOOD5_ID};")
    lines.append("")
    lines.append("/// (category name, sort_order, items), in the order the card prints them.")
    lines.append("pub const GONG_CATEGORIES: &[(&str, i64, &[SeedItem])] = &[")

    sort_order = 2  # 1 is the legacy fixture category devseed keeps for the harness.
    for (menu, section), items in categories.items():
        lines.append("    (")
        lines.append(f"        {rust_str(section)},")
        lines.append(f"        {sort_order},")
        lines.append("        &[")
        for item_name, item_rows in items.items():
            prices = [int(r["price_paise"]) for r in item_rows]
            base = min(prices)
            stations = {r["station"] for r in item_rows}
            taxes = {r["tax"] for r in item_rows}
            if len(stations) > 1 or len(taxes) > 1:
                rejected.append(
                    (
                        f"{menu}/{section}/{item_name}",
                        "variant rows disagree on station or tax",
                        f"{sorted(stations)} {sorted(taxes)}",
                    )
                )
                continue

            variants = []
            for r in item_rows:
                variant_name = r.get("variant") or "Regular"
                variants.append((variant_name, int(r["price_paise"]) - base))

            groups = []
            if section == ADDON_SECTION:
                groups.append((ADDON_GROUP_NAME, addon_options))

            station_code = STATION_CODES[item_rows[0]["station"]]
            item_count += 1
            variant_count += len(variants)
            modifier_count += sum(len(options) for _, options in groups)

            lines.append("            SeedItem {")
            lines.append(f"                name: {rust_str(item_name)},")
            lines.append(f"                price_paise: {base},")
            lines.append(f"                tax_profile_id: {TAX_PROFILES[item_rows[0]['tax']]},")
            lines.append(f"                hsn_sac: {rust_str(HSN_SAC)},")
            lines.append(f"                station_code: {rust_str(station_code)},")
            variant_literal = ", ".join(
                f"({rust_str(name)}, {delta})" for name, delta in variants
            )
            lines.append(f"                variants: &[{variant_literal}],")
            if groups:
                group_literal = ", ".join(
                    f"({rust_str(group)}, &[{', '.join(f'({rust_str(o)}, {d})' for o, d in options)}])"
                    for group, options in groups
                )
                lines.append(f"                modifier_groups: &[{group_literal}],")
            else:
                lines.append("                modifier_groups: &[],")
            lines.append("            },")

            manifest_items.append(
                {
                    "category": section,
                    "name": item_name,
                    "base_price_paise": base,
                    "station_code": station_code,
                    "tax": "ALCOHOL" if item_rows[0]["tax"] == "VAT (alcohol)" else "FOOD5",
                    "variants": [{"name": n, "price_delta_paise": d} for n, d in variants],
                    "modifier_groups": [
                        {"group": g, "options": [{"name": o, "price_delta_paise": d} for o, d in opts]}
                        for g, opts in groups
                    ],
                }
            )
        lines.append("        ],"),
        lines.append("    ),")
        sort_order += 1
    lines.append("];")
    lines.append("")

    RUST_OUT.write_text("\n".join(lines), encoding="utf8", newline="\n")

    projection = chr(10).join(
        "|".join(
            [
                item["category"],
                item["name"],
                str(item["base_price_paise"]),
                item["station_code"],
                item["tax"],
                ";".join(f"{v['name']}={v['price_delta_paise']}" for v in item["variants"]),
                ";".join(
                    f"{g['group']}/{o['name']}={o['price_delta_paise']}"
                    for g in item["modifier_groups"]
                    for o in g["options"]
                ),
            ]
        )
        for item in manifest_items
    )
    manifest = {
        "generated_by": "scripts/gong-menu-to-seed.py",
        "source": "menu_imgs_gong/gong_menu.xlsx",
        "source_sha256": hashlib.sha256(WORKBOOK.read_bytes()).hexdigest(),
        "categories": sort_order - 2,
        "items": item_count,
        "variants": variant_count,
        "modifier_options": modifier_count,
        "projection_fnv1a64": f"{fnv1a64(projection):016x}",
    }
    MANIFEST_OUT.write_text(
        json.dumps(manifest, indent=2, ensure_ascii=False) + "\n", encoding="utf8", newline="\n"
    )

    print(f"gong-menu-to-seed: {RUST_OUT.relative_to(REPO_ROOT)}")
    print(
        f"  {manifest['categories']} categories, {item_count} items, "
        f"{variant_count} variants, {modifier_count} modifier options"
    )
    print(f"  projection fnv1a64 {manifest['projection_fnv1a64']}")
    if rejected:
        print(f"  {len(rejected)} row(s) NOT seeded:")
        for where, why, detail in rejected:
            print(f"    {where}: {why} {detail}".rstrip())
    else:
        print("  every include=Y row was seeded")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
