import type { CanonicalOrder } from "@holler/contracts";
import type { KotSummary } from "../lib/api";
import { Empty, humaniseStatus } from "./Waiting";

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
      {/* NEVER THE UUID. This screen fell back to holler_order_id when
          display_number was null, which puts a raw id in front of a customer
          on the one screen step 1a is about. An order with no number is worth
          saying plainly; it is not worth a UUID. */}
      <p>
        Order{" "}
        <strong>{order.display_number ?? "number pending"}</strong>
      </p>
      <div className="kot-list">
        {kots.length === 0 && (
          <Empty
            title="No kitchen ticket was produced."
            detail="The order was saved, but nothing routed to a station. Tell the till before the food is assumed to be coming."
          />
        )}
        {kots.map((k) => (
          <div className="kot-row" key={k.id}>
            <span>{k.station}</span>
            <span>{humaniseStatus(k.status)}</span>
          </div>
        ))}
      </div>
      <button type="button" className="btn btn--primary btn--lg btn--block" onClick={onDone}>
        Back to tables
      </button>
    </div>
  );
}
