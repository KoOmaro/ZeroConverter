import React, { useEffect, useLayoutEffect, useState } from "react";
import { Check, CircleAlert, X } from "lucide-react";
import { receiptPage, type Receipt } from "../../application/notifications";

const ICON = {
  success: <Check size={19} strokeWidth={2} aria-hidden="true" />,
  warning: <CircleAlert size={18} strokeWidth={2} aria-hidden="true" />,
  failure: <X size={18} strokeWidth={2} aria-hidden="true" />,
};

/**
 * The desktop notification window's page (`#/receipt`): one card that the
 * Rust side fills, shows in the screen corner, and hides again.
 */
export const ReceiptCard: React.FC = () => {
  const [receipt, setReceipt] = useState<Receipt | null>(null);

  useEffect(() => {
    document.documentElement.classList.add("zc-receipt-page");
    receiptPage.current().then((r) => r && setReceipt(r)).catch(() => {});
    const off = receiptPage.onShow(setReceipt);
    return () => {
      off.then((stop) => stop()).catch(() => {});
    };
  }, []);

  // Tell Rust the new card is on screen before the window appears.
  useLayoutEffect(() => {
    if (!receipt) return;
    requestAnimationFrame(() => receiptPage.painted().catch(() => {}));
  }, [receipt]);

  if (!receipt) return null;
  return (
    <div className={`zc-receipt zc-receipt--${receipt.tone}`} role="status">
      <p className="zc-receipt-brand">ZeroConverter</p>
      <div className="zc-receipt-row">
        <span className="zc-receipt-icon">{ICON[receipt.tone]}</span>
        <div className="min-w-0">
          <p className="zc-receipt-title">{receipt.title}</p>
          <p className="zc-receipt-body">{receipt.body}</p>
        </div>
      </div>
    </div>
  );
};
