import React from "react";
import { JobStatusDto, AppCapabilitiesDto } from "../../application/converterTypes";
import { Check, FolderOpen } from "lucide-react";
import { useTranslation } from "react-i18next";
import { useThumbnail } from "../../application/thumbnailService";

interface QueueRowProps {
  job: JobStatusDto;
  isSelected: boolean;
  onSelect: (id: string) => void;
  onCancel: (id: string) => void;
  onReveal?: (jobId: string) => void | Promise<void>;
  capabilities: AppCapabilitiesDto;
}

const getStatusColor = (status: string) => {
  switch (status) {
    case "Running": return "text-text-secondary";
    case "Completed": return "text-success";
    case "Failed": return "text-danger";
    case "Cancelled": return "text-text-tertiary";
    default: return "text-text-secondary";
  }
};

const QueueRowInner: React.FC<QueueRowProps> = ({
  job,
  isSelected,
  onSelect,
  onCancel,
  onReveal,
  capabilities,
}) => {
  const { t } = useTranslation();
  const fileName = job.request.input_path.split(/[\\/]/).pop() || job.id;
  const inputExtension = job.request.input_path.split(/[\\/]/).pop()?.split(".").pop()?.toLowerCase() ?? "";
  const isImage = capabilities.families.find(f => f.family === "image")?.descriptors.some(d =>
    d.can_read && [d.canonical_extension, ...d.aliases].some(ext => ext.toLowerCase() === inputExtension)
  ) ?? false;

  const thumbnailPath = React.useMemo(() => {
    if (!isImage) return undefined;
    return job.request.input_path;
  }, [isImage, job.request.input_path]);

  const thumbnail = useThumbnail(thumbnailPath);

  const isCompleted = job.status === "Completed";
  const isFailed = job.status === "Failed";
  const isCancelled = job.status === "Cancelled";
  const isTerminal = isCompleted || isFailed || isCancelled;
  const percent = isCompleted ? 100 : Math.max(0, Math.min(100, Math.round(job.progress.percent ?? 0)));
  const outputFileName = isCompleted && job.result?.output_path ? job.result.output_path.split(/[\\/]/).pop() : undefined;
  // What happened, in one line: terminal status wins over stale progress text.
  const detail = isCompleted
    ? outputFileName ?? t("status.done")
    : isFailed
      ? job.failure?.message || t("queue.conversionFailed")
      : isCancelled
        ? t("status.cancelled")
        : job.progress.message || job.progress.stage || t("queue.waiting");

  const reveal = () => {
    if (!onReveal) return;
    const result = onReveal(job.id);
    if (result instanceof Promise) result.catch(console.error);
  };

  return (
    <div
      onClick={() => onSelect(job.id)}
      onDoubleClick={() => { if (isCompleted && job.result?.output_path) reveal(); }}
      className={`group relative flex h-12 cursor-pointer items-center gap-3 border-b border-s-2 border-b-separator px-6 transition-colors duration-150 ${
        isSelected ? "border-s-selected-border bg-selected" : "border-s-transparent hover:bg-hover"
      }`}
    >
      <div className="relative grid h-7 w-7 shrink-0 place-items-center overflow-hidden rounded-control bg-subtle">
        {thumbnail ? (
          <img src={thumbnail} className="h-full w-full object-cover" alt="" loading="lazy" decoding="async" />
        ) : (
          <span className="text-[9px] font-semibold uppercase text-text-tertiary">{inputExtension.slice(0, 4)}</span>
        )}
      </div>

      <button
        type="button"
        onClick={(e) => { e.stopPropagation(); onSelect(job.id); }}
        data-press="none"
        className="min-w-0 flex-1 text-start"
        aria-label={t("queue.selectJob", { name: fileName })}
      >
        <div className="truncate text-[12px] font-medium text-text-primary">{fileName}</div>
        <div className={`truncate text-[11px] ${isFailed ? "text-danger" : "text-text-tertiary"}`} title={detail}>
          <span className="tabular-nums">{inputExtension.toUpperCase()} → {job.request.output_format.toUpperCase()}</span>
          {" · "}
          {detail}
        </div>
      </button>

      {/* Progress while it runs, a plain status word once it is over. */}
      {isTerminal ? (
        <span className={`flex w-24 shrink-0 items-center justify-end gap-1.5 text-[12px] ${getStatusColor(job.status)}`}>
          {isCompleted && <Check size={13} strokeWidth={2} aria-hidden="true" />}
          {isCompleted ? t("status.done") : isFailed ? t("status.failed") : t("status.cancelled")}
        </span>
      ) : (
        <div className="hidden w-40 shrink-0 items-center gap-2.5 md:flex">
          <div
            className="h-1 flex-1 overflow-hidden rounded-full bg-surface-3"
            role="progressbar"
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={percent}
            aria-label={t("queue.progressFor", { name: fileName })}
          >
            <div className="h-full bg-accent-strong transition-[width] duration-200 ease-out" style={{ width: `${percent}%` }} />
          </div>
          <span className="w-8 text-end text-[12px] tabular-nums text-text-secondary">{percent}%</span>
        </div>
      )}

      <div className="flex w-[76px] shrink-0 justify-end">
        {!isTerminal && (
          <button
            type="button"
            onClick={(e) => { e.stopPropagation(); onCancel(job.id); }}
            className="zc-btn zc-btn-sm zc-btn-ghost opacity-0 focus-visible:opacity-100 group-hover:opacity-100"
            title={t("queue.stopFor", { name: fileName })}
            aria-label={t("queue.stopFor", { name: fileName })}
          >
            {t("queue.stop")}
          </button>
        )}
        {isCompleted && job.result?.output_path && (
          <button
            type="button"
            onClick={(e) => { e.stopPropagation(); reveal(); }}
            className="zc-btn zc-btn-sm zc-btn-ghost"
            title={t("queue.showInExplorer", { name: fileName })}
            aria-label={t("queue.showOutputFor", { name: fileName })}
          >
            <FolderOpen size={13} strokeWidth={1.75} aria-hidden="true" />
            {t("queue.show")}
          </button>
        )}
      </div>
    </div>
  );
};
export const QueueRow = React.memo(QueueRowInner, (prev, next) => {
  return (
    prev.job.id === next.job.id &&
    prev.job.status === next.job.status &&
    prev.job.progress.percent === next.job.progress.percent &&
    prev.isSelected === next.isSelected
  );
});
