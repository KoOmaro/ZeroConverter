import React, { useMemo } from "react";
import { Upload } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useAppCapabilities } from "../../application/capabilitiesStore";

interface EmptyWorkspaceProps {
  onSelectFiles: () => void;
  isDragging?: boolean;
}

export const EmptyWorkspace: React.FC<EmptyWorkspaceProps> = ({ onSelectFiles, isDragging }) => {
  const { capabilities } = useAppCapabilities();
  const { t } = useTranslation();
  /** Notes for formats whose name alone could mislead. */
  const FORMAT_NOTES: Record<string, string> = { gif: t("empty.gifNote") };

  /** Deduplicated canonical readable formats derived from capability descriptors. */
  const readableFormats = useMemo(() => {
    if (!capabilities) return [];
    const seen = new Set<string>();
    const result: { id: string; label: string }[] = [];
    for (const family of capabilities.families) {
      for (const desc of family.descriptors) {
        if (desc.can_read && !seen.has(desc.id)) {
          seen.add(desc.id);
          result.push({ id: desc.id, label: desc.canonical_extension.toUpperCase() });
        }
      }
    }
    return result;
  }, [capabilities]);

  return (
    <div className="flex min-h-full flex-col px-6 pb-6 pt-1">
      <div
        className={`flex flex-1 flex-col items-center justify-center rounded-panel border border-dashed transition-colors duration-150 ${
          isDragging ? "border-selected-border bg-selected" : "border-border-subtle"
        }`}
      >
        <div className="flex max-w-sm flex-col items-center gap-5 p-10 text-center">
          <Upload size={22} strokeWidth={1.5} aria-hidden="true" className={isDragging ? "text-accent-strong" : "text-text-tertiary"} />

          <div className="space-y-1.5">
            <h2 className="type-page-title text-text-primary">
              {isDragging ? t("empty.dropActive") : t("empty.drop")}
            </h2>
            <p className="text-[12px] leading-relaxed text-text-secondary">
              {t("empty.renameHint", { from: "photo.png", to: "photo.webp" })}
            </p>
          </div>

          <button
            type="button"
            onClick={(e) => { e.stopPropagation(); onSelectFiles(); }}
            className="zc-btn zc-btn-primary"
          >
            {t("empty.choose")}
          </button>

          {readableFormats.length > 0 && (
            <p className="text-[11px] leading-[1.7] text-text-tertiary">
              {t("empty.reads")}{" "}
              {readableFormats.map((fmt, i) => (
                <React.Fragment key={fmt.id}>
                  <span title={FORMAT_NOTES[fmt.id]}>{fmt.label}</span>
                  {i < readableFormats.length - 1 ? ", " : "."}
                </React.Fragment>
              ))}
            </p>
          )}
        </div>
      </div>
    </div>
  );
};