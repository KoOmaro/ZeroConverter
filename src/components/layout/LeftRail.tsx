import React from "react";
import { ResizeMode, PreflightResultDto } from "../../application/converterTypes";
import { ArrowRight, ChevronLeft } from "lucide-react";
import { useTranslation } from "react-i18next";
import { FormatPicker } from "../common/FormatPicker";
import {
  AUDIO_TARGETS,
  BITRATES,
  DDS_FORMATS,
  QUALITY_TARGETS,
  SAMPLE_RATES,
  bitrateFor,
  type FormatOptions,
} from "../../application/formatOptions";

interface LeftRailProps {
  onAddFiles: () => void;
  selectedCount: number;
  format: string;
  setFormat: (f: string) => void;
  availableFormats: string[];
  isProcessing: boolean;
  isMixedBatch: boolean;
  detectedFamilies: Array<'image' | 'audio' | 'video'>;
  familyFormats: Record<string, string>;
  setFamilyFormat: (family: string, fmt: string) => void;
  familyFormatOptions: Record<string, string[]>;
  quality: number;
  setQuality: (val: number) => void;
  familyQualities: Record<string, number>;
  setFamilyQuality: (family: string, val: number) => void;
  overwrite: boolean;
  setOverwrite: (val: boolean) => void;
  resizeMode: ResizeMode;
  setResizeMode: (mode: ResizeMode) => void;
  resizeWidth: number | null;
  setResizeWidth: (w: number | null) => void;
  resizeHeight: number | null;
  setResizeHeight: (h: number | null) => void;
  onSubmit: () => void;
  canSubmit: boolean;
  submitHint?: string | null;
  version: string;
  preflightResult: PreflightResultDto | null;
  outputDirectory: string | null;
  formatOptions: FormatOptions;
  setFormatOptions: (next: FormatOptions) => void;
  collapsed: boolean;
  onToggleCollapsed: () => void;
}

export const LeftRail: React.FC<LeftRailProps> = ({
  onAddFiles: _onAddFiles,
  selectedCount,
  format,
  setFormat,
  availableFormats,
  isProcessing,
  isMixedBatch,
  detectedFamilies,
  familyFormats,
  setFamilyFormat,
  familyFormatOptions,
  quality,
  setQuality,
  familyQualities,
  setFamilyQuality,
  overwrite,
  setOverwrite,
  resizeMode,
  setResizeMode,
  resizeWidth,
  setResizeWidth,
  resizeHeight,
  setResizeHeight,
  onSubmit,
  canSubmit,
  submitHint,
  version: _version,
  preflightResult,
  outputDirectory: _outputDirectory,
  formatOptions,
  setFormatOptions,
  collapsed,
  onToggleCollapsed,
}) => {
  const { t } = useTranslation();
  const isIcoTarget = isMixedBatch
    ? Object.entries(familyFormats).some(([fam, fmt]) => fam === "image" && fmt === "ico")
    : format === "ico";
  // Resize and the image quality slider are for pictures only (not audio, not video).
  const hasImages = detectedFamilies.includes("image") || detectedFamilies.length === 0;
  const showResize = hasImages;
  const imageTarget = isMixedBatch ? familyFormats.image : hasImages ? format : undefined;
  const audioTarget = isMixedBatch ? familyFormats.audio : detectedFamilies.includes("audio") ? format : undefined;
  const showQuality = !!imageTarget && QUALITY_TARGETS.has(imageTarget);
  const showAudio = !!audioTarget && AUDIO_TARGETS.has(audioTarget);
  const bitrate = audioTarget ? BITRATES[audioTarget] : undefined;
  const showDds = imageTarget === "dds";
  const patch = (next: Partial<FormatOptions>) => setFormatOptions({ ...formatOptions, ...next });
  const field = "zc-field";

  const qualityRow = (id: string, label: React.ReactNode, value: number, onChange: (v: number) => void) => (
    <div key={id} className="space-y-1.5">
      <div className="flex items-center justify-between">
        <label htmlFor={id} className="type-control-label text-text-secondary">{label}</label>
        <span className="text-[12px] tabular-nums text-text-primary">{value}</span>
      </div>
      <input
        id={id}
        type="range"
        min="1"
        max="100"
        value={value}
        disabled={isProcessing}
        onChange={(e) => onChange(Number(e.target.value))}
        className="zc-range w-full"
        style={{ "--fill": `${value}%` } as React.CSSProperties}
      />
    </div>
  );

  const convertLabel = isProcessing
    ? t("rail.converting")
    : selectedCount > 0
      ? t("rail.convertCount", { count: selectedCount })
      : t("rail.convert");
  // Rail header: the whole row folds and unfolds the panel.
  const railHeader = (
    <button
      type="button"
      onClick={onToggleCollapsed}
      className="zc-rail-logo"
      data-press="wide"
      aria-label={collapsed ? t("rail.show") : t("rail.hide")}
      aria-expanded={!collapsed}
      title={collapsed ? t("rail.showTitle") : undefined}
    >
      {collapsed ? (
        <img src="/zeroconverter-mark.svg" alt="" width={20} height={20} className="h-5 w-5 shrink-0" />
      ) : (
        <>
          <span className="zc-rail-brand">ZeroConverter</span>
          <ChevronLeft size={14} aria-hidden="true" className="zc-rail-fold rtl:-scale-x-100" />
        </>
      )}
    </button>
  );

  if (collapsed) {
    return (
      <aside className="shell-left overflow-hidden flex flex-col items-center select-none">
        {railHeader}
        
        <div className="flex-1" />
        <button
          type="button"
          onClick={onSubmit}
          disabled={isProcessing || !canSubmit}
          title={submitHint ?? convertLabel}
          aria-label={convertLabel}
          className="zc-btn zc-btn-primary mb-3 !h-9 !w-9 !p-0"
        >
          <ArrowRight size={17} strokeWidth={2} aria-hidden="true" />
        </button>
      </aside>
    );
  }

  return (
    <aside className="shell-left overflow-hidden flex flex-col select-none">
      {railHeader}

      <div className="flex h-8 shrink-0 items-end px-4 pb-1.5">
        <h2 className="zc-label">{t("rail.convertTo")}</h2>
      </div>

      <div className="flex-1 min-h-0 overflow-y-auto px-4 pb-4 space-y-5">
        <section className="space-y-4">
          {isMixedBatch ? (
            detectedFamilies.map((family) => (
              <div key={family} className="space-y-2">
                <h3 className="zc-label">{t(`common.family.${family}`)}</h3>
                <FormatPicker
                  label={t("rail.familyOutputFormat", { family: t(`common.family.${family}`) })}
                  options={familyFormatOptions[family] ?? []}
                  value={familyFormats[family] ?? ""}
                  onChange={(selected) => setFamilyFormat(family, selected)}
                  disabled={isProcessing}
                />
              </div>
            ))
          ) : (
            <FormatPicker
              label={t("rail.outputFormat")}
              options={availableFormats}
              value={format}
              onChange={setFormat}
              disabled={isProcessing}
            />
          )}
          {isIcoTarget && (
            <p className="type-helper text-text-tertiary">{t("rail.icoLimit")}</p>
          )}
        </section>

        <section className="space-y-3">
          <h3 className="zc-label">{t("rail.options")}</h3>

          {showQuality &&
            (isMixedBatch
              ? qualityRow("quality-slider-image", t("rail.quality"), familyQualities.image ?? 80, (v) => setFamilyQuality("image", v))
              : qualityRow("quality-slider", t("rail.quality"), quality, setQuality))}

          {showDds && (
            <div className="space-y-3">
              <div className="space-y-1.5">
                <label htmlFor="dds-format" className="type-control-label text-text-secondary">{t("rail.compression")}</label>
                <select
                  id="dds-format"
                  disabled={isProcessing}
                  value={formatOptions.ddsFormat}
                  onChange={(e) => patch({ ddsFormat: e.target.value })}
                  className={field}
                >
                  {DDS_FORMATS.map((f) => (
                    <option key={f.id} value={f.id}>{t(`dds.${f.id}`, { name: f.id.toUpperCase() })}</option>
                  ))}
                </select>
              </div>
              <label className="flex cursor-pointer items-center gap-2.5 py-1">
                <input
                  type="checkbox"
                  checked={formatOptions.ddsMipmaps}
                  disabled={isProcessing}
                  onChange={(e) => patch({ ddsMipmaps: e.target.checked })}
                  className="zc-check"
                />
                <span className="type-control-label text-text-secondary">{t("rail.mipmaps")}</span>
              </label>
            </div>
          )}

          {showAudio && audioTarget && (
            <div className="space-y-3">
              {bitrate && (
                <div className="space-y-1.5">
                  <label htmlFor="audio-bitrate" className="type-control-label text-text-secondary">{t("rail.bitrate")}</label>
                  <select
                    id="audio-bitrate"
                    disabled={isProcessing}
                    value={bitrateFor(audioTarget, formatOptions) ?? bitrate.fallback}
                    onChange={(e) =>
                      patch({ bitrates: { ...formatOptions.bitrates, [audioTarget]: Number(e.target.value) } })
                    }
                    className={field}
                  >
                    {bitrate.choices.map((kbps) => (
                      <option key={kbps} value={kbps}>{kbps} kbps</option>
                    ))}
                  </select>
                </div>
              )}
              <div className="flex gap-2">
                <div className="flex-1 space-y-1.5">
                  <label htmlFor="audio-rate" className="type-control-label text-text-secondary">{t("rail.sampleRate")}</label>
                  <select
                    id="audio-rate"
                    disabled={isProcessing}
                    value={formatOptions.sampleRate ?? ""}
                    onChange={(e) => patch({ sampleRate: e.target.value ? Number(e.target.value) : null })}
                    className={field}
                  >
                    <option value="">{t("rail.original")}</option>
                    {SAMPLE_RATES.map((hz) => (
                      <option key={hz} value={hz}>{hz / 1000} kHz</option>
                    ))}
                  </select>
                </div>
                <div className="flex-1 space-y-1.5">
                  <label htmlFor="audio-channels" className="type-control-label text-text-secondary">{t("rail.channels")}</label>
                  <select
                    id="audio-channels"
                    disabled={isProcessing}
                    value={formatOptions.mono ? "mono" : "keep"}
                    onChange={(e) => patch({ mono: e.target.value === "mono" })}
                    className={field}
                  >
                    <option value="keep">{t("rail.original")}</option>
                    <option value="mono">{t("rail.mono")}</option>
                  </select>
                </div>
              </div>
            </div>
          )}

          {showResize && (
            <div className="space-y-1.5">
              <label htmlFor="resize-mode" className="type-control-label text-text-secondary">{t("rail.resize")}</label>
              <select
                id="resize-mode"
                disabled={isProcessing}
                value={resizeMode}
                onChange={(e) => setResizeMode(e.target.value as ResizeMode)}
                className={field}
              >
                <option value="none">{t("rail.resizeNone")}</option>
                <option value="max_width">{t("rail.resizeMaxWidth")}</option>
                <option value="max_height">{t("rail.resizeMaxHeight")}</option>
                <option value="fit_box">{t("rail.resizeFitBox")}</option>
              </select>

              {resizeMode !== "none" && (
                <div className="flex gap-2 pt-1">
                  {(resizeMode === "max_width" || resizeMode === "fit_box") && (
                    <input
                      type="number"
                      inputMode="numeric"
                      autoComplete="off"
                      min="1"
                      max="16384"
                      placeholder={t("rail.widthPlaceholder")}
                      disabled={isProcessing}
                      value={resizeWidth ?? ""}
                      onChange={(e) => setResizeWidth(e.target.value ? Math.floor(Number(e.target.value)) : null)}
                      aria-label={t("rail.widthLabel")}
                      className={field}
                    />
                  )}
                  {(resizeMode === "max_height" || resizeMode === "fit_box") && (
                    <input
                      type="number"
                      inputMode="numeric"
                      autoComplete="off"
                      min="1"
                      max="16384"
                      placeholder={t("rail.heightPlaceholder")}
                      disabled={isProcessing}
                      value={resizeHeight ?? ""}
                      onChange={(e) => setResizeHeight(e.target.value ? Math.floor(Number(e.target.value)) : null)}
                      aria-label={t("rail.heightLabel")}
                      className={field}
                    />
                  )}
                </div>
              )}
            </div>
          )}

          <label
            className="flex cursor-pointer items-center gap-2.5 py-1"
            title={overwrite ? t("rail.overwriteOn") : t("rail.overwriteOff")}
          >
            <input
              type="checkbox"
              checked={overwrite}
              disabled={isProcessing}
              onChange={(e) => setOverwrite(e.target.checked)}
              className="zc-check"
            />
            <span className="type-control-label text-text-secondary">{t("rail.overwrite")}</span>
          </label>
        </section>

        {preflightResult && preflightResult.is_valid === false && preflightResult.severity === "error" && (
          <div className="rounded-card border border-warning-border bg-warning-soft p-3" role="alert">
            <p className="text-[12px] leading-snug text-warning">{preflightResult.message}</p>
            {preflightResult.suggested_action === "FIT_BOX_256" && (
              <button
                type="button"
                onClick={() => {
                  setResizeMode("fit_box");
                  setResizeWidth(256);
                  setResizeHeight(256);
                }}
                className="zc-btn zc-btn-sm mt-2 border-warning-border text-warning hover:bg-warning-border/40"
              >
                {t("rail.fitTo256")}
              </button>
            )}
          </div>
        )}
      </div>

      <div className="shrink-0 space-y-2 p-4">
        {submitHint && !isProcessing && (
          <p className={`text-[12px] leading-relaxed ${canSubmit ? "text-text-tertiary" : "text-warning"}`} role="status">
            {submitHint}
          </p>
        )}
        <button
          type="button"
          onClick={onSubmit}
          disabled={isProcessing || !canSubmit}
          className="zc-btn zc-btn-lg zc-btn-primary w-full"
          data-press="wide"
        >
          {convertLabel}
        </button>
      </div>
    </aside>
  );
};
