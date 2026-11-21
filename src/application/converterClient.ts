import { converterGateway } from "../platform/tauri/converterGateway";
import i18n from "../i18n";
import { JobStatusDto, BatchItemRequest, QueueSummaryDto, JobEventDto, FileMetadata, PreflightRequestDto, PreflightResultDto, AppCapabilitiesDto, FfmpegStatus, FfmpegInstallProgress, VideoEngineState } from "./converterTypes";

/** Thrown when the user says "Not now" or stops the download: not an error to show. */
export class VideoEngineDeclined extends Error {
  constructor() {
    super("Video conversion needs FFmpeg. Nothing was downloaded.");
    this.name = "VideoEngineDeclined";
  }
}

/**
 * Application-layer client for conversion operations.
 * 
 * ARCHITECTURE: This layer exists as a mandatory boundary between
 * presentation stores and the transport gateway (converterGateway).
 * Per project rules, React components and stores must never call
 * Tauri invoke directly.
 * 
 * Currently pass-through by design — domain-level concerns like
 * retry logic, request normalization, error mapping, and caching
 * will be added here as the product matures, WITHOUT modifying
 * the gateway or stores.
 */
export const converterClient = {
  async getStatus() {
    return await converterGateway.getAppStatus();
  },

  async getAppCapabilities(): Promise<AppCapabilitiesDto> {
    return await converterGateway.getAppCapabilities();
  },

  async attachJobEvents(onEvent: (ev: JobEventDto) => void): Promise<() => void> {
    return await converterGateway.attachJobEvents(onEvent);
  },

  async ffmpegStatus(): Promise<FfmpegStatus> {
    return await converterGateway.ffmpegStatus();
  },

  /**
   * Video conversion needs FFmpeg, which is downloaded once with the user's
   * consent. `show` drives the in-app card: first the question (its promise
   * resolves with the answer), then progress. Resolves when FFmpeg is usable;
   * throws `VideoEngineDeclined` on "Not now" or Cancel, an Error on failure.
   */
  async ensureVideoEngine(show: (state: VideoEngineState) => Promise<boolean> | void): Promise<void> {
    const status = await converterGateway.ffmpegStatus();
    if (status.available) return;
    if (!status.canInstall) {
      throw new Error(i18n.t("error.needsFfmpeg"));
    }
    const agreed = await show({
      stage: "ask",
      label: status.label,
      bytes: status.downloadBytes,
      host: new URL(status.downloadUrl).host,
      license: status.license,
    });
    if (!agreed) throw new VideoEngineDeclined();
    show({ stage: "download", label: status.label, done: 0, total: status.downloadBytes });
    try {
      await converterGateway.ffmpegInstall((p) => {
        show({ stage: p.phase, label: status.label, done: p.done, total: p.total });
      });
    } catch (e) {
      if (String(e).includes("CANCELLED")) throw new VideoEngineDeclined();
      throw e;
    }
  },

  /** Downloads FFmpeg now (Settings → Video). Rejects with "CANCELLED" when stopped. */
  async installVideoEngine(onProgress: (p: FfmpegInstallProgress) => void): Promise<FfmpegStatus> {
    return await converterGateway.ffmpegInstall(onProgress);
  },

  /** Stops the FFmpeg download; `ensureVideoEngine` then throws `VideoEngineDeclined`. */
  async cancelVideoEngine(): Promise<void> {
    await converterGateway.ffmpegInstallCancel();
  },

  async submitBatch(items: BatchItemRequest[]): Promise<{ job_ids: string[] }> {
    return await converterGateway.submitBatch(items);
  },

  async preflightConversion(req: PreflightRequestDto): Promise<PreflightResultDto> {
    return await converterGateway.preflightConversion(req);
  },

  async getQueueStatus(): Promise<QueueSummaryDto> {
    return await converterGateway.getQueueStatus();
  },

  async listJobs(): Promise<JobStatusDto[]> {
    return await converterGateway.listJobs();
  },

  async getJobStatus(jobId: string): Promise<JobStatusDto> {
    return await converterGateway.getJobStatus(jobId);
  },

  async cancelJob(jobId: string): Promise<void> {
    return await converterGateway.cancelJob(jobId);
  },

  async purgeJobs(): Promise<void> {
    return await converterGateway.purgeJobs();
  },

  async getFilesMetadata(paths: string[]): Promise<FileMetadata[]> {
    return await converterGateway.getFilesMetadata(paths);
  },
};
