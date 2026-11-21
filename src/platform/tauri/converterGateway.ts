import { transportInvoke, createChannel } from "./ipc";

import {
  FileMetadata,
  JobStatusDto,
  QueueSummaryDto,
  BatchItemRequest,
  JobEventDto,
  PreflightRequestDto,
  PreflightResultDto,
  AppCapabilitiesDto,
  FfmpegStatus,
  FfmpegInstallProgress
} from "../../application/converterTypes";
interface RawFfmpegStatus {
  available: boolean;
  source: FfmpegStatus["source"];
  path?: string | null;
  version: string | null;
  can_install: boolean;
  download_bytes: number;
  download_url: string;
  sha256: string;
  label: string;
  license: string;
}

function mapFfmpegStatus(raw: RawFfmpegStatus): FfmpegStatus {
  return {
    available: raw.available,
    source: raw.source,
    path: raw.path ?? null,
    version: raw.version,
    canInstall: raw.can_install,
    downloadBytes: raw.download_bytes,
    downloadUrl: raw.download_url,
    sha256: raw.sha256,
    label: raw.label,
    license: raw.license,
  };
}

/**
 * Low-level transport gateway.
 * Zero logic, only mapping to Tauri transport primitives.
 */
export const converterGateway = {
  async getAppStatus() {
    return await transportInvoke<any>("get_app_status");
  },

  async getAppCapabilities(): Promise<AppCapabilitiesDto> {
    return await transportInvoke<AppCapabilitiesDto>("get_app_capabilities");
  },

  async attachJobEvents(onEvent: (ev: JobEventDto) => void): Promise<() => void> {
    const channel = createChannel<JobEventDto>();
    channel.onmessage = onEvent;
    
    // Attach the observer to the backend
    await transportInvoke("attach_job_events", { channel });
    
    // Return cleanup function to detach the listener
    return () => {
      channel.onmessage = () => {};
    };
  },

  async ffmpegStatus(): Promise<FfmpegStatus> {
    return mapFfmpegStatus(await transportInvoke<RawFfmpegStatus>("ffmpeg_status"));
  },

  async ffmpegInstall(onProgress: (p: FfmpegInstallProgress) => void): Promise<FfmpegStatus> {
    const channel = createChannel<FfmpegInstallProgress>();
    channel.onmessage = onProgress;
    return mapFfmpegStatus(await transportInvoke<RawFfmpegStatus>("ffmpeg_install", { onEvent: channel }));
  },

  async ffmpegInstallCancel(): Promise<void> {
    await transportInvoke("ffmpeg_install_cancel");
  },

  async submitBatch(conversions: BatchItemRequest[]): Promise<{ job_ids: string[] }> {
    return await transportInvoke<{ job_ids: string[] }>("submit_conversion_batch", {
      request: { conversions },
    });
  },

  async preflightConversion(request: PreflightRequestDto): Promise<PreflightResultDto> {
    return await transportInvoke<PreflightResultDto>("preflight_conversion", { request });
  },

  async getQueueStatus(): Promise<QueueSummaryDto> {
    return await transportInvoke<QueueSummaryDto>("get_queue_status");
  },

  async listJobs(): Promise<JobStatusDto[]> {
    return await transportInvoke<JobStatusDto[]>("list_jobs");
  },

  async getJobStatus(jobId: string): Promise<JobStatusDto> {
    return await transportInvoke<JobStatusDto>("get_job_status", { jobId });
  },

  async cancelJob(jobId: string): Promise<void> {
    return await transportInvoke<void>("cancel_job", { jobId });
  },

  async purgeJobs(): Promise<void> {
    return await transportInvoke<void>("purge_jobs");
  },

  async getFilesMetadata(paths: string[]): Promise<FileMetadata[]> {
    const res = await transportInvoke<{ results: FileMetadata[] }>("get_files_metadata", {
      request: { paths },
    });
    return res.results;
  },
};
