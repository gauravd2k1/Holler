// The till listening to its own kitchen hub (D14).
//
// The KDS bumps a ticket over the LAN; the hub inside this very process
// broadcasts the new state to its subscribers; the till subscribes as an
// ordinary client and forwards each frame here as a Tauri event. See
// `apps/pos/src-tauri/src/lib.rs` `KITCHEN_CHANGED_EVENT`.
//
// WHAT THIS DELIBERATELY DOES NOT DO: carry the ticket. The payload is an id
// and nothing else, and the arrival of one only invalidates the query. Every
// KOT a screen renders therefore still comes from `list_kots_for_order`,
// through the same command, parsed by the same schema — one path, not two.
// A push channel that also carried state would be a second source for the
// same row, and the two would disagree the first time one of them was wrong.

import { listen, type UnlistenFn } from "@tauri-apps/api/event";

/** Must match `KITCHEN_CHANGED_EVENT` in `apps/pos/src-tauri/src/lib.rs`.
 * A mismatch is a silent no-op — nothing throws, the till simply goes back to
 * being stale — which is why both sides name a constant and
 * `scripts/check-kitchen-event-drift.mjs` compares them. */
export const KITCHEN_CHANGED_EVENT = "holler://kitchen-changed";

/** Subscribes to kitchen changes. `onChange` receives the KOT id that moved.
 *
 * Returns a promise for the unlisten function. `listen` is imported as a free
 * module function and called as one: CLAUDE.md's detached-global rule exists
 * because a receiver-bound builtin stored on a field throws `Illegal
 * invocation` in a browser and nowhere else. */
export async function onKitchenChanged(
  onChange: (kotId: string) => void,
): Promise<UnlistenFn> {
  // NO-OP OUTSIDE A TAURI WINDOW, AND THE DEV SERVER IS OUTSIDE ONE.
  //
  // `listen` reaches for `window.__TAURI_INTERNALS__.transformCallback`,
  // which exists only inside the Tauri webview. In a plain browser it throws
  // `Cannot read properties of undefined (reading 'transformCallback')`
  // during module evaluation.
  //
  // This never fired while the only caller was `KotsPanel`, mounted just
  // while a Kitchen panel was expanded -- a browser sitting on the login
  // screen never reached it. Moving the subscription to `App` (2026-09-18)
  // made it run at boot on every screen, and `pos-dev-server-smoke` caught it
  // on the first push: two `pageerror`s, with `tsc`, eslint, `pnpm build` and
  // all 264 unit tests green. The third runtime again, exactly as CLAUDE.md
  // says.
  //
  // The fix is here rather than in the smoke test's ignore list, which is
  // deliberately empty: filtering the error would buy a green suite and cost
  // the ability to see this whole class of failure.
  //
  // The warn is deliberate. If this ever fires INSIDE the Tauri window the
  // till goes quietly back to being stale, which is the failure mode this
  // whole track exists to remove -- so it says so in the console rather than
  // returning a silent no-op that reads as success.
  if (typeof window === "undefined" || !("__TAURI_INTERNALS__" in window)) {
    console.warn(
      `${KITCHEN_CHANGED_EVENT}: not running inside a Tauri window, so kitchen ` +
        `updates will not arrive. Expected in a browser or the dev server; a DEFECT in the app.`,
    );
    return () => {};
  }

  return listen<string>(KITCHEN_CHANGED_EVENT, (event) => {
    onChange(event.payload);
  });
}
