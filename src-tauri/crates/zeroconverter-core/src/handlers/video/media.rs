//! What is inside a media file, as reported by `ffprobe -show_streams`.
use std::path::Path;

use serde_json::Value;

use super::ffmpeg::{self, Ffmpeg};
use crate::app_error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Kind {
    Video,
    Audio,
    Subtitle,
    #[default]
    Other,
}

#[derive(Debug, Clone, Default)]
pub struct Stream {
    pub index: u32,
    pub kind: Kind,
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub pix_fmt: String,
    /// Display-matrix / `rotate` tag rotation in degrees.
    pub rotation: i32,
    pub channels: u32,
    /// Audio sample depth when the file states one (0 = unknown).
    pub bits: u32,
    pub bit_rate: Option<u64>,
    pub fps: Option<f64>,
    /// Cover art stored as a video stream.
    pub attached_pic: bool,
}

impl Stream {
    /// Size as displayed, after the rotation metadata is applied.
    pub fn display_size(&self) -> (u32, u32) {
        if self.rotation.rem_euclid(180) == 90 {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct MediaInfo {
    pub duration: Option<f64>,
    pub bit_rate: Option<u64>,
    pub streams: Vec<Stream>,
}

impl MediaInfo {
    pub fn of(&self, kind: Kind) -> impl Iterator<Item = &Stream> {
        self.streams
            .iter()
            .filter(move |s| s.kind == kind && !s.attached_pic)
    }

    pub fn primary_video(&self) -> Option<&Stream> {
        self.of(Kind::Video).next()
    }
}

fn num(v: &Value) -> Option<f64> {
    v.as_f64()
        .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

fn fraction(v: &Value) -> Option<f64> {
    let (n, d) = v.as_str()?.split_once('/')?;
    let (n, d): (f64, f64) = (n.parse().ok()?, d.parse().ok()?);
    (d > 0.0 && n > 0.0).then_some(n / d)
}

pub fn parse(json: &str) -> Result<MediaInfo, AppError> {
    let root: Value =
        serde_json::from_str(json).map_err(|e| AppError::Probe(format!("ffprobe output: {e}")))?;
    let mut info = MediaInfo {
        duration: root["format"]["duration"]
            .as_str()
            .and_then(|s| s.parse().ok()),
        bit_rate: num(&root["format"]["bit_rate"]).map(|b| b as u64),
        streams: Vec::new(),
    };
    for s in root["streams"].as_array().into_iter().flatten() {
        let kind = match s["codec_type"].as_str() {
            Some("video") => Kind::Video,
            Some("audio") => Kind::Audio,
            Some("subtitle") => Kind::Subtitle,
            _ => Kind::Other,
        };
        let rotation = s["side_data_list"]
            .as_array()
            .into_iter()
            .flatten()
            .find_map(|d| num(&d["rotation"]))
            .or_else(|| num(&s["tags"]["rotate"]))
            .map(|r| r.round() as i32)
            .unwrap_or(0);
        info.streams.push(Stream {
            index: s["index"].as_u64().unwrap_or(0) as u32,
            kind,
            codec: s["codec_name"].as_str().unwrap_or("").to_string(),
            width: s["width"].as_u64().unwrap_or(0) as u32,
            height: s["height"].as_u64().unwrap_or(0) as u32,
            pix_fmt: s["pix_fmt"].as_str().unwrap_or("").to_string(),
            rotation,
            channels: s["channels"].as_u64().unwrap_or(0) as u32,
            bits: num(&s["bits_per_raw_sample"])
                .into_iter()
                .chain(num(&s["bits_per_sample"]))
                .fold(0.0, f64::max) as u32,
            bit_rate: num(&s["bit_rate"]).map(|b| b as u64),
            fps: fraction(&s["avg_frame_rate"]).or_else(|| fraction(&s["r_frame_rate"])),
            attached_pic: s["disposition"]["attached_pic"].as_i64() == Some(1),
        });
        if info.duration.is_none() {
            info.duration = num(&s["duration"]);
        }
    }
    Ok(info)
}

const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const PROBE_MAX_BYTES: usize = 4 * 1024 * 1024;

pub fn probe(ff: &Ffmpeg, path: &Path) -> Result<MediaInfo, AppError> {
    let mut cmd = ffmpeg::command(&ff.ffprobe);
    cmd.args([
        "-v",
        "error",
        "-print_format",
        "json",
        "-show_format",
        "-show_streams",
        // `-i` so a file named `-something` is never read as an option.
        "-i",
    ])
    .arg(path);
    // Reading headers takes well under a second; a file that keeps ffprobe
    // busy for half a minute or prints megabytes of streams is not media we
    // can convert.
    let out = ffmpeg::output_limited(cmd, PROBE_TIMEOUT, PROBE_MAX_BYTES).map_err(|e| {
        AppError::Probe(format!("This file could not be read as media: ffprobe {e}"))
    })?;
    if out.stdout.len() >= PROBE_MAX_BYTES {
        return Err(AppError::Probe(
            "This file could not be read as media: too many streams".to_string(),
        ));
    }
    if !out.status.success() {
        let why = String::from_utf8_lossy(&out.stderr);
        return Err(AppError::Probe(format!(
            "This file could not be read as media: {}",
            why.trim().lines().last().unwrap_or("unknown error")
        )));
    }
    parse(&String::from_utf8_lossy(&out.stdout))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Runs only where FFmpeg is installed: the bounded runner still reads a
    /// real ffprobe answer.
    #[test]
    fn real_ffprobe_reads_a_clip() {
        let Some(ff) = ffmpeg::locate() else { return };
        let dir = tempfile::tempdir().unwrap();
        let clip = dir.path().join("clip.mp4");
        let made = ffmpeg::command(&ff.ffmpeg)
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "testsrc=size=64x48:duration=1",
            ])
            .arg(&clip)
            .status()
            .unwrap();
        assert!(made.success());
        let info = probe(&ff, &clip).unwrap();
        assert!(info.duration.unwrap_or(0.0) > 0.5);
        assert!(!info.streams.is_empty());
    }

    const PHONE: &str = r#"{"streams":[
      {"index":0,"codec_type":"video","codec_name":"h264","width":1920,"height":1080,
       "pix_fmt":"yuv420p","avg_frame_rate":"30000/1001","disposition":{"attached_pic":0},
       "side_data_list":[{"side_data_type":"Display Matrix","rotation":-90}]},
      {"index":1,"codec_type":"audio","codec_name":"aac","channels":2,"bit_rate":"128000"},
      {"index":2,"codec_type":"video","codec_name":"mjpeg","width":300,"height":300,
       "disposition":{"attached_pic":1}},
      {"index":3,"codec_type":"data","codec_name":"tmcd"}],
      "format":{"duration":"12.5","bit_rate":"4000000"}}"#;

    #[test]
    fn parses_phone_clip_with_rotation_and_cover_art() {
        let info = parse(PHONE).unwrap();
        assert_eq!(info.duration, Some(12.5));
        assert_eq!(info.streams.len(), 4);
        let v = info.primary_video().unwrap();
        assert_eq!((v.width, v.height, v.rotation), (1920, 1080, -90));
        assert_eq!(v.display_size(), (1080, 1920));
        assert!((v.fps.unwrap() - 29.97).abs() < 0.01);
        assert_eq!(info.of(Kind::Video).count(), 1, "cover art is not video");
        assert_eq!(info.of(Kind::Audio).next().unwrap().channels, 2);
        assert_eq!(info.streams[3].kind, Kind::Other);
    }

    #[test]
    fn rotate_tag_is_read_when_there_is_no_side_data() {
        let info = parse(
            r#"{"streams":[{"index":0,"codec_type":"video","codec_name":"h264","width":4,"height":2,"tags":{"rotate":"270"}}],"format":{}}"#,
        )
        .unwrap();
        assert_eq!(info.streams[0].display_size(), (2, 4));
    }

    #[test]
    fn garbage_is_a_probe_error() {
        assert!(matches!(parse("nope"), Err(AppError::Probe(_))));
    }
}
