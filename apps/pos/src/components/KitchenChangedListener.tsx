import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";

import { onKitchenChanged } from "../lib/kitchenEvents";
import { queryKeys } from "../lib/queries";

/**
 * THE TILL HEARS THE KITCHEN FROM ANYWHERE, NOT ONLY FROM AN OPEN PANEL.
 *
 * D14 (`97bc3dc`) built the whole channel and it works: the KDS bumps a ticket
 * over the LAN, the hub inside this very process broadcasts it, and
 * `apps/pos/src-tauri/src/lib.rs` forwards it as `holler://kitchen-changed`.
 * What it got wrong was WHERE IT LISTENED. The subscription lived in
 * `KotsPanel`, which React mounts only while an order's Kitchen panel is
 * expanded — so in the normal state, with every panel collapsed, nothing in
 * the process was listening and the event was emitted into nothing.
 *
 * That one mounting mistake produced two bug reports that read as unrelated:
 * "kitchen status only updates when I collapse and re-expand the order"
 * (the panel was the only listener, and remounting it refetched under
 * `staleTime: 0`), and "a waiter's order never appeared in the order list".
 * See `docs/m7-b2-sinks.md`.
 *
 * Mounted in `App`, inside `QueryClientProvider` and outside the router, so it
 * is alive for every screen and across every navigation.
 *
 * WHAT THIS DELIBERATELY DOES NOT DO: carry the ticket. The payload is an id
 * and its arrival only invalidates. Every KOT a screen renders still comes
 * from `list_kots_for_order`, through the same command, parsed by the same
 * schema — one path, not two. A push channel that also carried state would be
 * a second source for the same row, and the two would disagree the first time
 * one of them was wrong.
 */
export function KitchenChangedListener() {
  const queryClient = useQueryClient();

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;

    void onKitchenChanged(() => {
      void queryClient.invalidateQueries({ queryKey: queryKeys.orders });
      // Every KOT list, not one order's. This listener has no order in scope
      // and must not need one: a bump can land on any order, including one
      // whose panel is closed or on a screen that is not the order list.
      void queryClient.invalidateQueries({
        predicate: (query) => query.queryKey[0] === "kots",
      });
    }).then((fn) => {
      // The effect may have been torn down while `listen` was in flight;
      // without this the listener outlives the component and leaks one per
      // mount.
      if (cancelled) fn();
      else unlisten = fn;
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [queryClient]);

  return null;
}
