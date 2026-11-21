import React from "react";
import { Film } from "lucide-react";
import { useTranslation } from "react-i18next";
import type { VideoEngineState } from "../../application/converterTypes";

interface VideoEngineCardProps {
  state: VideoEngineState;
  onAnswer: (agreed: boolean) => void;
  onCancel: () => void;
}

const mb = (bytes: number) => `${Math.max(1, Math.round(bytes / (1024 * 1024)))} MB`;

/**
 * Shown once, the first time a video goes in: asks before FFmpeg is
 * downloaded, then shows the download until it is ready.
 */
export const VideoEngineCard: React.FC<VideoEngineCardProps> = ({ state, onAnswer, onCancel }) => {
  const { t } = useTranslation();
  if (state.stage === "ask") {
    return (
      <section className="zc-engine-card" aria-labelledby="zc-engine-title">
        <Film size={18} strokeWidth={1.75} aria-hidden="true" className="mt-0.5 shrink-0 text-text-secondary" />
        <div className="min-w-0 flex-1">
          <h3 id="zc-engine-title" className="text-[13px] font-semibold text-text-primary">
            {t("video.needsTitle")}
          </h3>
          <p className="mt-1 text-[12px] leading-relaxed text-text-secondary">
            {t("video.needsBody", { size: mb(state.bytes), host: state.host })}
          </p>
          <p className="mt-1.5 text-[11px] text-text-tertiary">
            {state.label} · {state.license}
          </p>
          <div className="mt-3 flex gap-2">
            <button type="button" data-press className="zc-btn zc-btn-primary" onClick={() => onAnswer(true)} autoFocus>
              {t("common.download")}
            </button>
            <button type="button" data-press className="zc-btn zc-btn-secondary" onClick={() => onAnswer(false)}>
              {t("video.notNow")}
            </button>
          </div>
        </div>
      </section>
    );
  }

  const percent = state.total > 0 ? Math.min(100, Math.round((state.done / state.total) * 100)) : 0;
  const title =
    state.stage === "download" ? t("video.downloading") : state.stage === "verify" ? t("video.verifying") : t("video.unpacking");
  const detail =
    state.stage === "download"
      ? t("video.progressOf", { done: mb(state.done), total: mb(state.total) })
      : state.stage === "verify"
        ? t("video.compareChecksum")
        : t("video.filesOf", { done: state.done, total: state.total });

  return (
    <section className="zc-engine-card" aria-labelledby="zc-engine-title">
      <Film size={18} strokeWidth={1.75} aria-hidden="true" className="mt-0.5 shrink-0 text-text-secondary" />
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline justify-between gap-3">
          <h3 id="zc-engine-title" className="text-[13px] font-semibold text-text-primary" aria-live="polite">
            {title}
          </h3>
          <span className="text-[12px] tabular-nums text-text-secondary">{percent}%</span>
        </div>
        <div
          className="mt-2 h-1 overflow-hidden rounded-full bg-surface-3"
          role="progressbar"
          aria-label={title}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-valuenow={percent}
        >
          <div className="h-full bg-accent-strong transition-[width] duration-200 ease-out" style={{ width: `${percent}%` }} />
        </div>
        <div className="mt-2 flex items-center justify-between gap-3">
          <p className="text-[11px] tabular-nums text-text-tertiary">{detail}</p>
          {state.stage !== "verify" && (
            <button type="button" data-press className="zc-btn zc-btn-sm zc-btn-ghost" onClick={onCancel}>
              {t("common.cancel")}
            </button>
          )}
        </div>
      </div>
    </section>
  );
};
