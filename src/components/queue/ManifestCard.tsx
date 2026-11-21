import React from "react";
import { X } from "lucide-react";
import { useTranslation } from "react-i18next";
import { PendingFile } from "../../application/converterStore";
import { FileTypeIcon } from "../common/FileTypeIcon";
import { useThumbnail } from "../../application/thumbnailService";

interface ManifestCardProps {
  file: PendingFile;
  isSelected: boolean;
  onSelect: (id: string) => void;
  onRemove: (id: string) => void;
}

const formatSize = (bytes: number) => {
  if (bytes === 0) return "";
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
};

const ManifestCardInner: React.FC<ManifestCardProps> = ({
  file,
  isSelected,
  onSelect,
  onRemove,
}) => {
  const { t } = useTranslation();
  const isError = file.status === 'Error' || file.status === 'Unsupported';
  const isPending = file.status === 'Idle';

  // Use lazy thumbnail instead of full-res src
  const thumbnail = useThumbnail(file.kind === 'image' ? file.path : undefined);
  const size = formatSize(file.sizeBytes);

  return (
    <div
      onClick={() => onSelect(file.id)}
      data-press="wide"
      className={`group relative flex cursor-pointer select-none flex-col overflow-hidden rounded-card border ${
        isSelected
          ? "border-selected-border bg-selected outline outline-1 -outline-offset-1 outline-selected-border"
          : "border-separator bg-surface-1 hover:border-border-strong"
      }`}
    >
      {/* The picture runs edge to edge; one hairline separates it from the text. */}
      <div className="grid aspect-[4/3] w-full place-items-center overflow-hidden border-b border-separator bg-subtle">
        {isPending ? (
          <div className="h-4 w-4 animate-spin rounded-full border-2 border-border-strong border-t-transparent" />
        ) : thumbnail ? (
          <img
            src={thumbnail}
            alt=""
            className={`h-full w-full object-cover ${isError ? "opacity-50" : ""}`}
            loading="lazy"
            decoding="async"
          />
        ) : (file.kind === 'image' && !isError) ? (
          // Thumbnail is generating — show subtle shimmer placeholder
          <div className="h-full w-full animate-pulse bg-surface-2" />
        ) : (
          <FileTypeIcon kind={file.kind} className="h-7 w-7 opacity-40" />
        )}
      </div>

      <button
        type="button"
        onClick={(e) => { e.stopPropagation(); onSelect(file.id); }}
        data-press="none"
        className="w-full px-2.5 py-2 text-start"
        aria-label={t("queue.selectFile", { name: file.name })}
        aria-pressed={isSelected}
        title={file.name}
      >
        <div className="truncate text-[12px] font-medium text-text-primary">{file.name}</div>
        <div className={`mt-0.5 truncate text-[11px] tabular-nums ${isError ? "text-danger" : "text-text-tertiary"}`}>
          {isError
            ? file.status === "Unsupported" ? t("status.notSupported") : t("status.cantBeRead")
            : [file.extension.toUpperCase(), size].filter(Boolean).join(" · ")}
        </div>
      </button>

      <button
        type="button"
        onClick={(e) => {
          e.stopPropagation();
          onRemove(file.id);
        }}
        data-press="icon"
        className="absolute end-1.5 top-1.5 grid h-6 w-6 place-items-center rounded-control bg-app/80 text-text-secondary opacity-0 hover:text-text-primary focus-visible:opacity-100 group-hover:opacity-100"
        title={t("queue.remove", { name: file.name })}
        aria-label={t("queue.remove", { name: file.name })}
      >
        <X size={13} strokeWidth={2} aria-hidden="true" />
      </button>
    </div>
  );
};

// React.memo prevents re-renders when parent list re-renders but this card's props haven't changed.
// This is critical for virtualized lists — without memo, every scroll triggers re-render of all visible cards.
export const ManifestCard = React.memo(ManifestCardInner, (prev, next) => {
  return (
    prev.file.id === next.file.id &&
    prev.file.status === next.file.status &&
    prev.file.path === next.file.path &&
    prev.file.sizeBytes === next.file.sizeBytes &&
    prev.isSelected === next.isSelected
  );
});
