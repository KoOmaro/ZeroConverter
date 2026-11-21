//! Runs one FFmpeg process: reports progress from `-progress pipe:1`, kills
//! the child when the job is cancelled, and keeps the tail of stderr for
//! error messages.
use std::io::{BufRead, BufReader, Read};
use std::path::Path;
use std::process::Stdio;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// `-progress` prints a block about twice a second, even while FFmpeg is
/// still reading a slow input. Minutes of silence means it is stuck.
const STALL_LIMIT: Duration = Duration::from_secs(5 * 60);
/// Bytes of stderr kept for the error message (only the tail is shown).
const STDERR_KEEP: usize = 16 * 1024;

use super::ffmpeg::{self, Ffmpeg};
use crate::conversion::traits::ExecContext;

#[derive(Debug)]
pub enum RunError {
    Cancelled,
    /// FFmpeg ran and failed; the stderr tail explains why.
    Failed(String),
    Spawn(String),
}

/// One FFmpeg invocation. `lo..hi` is the slice of the job's 0–100 progress
/// bar this run covers (two-pass GIFs and the audio-through-WAV path split it).
pub struct Run<'a> {
    pub ff: &'a Ffmpeg,
    /// Everything between the fixed prologue and the output path.
    pub args: Vec<String>,
    pub output: &'a Path,
    pub duration: Option<f64>,
    pub lo: f32,
    pub hi: f32,
    pub label: &'a str,
}

/// `-progress` lines carry `out_time_us=<micro>`; returns seconds.
pub fn parse_progress_line(line: &str) -> Option<f64> {
    let value = line
        .strip_prefix("out_time_us=")
        .or_else(|| line.strip_prefix("out_time_ms="))?;
    let micros: f64 = value.trim().parse().ok()?;
    (micros >= 0.0).then_some(micros / 1_000_000.0)
}

pub fn percent(seconds: f64, duration: Option<f64>, lo: f32, hi: f32) -> f32 {
    match duration {
        Some(d) if d > 0.0 => lo + (hi - lo) * (seconds / d).clamp(0.0, 1.0) as f32,
        _ => lo,
    }
}

fn tail(text: &str) -> String {
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let from = lines.len().saturating_sub(4);
    lines[from..].join(" | ")
}

pub fn run(run: &Run, ctx: &ExecContext, input: &Path) -> Result<(), RunError> {
    let mut cmd = ffmpeg::command(&run.ff.ffmpeg);
    cmd.args([
        "-nostdin",
        "-hide_banner",
        "-loglevel",
        "error",
        "-progress",
        "pipe:1",
        "-nostats",
        "-y",
        "-i",
    ])
    .arg(input)
    .args(&run.args)
    .arg(run.output)
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    let mut child = cmd.spawn().map_err(|e| RunError::Spawn(e.to_string()))?;

    let stderr_text = Arc::new(Mutex::new(String::new()));
    // Only the last lines are ever shown, so keep a rolling window instead of
    // everything a misbehaving run might print.
    let stderr_thread = child.stderr.take().map(|mut err| {
        let sink = Arc::clone(&stderr_text);
        std::thread::spawn(move || {
            let mut kept: Vec<u8> = Vec::new();
            let mut chunk = [0u8; 8192];
            while let Ok(n) = err.read(&mut chunk) {
                if n == 0 {
                    break;
                }
                kept.extend_from_slice(&chunk[..n]);
                if kept.len() > STDERR_KEEP {
                    kept.drain(..kept.len() - STDERR_KEEP);
                }
            }
            *sink.lock().unwrap() = String::from_utf8_lossy(&kept).into_owned();
        })
    });

    let (tx, rx) = mpsc::channel::<String>();
    let stdout = child.stdout.take();
    let reader = std::thread::spawn(move || {
        if let Some(out) = stdout {
            for line in BufReader::new(out).lines().map_while(Result::ok) {
                if tx.send(line).is_err() {
                    break;
                }
            }
        }
    });

    let mut cancelled = false;
    let mut stalled = false;
    let mut last_percent = -1.0f32;
    let mut last_line = Instant::now();
    loop {
        if ctx.is_cancelled() {
            cancelled = true;
            let _ = child.kill();
            break;
        }
        if last_line.elapsed() > STALL_LIMIT {
            stalled = true;
            let _ = child.kill();
            break;
        }
        match rx.recv_timeout(Duration::from_millis(100)) {
            Ok(line) => {
                last_line = Instant::now();
                if let Some(seconds) = parse_progress_line(&line) {
                    let p = percent(seconds, run.duration, run.lo, run.hi);
                    if p - last_percent >= 0.5 {
                        last_percent = p;
                        ctx.report(p, run.label);
                    }
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }
    }
    let status = child.wait();
    let _ = reader.join();
    if let Some(t) = stderr_thread {
        let _ = t.join();
    }
    if cancelled {
        return Err(RunError::Cancelled);
    }
    if stalled {
        return Err(RunError::Failed(format!(
            "FFmpeg stopped responding (no progress for {} minutes)",
            STALL_LIMIT.as_secs() / 60
        )));
    }
    match status {
        Ok(s) if s.success() => {
            ctx.report(run.hi, run.label);
            Ok(())
        }
        Ok(s) => {
            let text = tail(&stderr_text.lock().unwrap());
            Err(RunError::Failed(format!(
                "FFmpeg exited with {}: {}",
                s.code().map_or("a signal".into(), |c| c.to_string()),
                if text.is_empty() {
                    "no details".into()
                } else {
                    text
                }
            )))
        }
        Err(e) => Err(RunError::Spawn(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_progress_lines() {
        assert_eq!(parse_progress_line("out_time_us=2500000"), Some(2.5));
        assert_eq!(parse_progress_line("out_time_ms=1000000"), Some(1.0));
        assert_eq!(parse_progress_line("out_time_us=N/A"), None);
        assert_eq!(parse_progress_line("frame=12"), None);
        assert_eq!(
            parse_progress_line("out_time_us=-9223372036854775808"),
            None
        );
    }

    #[test]
    fn percent_maps_into_the_slice_and_clamps() {
        assert_eq!(percent(5.0, Some(10.0), 0.0, 100.0), 50.0);
        assert_eq!(percent(5.0, Some(10.0), 40.0, 100.0), 70.0);
        assert_eq!(percent(99.0, Some(10.0), 0.0, 100.0), 100.0);
        assert_eq!(percent(5.0, None, 10.0, 100.0), 10.0);
    }

    #[test]
    fn error_tail_keeps_the_last_lines() {
        assert_eq!(tail("a\n\nb\nc\nd\ne\nf"), "c | d | e | f");
    }
}
