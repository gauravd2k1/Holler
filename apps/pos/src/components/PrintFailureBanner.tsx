import { useState } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useFailedPrintJobsQuery, queryKeys } from "../lib/queries";
import { retryFailedPrintJobs, isTauriCommandError } from "../lib/tauri";

// docs/spec/hardware-printing.md: "Print failures must be visible to
// staff." A ticket that silently failed to print is the failure mode this
// exists to prevent — rendered as a fixed, impossible-to-miss banner on
// every authenticated screen, not a badge tucked into a corner.
export function PrintFailureBanner() {
  const queryClient = useQueryClient();
  const failedQuery = useFailedPrintJobsQuery();
  const [retrying, setRetrying] = useState(false);
  const [retryError, setRetryError] = useState<string | null>(null);

  const failed = failedQuery.data ?? [];
  if (failed.length === 0) return null;

  async function handleRetry() {
    setRetrying(true);
    setRetryError(null);
    try {
      await retryFailedPrintJobs();
      await queryClient.invalidateQueries({ queryKey: queryKeys.failedPrintJobs });
    } catch (err) {
      setRetryError(isTauriCommandError(err) ? err.message : "Retry failed.");
    } finally {
      setRetrying(false);
    }
  }

  return (
    <div className="print-failure-banner" role="alert">
      <span className="print-failure-summary">
        {failed.length} ticket{failed.length === 1 ? "" : "s"} failed to print
      </span>
      <ul className="print-failure-list">
        {failed.map((job) => (
          <li key={job.id}>
            {job.kot_station} · {job.printer_name} · {job.attempt_count} attempt
            {job.attempt_count === 1 ? "" : "s"} · {job.last_error ?? "unknown error"}
          </li>
        ))}
      </ul>
      {retryError && <span className="print-failure-retry-error">{retryError}</span>}
      <button type="button" onClick={() => void handleRetry()} disabled={retrying}>
        {retrying ? "Retrying…" : "Retry now"}
      </button>
    </div>
  );
}
