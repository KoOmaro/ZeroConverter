//! Streaming, bounded per-packet PCM handoff used by every encoder.
//! This code never collects the entire decoded source in memory.
use crate::app_error::AppError;
use std::path::Path;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::default::{get_codecs, get_probe};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioSpec {
    pub sample_rate: u32,
    pub channels: usize,
}

impl AudioSpec {
    fn checked(self) -> Result<Self, AppError> {
        if !(8_000..=192_000).contains(&self.sample_rate) || !(1..=8).contains(&self.channels) {
            return Err(AppError::Execute(format!(
                "Unsupported or unsafe audio header: {} Hz, {} channels",
                self.sample_rate, self.channels
            )));
        }
        Ok(self)
    }
}

pub fn inspect(input: &Path) -> Result<AudioSpec, AppError> {
    if input
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("opus"))
    {
        return super::formats::opus::inspect(input);
    }

    let metadata = super::probe::probe_audio(input)?;
    if metadata.channels == 0 {
        // AAC in MP4 often leaves the layout to the decoder; learn it from
        // the first decoded packet instead of rejecting the file.
        let mut found = None;
        let _ = stream_pcm(input, |_, spec| {
            found = Some(spec);
            Err(AppError::Execute(FIRST_PACKET_ONLY.into()))
        });
        return found
            .ok_or_else(|| AppError::Execute("Audio stream contained no decoded samples".into()));
    }
    AudioSpec {
        sample_rate: metadata.sample_rate,
        channels: metadata.channels as usize,
    }
    .checked()
}

const FIRST_PACKET_ONLY: &str = "first packet only";

/// Bit depth for WAV, FLAC and AIFF output: 24 when the source carries more
/// than 16 bits (24-bit, 32-bit or float), so a lossless format never drops
/// quality; 16 otherwise.
pub fn lossless_bits(input: &Path) -> u32 {
    match super::probe::probe_audio(input) {
        Ok(m) if m.bits_per_sample > 16 => 24,
        _ => 16,
    }
}

/// Decoding scales an N-bit sample by 1 / 2^(N-1); this is the exact
/// inverse, so N-bit in gives the same N-bit samples out.
pub fn to_int(sample: f32, bits: u32) -> i32 {
    let scale = (1i64 << (bits - 1)) as f32;
    (sample * scale).round().clamp(-scale, scale - 1.0) as i32
}

/// Each callback sees interleaved f32 samples from at most one bounded decoder packet.
pub fn stream_pcm<F>(input: &Path, mut consume: F) -> Result<(), AppError>
where
    F: FnMut(&[f32], AudioSpec) -> Result<(), AppError>,
{
    if input
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("opus"))
    {
        return super::formats::opus::stream_decode(input, consume);
    }

    let file = std::fs::File::open(input).map_err(|e| AppError::Io(e.to_string()))?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut probed = get_probe()
        .format(
            &Default::default(),
            stream,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(|e| AppError::Probe(format!("Probe failed: {e:?}")))?;

    let track = probed
        .format
        .tracks()
        .iter()
        // Video containers (MP4) list the picture track first; take the
        // first track that actually carries decodable audio.
        .find(|t| {
            t.codec_params.codec != symphonia::core::codecs::CODEC_TYPE_NULL
                && t.codec_params.sample_rate.is_some()
        })
        .or_else(|| probed.format.default_track())
        .ok_or_else(|| AppError::Execute("No audio track found".into()))?;
    let track_id = track.id;
    // For lossless containers with exact frame counts, a premature EOF
    // must not be mistaken for successful decoding.
    let exact_frame_count = input.extension().and_then(|s| s.to_str()).is_some_and(|s| {
        ["wav", "flac", "aiff", "aif"]
            .iter()
            .any(|ext| s.eq_ignore_ascii_case(ext))
    });
    // Symphonia derives AIFF frame counts from the SSND chunk length, which
    // includes its 8-byte offset/blockSize header, so it over-reports by up to
    // ceil(8 / bytes-per-frame) frames. Subtract that slack before comparing.
    let is_aiff = input
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("aiff") || s.eq_ignore_ascii_case("aif"));
    let aiff_slack = if is_aiff {
        let channels = track.codec_params.channels.map(|c| c.count()).unwrap_or(1) as u64;
        let bytes = u64::from(track.codec_params.bits_per_sample.unwrap_or(16)).div_ceil(8);
        let per_frame = (channels * bytes).max(1);
        8u64.div_ceil(per_frame)
    } else {
        0
    };
    let advertised_frames = if exact_frame_count {
        track
            .codec_params
            .n_frames
            .map(|n| n.saturating_sub(aiff_slack))
    } else {
        None
    };
    // Containers that leave the channel layout to the codec (AAC in MP4)
    // are pinned by the first decoded packet instead.
    let mut expected = match track.codec_params.channels {
        Some(channels) => Some(
            AudioSpec {
                sample_rate: track.codec_params.sample_rate.unwrap_or(0),
                channels: channels.count(),
            }
            .checked()?,
        ),
        None => None,
    };
    let mut decoder = get_codecs()
        .make(&track.codec_params, &Default::default())
        .map_err(|e| AppError::Execute(format!("Decoder initialization failed: {e:?}")))?;

    let mut sample_buf: Option<SampleBuffer<f32>> = None;
    let mut sample_capacity = 0usize;
    let mut saw_audio = false;
    let mut decoded_frames = 0u64;
    loop {
        let packet = match probed.format.next_packet() {
            Ok(packet) => packet,
            Err(ref error) if super::execute::is_end_of_stream(error) => break,
            Err(e) => {
                return Err(AppError::Execute(format!("Audio packet failed: {e:?}")));
            }
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = decoder
            .decode(&packet)
            .map_err(|e| AppError::Execute(format!("Audio decode failed: {e:?}")))?;
        let spec = AudioSpec {
            sample_rate: decoded.spec().rate,
            channels: decoded.spec().channels.count(),
        }
        .checked()?;
        let expected = *expected.get_or_insert(spec);
        if spec != expected {
            return Err(AppError::Execute(
                "Audio stream changed sample rate or channel layout".into(),
            ));
        }
        // Do not let a corrupt codec packet request unlimited PCM memory.
        if decoded.capacity() > 65_536 {
            return Err(AppError::Execute(
                "Decoded audio packet exceeds PCM limit".into(),
            ));
        }
        if sample_capacity < decoded.capacity() || sample_buf.is_none() {
            sample_capacity = decoded.capacity();
            sample_buf = Some(SampleBuffer::<f32>::new(
                sample_capacity as u64,
                *decoded.spec(),
            ));
        }
        if let Some(buf) = sample_buf.as_mut() {
            buf.copy_interleaved_ref(decoded);
            if !buf.samples().is_empty() {
                saw_audio = true;
                decoded_frames = decoded_frames
                    .checked_add((buf.samples().len() / spec.channels) as u64)
                    .ok_or_else(|| AppError::Execute("Audio frame counter overflow".into()))?;
                consume(buf.samples(), spec)?;
            }
        }
    }

    if !saw_audio {
        return Err(AppError::Execute(
            "Audio stream contained no decoded samples".into(),
        ));
    }
    if advertised_frames.is_some_and(|expected| decoded_frames < expected) {
        return Err(AppError::Execute(
            "Truncated audio: fewer decoded frames than container declares".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_utils::create_test_wav;

    #[test]
    fn streaming_wav_is_bounded_and_preserves_frame_count() {
        let wav = create_test_wav(2, 48_000, 100);
        let spec = inspect(wav.path()).expect("inspect generated WAV");
        assert_eq!(spec.sample_rate, 48_000);
        assert_eq!(spec.channels, 2);
        let mut frames = 0usize;
        stream_pcm(wav.path(), |chunk, found| {
            assert_eq!(found, spec);
            assert_eq!(chunk.len() % found.channels, 0);
            frames += chunk.len() / found.channels;
            Ok(())
        })
        .expect("stream generated WAV");
        assert!(frames >= 4_800);
    }
}
