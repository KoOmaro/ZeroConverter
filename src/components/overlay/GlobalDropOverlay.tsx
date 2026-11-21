import React from "react";
import { useTranslation } from "react-i18next";

interface GlobalDropOverlayProps {
  isDragging: boolean;
}

export const GlobalDropOverlay: React.FC<GlobalDropOverlayProps> = ({ isDragging }) => {
  const { t } = useTranslation();
  if (!isDragging) return null;

  return (
    <div className="fixed inset-0 z-[100] flex items-center justify-center p-12 bg-drop-overlay-bg animate-in fade-in duration-300 pointer-events-none">
      <div className="w-full h-full border-2 border-dashed border-accent/40 rounded-panel flex flex-col items-center justify-center space-y-6">
        <div className="w-20 h-20 rounded-panel bg-accent/10 flex items-center justify-center text-accent-strong animate-pulse">
           <svg xmlns="http://www.w3.org/2000/svg" width="32" height="32" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.5" strokeLinecap="round" strokeLinejoin="round"><path d="M21 15v4a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2v-4"></path><polyline points="17 8 12 3 7 8"></polyline><line x1="12" y1="3" x2="12" y2="15"></line></svg>
        </div>
        <div className="text-center space-y-2">
            <h2 className="text-2xl font-main font-semibold text-accent-strong tracking-tighter">{t("dropOverlay.title")}</h2>
            <p className="text-xs font-semibold text-accent-strong opacity-60">{t("dropOverlay.body")}</p>
        </div>
      </div>
    </div>
  );
};
