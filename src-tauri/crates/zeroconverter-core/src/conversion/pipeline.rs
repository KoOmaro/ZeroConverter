// ==============================================================================
// OWNERSHIP:
//   Orchestrates the core conversion lifecycle: validate, plan, execute, finalize, and integrity checks.
//
// DO NOT ADD:
//   - Format-specific encoding/decoding algorithms
//   - Frontend DTO mapping
//   - Thumbnail processing logic
//   - UI/application state concerns
//
// NOTES:
//   Planning may reserve output paths, but conversion output bytes are written only during execution.
// ==============================================================================
use crate::app_error::AppError;
use crate::conversion::registry::ConverterRegistry;
use crate::conversion::traits::{ExecContext, ProgressFn};
use crate::conversion::types::{AssetFamily, ConversionRequest, ConversionResult};
use crate::jobs::{JobManager, JobProgress, JobStage, JobStatus};
use std::path::Path;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

/// Runs CPU- and disk-bound work (hashing, decode/encode, manifest write) on the
/// blocking pool so conversions never stall the async runtime that serves job
/// events, thumbnails and IPC.
async fn run_blocking<T, F>(f: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
{
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| AppError::Execute(format!("Conversion worker failed: {}", e)))?
}

/// Forwards handler progress to the job as "Executing" updates.
fn progress_reporter(manager: Arc<JobManager>, job_id: String) -> Arc<ProgressFn> {
    Arc::new(move |percent, message| {
        let _ = manager.update_progress(
            &job_id,
            JobProgress {
                stage: "Executing".to_string(),
                message: message.to_string(),
                percent: Some(percent.clamp(0.0, 100.0)),
            },
        );
    })
}

/// The conversion every caller shares, the queue and rename-to-convert
/// alike. The converter writes into a private scratch folder next to the
/// output; only a finished, not-cancelled result is then moved to the output
/// name, in one step. A cancel before that leaves nothing behind; a cancel
/// after it is too late and the job counts as done. Nothing under the
/// output name is ever deleted afterwards (it may no longer be ours). The
/// request must be validated and its output path planned.
pub fn convert(
    registry: &ConverterRegistry,
    request: &ConversionRequest,
    ctx: &ExecContext,
) -> Result<ConversionResult, AppError> {
    if let Some(bytes) = crate::conversion::types::large_input(Path::new(&request.input_path)) {
        if !request.large_confirmed() {
            return Err(AppError::ConsentRequired { bytes });
        }
    }
    let final_path = std::path::PathBuf::from(&request.output_path);
    let file_name = final_path
        .file_name()
        .ok_or_else(|| AppError::Validate("The output path has no file name".into()))?;
    let parent = match final_path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        _ => Path::new("."),
    };
    // Same folder, so the final move is a rename on one volume.
    let work = zero_fs::TempWorkspace::create(parent)
        .map_err(|e| AppError::Io(format!("Could not prepare the output folder: {e}")))?;
    let staged_path = work.path().join(file_name);
    let mut staged = request.clone();
    staged.output_path = staged_path.to_string_lossy().into_owned();

    let mut result = registry.execute_with(request.family.clone(), &staged, ctx)?;
    // Image and audio encoders run to the end without looking at the token.
    if ctx.is_cancelled() {
        return Err(AppError::Cancelled);
    }
    if !result.success {
        result.output_path = None;
        return Ok(result);
    }
    match result.output_path.as_deref().map(Path::new) {
        Some(path) if path == staged_path && path.is_file() => {}
        Some(path) => {
            return Err(AppError::Execute(format!(
                "Converted file is missing: {}",
                path.display()
            )))
        }
        None => {
            return Err(AppError::Execute(
                "Handler reported success but provided no output path".to_string(),
            ))
        }
    }
    zero_fs::commit(&staged_path, &final_path, request.clobber()).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AlreadyExists {
            AppError::Io(format!(
                "{} appeared while converting; it was left as it is",
                final_path.display()
            ))
        } else {
            AppError::Io(format!("Could not save {}: {e}", final_path.display()))
        }
    })?;
    result.output_path = Some(request.output_path.clone());
    Ok(result)
}

pub struct ConversionPipeline {
    registry: Arc<ConverterRegistry>,
}

pub struct ConversionPlan {
    pub family: AssetFamily,
    pub request: ConversionRequest,
}

impl ConversionPipeline {
    pub fn new(registry: Arc<ConverterRegistry>) -> Self {
        Self { registry }
    }

    /// Expose registry for probe service and family resolution.
    pub fn registry(&self) -> &ConverterRegistry {
        &self.registry
    }

    /// Delegate family resolution to the registry.
    pub fn resolve_family(&self, extension: &str) -> AssetFamily {
        self.registry.resolve_family(extension)
    }

    /// Orchestrate the conversion process with Job integration:
    /// Probe → Validate → Plan → Execute → Report
    pub async fn run(
        &self,
        request: ConversionRequest,
        job_id: String,
        manager: Arc<JobManager>,
        token: CancellationToken,
    ) -> Result<ConversionResult, AppError> {
        let family = request.family.clone();

        // 1. Lifecycle: Active Processing
        manager.update_status(&job_id, JobStatus::Running)?;

        let mut current_stage = JobStage::Validate;

        // Closure to handle the steps and capture errors for reporting
        let result = async {
            // 2. Phase: Validating
            manager.update_progress(
                &job_id,
                JobProgress {
                    stage: "Validating".to_string(),
                    message: "Checking input and options...".to_string(),
                    percent: None,
                },
            )?;

            if token.is_cancelled() {
                return Err(AppError::Cancelled);
            }

            self.registry.validate(family.clone(), &request)?;

            // 3. Phase: Planning
            current_stage = JobStage::Plan;
            manager.update_progress(
                &job_id,
                JobProgress {
                    stage: "Planning".to_string(),
                    message: "Preparing execution steps...".to_string(),
                    percent: None,
                },
            )?;

            if token.is_cancelled() {
                return Err(AppError::Cancelled);
            }

            let plan = self.plan(family.clone(), request, &job_id, manager.clone())?;
            let request = plan.request;

            // Update the stored request so JobStatusDto shows the planned output path
            manager.update_planned_request(&job_id, &request)?;

            // 4. Phase: Input size (metadata only; the file is read once, by the converter)
            current_stage = JobStage::Execute;
            let input_integrity = std::fs::metadata(&request.input_path)
                .map(|m| crate::conversion::types::FileIntegrityDto { byte_size: m.len() })
                .map_err(|e| AppError::Io(format!("Input file could not be read: {}", e)))?;

            // 5. Phase: Prep — Ensure parent directory exists
            let output_path = Path::new(&request.output_path);
            if let Some(parent) = output_path.parent() {
                if !parent.exists() && !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent).map_err(|e| {
                        AppError::Io(format!("Failed to create output directory: {}", e))
                    })?;
                }
            }

            // 6. Phase: Executing
            manager.update_progress(
                &job_id,
                JobProgress {
                    stage: "Executing".to_string(),
                    message: "Performing conversion...".to_string(),
                    percent: None,
                },
            )?;

            if token.is_cancelled() {
                return Err(AppError::Cancelled);
            }

            let registry = Arc::clone(&self.registry);
            let ctx = ExecContext {
                token: Some(token.clone()),
                progress: Some(progress_reporter(Arc::clone(&manager), job_id.clone())),
            };
            let (result, request) = run_blocking(move || {
                let result = convert(&registry, &request, &ctx)?;
                Ok((result, request))
            })
            .await?;
            Ok((result, input_integrity, request))
        }
        .await;

        // 7. Post-Execute: attach input and output sizes
        let result = result.map(|(mut res, input_int, _req)| {
            if res.success {
                res.input_integrity = Some(input_int);
                res.output_integrity = res
                    .output_path
                    .as_ref()
                    .and_then(|p| std::fs::metadata(p).ok())
                    .map(|m| crate::conversion::types::FileIntegrityDto { byte_size: m.len() });
            }
            res
        });

        // 7. Finalize State
        match &result {
            Ok(res) => {
                manager.finalize_job(&job_id, res.clone())?;
            }
            Err(AppError::Cancelled) => {
                manager.update_status(&job_id, JobStatus::Cancelled)?;
            }
            Err(e) => {
                manager.fail_job(&job_id, e.to_failure(current_stage))?;
            }
        }

        result
    }

    fn plan(
        &self,
        family: AssetFamily,
        mut request: ConversionRequest,
        job_id: &str,
        manager: Arc<JobManager>,
    ) -> Result<ConversionPlan, AppError> {
        let input_path = std::path::Path::new(&request.input_path);
        let output_root = std::path::Path::new(&request.output_root);
        let format = &request.output_format.0;
        let overwrite = request.overwrite();

        // Resolve format descriptor from registry — single source of truth for canonical extension
        let descriptor = self.registry.get_format_descriptor(&family, format)?;

        let resolved_path = crate::conversion::output_naming::generate_output_path(
            input_path,
            output_root,
            &descriptor,
            overwrite,
            |path_str| manager.try_reserve_output_path(job_id, path_str),
        )?;

        request.output_path = resolved_path.to_string_lossy().to_string();

        Ok(ConversionPlan { family, request })
    }
}

#[cfg(test)]
#[path = "pipeline_tests.rs"]
mod tests;
