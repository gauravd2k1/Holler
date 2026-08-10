# KDS <-> edge/device LAN interop (T10, Milestone 2)

Proves that the real Rust WebSocket server in `edge/device` and the real
TypeScript client in `apps/kds` actually interoperate — not that each is
internally consistent with its own fakes, which is all their existing unit
tests prove.

This exists because they had never been executed against each other, and the
first time they were connected by inspection, they disagreed on the
handshake (see ADR-015 and the header comment in
`packages/contracts/src/types/lan.ts`).

## What runs

Two pieces, both under `tests/`:

- **`tests/integration/kds-lan-bridge`** — a small Rust binary crate. It
  links `holler-edge-device` and `holler-edge-database` directly (no
  reimplementation of either), seeds one outlet/device/menu item/order and
  sends it to the kitchen (producing one active `kot` row in `NEW`), starts
  the real `holler_edge_device::server` on an ephemeral port, and prints one
  JSON line to stdout: `{"port", "outlet_id", "kds_device_id", "kot_id",
  "order_id"}`. It then blocks on stdin; any line, or EOF, triggers a clean
  shutdown. It is test-only infrastructure — not a workspace member of any
  product crate, not referenced by `edge/device` or `apps/pos/src-tauri`.

- **`tests/integration/kds-lan`** — a Vitest suite (Node environment, not
  jsdom, so it uses Node's own built-in global `WebSocket`, available since
  Node 22). `bridge.ts` spawns the harness above via `cargo run` and parses
  its ready line. `kds-lan.test.ts` imports the **real** client modules
  straight from `apps/kds/src/lib` and `apps/kds/src/store` —
  `buildConnectionUrl`, `ConnectionController`, `useKdsStore` — and drives
  them against a genuine socket to the spawned server. Nothing here
  hand-writes the handshake URL, a fake socket, or a reimplementation of the
  KOT state machine.

## Running it

```sh
cd tests/integration/kds-lan
pnpm install   # first time only
pnpm test
```

The first run pays the cost of compiling `kds-lan-bridge` (and its
dependency graph — `holler-edge-device`, `holler-edge-database`); this can
take upwards of a minute. Subsequent runs reuse cargo's incremental build
cache and are fast (each test spawns a fresh bridge process for DB
isolation, but `cargo run` against an unchanged crate starts in well under a
second).

No internet access is required beyond the one-time `cargo`/`pnpm`
dependency fetch — the whole test runs over `127.0.0.1`.

## What it proves (and where)

1. **Handshake succeeds with the client's own `buildConnectionUrl`** — test
   `1+2`, plus every other test (all of them connect this way; none
   hand-builds the URL).
2. **A snapshot arrives first and the client applies it** — test `1+2`.
3. **A ticket transition round-trips** — test `3`: `requestStatusChange` ->
   `set_kot_status` intent -> edge validates and confirms -> `kdsStore`
   renders the confirmed status, and the pending marker clears.
4. **An illegal transition is rejected and no false state is shown** — test
   `4`: `NEW -> SERVED` (skipping the required intermediate states) is sent;
   no confirming message ever arrives, so the pending transition times out
   (visible as "not confirmed") rather than the ticket silently jumping to
   `SERVED`.
5. **The 400 path** — test `5`: a handshake with an empty `outlet_id` is
   checked two ways — a raw hand-built HTTP Upgrade request asserts the
   literal status code `400`, and a real `WebSocket` against the same URL is
   asserted to never fire `onopen`.

## CI

Add a job step (after the existing `apps/kds` and `edge/device` steps) that
runs:

```sh
cd tests/integration/kds-lan
pnpm install --frozen-lockfile=false
pnpm test
```

`cargo` and a Rust toolchain must already be on the runner (true today, since
`edge/device`'s own test suite requires the same thing). No other new CI
dependency is introduced.

## Verifying this test can actually fail

Two deliberate-break checks were run manually while building this suite
(not checked in — `edge/device` is left byte-identical to before):

- Renamed the handshake query param the server requires
  (`outlet_id` -> `outlet_id_DELIBERATE_BREAK_T10` in
  `HandshakeCallback::on_request`) while leaving the client's
  `buildConnectionUrl` untouched. Result: tests `1+2`, `3`, `4` failed with
  "timed out waiting for condition" (the client never sees `connected` or a
  snapshot); test `5` still passed (a mismatched-param connection is still a
  400, just for a different reason) — as expected.
- Renamed the `kot_upserted` wire discriminant in
  `edge/device/src/contract.rs` to `kot_upserted_DELIBERATE_BREAK_T10`.
  Result: test `3` (the transition round-trip) failed on a real Zod
  `invalid_union_discriminator` error surfaced through `LanClient`'s
  `onInvalidMessage` handler; snapshot-only test `1+2` still passed,
  correctly localizing the failure to the message type it actually broke.

Both were reverted immediately after observing the red run, and `cargo
build` plus a full green `pnpm test` were re-run afterward to confirm the
working tree matched pre-break behaviour exactly (`git status --porcelain
edge/device` empty both times).
