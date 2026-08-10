import type { ConnectionStatus } from "../store/kdsStore";

// Always visible, never colour-only: text state accompanies whatever colour
// backs the banner (task requirement #3/#4 — staleness must be visible, not
// silent, and never colour-only).
const LABEL: Record<ConnectionStatus, string> = {
  connecting: "Connecting to kitchen…",
  connected: "Connected",
  stale: "No update from kitchen system — check connection",
  disconnected: "Disconnected from kitchen system",
};

export function ConnectionBanner({ status }: { status: ConnectionStatus }) {
  if (status === "connected") return null;
  return (
    <div role="status" className={`connection-banner connection-banner--${status}`}>
      {LABEL[status]}
    </div>
  );
}
