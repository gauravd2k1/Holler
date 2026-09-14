import { useEffect, useState } from "react";
import { billOpenSummary, isPerfEnabled, setPerfEnabled } from "../lib/perf";

/**
 * The till's latency readout: tap "Bill" to bill-on-screen, in milliseconds.
 *
 * OFF BY DEFAULT, AND THAT IS NOT A PREFERENCE. A debug box on the till in
 * front of a client is exactly the dev furniture the demo brief forbids. It is
 * rehearsal instrumentation and it should be invisible unless someone asked
 * for it.
 *
 * Ctrl+Alt+P toggles it. The KDS uses `?perf=1`, which is not reachable here:
 * the till is a Tauri window with no address bar. The choice persists, so a
 * rehearsal turns it on once rather than every launch.
 *
 * It polls rather than subscribing. The perf module is a plain array with no
 * store attached, and wiring a subscription into it would put instrumentation
 * into the app's state graph -- where a bug in it could take the till down.
 * A 1s interval that only runs while the overlay is visible cannot.
 */
export function PerfOverlay() {
  const [enabled, setEnabled] = useState(() => isPerfEnabled());
  const [, tick] = useState(0);

  useEffect(() => {
    function onKey(e: KeyboardEvent) {
      if (e.ctrlKey && e.altKey && (e.key === "p" || e.key === "P")) {
        e.preventDefault();
        setEnabled((was) => {
          setPerfEnabled(!was);
          return !was;
        });
      }
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  useEffect(() => {
    if (!enabled) return;
    const timer = setInterval(() => tick((n) => n + 1), 1_000);
    return () => clearInterval(timer);
  }, [enabled]);

  if (!enabled) return null;

  const summary = billOpenSummary();

  return (
    <div className="perf-overlay" role="status">
      <span className="perf-overlay__label">bill open</span>
      {summary.count === 0 ? (
        <span>tap Bill on an order…</span>
      ) : (
        <>
          <span>
            last <strong>{summary.last!.ms} ms</strong>
          </span>
          <span>
            worst <strong>{summary.worstMs} ms</strong>
          </span>
          <span>n={summary.count}</span>
        </>
      )}
      <span className="perf-overlay__note">Ctrl+Alt+P to hide</span>
    </div>
  );
}
