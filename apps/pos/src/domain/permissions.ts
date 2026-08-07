import type { AuthenticatedPrincipal, Permission } from "@holler/contracts";

/**
 * The single place that decides whether a principal may issue an action.
 * UI components must call this before *invoking* a command, not merely
 * before *enabling* a button — a cashier lacking a permission must not be
 * able to trigger the action at all (task requirement).
 */
export function hasPermission(
  principal: AuthenticatedPrincipal | null,
  permission: Permission,
): boolean {
  if (!principal) return false;
  return principal.permissions.includes(permission);
}

export function requirePermission(
  principal: AuthenticatedPrincipal | null,
  permission: Permission,
): void {
  if (!hasPermission(principal, permission)) {
    throw new Error(`missing permission: ${permission}`);
  }
}
