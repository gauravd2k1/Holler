import { z } from "zod";
import {
  AuthenticatedPrincipalSchema,
  type AuthenticatedPrincipal,
} from "@holler/contracts";

/**
 * The access token for this browser session.
 *
 * IN MEMORY, NOT localStorage. A bearer token in localStorage survives the tab,
 * is readable by any script that reaches this origin, and outlives the operator
 * walking away from a back-office machine. Losing the session on refresh is the
 * cost, and it is the cheaper of the two.
 *
 * This module is the only thing that holds it, so there is exactly one place to
 * change when refresh-token rotation lands.
 */
let accessToken: string | null = null;

/**
 * THE PRINCIPAL SHAPE COMES FROM THE CONTRACT, NOT FROM THIS FILE.
 *
 * An earlier version of this module hand-wrote the principal object and got it
 * wrong: it declared an `email` field the principal has never carried, and
 * omitted `full_name`, `authenticated_offline` and `schema_version`. Sign-in
 * then failed at the parse step with `principal.email Required` -- AFTER a
 * successful login, which made it read like an auth fault when it was a
 * contract fault entirely inside the client.
 *
 * `packages/contracts` is the source of truth and it already exported this
 * schema. Re-describing a contract shape by hand is the same defect as a column
 * nothing reads, pointed the other way: a shape described twice is a shape that
 * disagrees with itself the moment one side moves.
 */
const SessionSchema = z.object({
  access_token: z.string(),
  refresh_token: z.string(),
  principal: AuthenticatedPrincipalSchema,
});

export type Principal = AuthenticatedPrincipal;

let principal: Principal | null = null;

export function currentPrincipal(): Principal | null {
  return principal;
}

export function authHeader(): Record<string, string> {
  return accessToken === null ? {} : { authorization: `Bearer ${accessToken}` };
}

export function signOut(): void {
  accessToken = null;
  principal = null;
}

/**
 * POST /auth/login.
 *
 * The tenant travels as `X-Tenant-ID`, which ADR-012 records as a time-boxed
 * interim until host-based resolution replaces it. It is sent only here — every
 * other request in this app takes its tenant from the authenticated principal,
 * never from a header, and that asymmetry is deliberate rather than an
 * oversight to tidy up.
 */
export async function signIn(
  baseUrl: string,
  tenantId: string,
  outletId: string,
  email: string,
  password: string,
): Promise<Principal> {
  const response = await fetch(`${baseUrl}/auth/login`, {
    method: "POST",
    headers: { "content-type": "application/json", "x-tenant-id": tenantId },
    body: JSON.stringify({ email, password, outlet_id: outletId }),
  });

  if (!response.ok) {
    // DELIBERATELY UNDIFFERENTIATED. ADR-012 returns the identical answer for a
    // wrong password and a throttled attempt, because a distinguishable
    // throttle response is an account-enumeration oracle and tells an attacker
    // when to back off. Do not "improve" this message by reading the status.
    throw new Error(
      "Sign-in failed. Check the email and password. Repeated attempts are rate limited, and a limited attempt looks the same as a wrong password.",
    );
  }

  const session = SessionSchema.parse(await response.json());
  accessToken = session.access_token;
  principal = session.principal;
  return session.principal;
}
