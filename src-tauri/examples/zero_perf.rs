//! Offline, repeatable end-to-end conversion benchmark.
//!
//! Examples:
//! cargo run --release --example zero_perf -- generate ./fixtures 24 1024 1024
//! cargo run --release --example zero_perf -- run ./fixtures ./out_png png 24
//! Run each measurement in a fresh process, with an empty output directory.

use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tauri_app_lib::core::conversion::pipeline::ConversionPipeline;
use tauri_app_lib::core::conversion::registry::ConverterRegistry;
use tauri_app_lib::core::conversion::types::{AssetFamily, ConversionRequest, OutputFormat};
use tauri_app_lib::core::handlers::image::ImageConverterHandler;
use tauri_app_lib::core::jobs::{JobBroadcaster, JobManager, JobStatus};

fn input_error(message: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidInput, message.to_owned())
}

fn parse_number(argument: &str) -> Result<u32, Box<dyn Error>> {
    Ok(argument.parse::<u32>()?)
}

fn generate(directory: &Path, count: u32, width: u32, height: u32) -> Result<(), Box<dyn Error>> {
    if count == 0 || width == 0 || height == 0 || u64::from(width) * u64::from(height) > 16_000_000
    {
        return Err(input_error("Generate needs count > 0 and images of 1..=16 megapixels").into());
    }
    if directory.exists() && directory.read_dir()?.next().is_some() {
        return Err(input_error("Fixture directory must be empty").into());
    }
    std::fs::create_dir_all(directory)?;
    for i in 0..count {
        let image = image::RgbImage::from_fn(width, height, |x, y| {
            let sample = x.wrapping_mul(73_856_093)
                ^ y.wrapping_mul(19_349_663)
                ^ i.wrapping_mul(83_492_791);
            image::Rgb([sample as u8, (sample >> 8) as u8, (sample >> 16) as u8])
        });
        let path = directory.join(format!("fixture_{i:04}.png"));
        image.save_with_format(path, image::ImageFormat::Png)?;
    }
    println!(
        "fixture_count={count} dimensions={width}x{height} format=png deterministic_seed=coordinates"
    );
    Ok(())
}

fn peak_rss_bytes() -> Option<u64> {
    #[cfg(target_os = "linux")]
    {
        let status = std::fs::read_to_string("/proc/self/status").ok()?;
        let line = status.lines().find(|line| line.starts_with("VmHWM:"))?;
        return line
            .split_whitespace()
            .nth(1)?
            .parse::<u64>()
            .ok()
            .map(|kib| kib * 1024);
    }
    #[cfg(target_os = "windows")]
    {
        #[repr(C)]
        #[derive(Default)]
        struct ProcessMemoryCounters {
            cb: u32,
            page_fault_count: u32,
            peak_working_set_size: usize,
            working_set_size: usize,
            quota_peak_paged_pool_usage: usize,
            quota_paged_pool_usage: usize,
            quota_peak_non_paged_pool_usage: usize,
            quota_non_paged_pool_usage: usize,
            pagefile_usage: usize,
            peak_pagefile_usage: usize,
        }
        #[link(name = "kernel32")]
        extern "system" {
            fn GetCurrentProcess() -> *mut std::ffi::c_void;
        }
        #[link(name = "psapi")]
        extern "system" {
            fn GetProcessMemoryInfo(
                process: *mut std::ffi::c_void,
                counters: *mut ProcessMemoryCounters,
                size: u32,
            ) -> i32;
        }
        let mut counters = ProcessMemoryCounters {
            cb: std::mem::size_of::<ProcessMemoryCounters>() as u32,
            ..Default::default()
        };
        // Windows PSAPI reports peak process working set from process start.
        let size = counters.cb;
        let ok = unsafe { GetProcessMemoryInfo(GetCurrentProcess(), &mut counters, size) };
        (ok != 0).then_some(counters.peak_working_set_size as u64)
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        None
    }
}

async fn run(
    fixture_dir: &Path,
    output_dir: &Path,
    format: &str,
    limit: usize,
) -> Result<(), Box<dyn Error>> {
    if limit == 0 || !matches!(format, "png" | "jpg" | "webp") {
        return Err(input_error("Run needs count > 0 and format png|jpg|webp").into());
    }
    if output_dir.exists() && output_dir.read_dir()?.next().is_some() {
        return Err(input_error("Output directory must be empty for each trial").into());
    }
    std::fs::create_dir_all(output_dir)?;

    let mut inputs: Vec<PathBuf> = std::fs::read_dir(fixture_dir)?
        .map(|entry| entry.map(|item| item.path()))
        .collect::<Result<_, _>>()?;
    inputs.retain(|p| {
        p.extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("png"))
    });
    inputs.sort();
    if inputs.len() < limit {
        return Err(input_error("Not enough PNG fixtures for requested count").into());
    }

    let root = output_dir.to_string_lossy().into_owned();
    let requests: Vec<_> = inputs
        .iter()
        .take(limit)
        .map(|input| ConversionRequest {
            family: AssetFamily::Image,
            input_path: input.to_string_lossy().into_owned(),
            output_path: String::new(),
            output_root: root.clone(),
            output_format: OutputFormat(format.to_owned()),
            options: serde_json::json!({"quality": 80, "overwrite": false}),
        })
        .collect();

    let mut registry = ConverterRegistry::new();
    registry.register(AssetFamily::Image, Arc::new(ImageConverterHandler));
    let pipeline = Arc::new(ConversionPipeline::new(Arc::new(registry)));
    let broadcaster = Arc::new(JobBroadcaster::new(Box::new(|_| serde_json::Value::Null)));
    let manager = Arc::new(JobManager::new(broadcaster));

    let cpu_threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(0);
    let configured_memory =
        std::env::var("ZERO_CONVERTER_MEMORY_MIB").unwrap_or_else(|_| "default:1024".to_owned());
    let now = Instant::now();
    let ids = manager.submit_batch(requests, pipeline);
    loop {
        if ids.iter().all(|id| {
            manager
                .get_job(id)
                .is_some_and(|job| job.status.is_terminal())
        }) {
            break;
        }
        if now.elapsed() > Duration::from_secs(600) {
            return Err(input_error("Benchmark timed out after 600 seconds").into());
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let seconds = now.elapsed().as_secs_f64();
    let completed = ids
        .iter()
        .filter(|id| {
            manager
                .get_job(id)
                .is_some_and(|job| job.status == JobStatus::Completed)
        })
        .count();
    let failed = limit - completed;
    let rss_mib = peak_rss_bytes()
        .map(|n| format!("{:.1}", n as f64 / 1_048_576.0))
        .unwrap_or_else(|| "unsupported".to_owned());
    println!(
        "benchmark=ZeroConverter real_pipeline count={limit} completed={completed} failed={failed}"
    );
    println!(
        "output_format={format} available_parallelism={cpu_threads} memory_budget_mib={configured_memory}"
    );
    println!(
        "elapsed_seconds={seconds:.3} throughput_files_per_second={:.3} peak_process_rss_mib={rss_mib}",
        completed as f64 / seconds
    );
    if limit == 1 {
        println!("single_file_latency_ms={:.2}", seconds * 1000.0);
    }
    if failed > 0 {
        for id in ids {
            if let Some(job) = manager.get_job(&id) {
                if job.status != JobStatus::Completed {
                    eprintln!(
                        "failed_job={} status={:?} failure={:?}",
                        id, job.status, job.failure
                    );
                }
            }
        }
        return Err(input_error("One or more conversions failed").into());
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    match arguments.as_slice() {
        [mode, path, count, width, height] if mode == "generate" => {
            generate(
                Path::new(path),
                parse_number(count)?,
                parse_number(width)?,
                parse_number(height)?,
            )
        }
        [mode, fixture, output, format, count] if mode == "run" => {
            run(
                Path::new(fixture),
                Path::new(output),
                format,
                count.parse::<usize>()?,
            ).await
        }
        _ => Err(input_error(
            "Usage: generate <fixture_dir> <count> <width> <height> | run <fixture_dir> <empty_output_dir> <png|jpg|webp> <count>"
        ).into()),
    }
}
