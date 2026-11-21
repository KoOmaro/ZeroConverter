use crate::conversion::types::{AssetFamily, ConversionRequest, OutputFormat};
use hound::{WavSpec, WavWriter};
use serde_json::json;
use std::io::Write;
use std::path::Path;
use tempfile::{Builder, NamedTempFile};

pub enum MalformedMode {
    Empty,
    TruncatedWav,
    Garbage,
}

/// Generates a valid WAV fixture for testing probe/execute logic.
pub fn create_test_wav(channels: u16, sample_rate: u32, duration_ms: u32) -> NamedTempFile {
    let file = Builder::new()
        .suffix(".wav")
        .tempfile()
        .expect("Failed to create temp WAV");

    let spec = WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };

    {
        let mut writer = WavWriter::new(file.as_file(), spec).expect("Failed to create WavWriter");
        let num_samples = (sample_rate as u64 * channels as u64 * duration_ms as u64) / 1000;

        for _ in 0..num_samples {
            writer.write_sample(0i16).unwrap();
        }
        writer.finalize().expect("Failed to finalize WAV");
    }

    file
}

/// Generates a malformed file based on the specified mode.
pub fn create_malformed_file(mode: MalformedMode, extension: &str) -> NamedTempFile {
    let suffix = format!(".{}", extension);
    let mut file = Builder::new()
        .suffix(&suffix)
        .tempfile()
        .expect("Failed to create malformed file");

    match mode {
        MalformedMode::Empty => {
            // Write nothing
        }
        MalformedMode::TruncatedWav => {
            // Write just the "RIFF" header but stop early
            file.write_all(b"RIFF\x00\x00\x00\x00WAVE").unwrap();
        }
        MalformedMode::Garbage => {
            file.write_all(b"this is definitely not a valid file format")
                .unwrap();
        }
    }

    file
}

pub fn make_request(
    family: AssetFamily,
    input: &Path,
    output: &Path,
    format: &str,
    options: Option<serde_json::Value>,
) -> ConversionRequest {
    make_request_with_root(
        family,
        input,
        output,
        output.parent().unwrap_or(Path::new("")),
        format,
        options,
    )
}

/// Standardized helper for creating conversion requests with an explicit root.
pub fn make_request_with_root(
    family: AssetFamily,
    input: &Path,
    output: &Path,
    root: &Path,
    format: &str,
    options: Option<serde_json::Value>,
) -> ConversionRequest {
    ConversionRequest {
        family,
        input_path: input.to_str().unwrap().to_string(),
        output_path: output.to_str().unwrap().to_string(),
        output_root: root.to_str().unwrap().to_string(),
        output_format: OutputFormat(format.to_string()),
        options: options.unwrap_or(json!({})),
    }
}
