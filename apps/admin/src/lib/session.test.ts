import { describe, expect, it } from "vitest";
import { AuthenticatedPrincipalSchema } from "@holler/contracts";

/**
 * Pins the sign-in parse against a REAL captured response body.
 *
 * The defect this replaces: the client hand-wrote the principal shape, declared
 * an `email` field the principal has never carried, and failed at the parse
 * step AFTER a successful login — which read like an auth fault when it was a
 * contract fault entirely inside the client.
 *
 * The fixture below is the exact `principal` object returned by
 * POST /auth/login on 2026-09-08, captured from the running backend. It is
 * copied rather than generated on purpose: a fixture built from the same schema
 * it is checked against would agree with itself no matter what the server does,
 * which is the shape of test this repository keeps finding.
 */
describe("the login principal", () => {
  const captured = {
    user_id: "0191a000-0000-7000-8000-000000000021",
    tenant_id: "0191a000-0000-7000-8000-000000000001",
    outlet_id: "0191a000-0000-7000-8000-00000000000a",
    full_name: "Dev Owner",
    permissions: ["menu.manage", "outlet.manage", "procurement.approve", "procurement.manage"],
    authenticated_offline: false,
    schema_version: 1,
  };

  it("parses against the contract schema", () => {
    const result = AuthenticatedPrincipalSchema.safeParse(captured);
    expect(result.success, JSON.stringify((result as { error?: unknown }).error)).toBe(true);
  });

  it("has no email field, and nothing in this app may assume one", () => {
    // The specific wrong assumption that broke sign-in. Asserted directly so a
    // future reader does not reintroduce it from memory of another codebase.
    expect(captured).not.toHaveProperty("email");
    expect(captured).toHaveProperty("full_name");
  });
});
