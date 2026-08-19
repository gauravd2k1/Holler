import React from "react";
import { CrashScreen } from "./CrashScreen";

// Why this exists (docs/retro.md, the stale-optimizeDeps white screen):
// React unmounts the entire tree when a render throws, so without a boundary
// the POS goes blank and the reason is visible only in a devtools console
// nobody has open. On a till that is the worst possible failure mode — a
// cashier mid-order sees a white rectangle and has nothing to read out to
// whoever they call.
//
// SCOPE, measured rather than assumed: this catches throws OUTSIDE the
// router only. TanStack Router wraps each route match in its own
// CatchBoundary, so a throw inside a route component is intercepted there
// and never reaches this — see `defaultErrorComponent` in routes/router.tsx,
// which routes those to the same CrashScreen. Confirmed by throwing in
// LoginScreen and observing which component rendered.
//
// This does NOT try to recover. A render that threw has left component state
// unknowable, and pretending otherwise on a screen that takes money is worse
// than stopping.

interface Props {
  children: React.ReactNode;
}

interface State {
  error: Error | null;
  componentStack: string | null;
}

export class ErrorBoundary extends React.Component<Props, State> {
  constructor(props: Props) {
    super(props);
    this.state = { error: null, componentStack: null };
  }

  static getDerivedStateFromError(error: Error): Partial<State> {
    return { error };
  }

  componentDidCatch(error: Error, info: React.ErrorInfo) {
    this.setState({ componentStack: info.componentStack ?? null });
    // Still log: a developer with the console open should not have to read
    // the screen, and this preserves the stack the UI truncates.
    console.error("POS crashed during render:", error, info.componentStack);
  }

  render() {
    if (!this.state.error) return this.props.children;
    return <CrashScreen error={this.state.error} componentStack={this.state.componentStack} />;
  }
}
