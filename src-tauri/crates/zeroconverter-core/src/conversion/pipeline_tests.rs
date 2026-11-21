use super::*;
use crate::conversion::registry::ConverterRegistry;
use crate::conversion::traits::ConverterHandler;
use crate::conversion::types::*;
use crate::jobs::{JobBroadcaster, JobFailureKind, JobManager, JobStage, JobStatus};
use crate::test_utils::{create_test_wav, make_request};
use std::path::Path;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

static MOCK_PIPELINE_DESCRIPTORS: &[FormatDescriptor] = &[
    FormatDescriptor {
        id: "wav",
        canonical_extension: "wav",
        aliases: &[],
        can_read: true,
        can_write: true,
    },
    FormatDescriptor {
        id: "mp3",
        canonical_extension: "mp3",
        aliases: &[],
        can_read: true,
        can_write: true,
    },
];

struct MockHandler;
impl ConverterHandler for MockHandler {
    fn format_descriptors(&self) -> &'static [FormatDescriptor] {
        MOCK_PIPELINE_DESCRIPTORS
    }
    fn probe(&self, _path: &Path) -> Result<InputFormat, AppError> {
        Ok(InputFormat("wav".to_string()))
    }
    fn capabilities(&self) -> Vec<ConversionCapability> {
        vec![]
    }
    fn validate(&self, request: &ConversionRequest) -> Result<(), AppError> {
        let path = Path::new(&request.input_path);
        if !path.exists() {
            return Err(AppError::Validate(format!(
                "Input file {} does not exist",
                request.input_path
            )));
        }
        Ok(())
    }
    fn execute(&self, request: &ConversionRequest) -> Result<ConversionResult, AppError> {
        // Write a mock output file so post-execute integrity can be computed
        let output = Path::new(&request.output_path);
        std::fs::write(output, b"mock-output-data")
            .map_err(|e| AppError::Execute(format!("mock write failed: {}", e)))?;
        Ok(ConversionResult {
            success: true,
            output_path: Some(request.output_path.clone()),
            message: "mocked".to_string(),
            input_integrity: None,
            output_integrity: None,
        })
    }
}

async fn setup() -> (ConversionPipeline, Arc<JobManager>) {
    let mut registry = ConverterRegistry::new();
    registry.register(AssetFamily::Audio, Arc::new(MockHandler));
    let registry = Arc::new(registry);
    let pipeline = ConversionPipeline::new(registry);
    let broadcaster = Arc::new(JobBroadcaster::new(Box::new(|_| serde_json::Value::Null)));
    let manager = Arc::new(JobManager::new(broadcaster));
    (pipeline, manager)
}

#[tokio::test]
async fn pipeline_orchestrates_successful_conversion() {
    let (pipeline, manager) = setup().await;
    // Ensure the input file is persisted long enough
    let input_named = create_test_wav(1, 44100, 100);
    let input_path = input_named.path().to_owned();
    let output_dir = tempfile::tempdir().unwrap();
    let output_path = output_dir.path().join("out.mp3");

    let req = make_request(AssetFamily::Audio, &input_path, &output_path, "mp3", None);

    let job_id = manager.create_job(req.clone());
    manager.update_status(&job_id, JobStatus::Pending).unwrap();

    let token = CancellationToken::new();
    let result = pipeline
        .run(req, job_id.clone(), manager.clone(), token)
        .await;

    assert!(result.is_ok());
    let job = manager.get_job(&job_id).unwrap();
    assert_eq!(job.status, JobStatus::Completed);
    assert!(
        job.failure.is_none(),
        "Successful job should not have failure data"
    );
}

#[tokio::test]
async fn pipeline_short_circuits_on_validation_failure() {
    let (pipeline, manager) = setup().await;
    // Non-existent input will fail validation
    let req = make_request(
        AssetFamily::Audio,
        Path::new("not_real.wav"),
        Path::new("out.mp3"),
        "mp3",
        None,
    );

    let job_id = manager.create_job(req.clone());
    manager.update_status(&job_id, JobStatus::Pending).unwrap();

    let token = CancellationToken::new();
    let result = pipeline
        .run(req, job_id.clone(), manager.clone(), token)
        .await;

    assert!(result.is_err());
    let job = manager.get_job(&job_id).unwrap();
    assert_eq!(job.status, JobStatus::Failed);

    let failure = job.failure.expect("Expected structured failure data");
    assert_eq!(failure.stage, JobStage::Validate);
    assert_eq!(failure.kind, JobFailureKind::Validation);
}

#[tokio::test]
async fn pipeline_respects_cancellation() {
    let (pipeline, manager) = setup().await;
    let input = create_test_wav(1, 44100, 100);
    let output_dir = tempfile::tempdir().unwrap();
    let output_path = output_dir.path().join("cancelled.mp3");

    let req = make_request(AssetFamily::Audio, input.path(), &output_path, "mp3", None);

    let job_id = manager.create_job(req.clone());
    manager.update_status(&job_id, JobStatus::Pending).unwrap();

    let token = CancellationToken::new();
    token.cancel(); // Cancel immediately

    let result = pipeline
        .run(req, job_id.clone(), manager.clone(), token)
        .await;

    assert!(result.is_err());
    let job = manager.get_job(&job_id).unwrap();
    assert_eq!(job.status, JobStatus::Cancelled);
    assert!(
        job.failure.is_none(),
        "Cancelled job should not have failure data"
    );
}

/// Writes its output and cancels the job's token on the way, like a user
/// pressing Cancel while an encoder that never checks the token is running.
struct CancelledMidway(CancellationToken);
impl ConverterHandler for CancelledMidway {
    fn format_descriptors(&self) -> &'static [FormatDescriptor] {
        MOCK_PIPELINE_DESCRIPTORS
    }
    fn probe(&self, _path: &Path) -> Result<InputFormat, AppError> {
        Ok(InputFormat("wav".to_string()))
    }
    fn capabilities(&self) -> Vec<ConversionCapability> {
        vec![]
    }
    fn validate(&self, _request: &ConversionRequest) -> Result<(), AppError> {
        Ok(())
    }
    fn execute(&self, request: &ConversionRequest) -> Result<ConversionResult, AppError> {
        std::fs::write(&request.output_path, b"late output").unwrap();
        self.0.cancel();
        Ok(ConversionResult {
            success: true,
            output_path: Some(request.output_path.clone()),
            message: "done".to_string(),
            input_integrity: None,
            output_integrity: None,
        })
    }
}

#[tokio::test]
async fn cancel_during_execution_removes_the_new_file() {
    let token = CancellationToken::new();
    let mut registry = ConverterRegistry::new();
    registry.register(AssetFamily::Audio, Arc::new(CancelledMidway(token.clone())));
    let pipeline = ConversionPipeline::new(Arc::new(registry));
    let broadcaster = Arc::new(JobBroadcaster::new(Box::new(|_| serde_json::Value::Null)));
    let manager = Arc::new(JobManager::new(broadcaster));

    let input = create_test_wav(1, 44100, 100);
    let output_dir = tempfile::tempdir().unwrap();
    let output_path = output_dir.path().join("late.mp3");
    let req = make_request(AssetFamily::Audio, input.path(), &output_path, "mp3", None);
    let job_id = manager.create_job(req.clone());
    manager.update_status(&job_id, JobStatus::Pending).unwrap();

    let result = pipeline
        .run(req, job_id.clone(), manager.clone(), token)
        .await;

    assert!(result.is_err());
    assert_eq!(
        manager.get_job(&job_id).unwrap().status,
        JobStatus::Cancelled
    );
    assert!(!output_path.exists(), "the late output must be removed");
    assert_eq!(
        std::fs::read_dir(output_dir.path()).unwrap().count(),
        0,
        "no scratch folder left"
    );
}

/// Another program puts a file under the output name while converting: it
/// is never replaced (overwrite off) and never deleted.
struct NameTakenMidway;
impl ConverterHandler for NameTakenMidway {
    fn format_descriptors(&self) -> &'static [FormatDescriptor] {
        MOCK_PIPELINE_DESCRIPTORS
    }
    fn probe(&self, _path: &Path) -> Result<InputFormat, AppError> {
        Ok(InputFormat("wav".to_string()))
    }
    fn capabilities(&self) -> Vec<ConversionCapability> {
        vec![]
    }
    fn validate(&self, _request: &ConversionRequest) -> Result<(), AppError> {
        Ok(())
    }
    fn execute(&self, request: &ConversionRequest) -> Result<ConversionResult, AppError> {
        std::fs::write(&request.output_path, b"ours").unwrap();
        let folder = Path::new(&request.output_path)
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        std::fs::write(folder.join("taken.mp3"), b"theirs").unwrap();
        Ok(ConversionResult {
            success: true,
            output_path: Some(request.output_path.clone()),
            message: "done".to_string(),
            input_integrity: None,
            output_integrity: None,
        })
    }
}

#[test]
fn a_file_that_takes_the_name_meanwhile_is_kept() {
    let mut registry = ConverterRegistry::new();
    registry.register(AssetFamily::Audio, Arc::new(NameTakenMidway));
    let input = create_test_wav(1, 44100, 100);
    let output_dir = tempfile::tempdir().unwrap();
    let output_path = output_dir.path().join("taken.mp3");
    let req = make_request(AssetFamily::Audio, input.path(), &output_path, "mp3", None);
    let err = convert(&registry, &req, &ExecContext::none()).unwrap_err();
    assert!(err.to_string().contains("appeared"), "{err}");
    assert_eq!(std::fs::read(&output_path).unwrap(), b"theirs");
    assert_eq!(std::fs::read_dir(output_dir.path()).unwrap().count(), 1);
}

/// Helper: create a request using the new backend-owned naming contract.
fn make_naming_request(
    family: AssetFamily,
    input: &Path,
    output_root: &Path,
    format: &str,
    options: Option<serde_json::Value>,
) -> ConversionRequest {
    ConversionRequest {
        family,
        input_path: input.to_str().unwrap().to_string(),
        output_path: String::new(),
        output_root: output_root.to_str().unwrap().to_string(),
        output_format: OutputFormat(format.to_string()),
        options: options.unwrap_or(serde_json::json!({})),
    }
}

// ═══════════════════════════════════════════
// Backend-Owned Naming Policy Tests
// ═══════════════════════════════════════════

#[tokio::test]
async fn plan_generates_output_path_from_input_stem() {
    let (pipeline, manager) = setup().await;
    let input_named = create_test_wav(1, 44100, 100);
    let output_dir = tempfile::tempdir().unwrap();

    let req = make_naming_request(
        AssetFamily::Audio,
        input_named.path(),
        output_dir.path(),
        "mp3",
        None,
    );

    let job_id = manager.create_job(req.clone());
    manager.update_status(&job_id, JobStatus::Pending).unwrap();

    let result = pipeline
        .run(
            req,
            job_id.clone(),
            manager.clone(),
            CancellationToken::new(),
        )
        .await;

    assert!(result.is_ok(), "Expected success, got: {:?}", result);
    let job = manager.get_job(&job_id).unwrap();
    assert!(
        !job.request.output_path.is_empty(),
        "Stored request must have generated path"
    );
    assert!(job.request.output_path.ends_with(".mp3"));
    assert_eq!(job.status, JobStatus::Completed);
}

#[tokio::test]
async fn plan_dedupes_if_overwrite_disabled_and_file_exists() {
    let (pipeline, manager) = setup().await;
    let input_named = create_test_wav(1, 44100, 100);
    let input_path = input_named.path().to_owned();
    let stem = input_path
        .file_stem()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    let output_dir = tempfile::tempdir().unwrap();
    let existing_path = output_dir.path().join(format!("{}.mp3", stem));
    std::fs::write(&existing_path, b"existing data").unwrap();

    let req = make_naming_request(
        AssetFamily::Audio,
        &input_path,
        output_dir.path(),
        "mp3",
        Some(serde_json::json!({ "overwrite": false })),
    );

    let job_id = manager.create_job(req.clone());
    manager.update_status(&job_id, JobStatus::Pending).unwrap();
    let result = pipeline
        .run(
            req,
            job_id.clone(),
            manager.clone(),
            CancellationToken::new(),
        )
        .await;

    assert!(result.is_ok(), "Expected auto-dedupe success, got error");
    let job = manager.get_job(&job_id).unwrap();
    assert_eq!(job.status, JobStatus::Completed);
    let final_path = &job.request.output_path;
    assert!(
        final_path.contains(&format!("{} (1).mp3", stem)),
        "Expected deduped path, got: {}",
        final_path
    );
}

#[tokio::test]
async fn overwrite_false_preserves_existing_file_bytes() {
    let (pipeline, manager) = setup().await;
    let input_named = create_test_wav(1, 44100, 100);
    let input_path = input_named.path().to_owned();
    let stem = input_path
        .file_stem()
        .unwrap()
        .to_str()
        .unwrap()
        .to_string();

    let output_dir = tempfile::tempdir().unwrap();
    let existing_path = output_dir.path().join(format!("{}.mp3", stem));
    let dummy_bytes = b"EXISTING_DUMMY_BYTES_THAT_SHOULD_BE_PRESERVED";
    std::fs::write(&existing_path, dummy_bytes).unwrap();

    let req = make_naming_request(
        AssetFamily::Audio,
        &input_path,
        output_dir.path(),
        "mp3",
        Some(serde_json::json!({ "overwrite": false })),
    );

    let job_id = manager.create_job(req.clone());
    manager.update_status(&job_id, JobStatus::Pending).unwrap();
    let result = pipeline
        .run(
            req,
            job_id.clone(),
            manager.clone(),
            CancellationToken::new(),
        )
        .await;

    assert!(result.is_ok(), "Pipeline should auto-dedupe and succeed");
    let output_bytes = std::fs::read(&existing_path).unwrap();
    assert_eq!(
        output_bytes, dummy_bytes,
        "Original file bytes must remain unchanged"
    );
}

// ═══════════════════════════════════════════
// Output Folder Validation Tests
// ═══════════════════════════════════════════

#[tokio::test]
async fn test_pipeline_rejects_missing_output_root() {
    let (pipeline, manager) = setup().await;
    let input_named = create_test_wav(1, 44100, 10);
    let root_dir = tempfile::tempdir().unwrap();
    let missing_root = root_dir.path().join("missing");

    let req = make_naming_request(
        AssetFamily::Audio,
        input_named.path(),
        &missing_root,
        "mp3",
        None,
    );
    let job_id = manager.create_job(req.clone());
    manager.update_status(&job_id, JobStatus::Pending).unwrap();
    let result = pipeline
        .run(req, job_id, manager, CancellationToken::new())
        .await;

    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Output folder does not exist"));
}

#[tokio::test]
async fn test_pipeline_rejects_output_root_that_is_file() {
    let (pipeline, manager) = setup().await;
    let input_named = create_test_wav(1, 44100, 10);
    let file_root = tempfile::NamedTempFile::new().unwrap();

    let req = make_naming_request(
        AssetFamily::Audio,
        input_named.path(),
        file_root.path(),
        "mp3",
        None,
    );
    let job_id = manager.create_job(req.clone());
    manager.update_status(&job_id, JobStatus::Pending).unwrap();
    let result = pipeline
        .run(req, job_id, manager, CancellationToken::new())
        .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("not a directory"));
}

#[tokio::test]
async fn test_pipeline_output_inside_root_succeeds() {
    let (pipeline, manager) = setup().await;
    let input_named = create_test_wav(1, 44100, 10);
    let output_dir = tempfile::tempdir().unwrap();

    let req = make_naming_request(
        AssetFamily::Audio,
        input_named.path(),
        output_dir.path(),
        "mp3",
        None,
    );
    let job_id = manager.create_job(req.clone());
    manager.update_status(&job_id, JobStatus::Pending).unwrap();
    let result = pipeline
        .run(req, job_id, manager, CancellationToken::new())
        .await;
    assert!(result.is_ok());
}

// ═══════════════════════════════════════════
// Concurrent Collision Tests
// ═══════════════════════════════════════════

struct MockSlowHandler;
impl ConverterHandler for MockSlowHandler {
    fn format_descriptors(&self) -> &'static [FormatDescriptor] {
        MOCK_PIPELINE_DESCRIPTORS
    }
    fn probe(&self, _path: &Path) -> Result<InputFormat, AppError> {
        Ok(InputFormat("wav".to_string()))
    }
    fn capabilities(&self) -> Vec<ConversionCapability> {
        vec![]
    }
    fn validate(&self, _request: &ConversionRequest) -> Result<(), AppError> {
        Ok(())
    }
    fn execute(&self, request: &ConversionRequest) -> Result<ConversionResult, AppError> {
        std::thread::sleep(std::time::Duration::from_millis(200));
        let output = Path::new(&request.output_path);
        std::fs::write(output, b"mock-output-data").unwrap();
        Ok(ConversionResult {
            success: true,
            output_path: Some(request.output_path.clone()),
            message: "mocked".to_string(),
            input_integrity: None,
            output_integrity: None,
        })
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_same_stem_overwrite_false_both_succeed_via_dedupe() {
    let mut registry = ConverterRegistry::new();
    registry.register(AssetFamily::Audio, Arc::new(MockSlowHandler));
    let pipeline = Arc::new(ConversionPipeline::new(Arc::new(registry)));
    let broadcaster = Arc::new(JobBroadcaster::new(Box::new(|_| serde_json::Value::Null)));
    let manager = Arc::new(JobManager::new(broadcaster));

    let output_dir = tempfile::tempdir().unwrap();
    let input_named = create_test_wav(1, 44100, 10);
    let input_path = input_named.path().to_owned();

    let mut job_ids = Vec::new();
    for _ in 0..2 {
        let req = make_naming_request(
            AssetFamily::Audio,
            &input_path,
            output_dir.path(),
            "mp3",
            Some(serde_json::json!({ "overwrite": false })),
        );
        let job_id = manager.create_job(req.clone());
        manager.update_status(&job_id, JobStatus::Pending).unwrap();
        job_ids.push((job_id, req));
    }

    let mut handles = Vec::new();
    for (job_id, req) in job_ids.clone() {
        let p = Arc::clone(&pipeline);
        let m = Arc::clone(&manager);
        handles.push(tokio::spawn(async move {
            let _ = p.run(req, job_id, m, CancellationToken::new()).await;
        }));
    }
    for h in handles {
        let _ = h.await;
    }

    let job1 = manager.get_job(&job_ids[0].0).unwrap();
    let job2 = manager.get_job(&job_ids[1].0).unwrap();
    assert_eq!(job1.status, JobStatus::Completed);
    assert_eq!(job2.status, JobStatus::Completed);
    assert_ne!(
        job1.request.output_path, job2.request.output_path,
        "Paths must be distinct via dedupe"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn concurrent_same_stem_overwrite_true_one_fails_reservation() {
    let mut registry = ConverterRegistry::new();
    registry.register(AssetFamily::Audio, Arc::new(MockSlowHandler));
    let pipeline = Arc::new(ConversionPipeline::new(Arc::new(registry)));
    let broadcaster = Arc::new(JobBroadcaster::new(Box::new(|_| serde_json::Value::Null)));
    let manager = Arc::new(JobManager::new(broadcaster));

    let output_dir = tempfile::tempdir().unwrap();
    let input_named = create_test_wav(1, 44100, 10);
    let input_path = input_named.path().to_owned();

    let mut job_ids = Vec::new();
    for _ in 0..2 {
        let req = make_naming_request(
            AssetFamily::Audio,
            &input_path,
            output_dir.path(),
            "mp3",
            Some(serde_json::json!({ "overwrite": true })),
        );
        let job_id = manager.create_job(req.clone());
        manager.update_status(&job_id, JobStatus::Pending).unwrap();
        job_ids.push((job_id, req));
    }

    let mut handles = Vec::new();
    for (job_id, req) in job_ids.clone() {
        let p = Arc::clone(&pipeline);
        let m = Arc::clone(&manager);
        handles.push(tokio::spawn(async move {
            let _ = p.run(req, job_id, m, CancellationToken::new()).await;
        }));
    }
    for h in handles {
        let _ = h.await;
    }

    let job1 = manager.get_job(&job_ids[0].0).unwrap();
    let job2 = manager.get_job(&job_ids[1].0).unwrap();
    let success_count =
        (job1.status == JobStatus::Completed) as i32 + (job2.status == JobStatus::Completed) as i32;
    let fail_count =
        (job1.status == JobStatus::Failed) as i32 + (job2.status == JobStatus::Failed) as i32;
    assert_eq!(success_count, 1, "Exactly one job should succeed");
    assert_eq!(fail_count, 1, "Exactly one job should fail");

    let failed = if job1.status == JobStatus::Failed {
        job1
    } else {
        job2
    };
    let msg = failed.result.unwrap().message;
    assert!(
        msg.contains("already reserved"),
        "Expected reservation conflict, got: {}",
        msg
    );
}

// ═══════════════════════════════════════════
// Output Integrity Tests
// ═══════════════════════════════════════════

#[tokio::test]
async fn test_pipeline_successful_conversion_includes_integrity() {
    let (pipeline, manager) = setup().await;
    let input_named = create_test_wav(1, 44100, 10);
    let output_dir = tempfile::tempdir().unwrap();

    let req = make_naming_request(
        AssetFamily::Audio,
        input_named.path(),
        output_dir.path(),
        "mp3",
        None,
    );
    let job_id = manager.create_job(req.clone());
    manager.update_status(&job_id, JobStatus::Pending).unwrap();
    let result = pipeline
        .run(
            req,
            job_id.clone(),
            manager.clone(),
            CancellationToken::new(),
        )
        .await;

    assert!(result.is_ok(), "Expected success, got: {:?}", result);
    let res = result.unwrap();
    assert!(
        res.output_integrity.is_some(),
        "Output integrity must be present"
    );
    assert!(
        res.input_integrity.is_some(),
        "Input integrity must be present"
    );

    let output_int = res.output_integrity.unwrap();
    assert!(output_int.byte_size > 0);

    let input_int = res.input_integrity.unwrap();
    assert!(input_int.byte_size > 0);

    let job = manager.get_job(&job_id).unwrap();
    let final_path = &job.request.output_path;
    let file_meta = std::fs::metadata(final_path).unwrap();
    assert_eq!(output_int.byte_size, file_meta.len());

    assert_eq!(job.status, JobStatus::Completed);
}

#[tokio::test]
async fn test_pipeline_successful_audio_conversion_includes_integrity() {
    let mut registry = ConverterRegistry::new();
    registry.register(
        AssetFamily::Audio,
        Arc::new(crate::handlers::audio::AudioHandler),
    );
    let pipeline = ConversionPipeline::new(Arc::new(registry));
    let broadcaster = Arc::new(JobBroadcaster::new(Box::new(|_| serde_json::Value::Null)));
    let manager = Arc::new(JobManager::new(broadcaster));

    let input_named = create_test_wav(1, 44100, 100);
    let output_dir = tempfile::tempdir().unwrap();

    let req = make_naming_request(
        AssetFamily::Audio,
        input_named.path(),
        output_dir.path(),
        "mp3",
        None,
    );
    let job_id = manager.create_job(req.clone());
    manager.update_status(&job_id, JobStatus::Pending).unwrap();
    let result = pipeline
        .run(
            req,
            job_id.clone(),
            manager.clone(),
            CancellationToken::new(),
        )
        .await;

    assert!(result.is_ok(), "Expected success, got: {:?}", result);
    let res = result.unwrap();
    assert!(res.output_integrity.unwrap().byte_size > 0);
    assert!(res.input_integrity.unwrap().byte_size > 0);
}

/// Mock handler that reports success but provides no output_path.
struct NoOutputPathHandler;
impl ConverterHandler for NoOutputPathHandler {
    fn format_descriptors(&self) -> &'static [FormatDescriptor] {
        MOCK_PIPELINE_DESCRIPTORS
    }
    fn probe(&self, _path: &Path) -> Result<InputFormat, AppError> {
        Ok(InputFormat("wav".to_string()))
    }
    fn capabilities(&self) -> Vec<ConversionCapability> {
        vec![]
    }
    fn validate(&self, request: &ConversionRequest) -> Result<(), AppError> {
        if !Path::new(&request.input_path).exists() {
            return Err(AppError::Validate("missing".into()));
        }
        Ok(())
    }
    fn execute(&self, _request: &ConversionRequest) -> Result<ConversionResult, AppError> {
        Ok(ConversionResult {
            success: true,
            output_path: None,
            message: "no output path".to_string(),
            input_integrity: None,
            output_integrity: None,
        })
    }
}

#[tokio::test]
async fn test_pipeline_fails_if_successful_result_has_no_output_path() {
    let mut registry = ConverterRegistry::new();
    registry.register(AssetFamily::Audio, Arc::new(NoOutputPathHandler));
    let pipeline = ConversionPipeline::new(Arc::new(registry));
    let broadcaster = Arc::new(JobBroadcaster::new(Box::new(|_| serde_json::Value::Null)));
    let manager = Arc::new(JobManager::new(broadcaster));

    let input_named = create_test_wav(1, 44100, 10);
    let output_dir = tempfile::tempdir().unwrap();
    let req = make_naming_request(
        AssetFamily::Audio,
        input_named.path(),
        output_dir.path(),
        "mp3",
        None,
    );
    let job_id = manager.create_job(req.clone());
    manager.update_status(&job_id, JobStatus::Pending).unwrap();
    let result = pipeline
        .run(
            req,
            job_id.clone(),
            manager.clone(),
            CancellationToken::new(),
        )
        .await;

    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("no output path"));
    assert_eq!(manager.get_job(&job_id).unwrap().status, JobStatus::Failed);
}

/// Mock handler that reports success with output_path but does NOT write the file.
struct GhostFileHandler;
impl ConverterHandler for GhostFileHandler {
    fn format_descriptors(&self) -> &'static [FormatDescriptor] {
        MOCK_PIPELINE_DESCRIPTORS
    }
    fn probe(&self, _path: &Path) -> Result<InputFormat, AppError> {
        Ok(InputFormat("wav".to_string()))
    }
    fn capabilities(&self) -> Vec<ConversionCapability> {
        vec![]
    }
    fn validate(&self, request: &ConversionRequest) -> Result<(), AppError> {
        if !Path::new(&request.input_path).exists() {
            return Err(AppError::Validate("missing".into()));
        }
        Ok(())
    }
    fn execute(&self, request: &ConversionRequest) -> Result<ConversionResult, AppError> {
        Ok(ConversionResult {
            success: true,
            output_path: Some(request.output_path.clone()),
            message: "ghost".to_string(),
            input_integrity: None,
            output_integrity: None,
        })
    }
}

#[tokio::test]
async fn test_pipeline_fails_if_handler_output_missing_before_integrity() {
    let mut registry = ConverterRegistry::new();
    registry.register(AssetFamily::Audio, Arc::new(GhostFileHandler));
    let pipeline = ConversionPipeline::new(Arc::new(registry));
    let broadcaster = Arc::new(JobBroadcaster::new(Box::new(|_| serde_json::Value::Null)));
    let manager = Arc::new(JobManager::new(broadcaster));

    let input_named = create_test_wav(1, 44100, 10);
    let output_dir = tempfile::tempdir().unwrap();
    let req = make_naming_request(
        AssetFamily::Audio,
        input_named.path(),
        output_dir.path(),
        "mp3",
        None,
    );
    let job_id = manager.create_job(req.clone());
    manager.update_status(&job_id, JobStatus::Pending).unwrap();
    let result = pipeline
        .run(
            req,
            job_id.clone(),
            manager.clone(),
            CancellationToken::new(),
        )
        .await;

    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Converted file is missing"));
    assert_eq!(manager.get_job(&job_id).unwrap().status, JobStatus::Failed);
}

#[tokio::test]
async fn test_pipeline_successful_conversion_leaves_no_hidden_files() {
    let (pipeline, manager) = setup().await;
    let input_named = create_test_wav(1, 44100, 10);
    let output_dir = tempfile::tempdir().unwrap();

    let req = make_naming_request(
        AssetFamily::Audio,
        input_named.path(),
        output_dir.path(),
        "mp3",
        None,
    );
    let job_id = manager.create_job(req.clone());
    manager.update_status(&job_id, JobStatus::Pending).unwrap();
    let result = pipeline
        .run(
            req,
            job_id.clone(),
            manager.clone(),
            CancellationToken::new(),
        )
        .await;

    assert!(result.is_ok(), "Pipeline should succeed");
    let _res = result.unwrap();
    let job = manager.get_job(&job_id).unwrap();
    assert_eq!(job.status, JobStatus::Completed);

    // Nothing besides the converted file lands in the user's folder.
    assert!(!output_dir.path().join(".zero_artifacts").exists());
}

// Large inputs need the user's yes.

#[test]
fn a_file_over_100_mb_is_refused_without_consent() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("big.wav");
    std::fs::File::create(&input)
        .unwrap()
        .set_len(LARGE_INPUT_BYTES + 1)
        .unwrap();
    let output = dir.path().join("big.mp3");
    let registry = ConverterRegistry::new();
    let request = make_request(AssetFamily::Audio, &input, &output, "mp3", None);

    let err = convert(
        &registry,
        &request,
        &crate::conversion::traits::ExecContext::none(),
    )
    .unwrap_err();
    assert!(matches!(err, AppError::ConsentRequired { .. }), "{err:?}");
    assert!(err.to_string().contains("100 MB"));
    assert!(!output.exists());
}

#[test]
fn consent_lets_a_large_file_through_and_small_files_never_ask() {
    let dir = tempfile::tempdir().unwrap();
    let big = dir.path().join("big.wav");
    std::fs::File::create(&big)
        .unwrap()
        .set_len(LARGE_INPUT_BYTES + 1)
        .unwrap();
    let small = dir.path().join("small.wav");
    std::fs::write(&small, b"RIFF").unwrap();

    assert_eq!(large_input(&big), Some(LARGE_INPUT_BYTES + 1));
    assert_eq!(large_input(&small), None);
    let yes = make_request(
        AssetFamily::Audio,
        &big,
        &dir.path().join("o.mp3"),
        "mp3",
        Some(serde_json::json!({ "confirm_large": true })),
    );
    assert!(yes.large_confirmed());
    let no = make_request(
        AssetFamily::Audio,
        &big,
        &dir.path().join("o.mp3"),
        "mp3",
        None,
    );
    assert!(!no.large_confirmed());
}
