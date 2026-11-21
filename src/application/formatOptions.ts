// Format-specific settings: which controls a target format shows, and what
// each one sends to the core. Formats not listed here take no extra settings.

export interface FormatOptions {
  /** Per target format, e.g. { mp3: 320, opus: 128 }. */
  bitrates: Record<string, number>;
  /** null keeps the source rate. */
  sampleRate: number | null;
  mono: boolean;
  ddsFormat: string;
  ddsMipmaps: boolean;
}

export const DEFAULT_FORMAT_OPTIONS: FormatOptions = {
  bitrates: {},
  sampleRate: null,
  mono: false,
  ddsFormat: "bc7",
  ddsMipmaps: false,
};

/** Image encoders that honour the quality slider. */
export const QUALITY_TARGETS = new Set(["jpg", "avif"]);

export const BITRATES: Record<string, { choices: number[]; fallback: number }> = {
  mp3: { choices: [128, 192, 256, 320], fallback: 320 },
  opus: { choices: [64, 96, 128, 192], fallback: 128 },
};

export const AUDIO_TARGETS = new Set(["wav", "mp3", "flac", "aiff", "opus"]);

export const SAMPLE_RATES = [44_100, 48_000];

export const DDS_FORMATS: Array<{ id: string; label: string }> = [
  { id: "bc7", label: "BC7 · best colour + alpha" },
  { id: "bc3", label: "BC3 · colour + alpha" },
  { id: "bc1", label: "BC1 · colour, smallest" },
  { id: "bc4", label: "BC4 · single channel" },
  { id: "bc5", label: "BC5 · normal maps" },
  { id: "bc2", label: "BC2 · sharp alpha (legacy)" },
];

export function bitrateFor(target: string, options: FormatOptions): number | null {
  const spec = BITRATES[target];
  return spec ? (options.bitrates[target] ?? spec.fallback) : null;
}

/** Request fields for one file. Fields that do not apply stay null. */
export function formatFields(kind: string, target: string, options: FormatOptions) {
  const audio = kind === "audio" && AUDIO_TARGETS.has(target);
  const dds = kind === "image" && target === "dds";
  return {
    bitrate_kbps: audio ? bitrateFor(target, options) : null,
    sample_rate: audio ? options.sampleRate : null,
    channels: audio && options.mono ? 1 : null,
    dds_format: dds ? options.ddsFormat : null,
    dds_mipmaps: dds && options.ddsMipmaps,
  };
}
