import { useQuery } from "@tanstack/react-query";
import {
  getCashShift,
  getOrder,
  getStockCount,
  getStockCountVarianceReport,
  listBlockedReplays,
  listCurrentStock,
  listDiscountDefinitions,
  listFailedPrintJobs,
  listInvoicesForOrder,
  listKotsForOrder,
  listMenuCategories,
  listMenuItems,
  listMenuItemVariants,
  listOrders,
  listPaymentsForOrder,
  listStations,
  listStockCountLines,
  listStockDeductionGaps,
  listTables,
} from "./tauri";

// Query keys centralised here so cache invalidation after a write stays
// consistent across the app.
export const queryKeys = {
  menuItems: ["menu-items"] as const,
  menuCategories: ["menu-categories"] as const,
  menuItemVariants: ["menu-item-variants"] as const,
  tables: ["tables"] as const,
  orders: ["orders"] as const,
  order: (orderId: string) => ["order", orderId] as const,
  stations: ["stations"] as const,
  kots: (orderId: string) => ["kots", orderId] as const,
  failedPrintJobs: ["failed-print-jobs"] as const,
  invoices: (orderId: string) => ["invoices", orderId] as const,
  payments: (orderId: string) => ["payments", orderId] as const,
  cashShift: (cashShiftId: string) => ["cash-shift", cashShiftId] as const,
  discountDefinitions: ["discount-definitions"] as const,
  currentStock: ["current-stock"] as const,
  stockDeductionGaps: ["stock-deduction-gaps"] as const,
  blockedReplays: ["blocked-replays"] as const,
  stockCount: (stockCountId: string) => ["stock-count", stockCountId] as const,
  stockCountLines: (stockCountId: string) => ["stock-count-lines", stockCountId] as const,
  stockCountVarianceReport: (stockCountId: string) =>
    ["stock-count-variance-report", stockCountId] as const,
};

export function useMenuItemsQuery() {
  return useQuery({ queryKey: queryKeys.menuItems, queryFn: listMenuItems });
}

export function useMenuItemVariantsQuery() {
  return useQuery({ queryKey: queryKeys.menuItemVariants, queryFn: listMenuItemVariants });
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

// -------------------------------------------------------------- billing (M3) --

export function useOrderQuery(orderId: string | null) {
  return useQuery({
    queryKey: queryKeys.order(orderId ?? "none"),
    queryFn: () => getOrder(orderId as string),
    enabled: orderId !== null,
  });
}

export function useInvoicesForOrderQuery(orderId: string | null) {
  return useQuery({
    queryKey: queryKeys.invoices(orderId ?? "none"),
    queryFn: () => listInvoicesForOrder(orderId as string),
    enabled: orderId !== null,
  });
}

export function usePaymentsForOrderQuery(orderId: string | null) {
  return useQuery({
    queryKey: queryKeys.payments(orderId ?? "none"),
    queryFn: () => listPaymentsForOrder(orderId as string),
    enabled: orderId !== null,
  });
}

export function useCashShiftQuery(cashShiftId: string | null) {
  return useQuery({
    queryKey: queryKeys.cashShift(cashShiftId ?? "none"),
    queryFn: () => getCashShift(cashShiftId as string),
    enabled: cashShiftId !== null,
  });
}

export function useDiscountDefinitionsQuery() {
  return useQuery({
    queryKey: queryKeys.discountDefinitions,
    queryFn: listDiscountDefinitions,
  });
}

// ------------------------------------------------------------ inventory (M4) --

/** Polled — this milestone has no push channel from the edge's stock ledger
 * to the UI (the same reason `useFailedPrintJobsQuery` polls). A short
 * interval keeps the low-stock signal (M4 acceptance criterion 4) live
 * during service without the cashier needing to navigate anywhere. */
export function useCurrentStockQuery() {
  return useQuery({
    queryKey: queryKeys.currentStock,
    queryFn: listCurrentStock,
    refetchInterval: 15000,
  });
}

export function useStockDeductionGapsQuery() {
  return useQuery({
    queryKey: queryKeys.stockDeductionGaps,
    queryFn: listStockDeductionGaps,
  });
}

/** Stock history this outlet has given up on sending (contracts 0.5.8).
 * Polled on the same cadence as the rest of the inventory surfaces — a
 * blocked entry is not urgent to the second, but it must not require anyone
 * to go looking for it either. */
export function useBlockedReplaysQuery() {
  return useQuery({
    queryKey: queryKeys.blockedReplays,
    queryFn: listBlockedReplays,
  });
}

export function useStockCountQuery(stockCountId: string | null) {
  return useQuery({
    queryKey: queryKeys.stockCount(stockCountId ?? "none"),
    queryFn: () => getStockCount(stockCountId as string),
    enabled: stockCountId !== null,
  });
}

export function useStockCountLinesQuery(stockCountId: string | null) {
  return useQuery({
    queryKey: queryKeys.stockCountLines(stockCountId ?? "none"),
    queryFn: () => listStockCountLines(stockCountId as string),
    enabled: stockCountId !== null,
  });
}

export function useStockCountVarianceReportQuery(stockCountId: string | null) {
  return useQuery({
    queryKey: queryKeys.stockCountVarianceReport(stockCountId ?? "none"),
    queryFn: () => getStockCountVarianceReport(stockCountId as string),
    enabled: stockCountId !== null,
  });
}
