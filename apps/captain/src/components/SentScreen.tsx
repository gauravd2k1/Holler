import type { CanonicalOrder } from "@holler/contracts";
import type { KotSummary } from "../lib/api";

interface Props {
  order: CanonicalOrder;
  kots: KotSummary[];
  onDone: () => void;
}

/**
 * Send result screen. Shows the order's display_number and the KOTs with
 * their stations — the half of step 1a the client is actually looking at
 * (docs/captain-api.md): the ticket landing on the hub, which the KDS is
 * watching.
 */
export function SentScreen({ order, kots, onDone }: Props) {
  return (
    <div className="screen">
      <h2>Sent to the kitchen</h2>
      <p>
        Order <strong>{order.display_number ?? order.holler_order_id}</strong>
      </p>
      <div className="kot-list">
        {kots.map((k) => (
          <div className="kot-row" key={k.id}>
            <span>{k.station}</span>
            <span>{k.status}</span>
          </div>
        ))}
      </div>
      <button type="button" className="btn btn--primary btn--lg btn--block" onClick={onDone}>
        Back to tables
      </button>
    </div>
  );
}
