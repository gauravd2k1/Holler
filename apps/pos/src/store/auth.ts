import { create } from "zustand";
import type { AuthenticatedPrincipal } from "@holler/contracts";

// Holds only the principal returned by the `login` Tauri command — never
// credential material (task requirement #1). This is in-memory only; a
// fresh app launch requires a fresh offline login.
interface AuthState {
  principal: AuthenticatedPrincipal | null;
  setPrincipal: (principal: AuthenticatedPrincipal) => void;
  logout: () => void;
}

export const useAuthStore = create<AuthState>((set) => ({
  principal: null,
  setPrincipal: (principal) => set({ principal }),
  logout: () => set({ principal: null }),
}));
