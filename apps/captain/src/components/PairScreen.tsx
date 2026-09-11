import { useState } from "react";
import { fetchSession, ApiError, type Session } from "../lib/api";
import { storeDeviceToken } from "../lib/session";

interface Props {
  onPaired: (token: string, session: Session) => void;
}

/**
 * Pair screen. Appears only when there is no stored token or the stored one
 * was refused (docs/captain-api.md). Validates against GET /api/session before
 * storing anything, so a mistyped token is never persisted as though it worked.
 */
export function PairScreen({ onPaired }: Props) {
  const [token, setToken] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function handlePair() {
    const trimmed = token.trim();
    if (trimmed === "") return;
    setBusy(true);
    setError(null);
    try {
      const session = await fetchSession(trimmed);
      storeDeviceToken(trimmed);
      onPaired(trimmed, session);
    } catch (err) {
      if (err instanceof ApiError && err.status === 401) {
        setError("That device token was rejected. Check it and paste it again.");
      } else {
        setError(`Could not reach the till: ${err instanceof Error ? err.message : String(err)}`);
      }
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="screen">
      <h2>Pair this phone</h2>
      <p>Paste the device token issued for this waiter device.</p>
      <textarea
        className="big-input"
        rows={4}
        value={token}
        onChange={(e) => setToken(e.target.value)}
        placeholder="credential_id.secret"
        autoCapitalize="off"
        autoCorrect="off"
        spellCheck={false}
      />
      {error !== null && <p className="error">{error}</p>}
      <button
        type="button"
        className="big-button"
        disabled={busy || token.trim() === ""}
        onClick={() => {
          handlePair().catch(() => undefined);
        }}
      >
        {busy ? "Checking…" : "Pair"}
      </button>
    </div>
  );
}
