import { useQuery } from "@tanstack/react-query";
import { listMenuItems, listOrders, listTables } from "./tauri";

// Query keys centralised here so cache invalidation after `create_order`
// stays consistent across the app.
export const queryKeys = {
  menuItems: ["menu-items"] as const,
  tables: ["tables"] as const,
  orders: ["orders"] as const,
};

export function useMenuItemsQuery() {
  return useQuery({ queryKey: queryKeys.menuItems, queryFn: listMenuItems });
}

export function useTablesQuery() {
  return useQuery({ queryKey: queryKeys.tables, queryFn: listTables });
}

export function useOrdersQuery() {
  return useQuery({ queryKey: queryKeys.orders, queryFn: listOrders });
}
