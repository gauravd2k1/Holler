// UPI QR for the bill amount (T8, demo build only).
//
// There is no UPI payee address anywhere in the frozen contract set
// (v0.8.1) — `outlet_fiscal_profile` and every other config shape carry no
// VPA field, and none is being added here: contracts are FROZEN and the
// demo needs no contract change. For the demo build the payee is read from
// build-time configuration instead.
//
// Vite only exposes client-side env vars prefixed `VITE_` on
// `import.meta.env` (`vite.config.ts` is outside this task's owned paths,
// so that prefix rule cannot be widened here). The demo brief names the two
// variables `HOLLER_DEMO_UPI_VPA` / `HOLLER_DEMO_UPI_PAYEE_NAME`; this app
// reads them as `VITE_HOLLER_DEMO_UPI_VPA` / `VITE_HOLLER_DEMO_UPI_PAYEE_NAME`
// so Vite actually bundles them — set them in `apps/pos/.env.local`
// (untracked) before building for a demo. This is a demo-build
// accommodation, not the intended production shape: production would carry
// a payee VPA as outlet config, not a build-time env var.
//
// This is NOT a payment integration. Nothing reads this QR back — no code
// path reconciles a scan against a received payment. The cashier still
// records the tender exactly as before; the QR only gives a customer's UPI
// app something to point at.

import { formatPaiseAsPlainDecimal } from "./money";

export interface UpiDemoPayee {
  vpa: string;
  payeeName: string;
}

const VPA_ENV_KEY = "VITE_HOLLER_DEMO_UPI_VPA";
const PAYEE_NAME_ENV_KEY = "VITE_HOLLER_DEMO_UPI_PAYEE_NAME";

/**
 * Reads the demo-build UPI payee from Vite's build-time env.
 *
 * Returns `null` when no VPA is configured (unset or blank after trimming)
 * — callers must render NO QR in that case. A QR pointing at an empty or
 * wrong payee is worse than no QR at a client demo: it opens a real payment
 * app on a customer's phone aimed at nobody. Fail visibly absent, never
 * silently wrong.
 *
 * `env` defaults to `import.meta.env` and is only ever overridden in tests
 * — `import.meta.env` is a build-time-substituted object, not a bindable
 * global, so no detached-method concern applies here.
 */
export function readUpiDemoPayee(
  env: Record<string, string | boolean | undefined> = import.meta.env,
): UpiDemoPayee | null {
  const vpa = (typeof env[VPA_ENV_KEY] === "string" ? env[VPA_ENV_KEY] : "").trim();
  if (vpa === "") return null;
  const configuredName = (
    typeof env[PAYEE_NAME_ENV_KEY] === "string" ? env[PAYEE_NAME_ENV_KEY] : ""
  ).trim();
  // No outlet-name query exists anywhere in this app today (the principal
  // carries only `outlet_id`, a UUID never shown to a user) — so the "falls
  // back to the outlet name" rule from the brief has no data source to fall
  // back to here. The closest available fallback is the VPA itself, which
  // is always present and always a legitimate (if unfriendly) display name.
  return { vpa, payeeName: configuredName !== "" ? configuredName : vpa };
}

export interface UpiPaymentLinkParams {
  vpa: string;
  payeeName: string;
  /** The exact invoice total, in integer paise — never a float. */
  amountPaise: number;
  /** The human-facing invoice/order number. Never a UUID (CLAUDE.md
   * §Money/time/identifiers) — mirrors the binding rule already enforced on
   * the print path by `require_order_display_number`
   * (`edge/printer/src/template.rs`). */
  note: string;
}

/**
 * Builds the standard UPI deep link for a fixed amount:
 * `upi://pay?pa=<vpa>&pn=<payee name>&am=<amount>&cu=INR&tn=<note>`.
 *
 * Every parameter is URL-encoded individually — an unencoded payee name
 * containing a space silently truncates the link in some UPI apps. The
 * amount is converted from integer paise to a plain two-decimal rupee
 * string using integer arithmetic only (`formatPaiseAsPlainDecimal`),
 * never a float division.
 */
export function buildUpiPaymentLink(params: UpiPaymentLinkParams): string {
  const { vpa, payeeName, amountPaise, note } = params;
  const amount = formatPaiseAsPlainDecimal(amountPaise);
  const query = [
    `pa=${encodeURIComponent(vpa)}`,
    `pn=${encodeURIComponent(payeeName)}`,
    `am=${encodeURIComponent(amount)}`,
    `cu=INR`,
    `tn=${encodeURIComponent(note)}`,
  ].join("&");
  return `upi://pay?${query}`;
}
