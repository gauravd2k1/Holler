# `@holler/ui` — one design token set for four apps

Consumed by `apps/pos`, `apps/kds`, `apps/captain` and `apps/admin`.

## Why

Presentability was raised from a work item to a **requirement** for the client
demo, and the operative half of it is that the four surfaces must read as **one
product**. Before this package there were 1,053 lines of unrelated CSS across
the four apps and the till alone used `#333`, `#f66`, `#b06fd0`, `#3a6` and a
dozen more one-off values chosen per screen, none of them tied to the brand the
client has already seen in the website and the deck.

**Every primitive here is lifted from `website/holler-branded-website.html`**,
the existing brand system. Two values are derived and both are marked DERIVED at
their definition with the reason. Nothing else is invented.

## Install

Each app is an independent pnpm project — there is no root workspace — so this
is consumed the same way `@holler/contracts` already is:

```jsonc
// apps/<app>/package.json
"dependencies": {
  "@holler/ui": "file:../../packages/ui"
}
```

Then, at the top of the app's own stylesheet or entry point, **in this order**:

```ts
import "@holler/ui/tokens.css";
import "@holler/ui/base.css";
import "./index.css";      // the app's own layout, and nothing else
```

## The rule

**Screens consume semantic tokens. Screens do not consume brand primitives, and
screens do not contain literal colour or size values.**

```css
/* yes */
color: var(--color-text-muted);
padding: var(--space-4);
font-size: var(--text-lg);

/* no — this is the thing the package replaces */
color: #657083;
padding: 17px;
font-size: 19px;

/* no — primitives exist for the semantic layer to point at, not for screens */
color: var(--holler-muted);
```

If a screen needs something the semantic layer does not offer, **add a semantic
token here** rather than reaching for a primitive or a literal. A token added
for one screen is still cheaper than a value that agrees with the brand only by
coincidence.

## Two surface modes, one palette

The default is light — the brand's own paper and ink — used by the till, the
captain page and the admin console.

The KDS sets `data-surface="dark"` on its root. That mode exists for an
**environment** reason, not a stylistic one: a kitchen display is glanced at from
across a hot line, and dark-on-light glare is worse there. Every KDS on the
market is dark for that reason.

**The dark mode remaps semantic tokens only. It never redefines a primitive.**
`--color-ok`, `--color-warn`, `--color-danger` and `--color-info` are
deliberately absent from the dark block, so they inherit the light values
unchanged — a cook and a cashier read the same green as the same green. If a
status colour ever needs adjusting for contrast on ink, change the tinted
**ground** beneath it (`--color-ok-bg` and siblings, which the dark block does
override), never the status colour itself.

That single constraint is what stops two surface modes becoming two products.

## What is in here

| Group | Tokens |
|---|---|
| Brand primitives | `--holler-*` — navy, teal, orange, amber, green, ink, muted, paper, cream, line, plus two DERIVED |
| Colour | `--color-bg`, `--color-surface`, `--color-surface-raised`, `--color-text`, `--color-text-muted`, `--color-text-inverse`, `--color-border`, `--color-border-strong`, `--color-accent*`, `--color-ok\|warn\|danger\|info` and their `-bg` grounds |
| Type | `--font-sans`, `--font-mono`, `--text-xs` … `--text-3xl`, `--leading-*`, `--weight-*` |
| Spacing | `--space-1` … `--space-7`, a 4px base with nothing between steps |
| Shape | `--radius-sm\|md\|lg\|pill`, `--shadow-sm\|md\|lg` |
| Controls | `--control-height` (44px), `--control-height-lg`, `--focus-ring` |
| Brand mark | `--logo-height`, `--logo-height-lg` |

`base.css` also ships the reset, typography, `.btn` family, form controls,
`.card`, `.status--*` blocks, tables, and `.holler-header` / `.holler-logo` so
the brand mark sits in the same place on all four surfaces.

## Three things in here that came from real defects

- **`[hidden] { display: none !important }`.** A `display: flex` rule beat the
  `hidden` attribute on the till's sync banner, so "Hide details" relabelled the
  button and left every row on screen. Found in a browser; invisible to 234
  passing tests, a clean `tsc` and a clean `vite build`. Fixed once here rather
  than per component.
- **`--control-height: 44px`.** The till is a touchscreen and the captain page is
  a phone held in one hand. This is the smallest target reliably hit without
  looking, and it must not be shrunk to fit more on a screen.
- **`.money { font-variant-numeric: tabular-nums }`.** Money is integer paise
  everywhere in this system and is formatted only at the render boundary; the
  least a column of totals can do is line up and not jitter as it changes.

## The font

Inter, matching the brand site. **Each app must ship the face locally rather
than fetching it.** An outlet with no uplink is the normal case (ADR-013), and a
web font that fails to load silently re-renders the entire product in a
fallback. The fallback stack in `--font-sans` is what has to still look
deliberate when that happens.

## The logo

`imgs/` holds the mark: `holler_no_bg.png` (transparent),
`android-chrome-512x512.png`, `android-chrome-192x192.png`,
`apple-touch-icon.png` and the favicon set. **There is no SVG in this
repository** — every asset is raster. Size it with `--logo-height`, not a
per-screen pixel value.
