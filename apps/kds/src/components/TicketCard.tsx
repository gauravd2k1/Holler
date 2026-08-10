import type { Kot } from "@holler/contracts";
import { DEFAULT_SLA_THRESHOLDS, elapsedMinutes, slaBucket } from "../domain/sla";
import { nextStatus, nextStatusLabel, statusLabel } from "../domain/kotTransitions";
import type { PendingTransition } from "../store/kdsStore";

export interface TicketCardProps {
  kot: Kot;
  now: Date;
  pending: PendingTransition | undefined;
  onAdvance: (kotId: string, status: Kot["status"]) => void;
}

/** One station ticket. Order number and channel are not on `Kot` (they live
 * on the order it was cut from, which the KDS receives already merged in via
 * `station`/`order_id`) — this card shows what the frozen `Kot` shape
 * actually carries: station, sequence, items, modifiers, notes, status, and
 * elapsed time computed from `created_at`. */
export function TicketCard({ kot, now, pending, onAdvance }: TicketCardProps) {
  const minutes = elapsedMinutes(kot.created_at, now);
  const bucket = slaBucket(minutes, DEFAULT_SLA_THRESHOLDS);
  const label = nextStatusLabel(kot.status);

  return (
    <article className={`ticket-card ticket-card--${bucket.toLowerCase()}`} data-kot-id={kot.id}>
      <header className="ticket-card__header">
        <span className="ticket-card__station">{kot.station}</span>
        <span className="ticket-card__sequence">Seq {kot.sequence}</span>
        <span className="ticket-card__elapsed">{minutes} min</span>
      </header>
      <div className="ticket-card__status-row">
        <span className="ticket-card__status">{statusLabel(kot.status)}</span>
        <span className="ticket-card__sla-bucket">{bucket}</span>
      </div>
      <ul className="ticket-card__items">
        {kot.items.map((item) => (
          <li key={item.order_item_id}>
            <span className="ticket-card__item-qty">{item.quantity}×</span>{" "}
            <span className="ticket-card__item-name">{item.name}</span>
            {item.modifiers.length > 0 && (
              <div className="ticket-card__modifiers">{item.modifiers.join(", ")}</div>
            )}
            {item.notes && <div className="ticket-card__notes">{item.notes}</div>}
          </li>
        ))}
      </ul>
      {pending && !pending.timedOut && (
        <div className="ticket-card__pending" role="status">
          Sending…
        </div>
      )}
      {pending?.timedOut && (
        <div className="ticket-card__pending ticket-card__pending--timed-out" role="alert">
          Not confirmed by kitchen system — try again
        </div>
      )}
      {label && (
        <button
          type="button"
          className="ticket-card__advance"
          disabled={pending !== undefined && !pending.timedOut}
          onClick={() => {
            const target = nextStatus(kot.status);
            if (target) onAdvance(kot.id, target);
          }}
        >
          {label}
        </button>
      )}
    </article>
  );
}
