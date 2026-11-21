import React from "react";
import { Plus, Trash2 } from "lucide-react";
import { useTranslation } from "react-i18next";
import { QueueSummaryDto } from "../../application/converterTypes";
import { PendingFile } from "../../application/converterStore";

interface QueueHeaderProps {
  summary: QueueSummaryDto | null;
  pendingFiles?: PendingFile[];
  onClearAll?: () => void;
  onResetBatch?: () => void;
  onAddFiles?: () => void;
}

export const QueueHeader: React.FC<QueueHeaderProps> = ({
  summary,
  pendingFiles = [],
  onClearAll,
  onResetBatch,
  onAddFiles,
}) => {
  const { t } = useTranslation();
  const hasJobs = summary && (summary.queued > 0 || summary.running > 0);
  const isTerminal = summary && summary.total > 0 && summary.queued === 0 && summary.running === 0;
  const hasPending = pendingFiles.length > 0;

  const showReset = isTerminal && !hasPending && onResetBatch;
  const totalPendingSize = pendingFiles.reduce((acc, f) => acc + f.sizeBytes, 0);
  const formatSize = (bytes: number) => {
    if (bytes === 0) return "0 B";
    const units = ['B', 'KB', 'MB', 'GB'];
    let val = bytes;
    let unitIndex = 0;
    while (val > 1024 && unitIndex < units.length - 1) {
      val /= 1024;
      unitIndex++;
    }
    return `${val.toFixed(1)} ${units[unitIndex]}`;
  };
  const percent = summary ? Math.round(((summary.completed + summary.failed) / (summary.total || 1)) * 100) : 0;

  return (
    <div className="relative z-10 flex h-11 shrink-0 items-center justify-between ps-6 pe-4">
      <div className="flex items-baseline gap-2.5">
        <h2 className="type-panel-header whitespace-nowrap text-text-primary">
          {hasJobs ? t("queue.converting") : hasPending ? t("queue.readyToConvert") : t("queue.title")}
        </h2>

        <span className="text-[12px] tabular-nums text-text-tertiary">
          {hasJobs && t("queue.running", { count: summary?.running ?? 0 })}
          {hasJobs && hasPending && " · "}
          {hasPending && `${t("common.fileCount", { count: pendingFiles.length })} · ${formatSize(totalPendingSize)}`}
        </span>
      </div>

      <div className="relative flex items-center gap-1">
        {hasPending && onClearAll && (
          <button onClick={onClearAll} type="button" className="zc-btn zc-btn-sm zc-btn-ghost">
            <Trash2 size={13} strokeWidth={1.75} aria-hidden="true" />
            {t("queue.clearAll")}
          </button>
        )}

        {onAddFiles && !showReset && (
          <button type="button" onClick={onAddFiles} className="zc-btn zc-btn-sm zc-btn-ghost">
            <Plus size={14} strokeWidth={1.75} aria-hidden="true" />
            {t("queue.addFiles")}
          </button>
        )}

        {hasJobs && summary && (
          <div className="ms-2 flex items-center gap-2.5">
            <div className="h-1 w-32 overflow-hidden rounded-full bg-surface-3">
              <div className="h-full bg-accent-strong transition-[width] duration-200" style={{ width: `${percent}%` }} />
            </div>
            <span className="w-8 text-end text-[12px] tabular-nums text-text-secondary">{percent}%</span>
          </div>
        )}

        {showReset && (
          <button onClick={onResetBatch} type="button" className="zc-btn zc-btn-sm zc-btn-primary">
            <Plus size={14} strokeWidth={2} aria-hidden="true" />
            {t("queue.newBatch")}
          </button>
        )}
      </div>
    </div>
  );
};
