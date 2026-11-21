import React from "react";
import { AlertTriangle, X } from "lucide-react";
import { useTranslation } from "react-i18next";

interface ErrorNoticeProps {
  message: string;
  onDismiss: () => void;
}

/** Internal prefixes ("Validation error: …") mean nothing to the person reading. */
function readable(message: string): string {
  return message
    .replace(/^Error:\s*/i, "")
    .replace(/^(Internal core|IO|Conversion|Probe|Validation|Execution|Registry) error:\s*/i, "")
    .trim();
}

/** Why the last action failed, above the queue until dismissed. */
export const ErrorNotice: React.FC<ErrorNoticeProps> = ({ message, onDismiss }) => {
  const { t } = useTranslation();
  return (
  <div className="zc-error-notice" role="alert">
    <AlertTriangle size={16} strokeWidth={1.75} aria-hidden="true" className="mt-0.5 shrink-0 text-danger" />
    <div className="min-w-0 flex-1">
      <p className="text-[13px] font-medium text-text-primary">{t("error.title")}</p>
      <p className="mt-0.5 break-words text-[12px] leading-relaxed text-text-secondary">{readable(message)}</p>
    </div>
    <button
      type="button"
      data-press="icon"
      className="zc-icon-btn !h-6 !w-6 shrink-0"
      onClick={onDismiss}
      aria-label={t("error.dismiss")}
    >
      <X size={13} strokeWidth={1.75} aria-hidden="true" />
    </button>
  </div>
  );
};
