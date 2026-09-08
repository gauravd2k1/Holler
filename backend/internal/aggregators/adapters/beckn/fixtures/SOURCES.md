# Provenance of the Beckn fake's fixtures

**Every payload in this directory is derived from ONDC's own published
artefacts, not from our reading of the specification.** A fake we author from
the prose proves only that we agree with ourselves, and conformance against it
would demonstrate that the adapter matches our *interpretation* rather than
ONDC — arriving in the one place where no external check exists until
certification.

## Artefacts, with source and version

| Artefact | Source | Version | Pinned at |
|---|---|---|---|
| `confirm` example payload | `github.com/ONDC-Official/ONDC-RET-Specifications`, path `api/components/Examples/B2B/confirm/confirm_domestic.yaml` | branch `release-2.0.2`, `context.version: 2.0.2` | commit `aa74e5d8e0543f0b7e943d029a08ec10f8b1c0e3`, committed 2024-08-23T05:04:57Z |
| `cancel` example payload | same repository, path `api/components/Examples/B2B/cancel/cancel.yaml` | branch `release-2.0.2` | same commit |
| `status` example payload | derived from the `cancel` envelope with the `status` message shape (`{order_id}`) | branch `release-2.0.2` | same commit |
| `on_confirm` example payload | same repository, path `api/components/Examples/B2B/on_confirm/on_confirm_domestic.yaml` | branch `release-2.0.2` | same commit |
| Attribute definitions consulted for field presence | same repository, `api/components/attributes/B2B/on_confirm/on_confirm.yaml`, `.../on_status/on_status.yaml` | branch `release-2.0.2` | same commit |

Retrieved 2026-09-08 via the GitHub API (`gh api repos/ONDC-Official/ONDC-RET-Specifications/...?ref=release-2.0.2`).

Branches present on the repository at that date: `master`, `release-2.0.2`,
`b2c_exports_2.0`, `draft-1.x`, `draft-1.2.1`, `draft-2.x`, `draft-b2c-1.2.5`,
`draft-b2c_exports`, `draft_ui`, `ret-ui`. **`release-2.0.2` was chosen as the
only non-draft release branch matching the 2.0.2 version the payloads
themselves declare in `context.version`.**

## WHICH DIRECTION EACH FIXTURE IS, AND WHY IT MATTERS

**Holler is the SELLER — the BPP.** A buyer app (BAP) sends us `search`,
`select`, `init`, `confirm`, `status`, `cancel` and `update`; we answer each with
its `on_*` callback.

- `confirm.json`, `cancel.json`, `status.json` are **INBOUND** — what we receive.
- `on_confirm.json` is **OUTBOUND** — an example of what we SEND, kept as the
  reference for the callback shape.

The first cut of this adapter parsed `on_confirm` as an inbound order, which is
the BUYER's view. It passed, because ONDC's request and callback envelopes share
`message.order` — the code was right about the JSON and wrong about the
direction, and no test built from the same assumption could have caught it. The
fixtures are labelled here so the next reader does not repeat it.

## What the fixtures preserve verbatim, and why

The envelope shape is ONDC's, unaltered:

- `context` carrying `domain`, `action`, `version`, `bap_id`, `bap_uri`,
  `bpp_id`, `bpp_uri`, `transaction_id`, `message_id`, `timestamp`, `ttl`
- `message.order` carrying `id`, `state`, `provider`, `items[]` with
  `quantity.selected.count`, `billing`, `fulfillments`, `quote`
- `quote.breakup[]` entries keyed by `@ondc/org/item_id`,
  `@ondc/org/item_quantity` and `@ondc/org/title_type`, with `price.value` as a
  **decimal STRING in rupees** (`'53600'`, `'250'`)

That last one is the detail worth stating separately: **ONDC prices are strings,
in rupees, and this product stores integer paise.** The conversion is the
adapter's job and is done in integer arithmetic, never by parsing to a float —
`53600` rupees is `5360000` paise, and `'250.50'` must not become
`25049.999999`. A fixture that only carried whole rupees would let a float bug
through, so one is deliberately fractional.

## What is NOT claimed

**These fixtures prove SHAPE, not INTEGRATION.** They are ONDC's published
examples replayed by a local fake; no message has ever been exchanged with an
ONDC system. M6 C8 is recorded as `SHAPE ONLY — no integration evidence`, and
the integration row travels to M6.1 as an explicitly unmet criterion.

Nothing here is signed. Ed25519 signing, the registry `subscribe` /
`on_subscribe` challenge, and the public HTTPS ingress are M6.1 and are
deliberately absent — a self-authored counterparty cannot falsify a signature
scheme, it can only agree with it.
