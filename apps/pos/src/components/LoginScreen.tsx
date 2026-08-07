import { useState } from "react";
import { useNavigate } from "@tanstack/react-router";
import { login, isTauriCommandError } from "../lib/tauri";
import { useAuthStore } from "../store/auth";

// Offline login (ADR-011): this screen never checks network state and never
// disables itself for being offline — the `login` Tauri command verifies
// against the locally cached user row regardless of connectivity.
export function LoginScreen() {
  const navigate = useNavigate();
  const setPrincipal = useAuthStore((s) => s.setPrincipal);
  const [email, setEmail] = useState("");
  const [password, setPassword] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

  async function handleSubmit(e: React.FormEvent) {
    e.preventDefault();
    setError(null);
    setSubmitting(true);
    try {
      const principal = await login(email, password);
      setPrincipal(principal);
      void navigate({ to: "/" });
    } catch (err) {
      const message = isTauriCommandError(err) ? err.message : "Login failed. Try again.";
      setError(message);
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <main className="login-screen">
      <form className="login-card" onSubmit={handleSubmit}>
        <h1>Holler POS</h1>
        <label htmlFor="login-email">Email</label>
        <input
          id="login-email"
          type="email"
          autoComplete="username"
          value={email}
          onChange={(e) => setEmail(e.target.value)}
          required
        />
        <label htmlFor="login-password">Password</label>
        <input
          id="login-password"
          type="password"
          autoComplete="current-password"
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          required
        />
        {error && (
          <p className="login-error" role="alert">
            {error}
          </p>
        )}
        <button type="submit" disabled={submitting}>
          {submitting ? "Signing in…" : "Sign in"}
        </button>
      </form>
    </main>
  );
}
