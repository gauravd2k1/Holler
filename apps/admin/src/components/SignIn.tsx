import { useState } from "react";
import { apiBaseUrl, outletId, tenantId } from "../lib/api";
import { signIn, type Principal } from "../lib/session";

/**
 * Sign-in for the back office.
 *
 * The failure message is deliberately the same for a wrong password and a
 * throttled attempt (ADR-012). That is a security decision, not a missing
 * feature: a distinguishable throttle response is an account-enumeration
 * oracle and tells an attacker exactly when to back off. It is recorded in
 * docs/backlog.md as decided — do not "fix" it by reading the status code.
 */
export function SignIn({ onSignedIn }: { onSignedIn: (p: Principal) => void }) {
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        setBusy(true);
        signIn(apiBaseUrl(), tenantId(), outletId(), email, password)
          .then((p) => {
            setError(null);
            onSignedIn(p);
          })
          .catch((err: unknown) => setError(err instanceof Error ? err.message : String(err)))
          .finally(() => setBusy(false));
      }}
    >
      <h1>Holler Admin</h1>
      <label>
        Email
        <input type="email" value={email} onChange={(e) => setEmail(e.target.value)} required />
      </label>
      <label>
        Password
        <input
          type="password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          required
        />
      </label>
      <button type="submit" disabled={busy}>
        {busy ? "Signing in…" : "Sign in"}
      </button>
      {error !== null && <p className="error">{error}</p>}
    </form>
  );
}
