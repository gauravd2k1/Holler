/**
 * The two states every screen on this phone needs and none of them had:
 * a WAIT that looks like a wait, and an EMPTY that says something.
 *
 * A waiter holding a phone across a room reads motion before text. A bare
 * "Loading tables…" line is indistinguishable from a screen that has finished
 * loading and has nothing on it, which is exactly the confusion an empty
 * state exists to prevent.
 */

/** A wait. `label` is read by screen readers and shown beside the spinner. */
export function Waiting({ label }: { label: string }) {
  return (
    <div className="screen waiting" role="status" aria-live="polite">
      <span className="spinner" aria-hidden="true" />
      <span>{label}</span>
    </div>
  );
}

/**
 * An empty result. Always a sentence, never a blank region: "no tables" and
 * "tables not loaded yet" look identical without one.
 */
export function Empty({ title, detail }: { title: string; detail: string }) {
  return (
    <div className="empty-state">
      <p className="empty-state__title">{title}</p>
      <p className="empty-state__detail">{detail}</p>
    </div>
  );
}

/**
 * A failure, in words a waiter can act on.
 *
 * NEVER `String(error)`: on an `Error` that renders "Error: ..." or
 * "ApiError: ...", putting a class name from our own source on a screen in
 * front of a customer. The message is the part written for a human.
 */
export function errorText(error: unknown): string {
  if (error instanceof Error && error.message !== "") return error.message;
  return "Something went wrong. Try again, or ask the till.";
}

/** SENT_TO_KITCHEN -> Sent to kitchen. Contract values are SCREAMING_SNAKE. */
export function humaniseStatus(value: string): string {
  const words = value.toLowerCase().replace(/_/g, " ");
  return words.charAt(0).toUpperCase() + words.slice(1);
}
