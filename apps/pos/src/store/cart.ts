import { create } from "zustand";
import type { OrderType } from "@holler/contracts";
import type { CartLine } from "../domain/cart";

interface CartState {
  orderType: OrderType;
  tableId: string | null;
  lines: CartLine[];
  setOrderType: (orderType: OrderType) => void;
  setTableId: (tableId: string | null) => void;
  addLine: (line: Omit<CartLine, "lineId">) => void;
  removeLine: (lineId: string) => void;
  setQuantity: (lineId: string, quantity: number) => void;
  clear: () => void;
}

let lineCounter = 0;
function nextLineId(): string {
  lineCounter += 1;
  return `cart-line-${lineCounter}`;
}

export const useCartStore = create<CartState>((set) => ({
  orderType: "DINE_IN",
  tableId: null,
  lines: [],
  setOrderType: (orderType) => set({ orderType }),
  setTableId: (tableId) => set({ tableId }),
  addLine: (line) =>
    set((state) => {
      const existing = state.lines.find(
        (l) => l.menuItemId === line.menuItemId && l.variantId === line.variantId && l.notes === line.notes,
      );
      if (existing) {
        return {
          lines: state.lines.map((l) =>
            l.lineId === existing.lineId ? { ...l, quantity: l.quantity + line.quantity } : l,
          ),
        };
      }
      return { lines: [...state.lines, { ...line, lineId: nextLineId() }] };
    }),
  removeLine: (lineId) => set((state) => ({ lines: state.lines.filter((l) => l.lineId !== lineId) })),
  setQuantity: (lineId, quantity) =>
    set((state) => ({
      lines:
        quantity <= 0
          ? state.lines.filter((l) => l.lineId !== lineId)
          : state.lines.map((l) => (l.lineId === lineId ? { ...l, quantity } : l)),
    })),
  clear: () => set({ lines: [], tableId: null }),
}));
