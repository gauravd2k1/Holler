import {
  useBlockedOutboxRowsQuery,
  usePersistentlyFailingOutboxRowsQuery,
} from "../lib/queries";

// M6 A3, and the half of M6 C7 that no wire format can satisfy: the criterion
// asks for a reason the edge RECORDS, which is only true if a person can read
// it back. Follows the `PrintFailureBanner` precedent — a fixed banner on the
// screens a cashier already looks at, not a badge in a corner or a report
// nobody opens.
//
// TWO CONDITIONS, DELIBERATELY DIFFERENT WORDS, because they need different
// actions:
//
//   - GIVEN UP ON (`blocked_at` set). The retry budget is spent and the till
//     will not try again. This part of the trading day will never reach the
//     cloud without someone intervening.
//   - STILL TRYING (`blocked_at` null, attempts high). A transient failure
//     never spends the budget — abandoning good rows because the cloud was
//     down for a day is data loss dressed as resilience — so these rows are
//     retried indefinitely and would otherwise be invisible. A cloud that has
//     been refusing everything since Tuesday must not look like a quiet
//     evening.
//
// Halting sync is survivable. Halting it silently is not, and M5 ended with
// 120 rows pending on a till that reported itself healthy.
export function SyncBlockedBanner() {
  const blockedQuery = useBlockedOutboxRowsQuery();
  const failingQuery = usePersistentlyFailingOutboxRowsQuery();

  const blocked = blockedQuery.data ?? [];
  const failing = failingQuery.data ?? [];
  if (blocked.length === 0 && failing.length === 0) return null;

  return (
    <div className="sync-blocked-banner" role="alert">
      {blocked.length > 0 && (
        <>
          <span className="sync-blocked-summary">
            {blocked.length} record{blocked.length === 1 ? "" : "s"} will not reach the cloud
          </span>
          <ul className="sync-blocked-list">
            {blocked.map((row) => (
              <li key={row.outbox_id}>
                <span className="sync-blocked-aggregate">
                  {row.aggregate_type} {row.aggregate_id}
                </span>{" "}
                · {row.attempts} attempt{row.attempts === 1 ? "" : "s"} ·{" "}
                {/* The machine-readable code first: it is stable, and it is
                    what the cloud actually said. The prose is a fallback for
                    a failure that never reached the wire. */}
                {row.last_code ?? row.last_error}
                {row.last_status !== null && ` (HTTP ${row.last_status})`}
              </li>
            ))}
          </ul>
          <span className="sync-blocked-action">
            Nothing is lost locally — these need someone to look at them.
          </span>
        </>
      )}
      {failing.length > 0 && (
        <span className="sync-failing-summary">
          {failing.length} record{failing.length === 1 ? "" : "s"} still retrying after repeated
          failures
        </span>
      )}
    </div>
  );
}
