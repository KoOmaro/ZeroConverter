//! The ZeroConverter engine: format detection, conversion rules, the job
//! queue and the families that convert (image, audio, video). It has no
//! Tauri or UI code; the desktop app and the `zeroconverter` command line
//! both build on it.
//!
//! # Stability
//!
//! This crate is not published and its `pub` items are not a stable API: they
//! are public so the app and the command line in this workspace can use them,
//! and they may change in any release. Programs outside this repository
//! should use the command line and its versioned JSON contract
//! (`docs/CLI.md`). Reusing the source under the MIT license is welcome.
//!
//! # Where to start
//!
//! - [`conversion::registry::ConverterRegistry`]: which family handles a
//!   format; `validate` a request before converting it.
//! - [`conversion::pipeline::convert`]: runs a checked request; the result is
//!   staged next to the destination and moved there only when complete.
//! - [`handlers::image::formats`] and [`handlers::audio::formats`]: one
//!   module per format, registered with one line in `MODULES`.
//! - [`jobs::manager::JobManager`]: queueing, progress events and the input
//!   guard.
//! - [`app_error::AppError`]: every failure, each with a stable `code()`.
pub mod app_error;

#[cfg(test)]
mod architecture_tests;
pub mod conversion;
pub mod handlers;
pub mod image_limits;
pub mod jobs;
pub mod probe;
pub mod sniff;
pub mod thumbnails;

#[cfg(test)]
pub mod test_utils;
