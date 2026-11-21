export type ResizeMode = "none" | "max_width" | "max_height" | "fit_box";

export interface FormatDescriptorDto {
  id: string;
  canonical_extension: string;
  aliases: string[];
  can_read: boolean;
  can_write: boolean;
}

export interface ConversionRouteDto {
  input: string;
  outputs: string[];
}

export interface FamilyCapabilitiesDto {
  family: string;
  descriptors: FormatDescriptorDto[];
  routes: ConversionRouteDto[];
}

export interface FfmpegStatus {
  available: boolean;
  /** "managed" (downloaded by the app), "system" (already installed) or "override". */
  source: "managed" | "system" | "override" | null;
  /** The ffmpeg executable in use, when found. */
  path: string | null;
  version: string | null;
  canInstall: boolean;
  downloadBytes: number;
  downloadUrl: string;
  sha256: string;
  label: string;
  license: string;
}

export interface FfmpegInstallProgress {
  phase: "download" | "verify" | "extract";
  done: number;
  total: number;
}

/** What the video engine card shows: the one-time consent, then progress. */
export type VideoEngineState =
  | { stage: "ask"; label: string; bytes: number; host: string; license: string }
  | { stage: FfmpegInstallProgress["phase"]; label: string; done: number; total: number };

export interface AppCapabilitiesDto {
  families: FamilyCapabilitiesDto[];
}

export interface FileMetadata {
  path: string;
  name: string;
  extension: string;
  size_bytes: number;
  kind: string;
  probe_status: string;
  duration_seconds?: number;
  sample_rate?: number;
  channels?: number;
}

export interface JobProgressDto {
  stage: string;
  message: string;
  percent: number | null;
}

export interface JobFailureDto {
  stage: string;
  kind: string;
  message: string;
}

export interface FileIntegrityDto {
  byte_size: number;
}

export interface ConversionResultDto {
  success: boolean;
  output_path: string | null;
  message: string;
  input_integrity?: FileIntegrityDto | null;
  output_integrity?: FileIntegrityDto | null;
}

export interface JobRequestSnapshot {
  input_path: string;
  output_path: string;
  output_format: string;
  quality: number;
  overwrite: boolean;
  resize_mode: ResizeMode;
  resize_width: number | null;
  resize_height: number | null;
}

export interface JobStatusDto {
  id: string;
  status: string;
  request: JobRequestSnapshot;
  progress: JobProgressDto;
  failure: JobFailureDto | null;
  result: ConversionResultDto | null;
}

export interface QueueSummaryDto {
  status: string;
  total: number;
  queued: number;
  running: number;
  completed: number;
  failed: number;
  cancelled: number;
}

export interface BatchItemRequest {
  input_path: string;
  output_folder: string | null;
  output_format: string;
  quality: number;
  overwrite: boolean;
  resize_mode: ResizeMode;
  resize_width: number | null;
  resize_height: number | null;
  bitrate_kbps?: number | null;
  sample_rate?: number | null;
  channels?: number | null;
  dds_format?: string | null;
  dds_mipmaps?: boolean;
  /** The user agreed to convert a file over 100 MB. */
  confirm_large?: boolean;
}

export type JobEventDto =
  | { type: "JobStarted"; payload: { id: string } }
  | { type: "JobStatusChanged"; payload: { id: string; status: string } }
  | { type: "JobProgressUpdated"; payload: { id: string; progress: JobProgressDto } }
  | { type: "JobCompleted"; payload: { id: string; result: ConversionResultDto } }
  | { type: "JobFailed"; payload: { id: string; failure: JobFailureDto } }
  | { type: "QueueSummaryChanged"; payload: { summary: QueueSummaryDto } };

export interface PreflightRequestDto {
  token: number;
  conversion: BatchItemRequest;
}

export interface PreflightResultDto {
  token: number;
  is_valid: boolean;
  severity: string | null;
  code: string | null;
  message: string | null;
  current_width: number | null;
  current_height: number | null;
  max_width: number | null;
  max_height: number | null;
  suggested_action: string | null;
}
