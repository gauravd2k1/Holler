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
 * Renders nothing at all — not even a placeholder — when no demo payee VPA
 * is configured (`domain/upi.ts`): a QR aimed at nobody is worse than no QR
 * at a client demo. The QR library (`qrcode`) is a bundled dependency, not
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

  if (payee === null) return null;

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
