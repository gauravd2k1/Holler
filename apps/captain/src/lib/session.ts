/**
 * The device token for this captain page.
 *
 * IN localStorage, DELIBERATELY, UNLIKE apps/admin's in-memory session.
 * docs/captain-api.md's pair screen says so in as many words: "paste the
 * device token once, store it in localStorage". This is a different threat
 * model from a human back-office login — it is a device credential enrolled
 * once and expected to survive a phone's browser being closed and reopened at
 * the next table, not a person's session on a shared machine.
 */

const STORAGE_KEY = "holler_captain_device_token";

export function storedDeviceToken(): string | null {
  return window.localStorage.getItem(STORAGE_KEY);
}

export function storeDeviceToken(token: string): void {
  window.localStorage.setItem(STORAGE_KEY, token);
}

export function clearDeviceToken(): void {
  window.localStorage.removeItem(STORAGE_KEY);
}

export function authHeader(token: string): Record<string, string> {
  return { authorization: `Bearer ${token}` };
}
