import React from "react";
import { Copy, Minus, Settings, Square, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useWindowChrome } from "../../application/useWindowChrome";

const windowButton =
  "grid h-11 w-[46px] place-items-center text-text-tertiary focus-visible:outline-offset-[-2px]";

/** Settings and the window buttons, top right of the shell (the native frame is off). */
export const WindowControls: React.FC<{ onOpenSettings: () => void }> = ({ onOpenSettings }) => {
  const chrome = useWindowChrome();
  const { t } = useTranslation();
  return (
    <div className="relative flex h-11 items-stretch" dir="ltr">
      <button
        type="button"
        onClick={onOpenSettings}
        data-press="icon"
        className="zc-icon-btn my-auto mr-1.5"
        title={t("window.settings")}
        aria-label={t("window.settings")}
      >
        <Settings size={15} strokeWidth={1.75} aria-hidden="true" />
      </button>
      <button type="button" onClick={chrome.minimize} className={`${windowButton} hover:bg-hover hover:text-text-primary`} aria-label={t("window.minimize")}>
        <Minus size={16} strokeWidth={1.5} aria-hidden="true" />
      </button>
      <button
        type="button"
        onClick={chrome.toggleMaximize}
        className={`${windowButton} hover:bg-hover hover:text-text-primary`}
        aria-label={chrome.isMaximized ? t("window.restore") : t("window.maximize")}
      >
        {chrome.isMaximized ? (
          <Copy size={13} strokeWidth={1.5} className="-scale-x-100" aria-hidden="true" />
        ) : (
          <Square size={12} strokeWidth={1.5} aria-hidden="true" />
        )}
      </button>
      <button type="button" onClick={chrome.close} className={`${windowButton} hover:bg-[#C42B1C] hover:text-white`} aria-label={t("window.close")}>
        <X size={16} strokeWidth={1.5} aria-hidden="true" />
      </button>
    </div>
  );
};
