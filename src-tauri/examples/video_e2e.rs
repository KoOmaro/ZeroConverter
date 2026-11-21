//! End-to-end run of the video handler against real clips.
//!
//! Generates a few clips with the FFmpeg it finds (managed copy, ZC_FFMPEG_DIR
//! or PATH), converts each to every supported target through the handler,
//! checks every output with ffprobe and prints a markdown table.
//!
//! cargo run --release --example video_e2e -- <empty work dir> [seconds]
//!
//! ZC_E2E_ENCODER=libx264 (default when present) is used ONLY to make the
//! test clips; the app itself never uses it.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri_app_lib::core::conversion::traits::{ConverterHandler, ExecContext};
use tauri_app_lib::core::conversion::types::{AssetFamily, ConversionRequest, OutputFormat};
use tauri_app_lib::core::handlers::video::media::{self, Kind};
use tauri_app_lib::core::handlers::video::{ffmpeg, VideoHandler};
use tokio_util::sync::CancellationToken;

fn ff_run(ff: &ffmpeg::Ffmpeg, args: &[&str]) -> Result<(), Box<dyn Error>> {
    let out = ffmpeg::command(&ff.ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args(args)
        .output()?;
    if !out.status.success() {
        return Err(format!(
            "ffmpeg {:?}: {}",
            args,
            String::from_utf8_lossy(&out.stderr)
        )
        .into());
    }
    Ok(())
}

struct Clip {
    name: &'static str,
    note: &'static str,
}

fn generate(
    ff: &ffmpeg::Ffmpeg,
    dir: &Path,
    secs: u32,
) -> Result<Vec<(Clip, PathBuf)>, Box<dyn Error>> {
    let p = |n: &str| dir.join(n).to_string_lossy().into_owned();
    let t = secs.to_string();
    let x264 = ffmpeg::has_encoder(ff, "libx264");
    let venc: &[&str] = if x264 {
        &["-c:v", "libx264", "-preset", "veryfast", "-crf", "23"]
    } else {
        &["-c:v", "h264_mf", "-b:v", "4M"]
    };
    let src_v = format!("testsrc2=size=1920x1080:rate=30:duration={secs}");
    let src_a = format!("sine=frequency=440:duration={secs}");

    let mp4 = p("h264_aac.mp4");
    let mut a: Vec<&str> = vec!["-f", "lavfi", "-i", &src_v, "-f", "lavfi", "-i", &src_a];
    a.extend_from_slice(venc);
    a.extend_from_slice(&[
        "-pix_fmt", "yuv420p", "-c:a", "aac", "-b:a", "128k", "-t", &t, &mp4,
    ]);
    ff_run(ff, &a)?;

    let mkv = p("h264_aac.mkv");
    ff_run(ff, &["-i", &mp4, "-c", "copy", &mkv])?;
    let mov = p("h264_aac.mov");
    ff_run(ff, &["-i", &mp4, "-c", "copy", &mov])?;
    // Phone-like: landscape pixels with a 90 degree display matrix.
    let phone = p("phone_portrait_rot90.mp4");
    ff_run(
        ff,
        &[
            "-display_rotation:v",
            "90",
            "-i",
            &mp4,
            "-c",
            "copy",
            &phone,
        ],
    )?;
    let silent = p("no_audio.mp4");
    ff_run(ff, &["-i", &mp4, "-an", "-c:v", "copy", &silent])?;
    let webm = p("vp9_opus.webm");
    ff_run(
        ff,
        &[
            "-i",
            &mp4,
            "-c:v",
            "libvpx-vp9",
            "-b:v",
            "0",
            "-crf",
            "36",
            "-deadline",
            "realtime",
            "-cpu-used",
            "8",
            "-c:a",
            "libopus",
            &webm,
        ],
    )?;
    let avi = p("mpeg4_mp3.avi");
    ff_run(
        ff,
        &[
            "-i",
            &mp4,
            "-c:v",
            "mpeg4",
            "-q:v",
            "5",
            "-c:a",
            "libmp3lame",
            &avi,
        ],
    )?;
    let mut clips = vec![
        (
            Clip {
                name: "h264_aac.mp4",
                note: "H.264+AAC 1080p",
            },
            mp4.clone(),
        ),
        (
            Clip {
                name: "h264_aac.mkv",
                note: "H.264+AAC in MKV",
            },
            mkv,
        ),
        (
            Clip {
                name: "h264_aac.mov",
                note: "H.264+AAC in MOV",
            },
            mov,
        ),
        (
            Clip {
                name: "phone_portrait_rot90.mp4",
                note: "rotation metadata, 1080x1920 as shown",
            },
            phone,
        ),
        (
            Clip {
                name: "no_audio.mp4",
                note: "no audio track",
            },
            silent,
        ),
        (
            Clip {
                name: "vp9_opus.webm",
                note: "VP9+Opus",
            },
            webm,
        ),
        (
            Clip {
                name: "mpeg4_mp3.avi",
                note: "MPEG-4+MP3 AVI",
            },
            avi,
        ),
    ];
    if ffmpeg::has_encoder(ff, "libx265") {
        let hevc = p("hevc_aac.mp4");
        ff_run(
            ff,
            &[
                "-i",
                &mp4,
                "-c:v",
                "libx265",
                "-preset",
                "ultrafast",
                "-crf",
                "30",
                "-c:a",
                "copy",
                "-tag:v",
                "hvc1",
                &hevc,
            ],
        )?;
        clips.push((
            Clip {
                name: "hevc_aac.mp4",
                note: "HEVC+AAC",
            },
            hevc,
        ));
    }
    Ok(clips
        .into_iter()
        .map(|(c, f)| (c, PathBuf::from(f)))
        .collect())
}

fn check(out: &Path, target: &str, src: &media::MediaInfo, ff: &ffmpeg::Ffmpeg) -> String {
    let info = match media::probe(ff, out) {
        Ok(i) => i,
        Err(e) => return format!("UNREADABLE: {e}"),
    };
    let mut issues = Vec::new();
    // ADTS has no duration field: ffprobe only estimates it from the bitrate.
    if let (Some(a), Some(b), false) = (src.duration, info.duration, target == "aac") {
        if (a - b).abs() > a * 0.05 + 0.3 {
            issues.push(format!("duration {b:.1}s vs {a:.1}s"));
        }
    }
    let is_audio = matches!(
        target,
        "wav" | "mp3" | "flac" | "aiff" | "opus" | "m4a" | "aac"
    );
    if is_audio {
        if info.of(Kind::Audio).count() == 0 {
            issues.push("no audio".into());
        }
    } else {
        let sv = src.primary_video();
        match (sv, info.primary_video()) {
            (Some(s), Some(o)) => {
                if target != "gif" && s.display_size() != o.display_size() {
                    issues.push(format!(
                        "shown size {:?} vs {:?}",
                        o.display_size(),
                        s.display_size()
                    ));
                }
            }
            (Some(_), None) => issues.push("video lost".into()),
            _ => {}
        }
        if target != "gif" && src.of(Kind::Audio).count() != info.of(Kind::Audio).count() {
            issues.push("audio track count changed".into());
        }
    }
    if issues.is_empty() {
        "ok".into()
    } else {
        issues.join("; ")
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let work = PathBuf::from(
        args.next()
            .ok_or("usage: video_e2e <empty work dir> [seconds]")?,
    );
    let secs: u32 = args.next().map_or(Ok(20), |s| s.parse())?;
    std::fs::create_dir_all(&work)?;
    let ff = ffmpeg::locate().ok_or("no ffmpeg found (PATH, ZC_FFMPEG_DIR or managed)")?;
    println!(
        "FFmpeg: {} ({:?})",
        ffmpeg::version_line(&ff).unwrap_or_default(),
        ff.source
    );
    println!("H.264 candidates: {:?}\n", ffmpeg::h264_candidates(&ff));

    std::fs::create_dir_all(work.join("in"))?;
    let clips = generate(&ff, &work.join("in"), secs)?;
    let out_dir = work.join("out");
    std::fs::create_dir_all(&out_dir)?;

    let handler = VideoHandler;
    let caps = handler.capabilities();
    println!("| clip | target | time | result | note |\n|---|---|---|---|---|");
    let (mut ok_count, mut fail_count) = (0u32, 0u32);
    for (clip, path) in &clips {
        let src_ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        let src_info = media::probe(&ff, path)?;
        let id = if src_ext == "mp4" { "mp4" } else { src_ext };
        let Some(cap) = caps.iter().find(|c| c.input.0 == id) else {
            continue;
        };
        for target in cap.outputs.iter().map(|o| o.0.as_str()) {
            let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("x");
            let out = out_dir.join(format!("{stem}.{src_ext}.{target}"));
            let req = ConversionRequest {
                family: AssetFamily::Video,
                input_path: path.to_string_lossy().into_owned(),
                output_path: out.to_string_lossy().into_owned(),
                output_root: out_dir.to_string_lossy().into_owned(),
                output_format: OutputFormat(target.into()),
                options: serde_json::json!({ "quality": 90, "overwrite": true }),
            };
            let ticks = Arc::new(AtomicU32::new(0));
            let t2 = Arc::clone(&ticks);
            let ctx = ExecContext {
                token: Some(CancellationToken::new()),
                progress: Some(Arc::new(move |_, _| {
                    t2.fetch_add(1, Ordering::Relaxed);
                })),
            };
            handler.validate(&req)?;
            let started = Instant::now();
            let res = handler.execute_with(&req, &ctx);
            let took = started.elapsed();
            let src_silent = src_info.of(Kind::Audio).count() == 0;
            let wants_audio = matches!(
                target,
                "wav" | "mp3" | "flac" | "aiff" | "opus" | "m4a" | "aac"
            );
            let (status, note) = match res {
                Err(e) if src_silent && wants_audio && e.to_string().contains("no audio") => {
                    ok_count += 1;
                    ("ok (refused, no audio track)".to_string(), e.to_string())
                }
                Ok(r) if r.success => {
                    let verdict = check(&out, target, &src_info, &ff);
                    if verdict == "ok" {
                        ok_count += 1;
                        ("ok".to_string(), r.message)
                    } else {
                        fail_count += 1;
                        (
                            "CHECK FAILED".to_string(),
                            format!("{verdict} ({})", r.message),
                        )
                    }
                }
                Ok(r) => {
                    fail_count += 1;
                    ("FAILED".into(), r.message)
                }
                Err(e) => {
                    fail_count += 1;
                    ("FAILED".into(), e.to_string())
                }
            };
            println!(
                "| {} | {} | {:.2}s | {} ({} progress ticks) | {} |",
                clip.name,
                target,
                took.as_secs_f64(),
                status,
                ticks.load(Ordering::Relaxed),
                note.replace('|', "/")
            );
        }
        let _ = clip.note;
    }
    println!("\n{ok_count} ok, {fail_count} failed");

    // Cancellation: a slow VP9 encode, cancelled one second in.
    let (_, slow) = &clips[0];
    let out = out_dir.join("cancel_me.webm");
    let token = CancellationToken::new();
    let ctx = ExecContext {
        token: Some(token.clone()),
        progress: None,
    };
    let req = ConversionRequest {
        family: AssetFamily::Video,
        input_path: slow.to_string_lossy().into_owned(),
        output_path: out.to_string_lossy().into_owned(),
        output_root: out_dir.to_string_lossy().into_owned(),
        output_format: OutputFormat("webm".into()),
        options: serde_json::json!({ "quality": 90 }),
    };
    let canceller = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1500));
        token.cancel();
    });
    let started = Instant::now();
    let res = handler.execute_with(&req, &ctx);
    canceller.join().ok();
    let debris = std::fs::read_dir(&out_dir)?
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().starts_with("zero_tmp_"))
        .count();
    println!(
        "\ncancel: {:?} after {:.2}s, output exists: {}, temp dirs left: {}",
        res.as_ref().map(|_| "completed").map_err(|e| e.to_string()),
        started.elapsed().as_secs_f64(),
        out.exists(),
        debris
    );
    if fail_count > 0 {
        std::process::exit(1);
    }
    Ok(())
}
