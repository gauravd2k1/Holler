// Minimal type-aware lint setup (T13, docs/retro.md 2026-08-11).
//
// This enables `@typescript-eslint/unbound-method`. Read this before trusting
// it for more than it covers:
//
// WHAT IT CATCHES: a *class method* referenced without its receiver — e.g.
// `const fn = this.doThing; later(fn)`, which silently loses `this` when
// `fn` is eventually called. That is a real, if different, bug shape, and
// both apps are currently clean under it — worth keeping for that alone.
//
// WHAT IT DOES NOT CATCH: the KDS bug this task exists because of —
// `this.setIntervalFn = deps.setIntervalFn ?? setInterval` (no `.bind`),
// storing the bare global `setInterval`/`clearInterval`/`fetch`/`WebSocket`
// on an instance field, detaching it from its receiver, which throws
// "Illegal invocation" only in a real browser. Verified directly: reverting
// the `.bind(globalThis)` fix in connectionController.ts and re-running this
// lint config produces ZERO errors. The reason is structural, not a
// misconfiguration — TypeScript's `lib.dom.d.ts` declares `setInterval` and
// friends as global **function declarations**, not as methods on an
// interface. `unbound-method` reasons about method access on a typed
// receiver; a bare function has no receiver for it to check, so this class of
// bug is invisible to it regardless of strictness settings.
//
// Prevention for THAT bug is the Playwright smoke test
// (apps/kds/tests/smoke.spec.ts), not this rule — see its regression proof.
// A grep-style check analogous to scripts/check-event-type-drift.mjs (flag
// `= <identifier>;` / `?? <identifier>` assignments of a known global-builtin
// name to a class field without a trailing `.bind(`) was considered and not
// built: the browser smoke test already catches this exact failure mode
// end-to-end, and a pattern-matched grep would be narrower coverage for
// marginal benefit. Revisit if a bare-global-on-a-field bug ships again
// without a corresponding browser test around it.
//
// Deliberately no stylistic rules beyond `unbound-method` — the goal here is
// this one rule enforced, not a house style debate.
import tseslint from "typescript-eslint";

export default tseslint.config(
  {
    ignores: ["dist/**", "node_modules/**"],
  },
  {
    files: ["src/**/*.{ts,tsx}"],
    plugins: {
      "@typescript-eslint": tseslint.plugin,
    },
    languageOptions: {
      parser: tseslint.parser,
      parserOptions: {
        project: ["./tsconfig.json"],
        tsconfigRootDir: import.meta.dirname,
      },
    },
    rules: {
      "@typescript-eslint/unbound-method": "error",
    },
  },
);
