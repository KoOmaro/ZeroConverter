import React, { useState, useEffect } from "react";
import { Virtuoso } from "react-virtuoso";
import { useTranslation } from "react-i18next";
import { JobStatusDto, QueueSummaryDto, AppCapabilitiesDto } from "../../application/converterTypes";
import { PendingFile } from "../../application/converterStore";
import { QueueRow } from "./QueueRow";
import { QueueHeader } from "./QueueHeader";
import { ManifestGrid } from "./ManifestGrid";
import { EmptyWorkspace } from "../states/EmptyWorkspace";
import { VideoEngineCard } from "./VideoEngineCard";
import { ErrorNotice } from "./ErrorNotice";
import type { VideoEngineState } from "../../application/converterTypes";

interface QueueWorkspaceProps {
  jobs: JobStatusDto[];
  pendingFiles: PendingFile[];
  summary: QueueSummaryDto | null;
  selectedJobId: string | null;
  selectedPendingId: string | null;
  onSelectJob: (id: string | null) => void;
  onSelectPending: (id: string | null) => void;
  onCancelJob: (id: string) => void;
  onRemoveFile: (id: string) => void;
  onClearAll: () => void;
  onResetBatch: () => void;
  onAddFiles: () => void;
  onReveal?: (jobId: string) => void | Promise<void>;
  capabilities: AppCapabilitiesDto;
  isDragging?: boolean;
  isSubmitting?: boolean;
  /** Search text from the top bar; filters what is shown, not what converts. */
  query?: string;
  /** FFmpeg consent / download for the first video job. */
  videoEngine?: VideoEngineState | null;
  onAnswerVideoEngine?: (agreed: boolean) => void;
  onCancelVideoEngine?: () => void;
  /** Why the last action failed, until dismissed. */
  error?: string | null;
  onDismissError?: () => void;
}

/**
 * Measures its container and returns the pixel height.
 * Uses a callback ref so it safely handles conditionally rendered elements.
 */
function useMeasuredHeight() {
  const [height, setHeight] = useState(0);
  const [node, setNode] = useState<HTMLDivElement | null>(null);

  const ref = React.useCallback((el: HTMLDivElement | null) => {
    setNode(el);
  }, []);

  useEffect(() => {
    if (!node) return;
    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const h = entry.contentRect.height;
        if (h > 0) setHeight(h);
      }
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, [node]);

  return { height, ref };
}

export const QueueWorkspace: React.FC<QueueWorkspaceProps> = ({
  jobs,
  pendingFiles,
  summary,
  selectedJobId,
  selectedPendingId,
  onSelectJob,
  onSelectPending,
  onCancelJob,
  onRemoveFile,
  onClearAll,
  onResetBatch,
  onAddFiles,
  onReveal,
  capabilities,
  isDragging,
  isSubmitting,
  query = "",
  videoEngine = null,
  onAnswerVideoEngine = () => {},
  onCancelVideoEngine = () => {},
  error = null,
  onDismissError = () => {},
}) => {
  const { t } = useTranslation();
  const notice = error ? <ErrorNotice message={error} onDismiss={onDismissError} /> : null;
  const hasJobs = jobs.length > 0;
  const hasPending = pendingFiles.length > 0;
  const isEmpty = !hasJobs && !hasPending && !isSubmitting;

  const { height: jobsHeight, ref: jobsContainerRef } = useMeasuredHeight();

  const needle = query.trim().toLowerCase();
  const fileName = (path: string) => path.split(/[\\/]/).pop() ?? path;
  const shownFiles = needle ? pendingFiles.filter((f) => f.name.toLowerCase().includes(needle)) : pendingFiles;
  const shownJobs = needle ? jobs.filter((j) => fileName(j.request.input_path).toLowerCase().includes(needle)) : jobs;
  const noMatches = !!needle && shownFiles.length === 0 && shownJobs.length === 0;

  // ─── Empty State ───
  if (isEmpty) {
    return (
      <main className="shell-main flex flex-col overflow-hidden relative">
        <QueueHeader summary={summary} pendingFiles={pendingFiles} onClearAll={onClearAll} onResetBatch={onResetBatch} />
        {notice}
        <div className="flex-1 overflow-y-auto">
          <EmptyWorkspace onSelectFiles={onAddFiles} isDragging={isDragging} />
        </div>
      </main>
    );
  }

  // ─── Submitting Transition State ───
  if (isSubmitting && !hasJobs && !hasPending) {
    return (
      <main className="shell-main flex flex-col overflow-hidden relative">
        <QueueHeader summary={summary} pendingFiles={pendingFiles} onClearAll={onClearAll} onResetBatch={onResetBatch} onAddFiles={onAddFiles} />
        {notice}
        <div className="flex-1 flex flex-col items-center justify-center text-center" role="status" aria-live="polite">
          <div className="mb-4 h-6 w-6 animate-spin rounded-full border-2 border-border-strong border-t-transparent"></div>
          <h3 className="text-[13px] font-medium text-text-primary">{t("queue.starting")}</h3>
          <p className="mt-1 max-w-xs text-[12px] text-text-tertiary">{t("queue.preparing")}</p>
        </div>
      </main>
    );
  }

  // ─── Active Content ───
  return (
    <main className="shell-main flex flex-col overflow-hidden relative">
      <QueueHeader summary={summary} pendingFiles={pendingFiles} onClearAll={onClearAll} onResetBatch={onResetBatch} onAddFiles={onAddFiles} />

      {notice}

      {videoEngine && (
        <VideoEngineCard state={videoEngine} onAnswer={onAnswerVideoEngine} onCancel={onCancelVideoEngine} />
      )}

      {noMatches && (
        <div className="flex flex-1 flex-col items-center justify-center gap-1 p-8 text-center" role="status">
          <p className="text-[13px] text-text-secondary">{t("queue.noMatches", { query: query.trim() })}</p>
          <p className="text-[12px] text-text-tertiary">{t("queue.noMatchesHint")}</p>
        </div>
      )}

      {/* Content: no overflow-y-auto — each section scrolls internally via Virtuoso */}
      <div className={`flex-1 flex flex-col min-h-0 ${noMatches ? "hidden" : ""}`}>
        {/* Pending Manifest Grid */}
        {hasPending && (
          <div className={`flex flex-col min-h-0 ${hasJobs ? 'h-[45%] shrink-0 border-b border-separator' : 'flex-1'}`}>
            <ManifestGrid
              files={shownFiles}
              selectedId={selectedPendingId}
              onSelect={onSelectPending}
              onRemove={onRemoveFile}
              onAddFiles={onAddFiles}
            />
          </div>
        )}

        {/* Active Jobs (Virtualized with measured height) */}
        {hasJobs && (
          <div className="flex-1 flex flex-col min-h-0">
            {hasPending && (
              <div className="flex h-9 shrink-0 items-center px-6">
                <h3 className="zc-label">{t("queue.conversions")}</h3>
              </div>
            )}
            {/* Measured container for Virtuoso */}
            <div ref={jobsContainerRef} className="flex-1 min-h-0 relative">
              {jobsHeight > 0 && (
                <Virtuoso
                  style={{ height: jobsHeight, width: '100%' }}
                  overscan={800}
                  data={shownJobs}
                  computeItemKey={(_, job) => job.id}
                  itemContent={(_, job) => (
                    <QueueRow
                      key={job.id}
                      job={job}
                      isSelected={selectedJobId === job.id}
                      onSelect={(id) => onSelectJob(id === selectedJobId ? null : id)}
                      onCancel={onCancelJob}
                      onReveal={onReveal}
                      capabilities={capabilities}
                    />
                  )}
                />
              )}
            </div>
          </div>
        )}
      </div>
    </main>
  );
};
