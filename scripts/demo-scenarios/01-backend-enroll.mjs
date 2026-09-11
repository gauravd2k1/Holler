/**
 * Stage 1 — backend auth and WAITER device enrolment against the live 8080.
 *
 * The stack has a POS and a KDS device enrolled and no WAITER at all, so the
 * captain surface cannot be driven until one exists. Enrolling one is data
 * created through the product's own API, which is the only write this suite is
 * permitted to make against the operator's environment.
 *
 * The issued token is written to the untracked .scenario-results/secrets.json
 * and NEVER to a result row, a screenshot or the sheet — only its fingerprint.
 */
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import {
  BACKEND, CASHIER, OUTLET_ID, OWNER, RESULT_DIR, TENANT_ID,
  fingerprint, http, loadState, login, record, saveState, sql,
} from "./lib.mjs";

const run = async () => {
  // ---------------------------------------------------------------- S-BE-01
  const health = await http(`${BACKEND}/health`);
  record({
    id: "S-BE-01",
    demoStep: "pre",
    scenario: "Backend API answers on 8080",
    surface: "backend 8080",
    precondition: "Operator's stack up; backend pid 69896",
    steps: "GET /health",
    expected: "2xx",
    actual: `HTTP ${health.status} ${health.text.slice(0, 120)}`,
    status: health.status >= 200 && health.status < 300 ? "PASS" : "FAIL",
    evidence: `HTTP ${health.status}`,
  });

  // ---------------------------------------------------------------- S-BE-03
  // Ordered before the wrong-password falsifier on purpose: the login budget
  // is five attempts per fifteen minutes per IP and the real sign-ins must
  // not be the ones that fall off the end of it.
  const cashier = await login(CASHIER);
  record({
    id: "S-BE-03",
    demoStep: "pre",
    scenario: "Cashier signs in and receives an access token",
    surface: "backend 8080",
    precondition: "X-Tenant-ID + outlet_id supplied",
    steps: "POST /auth/login cashier@holler.test",
    expected: "200 with access_token and a principal",
    actual: `HTTP ${cashier.status}; principal=${cashier.body?.principal?.email ?? "n/a"}`,
    status: cashier.status === 200 && cashier.body?.access_token ? "PASS" : "FAIL",
    evidence: `HTTP ${cashier.status}, principal ${cashier.body?.principal?.email ?? "n/a"}`,
  });

  // ---------------------------------------------------------------- S-BE-04
  const owner = await login(OWNER);
  record({
    id: "S-BE-04",
    demoStep: "pre",
    scenario: "Owner signs in (needed for outlet.manage device enrolment)",
    surface: "backend 8080",
    precondition: "owner@holler.test exists",
    steps: "POST /auth/login owner@holler.test",
    expected: "200 with access_token",
    actual: `HTTP ${owner.status}; principal=${owner.body?.principal?.email ?? "n/a"}`,
    status: owner.status === 200 && owner.body?.access_token ? "PASS" : "FAIL",
    evidence: `HTTP ${owner.status}`,
  });

  // ---------------------------------------------------------------- S-BE-02
  const badLogin = await login({ email: CASHIER.email, password: "wrong-password" }, { cache: false });
  record({
    id: "S-BE-02",
    demoStep: "pre",
    scenario: "Login with a wrong password is refused",
    surface: "backend 8080",
    precondition: "cashier@holler.test exists; S-BE-03 already holds a good token",
    steps: "POST /auth/login with a deliberately wrong password",
    expected: "401, no access token",
    actual: `HTTP ${badLogin.status}; token present=${Boolean(badLogin.body?.access_token)}`,
    status: badLogin.status === 401 && !badLogin.body?.access_token ? "PASS" : "FAIL",
    evidence: `HTTP ${badLogin.status} body ${badLogin.text.slice(0, 80)}`,
    notes: "Falsifier for S-BE-03: a login endpoint that accepts anything would pass S-BE-03 too.",
  });

  // ---------------------------------------------------------------- S-BE-09
  // Observed during this run, not constructed: six login attempts inside one
  // fifteen-minute window from this machine, and every attempt from the sixth
  // on returned {"code":"unauthorized","message":"authentication required"} —
  // byte-identical to a wrong password. The limit is real and intended
  // (ADR-012, ratelimit.go:16-17); the DEMO RISK is that it is unreadable.
  record({
    id: "S-BE-09",
    demoStep: "pre",
    scenario: "A rate-limited login is indistinguishable from a wrong password",
    surface: "backend 8080",
    precondition: "LoginRateLimitAttempts=5 per LoginRateLimitWindow=15m, keyed on client IP",
    steps:
      "Sign in repeatedly from one IP until the budget is spent, then sign in with the CORRECT password",
    expected:
      "Some signal a human can act on — 429, Retry-After, or a distinct code",
    actual:
      'HTTP 401 {"code":"unauthorized","message":"authentication required"} for a correct password, ' +
      "identical to the wrong-password body; no Retry-After; recovery only by waiting out the window",
    status: "FAIL",
    evidence:
      "backend/internal/auth/ratelimit.go:16-17; observed 2026-09-11 — correct owner password refused 4× in a row " +
      "after the budget was spent, then accepted once the window rolled",
    notes:
      "ADR-012 chose the identical body deliberately so the endpoint leaks nothing, so this is a UX gap not a " +
      "security defect. Demo risk: a few fumbled sign-ins locks the admin console for 15 minutes and the screen " +
      "says the password is wrong. CLAUDE.md already records this exact misread costing a debugging detour.",
  });

  const token = owner.body?.access_token ?? cashier.body?.access_token;
  if (!token) {
    record({
      id: "S-BE-05",
      demoStep: "1a",
      scenario: "Enrol a WAITER device",
      surface: "backend 8080",
      precondition: "An authenticated principal with outlet.manage",
      steps: "POST /devices/enroll",
      expected: "201 with device_id + one-time token",
      actual: "No access token obtained; cannot reach the enrolment route",
      status: "BLOCKED",
      evidence: "S-BE-03/04 both failed",
    });
    return;
  }
  const authed = { authorization: `Bearer ${token}`, "content-type": "application/json" };

  // -------------------------------------------------- S-BE-05 pre-condition
  // Captured ONCE and persisted. A re-run of this script must not report the
  // device it enrolled on the first run as a pre-existing one — that would
  // turn a correct precondition into a false negative about the environment.
  const prior = loadState();
  const before = prior.waiterBaselineCount ?? sql("select count(*) from device where kind='WAITER';");
  saveState({ waiterBaselineCount: before });
  record({
    id: "S-BE-05",
    demoStep: "1a",
    scenario: "Precondition: no WAITER device is enrolled at this outlet",
    surface: "postgres 5432",
    precondition: "Operator's stack as handed over",
    steps: "select count(*) from device where kind='WAITER'",
    expected: "0 — so a captain pairing cannot already be working by accident",
    actual: `count=${before}`,
    status: before === "0" ? "PASS" : "FAIL",
    evidence: `SQL count(device kind=WAITER) = ${before}`,
    notes: "Establishes that S-CAP-* pairing evidence comes from the device this run enrolled.",
  });

  // ---------------------------------------------------------------- S-BE-06
  // Re-runs reuse the credential already issued. The plaintext token is
  // returned ONCE at enrolment (ADR-017), so enrolling again on every run
  // would leave a trail of dead WAITER devices in the operator's outlet.
  const secretsPath = join(RESULT_DIR, "secrets.json");
  const reuse = existsSync(secretsPath) && prior.waiterDeviceId;
  const kept = reuse ? JSON.parse(readFileSync(secretsPath, "utf8")) : null;
  let deviceToken, deviceId;
  if (reuse) {
    deviceToken = kept.deviceToken;
    deviceId = prior.waiterDeviceId;
    writeFileSync(secretsPath, JSON.stringify({ deviceToken, jwt: token }, null, 2), "utf8");
    record({
      id: "S-BE-06",
      demoStep: "1a",
      scenario: "Enrol a WAITER device and receive a one-time token",
      surface: "backend 8080",
      precondition: "Owner JWT; outlet 0191a000…000a",
      steps: "Reused the credential issued by the first run of this script",
      expected: "201 with device_id, credential_id and a plaintext token returned once",
      actual: `Reused device_id=${prior.waiterDeviceId}; token fingerprint=${fingerprint(kept.deviceToken)}`,
      status: "PASS",
      evidence: `device_id ${prior.waiterDeviceId}, token ${fingerprint(kept.deviceToken)}`,
      notes: "Token is one-time at enrolment; re-runs must not enrol a second device.",
    });
  } else {
    const enroll = await http(`${BACKEND}/devices/enroll`, {
      method: "POST",
      headers: authed,
      body: JSON.stringify({
        outlet_id: OUTLET_ID,
        kind: "WAITER",
        name: "Scenario Suite Captain",
        label: "t27-scenario-run",
      }),
    });
    deviceToken = enroll.body?.token ?? null;
    deviceId = enroll.body?.device_id ?? null;
    record({
      id: "S-BE-06",
      demoStep: "1a",
      scenario: "Enrol a WAITER device and receive a one-time token",
      surface: "backend 8080",
      precondition: "Owner JWT; outlet 0191a000…000a; zero WAITER devices (S-BE-05)",
      steps: 'POST /devices/enroll {outlet_id, kind:"WAITER", name, label}',
      expected: "201 with device_id, credential_id and a plaintext token returned once",
      actual: `HTTP ${enroll.status}; device_id=${deviceId ?? "n/a"}; token fingerprint=${fingerprint(deviceToken)}`,
      status: enroll.status === 201 && deviceToken ? "PASS" : "FAIL",
      evidence: `HTTP ${enroll.status}, device_id ${deviceId ?? "n/a"}, token ${fingerprint(deviceToken)}`,
      notes: "Token never committed — fingerprint only.",
    });
    if (!deviceToken) return;
    writeFileSync(secretsPath, JSON.stringify({ deviceToken, jwt: token }, null, 2), "utf8");
  }
  saveState({ waiterDeviceId: deviceId, tokenFingerprint: fingerprint(deviceToken), tenantId: TENANT_ID, outletId: OUTLET_ID });

  // ---------------------------------------------------------------- S-BE-07
  const row = sql(`select kind, name from device where id='${deviceId}';`);
  record({
    id: "S-BE-07",
    demoStep: "1a",
    scenario: "The enrolled WAITER device is persisted in the cloud",
    surface: "postgres 5432",
    precondition: "S-BE-06 returned 201",
    steps: "select kind, name from device where id = <new device_id>",
    expected: "One row, kind WAITER",
    actual: `row=${row || "<none>"}`,
    status: row.startsWith("WAITER|") ? "PASS" : "FAIL",
    evidence: `SQL: ${row}`,
  });

  // ---------------------------------------------------------------- S-BE-08
  // The credential must reach the edge's device_credential_cache before the
  // captain listener can verify it offline. That is a cloud→edge config pull.
  const credRow = sql(`select id from device_credential where device_id='${deviceId}';`);
  record({
    id: "S-BE-08",
    demoStep: "1a",
    scenario: "A device_credential row exists cloud-side for the new WAITER",
    surface: "postgres 5432",
    precondition: "S-BE-06",
    steps: "select id from device_credential where device_id = <new device_id>",
    expected: "Exactly one credential row",
    actual: `credential_id=${credRow || "<none>"}`,
    status: credRow.length > 0 ? "PASS" : "FAIL",
    evidence: `SQL rows: ${credRow.split("\n").length}`,
    notes:
      "The edge caches this via GET /sync/config; the captain listener verifies against device_credential_cache, " +
      "so captain pairing cannot work until the A5 config loop has pulled it.",
  });
};

run().catch((err) => {
  console.error(err);
  process.exitCode = 1;
});
