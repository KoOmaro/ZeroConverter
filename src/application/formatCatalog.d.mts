import type { AppCapabilitiesDto, FormatDescriptorDto } from "./converterTypes";

export function findFormatByExtension(
  capabilities: AppCapabilitiesDto | null,
  extension: string
): { family: string; descriptor: FormatDescriptorDto } | null;

export function availableOutputFormats(
  capabilities: AppCapabilitiesDto | null,
  familyName: string,
  inputExtensions?: string[]
): string[];

export function pickSupportedFormat(preferred: string, available: string[]): string;
export function filterOutputFormats(formats: string[], query: string): string[];
