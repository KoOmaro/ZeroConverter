import React, { useEffect, useRef } from "react";
import { Search, X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { WindowControls } from "./WindowControls";

interface TopBarProps {
  query: string;
  onQueryChange: (q: string) => void;
  onOpenSettings: () => void;
}

/**
 * Title bar over the queue and details (the left panel runs to the top edge
 * on its own): drag area, file search in the middle, settings
 * and window buttons on the right.
 */
export const TopBar: React.FC<TopBarProps> = ({ query, onQueryChange, onOpenSettings }) => {
  const input = useRef<HTMLInputElement>(null);
  const { t } = useTranslation();

  // Ctrl+F jumps to the search, Esc inside it clears it.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") {
        e.preventDefault();
        input.current?.focus();
        input.current?.select();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  return (
    <header className="shell-header flex items-center justify-end select-none">
      <div data-tauri-drag-region className="absolute inset-0" />
      {/* Centred on the window, not on this bar (which starts after the rail). */}
      <div className="zc-search-slot flex items-center">
        <Search size={15} strokeWidth={1.75} aria-hidden="true" className="pointer-events-none absolute start-3 text-text-tertiary" />
        <input
          ref={input}
          type="search"
          value={query}
          onChange={(e) => onQueryChange(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Escape" && query) {
              e.stopPropagation();
              onQueryChange("");
            }
          }}
          placeholder={t("topBar.searchPlaceholder")}
          aria-label={t("topBar.searchLabel")}
          autoComplete="off"
          spellCheck={false}
          className="zc-search h-[30px] w-full rounded-control border border-border-subtle bg-subtle ps-9 pe-16 text-[12px] text-text-primary placeholder:text-text-tertiary transition-colors duration-150 hover:border-border-strong"
        />
        {query ? (
          <button
            type="button"
            onClick={() => onQueryChange("")}
            data-press="icon"
            className="zc-icon-btn absolute end-1.5 !h-6 !w-6"
            aria-label={t("topBar.clearSearch")}
          >
            <X size={13} strokeWidth={1.75} aria-hidden="true" />
          </button>
        ) : (
          <kbd className="pointer-events-none absolute end-2.5 rounded-status border border-border-subtle bg-surface-2 px-1.5 py-px font-mono text-[10px] font-medium text-text-tertiary">
            Ctrl F
          </kbd>
        )}
      </div>
      <div className="relative">
        <WindowControls onOpenSettings={onOpenSettings} />
      </div>
    </header>
  );
};
