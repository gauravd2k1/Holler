// Minimal type-aware lint setup (T13, docs/retro.md 2026-08-11).
//
// This enables `@typescript-eslint/unbound-method`. Read this before trusting
// it for more than it covers:
//
// WHAT IT CATCHES: a *class method* referenced without its receiver — e.g.
// `const fn = this.doThing; later(fn)`, which silently loses `this` when
// `fn` is eventually called. That is a real, if different, bug shape, and
// this app is currently clean under it — worth keeping for that alone.
//
// WHAT IT DOES NOT CATCH: the sibling KDS bug this task exists because of
// (apps/kds/src/lib/connectionController.ts, fixed in 1f31e98) — storing a
// bare global builtin (`setInterval`, `fetch`, `WebSocket`, ...) on an
// instance field without `.bind`, which detaches it from its receiver and
// throws "Illegal invocation" only in a real browser. Verified directly
// against that file: reverting its `.bind(globalThis)` fix and re-running
// this same lint config produces ZERO errors. The reason is structural, not
// a misconfiguration — TypeScript's `lib.dom.d.ts` declares `setInterval` and
// friends as global **function declarations**, not as methods on an
// interface. `unbound-method` reasons about method access on a typed
// receiver; a bare function has no receiver for it to check, so this class of
// bug is invisible to it regardless of strictness settings.
//
// This app has no browser smoke test (out of scope for T13 — KDS only), so
// if this app ever stores a global builtin on a field the way the KDS did,
// nothing here would catch it either. Noted as an open risk, not fixed here.
//
// Deliberately no stylistic rules beyond `unbound-method` — the goal here is
// this one rule enforced, not a house style debate.
//
// src-tauri/ is Rust and out of scope for this config entirely.
import tseslint from "typescript-eslint";

export default tseslint.config(
  {
    ignores: ["dist/**", "node_modules/**", "src-tauri/**"],
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
