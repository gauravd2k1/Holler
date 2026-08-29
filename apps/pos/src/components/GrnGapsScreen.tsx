import { useNavigate } from "@tanstack/react-router";
import { useGrnGapsQuery } from "../lib/queries";
import { canManageProcurement, grnGapDetailText, grnGapReasonCopy } from "../domain/procurement";
import { useAuthStore } from "../store/auth";

// ---------------------------------------------------------------------------
// DELIVERY PROBLEMS — M5 ACCEPTANCE CRITERION 3
// ---------------------------------------------------------------------------
//
// The criterion is not "a `grn_gap` row exists". It is "the gap is VISIBLE TO
// A HUMAN ON THE POS". `grn_gap.detail` is prose because a person reads it, so
// this screen renders that prose rather than a code.
//
// EVERY ROW CARRIES ITS OWN REASON. The M4 gaps screen titles every row "Items
// Sold With No Recipe" whatever the reason, so a DIMENSION_MISMATCH reads
// there as a missing recipe — a filed M6 defect. M5 has EIGHT reasons, from a
// walk-in delivery with no order to a unit that could not be converted, and
// they call for entirely different actions. Rendering one heading over all of
// them would repeat that defect at eight times the cost.
export function GrnGapsScreen() {
  const navigate = useNavigate();
  const principal = useAuthStore((s) => s.principal);
  const canView = canManageProcurement(principal);
  const gapsQuery = useGrnGapsQuery();
  const gaps = gapsQuery.data ?? [];

  return (
    <main className="grn-gaps-screen">
      <header>
        <h1>Delivery Problems</h1>
        <button type="button" onClick={() => void navigate({ to: "/procurement/receive" })}>
          Receive Delivery
        </button>
        <button type="button" onClick={() => void navigate({ to: "/inventory/stock" })}>
          Back to Stock
        </button>
      </header>

      {!canView && <p role="alert">You do not have permission to view delivery problems.</p>}

      {canView && (
        <>
          {/* Said plainly, because the opposite reading is the dangerous one:
              a row here does NOT mean stock is wrong or a delivery was
              rejected. Every one of these deliveries was accepted. */}
          <p>
            Every delivery listed here was recorded and stock was updated. These are the things that
            could not be matched at the time, kept so someone can settle them afterwards.
          </p>

          {gapsQuery.isLoading && <p>Loading…</p>}
          {gapsQuery.isError && <p role="alert">Could not load delivery problems.</p>}
          {!gapsQuery.isLoading && gaps.length === 0 && <p>Nothing outstanding.</p>}

          <ul className="grn-gap-list">
            {gaps.map((gap) => {
              const copy = grnGapReasonCopy(gap.reason);
              return (
                <li key={gap.id} className="grn-gap">
                  {/* THE ACTUAL REASON, not this screen's name. */}
                  <h2>{copy.title}</h2>
                  {/* The edge's own prose about this specific gap. */}
                  <p className="grn-gap-detail">{grnGapDetailText(gap)}</p>
                  <p className="grn-gap-next-step">{copy.nextStep}</p>
                  <dl className="grn-gap-meta">
                    <dt>Business date</dt>
                    <dd>{gap.business_date}</dd>
                    <dt>Recorded</dt>
                    <dd>{gap.occurred_at}</dd>
                    <dt>Delivery</dt>
                    <dd>{gap.grn_id}</dd>
                    <dt>Reason code</dt>
                    {/* The raw code as well as the words — the words are for
                        acting on it, the code is for reporting it. */}
                    <dd>{gap.reason}</dd>
                  </dl>
                </li>
              );
            })}
          </ul>
        </>
      )}
    </main>
  );
}
