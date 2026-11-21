import React from "react";
import { useTranslation } from "react-i18next";

/** Shown while the core reports its formats: the mark inside a turning ring. */
export const StartupScreen: React.FC = () => {
  const { t } = useTranslation();
  return (
  <div className="zc-startup" role="status" aria-label={t("startup.label")}>
    <div className="zc-startup-identity">
      <svg className="zc-startup-ring" viewBox="0 0 148 148" aria-hidden="true">
        <circle className="zc-startup-track" cx="74" cy="74" r="72" pathLength={100} />
        <circle className="zc-startup-arc" cx="74" cy="74" r="72" pathLength={100} strokeDasharray="24 76" />
      </svg>
      <img src="/zeroconverter-mark.svg" alt="" width={64} height={64} className="zc-startup-mark" />
    </div>
  </div>
  );
};
