import { useQuery } from "@tanstack/react-query";
import {
  listFailedPrintJobs,
  listKotsForOrder,
  listMenuCategories,
  listMenuItems,
  listOrders,
  listStations,
  listTables,
} from "./tauri";

// Query keys centralised here so cache invalidation after a write stays
// consistent across the app.
export const queryKeys = {
  menuItems: ["menu-items"] as const,
  menuCategories: ["menu-categories"] as const,
  tables: ["tables"] as const,
  orders: ["orders"] as const,
  stations: ["stations"] as const,
  kots: (orderId: string) => ["kots", orderId] as const,
  failedPrintJobs: ["failed-print-jobs"] as const,
};

export function useMenuItemsQuery() {
  return useQuery({ queryKey: queryKeys.menuItems, queryFn: listMenuItems });
}

export function useMenuCategoriesQuery() {
  return useQuery({ queryKey: queryKeys.menuCategories, queryFn: listMenuCategories });
}

export function useTablesQuery() {
  return useQuery({ queryKey: queryKeys.tables, queryFn: listTables });
}

export function useOrdersQuery() {
  return useQuery({ queryKey: queryKeys.orders, queryFn: listOrders });
}

export function useStationsQuery() {
  return useQuery({ queryKey: queryKeys.stations, queryFn: listStations });
}

export function useKotsForOrderQuery(orderId: string | null) {
  return useQuery({
    queryKey: queryKeys.kots(orderId ?? "none"),
    queryFn: () => listKotsForOrder(orderId as string),
    enabled: orderId !== null,
  });
}

/** Polled rather than event-driven (this milestone has no push channel from
 * the print spool to the UI) — a short interval keeps a failed print
 * noticeable without the cashier needing to navigate anywhere. */
export function useFailedPrintJobsQuery() {
  return useQuery({
    queryKey: queryKeys.failedPrintJobs,
    queryFn: listFailedPrintJobs,
    refetchInterval: 5000,
  });
}
