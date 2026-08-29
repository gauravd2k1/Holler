import { expect, test } from "@playwright/test";

// The whole test: drive `pnpm dev`, load the page, assert nothing threw.
// See `playwright.smoke.config.ts` for why those three words are each
// load-bearing and what happens when one is relaxed.

/** Console messages that are noise rather than a defect.
 *
 * DELIBERATELY EMPTY, and it should stay that way. Every entry added here is
 * a class of browser error this test stops being able to see — which is the
 * whole of its value. If something legitimately noisy appears, fix the noise
 * rather than filtering it, and if it truly cannot be fixed, add it here with
 * the reason and a trigger for removing it again. An exemption that outlives
 * its reason is a silenced failure. */
const IGNORED_CONSOLE_PATTERNS: RegExp[] = [];

function isIgnored(text: string): boolean {
  return IGNORED_CONSOLE_PATTERNS.some((p) => p.test(text));
}

test("the POS boots in a real browser against the dev server with nothing thrown", async ({
  page,
}) => {
  const problems: string[] = [];
  page.on("console", (message) => {
    if (message.type() !== "error") return;
    const text = message.text();
    if (!isIgnored(text)) problems.push(`console.error: ${text}`);
  });
  // A module-linking failure ("does not provide an export named X") and a
  // detached-global `Illegal invocation` both arrive here, not as a console
  // message, so both listeners are required.
  page.on("pageerror", (error) => {
    problems.push(`pageerror: ${error.message}`);
  });
  // A failed request for a prebundled dependency is the stale-cache symptom
  // one layer down from the export error, and the retro's standing advice is
  // to check the Network tab and not only the console.
  page.on("requestfailed", (request) => {
    problems.push(`requestfailed: ${request.url()} — ${request.failure()?.errorText ?? "unknown"}`);
  });

  await page.goto("/", { waitUntil: "networkidle" });

  // ORDER MATTERS HERE. The error assertion comes FIRST so that when a module
  // fails to link, the failure message names the actual cause — "does not
  // provide an export named 'GrnGapReasonSchema'" — instead of the symptom a
  // reader then has to diagnose backwards from ("waiting for heading"). Both
  // failures are the same incident; only one of them tells you what to fix.
  // `networkidle` above means anything thrown during module evaluation has
  // already arrived by this line.
  expect(problems, `browser reported ${problems.length} problem(s)`).toEqual([]);

  // An unauthenticated boot lands on the login screen. Asserting on rendered
  // text — not merely on a 200 — is the point: A WHITE SCREEN THROWS NOTHING.
  // A React render that bails without an exception, or an entry module that
  // never runs, produces a blank page and a clean console; only this line
  // sees that.
  await expect(page.getByRole("heading", { name: "Holler POS" })).toBeVisible();

  // Re-checked after the render, because a detached global builtin (the
  // 2026-08-11 KDS `Illegal invocation`) throws when a handler FIRES, which
  // can be after first paint.

  // Loading `/` links the WHOLE module graph, not just the login screen:
  // `main.tsx` imports the router, and `routes/router.tsx` statically imports
  // every screen in the app — billing, inventory and the M5 procurement
  // screens included. A bad import anywhere in that graph fails here, which
  // is why one page load is sufficient coverage for this defect class and
  // why this file must not grow into a navigation suite.
  expect(problems, `browser reported ${problems.length} problem(s)`).toEqual([]);
});
