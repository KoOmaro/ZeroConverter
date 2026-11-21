import React, { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { quickJob, QUALITY_TARGETS, BITRATES, type QuickJobInfo } from "../../application/renameConvert";

interface QuickConvertProps {
  id: string;
}

/**
 * The small window a `+` rename opens (`photo.webp+`): the few options that
 * matter for this conversion, Enter to convert, Esc to cancel.
 */
export const QuickConvert: React.FC<QuickConvertProps> = ({ id }) => {
  const { t } = useTranslation();
  const [info, setInfo] = useState<QuickJobInfo | null>(null);
  const [quality, setQuality] = useState(90);
  const [maxSide, setMaxSide] = useState<string>("");
  const [kbps, setKbps] = useState<number | null>(null);

  useEffect(() => {
    quickJob.info(id).then(setInfo).catch(() => setInfo(null));
  }, [id]);

  const confirm = () => {
    const side = Number(maxSide);
    const resize = side > 0;
    quickJob
      .confirm(id, {
        quality,
        resize_mode: resize ? "fit_box" : "none",
        resize_width: resize ? side : null,
        resize_height: resize ? side : null,
        bitrate_kbps: bitrate ? (kbps ?? bitrate.fallback) : null,
      })
      .catch(() => {});
  };
  const cancel = () => {
    quickJob.cancel(id).catch(() => {});
  };

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") cancel();
      // A focused button or menu answers Enter itself (Cancel must stay Cancel).
      const t = e.target as HTMLElement | null;
      const ownsEnter = !!t?.closest("button, select, a, textarea, [role='button']");
      if (e.key === "Enter" && !ownsEnter) confirm();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  const lossy = info ? QUALITY_TARGETS.has(info.target) : false;
  const bitrate = info ? BITRATES[info.target] : undefined;
  const isImage = info?.family === "image";
  const stem = info?.name.replace(/\.[^.]+$/, "") ?? "";

  return (
    <div className="flex h-screen flex-col bg-titlebar text-text-primary select-none">
      <div data-tauri-drag-region className="flex h-11 shrink-0 items-center gap-2 px-4">
        <img src="/zeroconverter-mark.svg" alt="" width={16} height={16} className="pointer-events-none h-4 w-4" />
        <span className="pointer-events-none font-brand text-[13px] font-semibold">ZeroConverter</span>
      </div>

      <div className="flex-1 space-y-5 px-4 pb-4">
        {info ? (
          <p className="truncate text-[15px] font-medium" title={info.name}>
            <span className="text-text-secondary">{stem}.</span>
            <span className="font-mono text-text-tertiary">{info.source}</span>
            <span className="px-2 text-text-tertiary">→</span>
            <span className="font-mono text-accent-strong">{info.target}</span>
          </p>
        ) : (
          <p className="text-[13px] text-text-tertiary">{t("quick.gone")}</p>
        )}

        {info?.large_bytes && (
          <p className="text-[12px] leading-snug text-warning" role="note">
            {t("quick.largeFile", { size: formatLargeSize(info.large_bytes) })}
          </p>
        )}

        {lossy && (
          <div className="space-y-1.5">
            <div className="flex items-center justify-between">
              <label htmlFor="quick-quality" className="type-control-label text-text-secondary">{t("quick.quality")}</label>
              <span className="font-mono text-[12px] tabular-nums">{quality}</span>
            </div>
            <input
              id="quick-quality"
              type="range"
              min="1"
              max="100"
              value={quality}
              onChange={(e) => setQuality(Number(e.target.value))}
              className="zc-range w-full"
              style={{ "--fill": `${quality}%` } as React.CSSProperties}
              autoFocus
            />
          </div>
        )}

        {bitrate && (
          <div className="space-y-1.5">
            <label htmlFor="quick-bitrate" className="type-control-label text-text-secondary">{t("quick.bitrate")}</label>
            <select
              id="quick-bitrate"
              value={kbps ?? bitrate.fallback}
              onChange={(e) => setKbps(Number(e.target.value))}
              className="h-8 w-full rounded-control border border-input-border bg-input-bg px-2 font-mono text-[12px] text-text-primary"
              autoFocus
            >
              {bitrate.choices.map((k) => (
                <option key={k} value={k}>{k} kbps</option>
              ))}
            </select>
          </div>
        )}

        {isImage && (
          <div className="space-y-1.5">
            <label htmlFor="quick-size" className="type-control-label text-text-secondary">{t("quick.longestSide")}</label>
            <input
              id="quick-size"
              type="number"
              inputMode="numeric"
              autoComplete="off"
              min="1"
              max="16384"
              placeholder={t("quick.original")}
              value={maxSide}
              onChange={(e) => setMaxSide(e.target.value)}
              className="h-8 w-full rounded-control border border-input-border bg-input-bg px-2 font-mono text-[12px] text-text-primary placeholder:text-text-tertiary"
              autoFocus={!lossy}
            />
          </div>
        )}
      </div>

      <div className="flex shrink-0 justify-end gap-2 px-4 pb-4">
        <button
          type="button"
          onClick={cancel}
          className="h-8 rounded-control px-3 text-[13px] text-text-secondary transition-colors duration-150 hover:bg-surface-2 hover:text-text-primary"
        >
          {t("quick.cancel")}
        </button>
        <button
          type="button"
          onClick={confirm}
          disabled={!info}
          className="h-8 rounded-control bg-accent px-4 text-[13px] font-medium text-on-accent transition-colors duration-150 hover:bg-accent-hover disabled:opacity-40"
        >
          {t("quick.convert")}
        </button>
      </div>
    </div>
  );
};

function formatLargeSize(bytes: number): string {
  const gb = bytes / 1024 ** 3;
  return gb >= 1 ? `${gb.toFixed(1)} GB` : `${Math.round(bytes / 1024 ** 2)} MB`;
}
