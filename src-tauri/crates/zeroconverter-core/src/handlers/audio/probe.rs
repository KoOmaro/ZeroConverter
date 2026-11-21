use crate::app_error::AppError;
use std::path::Path;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::default::get_probe;

#[derive(Debug)]
pub struct AudioMetadata {
    pub format: String,
    pub duration_seconds: f64,
    pub sample_rate: u32,
    pub channels: u32,
    pub bitrate_kbps: u32,
    /// Bits per sample of uncompressed or lossless sources; 0 when the
    /// codec has no fixed depth (MP3, AAC, Vorbis, Opus).
    pub bits_per_sample: u32,
}

/// Probes audio metadata with a "cheap-first" discipline.
/// Prioritizes responsiveness over exhaustive metadata scanning.
pub fn probe_audio(path: &Path) -> Result<AudioMetadata, AppError> {
    // Symphonia 0.5 does not decode Opus. Inspect Ogg/Opus headers with
    // the dedicated bounded pure-Rust reader instead.
    if path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("opus"))
    {
        let spec = super::formats::opus::inspect(path)?;
        return Ok(AudioMetadata {
            format: "audio".to_string(),
            duration_seconds: 0.0, // Unknown until Ogg granules are scanned.
            sample_rate: spec.sample_rate,
            channels: spec.channels as u32,
            bitrate_kbps: 0,
            bits_per_sample: 0,
        });
    }

    let file = std::fs::File::open(path)
        .map_err(|e| AppError::Io(format!("Resource access failed: {}", e)))?;

    let mss = MediaSourceStream::new(Box::new(file), Default::default());

    let probe = get_probe()
        .format(
            &Default::default(),
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| AppError::Probe(format!("Incompatible or malformed audio: {:?}", e)))?;

    // Identify track info (Cheaply available from headers)
    let track = probe
        .format
        .tracks()
        .iter()
        // Video containers (MP4) list the picture track first; take the
        // first track that actually carries decodable audio.
        .find(|t| {
            t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL
                && t.codec_params.sample_rate.is_some()
        })
        .or_else(|| probe.format.default_track())
        .ok_or_else(|| AppError::Probe("No valid audio track identified".to_string()))?;

    let params = &track.codec_params;
    let sample_rate = params.sample_rate.unwrap_or(0);
    let channels = params.channels.map(|c| c.count() as u32).unwrap_or(0);

    // Duration calculation from frames (avoiding deep scanning)
    let duration = if let Some(n_frames) = params.n_frames {
        if sample_rate > 0 {
            n_frames as f64 / sample_rate as f64
        } else {
            0.0
        }
    } else {
        0.0
    };

    // Approximate bitrate from codec parameters
    let bitrate = if let Some(br) = params.bits_per_sample {
        br.saturating_mul(channels).saturating_mul(sample_rate) / 1000
    } else {
        0 // Unknown or VBR
    };

    Ok(AudioMetadata {
        format: "audio".to_string(), // Canonical asset family
        duration_seconds: duration,
        sample_rate,
        channels,
        bitrate_kbps: bitrate,
        bits_per_sample: params.bits_per_sample.unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::{create_malformed_file, create_test_wav, MalformedMode};

    #[test]
    fn probe_valid_wav_returns_metadata() {
        let channels = 2;
        let sample_rate = 44100;
        let file = create_test_wav(channels, sample_rate, 1000);

        let result = probe_audio(file.path());
        assert!(result.is_ok());

        let meta = result.unwrap();
        assert_eq!(meta.channels, channels as u32);
        assert_eq!(meta.sample_rate, sample_rate);
        assert!(meta.duration_seconds >= 1.0 && meta.duration_seconds < 1.1);
    }

    #[test]
    fn probe_rejects_nonexistent_file() {
        let path = Path::new("nonexistent_audio_file_999.wav");
        let result = probe_audio(path);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Resource access failed"));
    }

    #[test]
    fn probe_rejects_empty_file() {
        let file = create_malformed_file(MalformedMode::Empty, "wav");
        let result = probe_audio(file.path());
        assert!(result.is_err());
        // Symphonia usually fails with a probe error on empty files
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Incompatible or malformed audio"));
    }

    #[test]
    fn probe_rejects_garbage_content() {
        let file = create_malformed_file(MalformedMode::Garbage, "wav");
        let result = probe_audio(file.path());
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Incompatible or malformed audio"));
    }
    #[test]
    fn probe_rejects_corrupt_aac_and_m4a_headers() {
        for (extension, data) in [
            ("aac", b"\xff\xf1\x50\x80".as_slice()),
            ("m4a", b"\0\0\0\x18ftypM4A ".as_slice()),
        ] {
            let dir = tempfile::tempdir().expect("tempdir");
            let path = dir.path().join(format!("truncated.{extension}"));
            std::fs::write(&path, data).expect("write incomplete header");
            assert!(
                probe_audio(&path).is_err(),
                "{extension} must reject truncation"
            );
        }
    }
}
