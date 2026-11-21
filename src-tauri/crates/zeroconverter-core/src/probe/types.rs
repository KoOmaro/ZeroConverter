use crate::conversion::types::AssetFamily;
use serde::{Deserialize, Serialize};

/// How far a file was recognized: by extension only, or by its contents.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProbeStatus {
    /// Extension-matched only; the contents were not looked at.
    Provisional,
    /// The format handler recognized the contents (signature or header).
    /// Not a guarantee the whole file decodes.
    Recognized,
    /// Probe attempted but failed (malformed, corrupt, or unsupported).
    Failed,
}

/// Core-domain file metadata. NOT a DTO — stays inside Core.
#[derive(Debug, Clone)]
pub struct FileProbeResult {
    pub path: String,
    pub name: String,
    pub extension: String,
    pub size_bytes: u64,
    pub family: AssetFamily,
    pub probe_status: ProbeStatus,
    /// Audio-specific metadata. None for non-audio families.
    pub audio_meta: Option<AudioProbeData>,
}

/// Cheap audio metadata extracted during probe.
#[derive(Debug, Clone)]
pub struct AudioProbeData {
    pub duration_seconds: f64,
    pub sample_rate: u32,
    pub channels: u32,
}
