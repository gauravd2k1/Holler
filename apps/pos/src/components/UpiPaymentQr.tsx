import { useEffect, useState } from "react";
import QRCode from "qrcode";
import { formatPaiseAsRupees } from "../domain/money";
import { buildUpiPaymentLink, readUpiDemoPayee } from "../domain/upi";

interface UpiPaymentQrProps {
  /** The exact invoice total, in integer paise. */
  amountPaise: number;
  /** The human-facing invoice number — never a UUID. */
  note: string;
}

/**
 * Customer-facing UPI QR for the bill amount (T8, demo build).
 *
 * Renders NO QR when no demo payee VPA is configured (`domain/upi.ts`) — a QR
 * aimed at nobody is worse than no QR at a client demo — but SAYS SO on the
 * screen rather than disappearing. Silence is indistinguishable from a
 * finished bill, which is exactly how a missing QR reached a rehearsal. The QR library (`qrcode`) is a bundled dependency, not
 * a CDN script, so this still renders with no internet reachable.
 *
 * NOT a payment integration: nothing here reconciles a scan against a
 * received payment. The cashier still records the tender as a separate
 * step, exactly as before this component existed.
 */
export function UpiPaymentQr({ amountPaise, note }: UpiPaymentQrProps) {
  const payee = readUpiDemoPayee();
  const link = payee ? buildUpiPaymentLink({ ...payee, amountPaise, note }) : null;

  const [dataUrl, setDataUrl] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (link === null) {
      setDataUrl(null);
      setError(null);
      return;
    }
    let cancelled = false;
    QRCode.toDataURL(link, { errorCorrectionLevel: "M", margin: 1, width: 220 })
      .then((url) => {
        if (!cancelled) setDataUrl(url);
      })
      .catch((err: unknown) => {
        if (!cancelled) setError(err instanceof Error ? err.message : String(err));
      });
    return () => {
      cancelled = true;
    };
  }, [link]);

  // AN ABSENT QR NOW SAYS IT IS ABSENT. It used to return null, so a bill with
  // no configured payee looked exactly like a finished bill -- and the one
  // thing nobody could tell from the screen was whether the QR was missing or
  // simply not switched on. Observed 2026-09-12: .env.dev carried both lines,
  // the dev server serving the window had no VITE_ variables in it at all, and
  // the screen said nothing.
  //
  // Deliberately QUIET, not an error: no red, no alert role. A missing demo QR
  // is a configuration fact for the operator, not a failure a cashier caused,
  // and this panel sits on a screen a customer can see. The rule that matters
  // is unchanged -- NO QR IS RENDERED without a payee, because a QR aimed at
  // nobody opens a real payment app pointed at no one.
  if (payee === null) {
    return (
      <div className="upi-payment-qr card upi-payment-qr--unconfigured">
        <h4>UPI QR not configured</h4>
        <p className="muted">
          No payee is set for this build, so no QR is shown. Run the bootstrap with{" "}
          <code>-UpiVpa</code>, then restart the Vite dev server — not just the POS window.
        </p>
      </div>
    );
  }

  return (
    <div className="upi-payment-qr card">
      <h4>Scan to pay via UPI</h4>
      {dataUrl && <img src={dataUrl} alt="UPI payment QR code" width={220} height={220} />}
      {error && (
        <p className="billing-error" role="alert">
          {error}
        </p>
      )}
      <p>
        <span className="money">{formatPaiseAsRupees(amountPaise)}</span> to {payee.payeeName}
      </p>
    </div>
  );
}
