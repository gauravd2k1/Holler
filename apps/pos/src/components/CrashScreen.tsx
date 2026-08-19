// The one crash surface, shared by both places a React error can surface:
//
//  - ErrorBoundary.tsx, for a throw OUTSIDE the router (providers, the root).
//  - routes/router.tsx's `defaultErrorComponent`, for a throw inside a route
//    component. TanStack Router wraps every match in its own CatchBoundary,
//    so it intercepts those before any boundary of ours can see them — and
//    its stock component says only "Something went wrong!", with no message
//    and nothing to copy into a bug report. Verified by deliberately
//    throwing in LoginScreen and observing which component rendered.
//
// Neither of these can catch a module that fails to LOAD (a bad import, a
// stale Vite prebundle): React never runs, so nothing React-shaped can
// report it. That case is handled by the inline bootstrap handler in
// index.html.

interface Props {
  error: unknown;
  /** React's component stack, when the reporter has one. */
  componentStack?: string | null;
}

export function CrashScreen({ error, componentStack }: Props) {
  const err = error as { message?: string; stack?: string } | null | undefined;
  const message = err?.message ?? String(error);
  const detail = [
    err?.stack ?? String(error),
    componentStack ? `\nComponent stack:${componentStack}` : "",
  ].join("");

  return (
    <main className="crash-screen" role="alert">
      <h1>The POS hit an error and stopped</h1>
      <p className="crash-message">{message}</p>
      <p className="crash-guidance">
        Nothing you had entered has been sent. Reload to start again — an order already
        confirmed or billed is safe on this machine and will still be there.
      </p>
      <button type="button" onClick={() => window.location.reload()}>
        Reload the POS
      </button>
      <details className="crash-detail">
        <summary>Technical detail (for a bug report)</summary>
        <pre>{detail}</pre>
      </details>
    </main>
  );
}
