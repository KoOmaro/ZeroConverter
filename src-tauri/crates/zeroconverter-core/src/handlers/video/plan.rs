//! Decides, stream by stream, what goes into the output container untouched
//! (`-c copy`, the fast path) and what has to be re-encoded. Pure functions:
//! no process is started here, so every rule is unit-tested.
use super::formats::{AudioTarget, Container};
use super::media::{Kind, MediaInfo, Stream};

pub const VP9: &str = "libvpx-vp9";

fn video_fits(c: Container, codec: &str) -> bool {
    match c {
        Container::Mp4 => matches!(
            codec,
            "h264" | "hevc" | "av1" | "mpeg4" | "mpeg2video" | "vp9"
        ),
        Container::Mov => matches!(
            codec,
            "h264" | "hevc" | "mpeg4" | "mpeg2video" | "prores" | "mjpeg" | "dnxhd" | "rawvideo"
        ),
        Container::Mkv => true,
        Container::Webm => matches!(codec, "vp8" | "vp9" | "av1"),
    }
}

fn audio_fits(c: Container, codec: &str) -> bool {
    match c {
        Container::Mp4 => matches!(codec, "aac" | "mp3" | "ac3" | "eac3" | "alac" | "opus"),
        Container::Mov => {
            matches!(codec, "aac" | "mp3" | "ac3" | "eac3" | "alac") || codec.starts_with("pcm_")
        }
        Container::Mkv => true,
        Container::Webm => matches!(codec, "opus" | "vorbis"),
    }
}

#[derive(Debug, PartialEq, Eq)]
enum SubAction {
    Copy,
    Convert(&'static str),
    Drop,
}

fn is_text_sub(codec: &str) -> bool {
    matches!(
        codec,
        "subrip" | "srt" | "ass" | "ssa" | "webvtt" | "mov_text" | "text"
    )
}

fn sub_action(c: Container, codec: &str) -> SubAction {
    match c {
        Container::Mkv => match codec {
            "subrip" | "srt" | "ass" | "ssa" | "webvtt" | "hdmv_pgs_subtitle" | "dvd_subtitle"
            | "dvb_subtitle" => SubAction::Copy,
            "mov_text" | "text" => SubAction::Convert("srt"),
            _ => SubAction::Drop,
        },
        Container::Mp4 | Container::Mov => match codec {
            "mov_text" => SubAction::Copy,
            c if is_text_sub(c) => SubAction::Convert("mov_text"),
            _ => SubAction::Drop,
        },
        Container::Webm => match codec {
            "webvtt" => SubAction::Copy,
            c if is_text_sub(c) => SubAction::Convert("webvtt"),
            _ => SubAction::Drop,
        },
    }
}

/// True when the primary video stream cannot be copied into `target`, so an
/// H.264 encoder must be found. (WebM uses VP9, which is always `libvpx`.)
pub fn needs_h264(target: Container, media: &MediaInfo) -> bool {
    target != Container::Webm
        && media
            .primary_video()
            .is_some_and(|v| !video_fits(target, &v.codec))
}

#[derive(Debug, Default)]
pub struct ContainerPlan {
    /// Output-side arguments, to follow `-i <input>`.
    pub args: Vec<String>,
    pub video_encoder: Option<String>,
    pub audio_reencoded: u32,
    pub subs_converted: u32,
    /// Human-readable list of what could not be carried over.
    pub dropped: Vec<String>,
}

impl ContainerPlan {
    pub fn is_pure_copy(&self) -> bool {
        self.video_encoder.is_none() && self.audio_reencoded == 0
    }
}

/// Target bitrate for a hardware/MF/OpenH264 encode, which have no CRF mode.
/// Bits per pixel per frame scales with quality, capped at the source bitrate.
pub fn h264_bitrate(v: &Stream, media: &MediaInfo, quality: u8) -> u64 {
    let (w, h) = v.display_size();
    let q = f64::from(quality.clamp(1, 100)) / 100.0;
    let bpp = 0.03 + 0.10 * q;
    let fps = v.fps.unwrap_or(30.0).clamp(1.0, 120.0);
    let wanted = f64::from(w) * f64::from(h) * fps * bpp;
    let source = v.bit_rate.or(media.bit_rate).map(|b| b as f64);
    let capped = match source {
        Some(s) if s > 1_000_000.0 => wanted.min(s),
        _ => wanted,
    };
    capped.clamp(500_000.0, 80_000_000.0) as u64
}

fn vp9_crf(quality: u8) -> u32 {
    (50.0 - f64::from(quality.clamp(1, 100)) * 0.2).round() as u32
}

fn h264_args(encoder: &str, out: usize, bitrate: u64) -> Vec<String> {
    let mut a = vec![format!("-c:v:{out}"), encoder.to_string()];
    let b = bitrate.to_string();
    let bufsize = (bitrate * 2).to_string();
    let maxrate = (bitrate * 3 / 2).to_string();
    a.extend(
        [format!("-b:v:{out}"), b]
            .into_iter()
            .chain([format!("-maxrate:v:{out}"), maxrate])
            .chain([format!("-bufsize:v:{out}"), bufsize]),
    );
    match encoder {
        "h264_nvenc" => a.extend(["-preset", "p5", "-rc", "vbr"].map(String::from)),
        "h264_qsv" => a.extend(["-preset", "medium"].map(String::from)),
        "h264_amf" => a.extend(["-quality", "balanced", "-rc", "vbr_peak"].map(String::from)),
        _ => {}
    }
    a.extend([format!("-pix_fmt:v:{out}"), "yuv420p".into()]);
    a
}

fn vp9_args(out: usize, quality: u8) -> Vec<String> {
    let mut a = vec![
        format!("-c:v:{out}"),
        VP9.to_string(),
        format!("-b:v:{out}"),
        "0".into(),
        format!("-crf:v:{out}"),
        vp9_crf(quality).to_string(),
    ];
    a.extend(
        [
            "-deadline",
            "good",
            "-cpu-used",
            "4",
            "-row-mt",
            "1",
            "-tile-columns",
            "2",
        ]
        .map(String::from),
    );
    a.extend([format!("-pix_fmt:v:{out}"), "yuv420p".into()]);
    a
}

/// `h264_encoder` is the encoder to use if the video must be re-encoded for
/// MP4/MOV/MKV (ignored otherwise).
pub fn plan_container(
    target: Container,
    media: &MediaInfo,
    h264_encoder: Option<&str>,
    quality: u8,
) -> ContainerPlan {
    let mut plan = ContainerPlan::default();
    let mut maps: Vec<String> = Vec::new();
    let mut codec_args: Vec<String> = Vec::new();

    // ── video: first real video stream only (cover art and extras are dropped)
    if let Some(v) = media.primary_video() {
        maps.extend(["-map".into(), format!("0:{}", v.index)]);
        if video_fits(target, &v.codec) {
            codec_args.extend(["-c:v:0".into(), "copy".into()]);
            if v.codec == "hevc" && matches!(target, Container::Mp4 | Container::Mov) {
                codec_args.extend(["-tag:v:0".into(), "hvc1".into()]);
            }
        } else if target == Container::Webm {
            codec_args.extend(vp9_args(0, quality));
            plan.video_encoder = Some(VP9.into());
        } else {
            let enc = h264_encoder.unwrap_or("h264_mf");
            codec_args.extend(h264_args(enc, 0, h264_bitrate(v, media, quality)));
            plan.video_encoder = Some(enc.to_string());
        }
        let (dw, dh) = v.display_size();
        if plan.video_encoder.is_some() && (dw % 2 == 1 || dh % 2 == 1) {
            codec_args.extend([
                "-filter:v:0".into(),
                "scale=trunc(iw/2)*2:trunc(ih/2)*2".into(),
            ]);
        }
    }
    let extra_video = media.of(Kind::Video).count().saturating_sub(1)
        + media.streams.iter().filter(|s| s.attached_pic).count();
    if extra_video > 0 {
        plan.dropped
            .push(format!("{extra_video} extra video/cover stream(s)"));
    }

    // ── audio: every track, copied unless the container cannot hold it
    for (n, a) in media.of(Kind::Audio).enumerate() {
        maps.extend(["-map".into(), format!("0:{}", a.index)]);
        if audio_fits(target, &a.codec) {
            codec_args.extend([format!("-c:a:{n}"), "copy".into()]);
        } else {
            plan.audio_reencoded += 1;
            if target == Container::Webm {
                codec_args.extend([format!("-c:a:{n}"), "libopus".into()]);
                codec_args.extend([format!("-b:a:{n}"), "128k".into()]);
            } else {
                let rate = if a.channels > 2 { "384k" } else { "192k" };
                codec_args.extend([format!("-c:a:{n}"), "aac".into()]);
                codec_args.extend([format!("-b:a:{n}"), rate.into()]);
            }
        }
    }

    // ── subtitles
    let mut sub_out = 0;
    for s in media.of(Kind::Subtitle) {
        match sub_action(target, &s.codec) {
            SubAction::Copy => {
                maps.extend(["-map".into(), format!("0:{}", s.index)]);
                codec_args.extend([format!("-c:s:{sub_out}"), "copy".into()]);
                sub_out += 1;
            }
            SubAction::Convert(to) => {
                maps.extend(["-map".into(), format!("0:{}", s.index)]);
                codec_args.extend([format!("-c:s:{sub_out}"), to.into()]);
                plan.subs_converted += 1;
                sub_out += 1;
            }
            SubAction::Drop => plan.dropped.push(format!("{} subtitles", s.codec)),
        }
    }
    let others = media
        .streams
        .iter()
        .filter(|s| s.kind == Kind::Other)
        .count();
    if others > 0 {
        plan.dropped
            .push(format!("{others} data/attachment stream(s)"));
    }

    plan.args = maps;
    plan.args.extend(codec_args);
    plan.args
        .extend(["-map_metadata", "0", "-map_chapters", "0"].map(String::from));
    if matches!(target, Container::Mp4 | Container::Mov) {
        plan.args
            .extend(["-movflags", "+faststart"].map(String::from));
    }
    plan
}

/// Output-side arguments for pulling the audio out of a video. `None` means
/// "decode to PCM and let the pure-Rust audio encoder finish" (the caller
/// writes a temporary WAV).
pub fn plan_audio_direct(target: AudioTarget, media: &MediaInfo) -> Option<Vec<String>> {
    let first = media.of(Kind::Audio).next()?;
    let copy = first.codec == "aac";
    let mut a: Vec<String> = ["-vn", "-sn", "-dn", "-map"].map(String::from).to_vec();
    a.push(format!("0:{}", first.index));
    match target {
        AudioTarget::M4a | AudioTarget::Aac => {
            if copy {
                a.extend(["-c:a", "copy"].map(String::from));
            } else {
                a.extend(["-c:a", "aac", "-b:a", "192k"].map(String::from));
            }
            a.extend(
                [
                    "-f",
                    if target == AudioTarget::M4a {
                        "ipod"
                    } else {
                        "adts"
                    },
                ]
                .map(String::from),
            );
            Some(a)
        }
        _ => None,
    }
}

/// Arguments for decoding the first audio track to a PCM WAV, the input of
/// the existing audio encoders (and WAV itself): 16-bit, or 24-bit for a
/// lossless target when the source is deeper than 16 bits.
pub fn plan_audio_wav(target: AudioTarget, media: &MediaInfo) -> Option<Vec<String>> {
    let first = media.of(Kind::Audio).next()?;
    let mut a: Vec<String> = ["-vn", "-sn", "-dn", "-map"].map(String::from).to_vec();
    a.push(format!("0:{}", first.index));
    // The MP3 and Opus encoders take at most two channels.
    if matches!(target, AudioTarget::Mp3 | AudioTarget::Opus) && first.channels > 2 {
        a.extend(["-ac", "2"].map(String::from));
    }
    // The pure-Rust Opus encoder has no resampler: it needs 48 kHz input.
    if target == AudioTarget::Opus {
        a.extend(["-ar", "48000"].map(String::from));
    }
    let lossless = matches!(
        target,
        AudioTarget::Wav | AudioTarget::Flac | AudioTarget::Aiff
    );
    let codec = if lossless && first.bits > 16 {
        "pcm_s24le"
    } else {
        "pcm_s16le"
    };
    a.extend(["-c:a", codec, "-f", "wav"].map(String::from));
    Some(a)
}

/// Common GIF filter chain: 15 fps, at most 480 px wide, rotation applied.
pub const GIF_BASE: &str = "fps=15,scale='min(480,iw)':-1:flags=lanczos";
/// Refuse absurd GIFs: they are enormous and rarely what was meant.
pub const GIF_MAX_SECONDS: f64 = 300.0;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::video::media::parse;

    fn clip(video: &str, audio: &str, extra: &str) -> MediaInfo {
        parse(&format!(
            r#"{{"streams":[
              {{"index":0,"codec_type":"video","codec_name":"{video}","width":1920,"height":1080,"avg_frame_rate":"30/1","pix_fmt":"yuv420p"}},
              {{"index":1,"codec_type":"audio","codec_name":"{audio}","channels":2}}{extra}],
              "format":{{"duration":"10","bit_rate":"8000000"}}}}"#
        ))
        .unwrap()
    }

    fn has(args: &[String], pair: [&str; 2]) -> bool {
        args.windows(2).any(|w| w[0] == pair[0] && w[1] == pair[1])
    }

    #[test]
    fn h264_aac_into_mkv_and_mov_is_pure_copy() {
        for t in [Container::Mkv, Container::Mov, Container::Mp4] {
            let plan = plan_container(t, &clip("h264", "aac", ""), None, 90);
            assert!(plan.is_pure_copy(), "{t:?}");
            assert!(has(&plan.args, ["-c:v:0", "copy"]));
            assert!(has(&plan.args, ["-c:a:0", "copy"]));
        }
    }

    #[test]
    fn webm_needs_vp9_and_opus_for_h264_aac() {
        let m = clip("h264", "aac", "");
        let plan = plan_container(Container::Webm, &m, None, 90);
        assert_eq!(plan.video_encoder.as_deref(), Some(VP9));
        assert_eq!(plan.audio_reencoded, 1);
        assert!(has(&plan.args, ["-c:a:0", "libopus"]));
        assert!(!needs_h264(Container::Webm, &m));
    }

    #[test]
    fn webm_vp9_opus_into_mkv_and_mp4_copies() {
        let m = clip("vp9", "opus", "");
        assert!(plan_container(Container::Mkv, &m, None, 90).is_pure_copy());
        assert!(plan_container(Container::Mp4, &m, None, 90).is_pure_copy());
        // MOV cannot hold VP9: only the video is re-encoded.
        let mov = plan_container(Container::Mov, &m, Some("h264_mf"), 90);
        assert_eq!(mov.video_encoder.as_deref(), Some("h264_mf"));
        assert_eq!(mov.audio_reencoded, 1, "opus does not fit MOV");
        assert!(needs_h264(Container::Mov, &m));
    }

    #[test]
    fn only_the_incompatible_stream_is_reencoded() {
        // H.264 video is fine in MP4; FLAC audio is not.
        let plan = plan_container(Container::Mp4, &clip("h264", "flac", ""), None, 90);
        assert!(plan.video_encoder.is_none());
        assert_eq!(plan.audio_reencoded, 1);
        assert!(has(&plan.args, ["-c:v:0", "copy"]));
        assert!(has(&plan.args, ["-c:a:0", "aac"]));
    }

    #[test]
    fn hevc_into_mp4_gets_the_apple_tag() {
        let plan = plan_container(Container::Mp4, &clip("hevc", "aac", ""), None, 90);
        assert!(has(&plan.args, ["-tag:v:0", "hvc1"]));
        let mkv = plan_container(Container::Mkv, &clip("hevc", "aac", ""), None, 90);
        assert!(!mkv.args.contains(&"-tag:v:0".to_string()));
    }

    #[test]
    fn faststart_only_for_mp4_and_mov() {
        let m = clip("h264", "aac", "");
        assert!(plan_container(Container::Mp4, &m, None, 90)
            .args
            .contains(&"+faststart".into()));
        assert!(!plan_container(Container::Mkv, &m, None, 90)
            .args
            .contains(&"+faststart".into()));
    }

    #[test]
    fn subtitles_are_converted_or_dropped_per_container() {
        let extra = r#",{"index":2,"codec_type":"subtitle","codec_name":"subrip"},
            {"index":3,"codec_type":"subtitle","codec_name":"hdmv_pgs_subtitle"},
            {"index":4,"codec_type":"attachment","codec_name":"ttf"}"#;
        let m = clip("h264", "aac", extra);
        let mp4 = plan_container(Container::Mp4, &m, None, 90);
        assert_eq!(mp4.subs_converted, 1);
        assert!(has(&mp4.args, ["-c:s:0", "mov_text"]));
        assert!(mp4.dropped.iter().any(|d| d.contains("hdmv_pgs_subtitle")));
        assert!(mp4.dropped.iter().any(|d| d.contains("attachment")));
        let mkv = plan_container(Container::Mkv, &m, None, 90);
        assert_eq!(mkv.subs_converted, 0);
        assert!(has(&mkv.args, ["-c:s:1", "copy"]), "PGS is kept in MKV");
    }

    #[test]
    fn cover_art_is_not_mapped() {
        let extra = r#",{"index":2,"codec_type":"video","codec_name":"mjpeg","width":300,"height":300,"disposition":{"attached_pic":1}}"#;
        let plan = plan_container(Container::Mkv, &clip("h264", "aac", extra), None, 90);
        assert!(!plan.args.contains(&"0:2".to_string()));
        assert_eq!(plan.dropped.len(), 1);
    }

    #[test]
    fn odd_dimensions_are_padded_to_even_only_when_encoding() {
        let m = parse(
            r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"mpeg4","width":853,"height":480}],"format":{}}"#,
        )
        .unwrap();
        let enc = plan_container(Container::Mkv, &m, None, 90);
        assert!(enc.is_pure_copy(), "mpeg4 fits MKV");
        let mov_enc = plan_container(Container::Webm, &m, None, 90);
        assert!(mov_enc.args.contains(&"-filter:v:0".to_string()));
        assert!(!enc.args.contains(&"-filter:v:0".to_string()));
    }

    #[test]
    fn bitrate_scales_with_quality_and_never_exceeds_a_high_source() {
        let m = clip("mpeg2video", "aac", "");
        let v = m.primary_video().unwrap();
        let low = h264_bitrate(v, &m, 10);
        let high = h264_bitrate(v, &m, 100);
        assert!(low < high);
        assert!(high <= 8_000_000, "capped at the 8 Mbit/s source");
    }

    #[test]
    fn vp9_crf_follows_quality() {
        assert_eq!(vp9_crf(90), 32);
        assert!(vp9_crf(100) < vp9_crf(10));
    }

    #[test]
    fn aac_audio_is_extracted_by_copy_and_others_go_through_wav() {
        let aac = clip("h264", "aac", "");
        let direct = plan_audio_direct(AudioTarget::M4a, &aac).unwrap();
        assert!(has(&direct, ["-c:a", "copy"]));
        assert!(has(&direct, ["-f", "ipod"]));
        let ac3 = clip("h264", "ac3", "");
        let re = plan_audio_direct(AudioTarget::Aac, &ac3).unwrap();
        assert!(has(&re, ["-c:a", "aac"]));
        assert!(plan_audio_direct(AudioTarget::Mp3, &aac).is_none());
        let wav = plan_audio_wav(AudioTarget::Mp3, &aac).unwrap();
        assert!(has(&wav, ["-c:a", "pcm_s16le"]));
    }

    #[test]
    fn surround_is_downmixed_for_the_two_channel_encoders() {
        let m = parse(
            r#"{"streams":[{"index":0,"codec_type":"audio","codec_name":"ac3","channels":6}],"format":{}}"#,
        )
        .unwrap();
        assert!(has(
            &plan_audio_wav(AudioTarget::Mp3, &m).unwrap(),
            ["-ac", "2"]
        ));
        assert!(!plan_audio_wav(AudioTarget::Flac, &m)
            .unwrap()
            .contains(&"-ac".into()));
    }

    #[test]
    fn deep_audio_stays_24_bit_for_lossless_targets() {
        let m = parse(
            r#"{"streams":[{"index":0,"codec_type":"audio","codec_name":"flac","channels":2,"bits_per_raw_sample":"24"}],"format":{}}"#,
        )
        .unwrap();
        for lossless in [AudioTarget::Wav, AudioTarget::Flac, AudioTarget::Aiff] {
            assert!(has(
                &plan_audio_wav(lossless, &m).unwrap(),
                ["-c:a", "pcm_s24le"]
            ));
        }
        assert!(has(
            &plan_audio_wav(AudioTarget::Mp3, &m).unwrap(),
            ["-c:a", "pcm_s16le"]
        ));
    }

    #[test]
    fn opus_input_is_resampled_to_48k() {
        let m = clip("h264", "aac", "");
        assert!(has(
            &plan_audio_wav(AudioTarget::Opus, &m).unwrap(),
            ["-ar", "48000"]
        ));
        assert!(!plan_audio_wav(AudioTarget::Wav, &m)
            .unwrap()
            .contains(&"-ar".into()));
    }

    #[test]
    fn no_audio_track_means_no_plan() {
        let m = parse(
            r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"h264","width":2,"height":2}],"format":{}}"#,
        )
        .unwrap();
        assert!(plan_audio_wav(AudioTarget::Wav, &m).is_none());
    }
}
