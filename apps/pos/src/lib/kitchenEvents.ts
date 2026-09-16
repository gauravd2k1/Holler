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
  return listen<string>(KITCHEN_CHANGED_EVENT, (event) => {
    onChange(event.payload);
  });
}
