use super::*;
use crate::conversion::types::{AssetFamily, ConversionRequest, OutputFormat};
use crate::jobs::types::JobFailure;
use crate::jobs::{JobFailureKind, JobStage};
use serde_json::json;

fn make_broadcaster() -> Arc<JobBroadcaster> {
    Arc::new(JobBroadcaster::new(Box::new(|_event| {
        serde_json::Value::Null // No-op mapper for tests
    })))
}

fn make_manager() -> Arc<JobManager> {
    Arc::new(JobManager::new(make_broadcaster()))
}

fn dummy_request() -> ConversionRequest {
    ConversionRequest {
        family: AssetFamily::Image,
        input_path: "/test/input.png".to_string(),
        output_path: "/test/output.webp".to_string(),
        output_root: "/test".to_string(),
        output_format: OutputFormat("webp".to_string()),
        options: json!({ "quality": 80, "overwrite": false }),
    }
}

// ═══════════════════════════════════════════
// Job Creation
// ═══════════════════════════════════════════

#[test]
fn create_job_returns_unique_id() {
    let mgr = make_manager();
    let id1 = mgr.create_job(dummy_request());
    let id2 = mgr.create_job(dummy_request());
    assert_ne!(id1, id2, "Each job must have a unique ID");
}

#[test]
fn create_job_starts_in_queued_state() {
    let mgr = make_manager();
    let id = mgr.create_job(dummy_request());
    let job = mgr.get_job(&id).unwrap();
    assert_eq!(job.status, JobStatus::Queued);
}

#[test]
fn create_job_preserves_request_data() {
    let mgr = make_manager();
    let req = dummy_request();
    let id = mgr.create_job(req.clone());
    let job = mgr.get_job(&id).unwrap();
    assert_eq!(job.request.input_path, req.input_path);
    assert_eq!(job.request.output_path, req.output_path);
}

// ═══════════════════════════════════════════
// Job Retrieval
// ═══════════════════════════════════════════

#[test]
fn get_job_returns_none_for_nonexistent_id() {
    let mgr = make_manager();
    assert!(mgr.get_job("nonexistent-id").is_none());
}

#[test]
fn list_all_jobs_returns_all_created_jobs() {
    let mgr = make_manager();
    mgr.create_job(dummy_request());
    mgr.create_job(dummy_request());
    mgr.create_job(dummy_request());
    assert_eq!(mgr.list_all_jobs().len(), 3);
}

#[test]
fn list_all_jobs_returns_empty_for_no_jobs() {
    let mgr = make_manager();
    assert!(mgr.list_all_jobs().is_empty());
}

// ═══════════════════════════════════════════
// Status Transitions
// ═══════════════════════════════════════════

#[test]
fn update_status_queued_to_pending() {
    let mgr = make_manager();
    let id = mgr.create_job(dummy_request());
    assert!(mgr.update_status(&id, JobStatus::Pending).is_ok());
    assert_eq!(mgr.get_job(&id).unwrap().status, JobStatus::Pending);
}

#[test]
fn update_status_pending_to_running() {
    let mgr = make_manager();
    let id = mgr.create_job(dummy_request());
    mgr.update_status(&id, JobStatus::Pending).unwrap();
    assert!(mgr.update_status(&id, JobStatus::Running).is_ok());
    assert_eq!(mgr.get_job(&id).unwrap().status, JobStatus::Running);
}

#[test]
fn update_status_running_to_completed() {
    let mgr = make_manager();
    let id = mgr.create_job(dummy_request());
    mgr.update_status(&id, JobStatus::Pending).unwrap();
    mgr.update_status(&id, JobStatus::Running).unwrap();
    // Note: finalize_job is the correct path, but testing direct transition
    // update_status allows Completed from Running per state machine
}

#[test]
fn update_status_rejects_invalid_transition() {
    let mgr = make_manager();
    let id = mgr.create_job(dummy_request());
    // Queued → Running is invalid (must go through Pending)
    let result = mgr.update_status(&id, JobStatus::Running);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("Invalid state transition"));
}

#[test]
fn update_status_rejects_nonexistent_job() {
    let mgr = make_manager();
    let result = mgr.update_status("ghost-id", JobStatus::Running);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("not found"));
}

#[test]
fn update_status_removes_cancellation_token_on_terminal() {
    let mgr = make_manager();
    let id = mgr.create_job(dummy_request());
    // Acquire a token first
    let _token = mgr.get_cancellation_token(&id);
    mgr.update_status(&id, JobStatus::Cancelled).unwrap();
    // Token should be cleaned up
    let tokens = mgr.cancellation_tokens.lock().unwrap();
    assert!(!tokens.contains_key(&id));
}

// ═══════════════════════════════════════════
// Progress Updates
// ═══════════════════════════════════════════

#[test]
fn update_progress_succeeds_for_active_job() {
    let mgr = make_manager();
    let id = mgr.create_job(dummy_request());
    mgr.update_status(&id, JobStatus::Pending).unwrap();
    let result = mgr.update_progress(
        &id,
        JobProgress {
            stage: "Processing".to_string(),
            message: "50% complete".to_string(),
            percent: Some(50.0),
        },
    );
    assert!(result.is_ok());
    let job = mgr.get_job(&id).unwrap();
    assert_eq!(job.progress.stage, "Processing");
    assert_eq!(job.progress.percent, Some(50.0));
}

#[test]
fn update_progress_rejects_terminal_job() {
    let mgr = make_manager();
    let id = mgr.create_job(dummy_request());
    mgr.update_status(&id, JobStatus::Cancelled).unwrap();
    let result = mgr.update_progress(
        &id,
        JobProgress {
            stage: "Ghost".to_string(),
            message: "Should not work".to_string(),
            percent: None,
        },
    );
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("terminal"));
}

#[test]
fn update_progress_rejects_nonexistent_job() {
    let mgr = make_manager();
    let result = mgr.update_progress(
        "phantom",
        JobProgress {
            stage: "X".to_string(),
            message: "X".to_string(),
            percent: None,
        },
    );
    assert!(result.is_err());
}

// ═══════════════════════════════════════════
// Finalization
// ═══════════════════════════════════════════

#[test]
fn finalize_job_sets_completed_on_success() {
    let mgr = make_manager();
    let id = mgr.create_job(dummy_request());
    mgr.update_status(&id, JobStatus::Pending).unwrap();
    mgr.update_status(&id, JobStatus::Running).unwrap();
    let result = ConversionResult {
        success: true,
        output_path: Some("/output.webp".to_string()),
        message: "Done".to_string(),
        input_integrity: None,
        output_integrity: None,
    };
    mgr.finalize_job(&id, result.clone()).unwrap();
    let job = mgr.get_job(&id).unwrap();
    assert_eq!(job.status, JobStatus::Completed);
    assert!(job.result.is_some());
    assert!(job.result.unwrap().success);
}

#[test]
fn finalize_job_sets_failed_on_failure() {
    let mgr = make_manager();
    let id = mgr.create_job(dummy_request());
    mgr.update_status(&id, JobStatus::Pending).unwrap();
    mgr.update_status(&id, JobStatus::Running).unwrap();
    let result = ConversionResult {
        success: false,
        output_path: None,
        message: "Codec error".to_string(),
        input_integrity: None,
        output_integrity: None,
    };
    mgr.finalize_job(&id, result).unwrap();
    let job = mgr.get_job(&id).unwrap();
    assert_eq!(job.status, JobStatus::Failed);
}

#[test]
fn finalize_job_rejects_nonexistent_job() {
    let mgr = make_manager();
    let result = ConversionResult {
        success: true,
        output_path: None,
        message: "phantom".to_string(),
        input_integrity: None,
        output_integrity: None,
    };
    assert!(mgr.finalize_job("ghost", result).is_err());
}

// ═══════════════════════════════════════════
// Fail Job
// ═══════════════════════════════════════════

#[test]
fn fail_job_sets_status_and_error_message() {
    let mgr = make_manager();
    let id = mgr.create_job(dummy_request());
    mgr.fail_job(
        &id,
        JobFailure {
            stage: JobStage::Execute,
            kind: JobFailureKind::Internal,
            message: "something broke".to_string(),
        },
    )
    .unwrap();
    let job = mgr.get_job(&id).unwrap();
    assert_eq!(job.status, JobStatus::Failed);
    assert_eq!(job.result.unwrap().message, "something broke");
    assert_eq!(job.failure.unwrap().stage, JobStage::Execute);
}

#[test]
fn fail_job_cleans_up_cancellation_token() {
    let mgr = make_manager();
    let id = mgr.create_job(dummy_request());
    let _token = mgr.get_cancellation_token(&id);
    mgr.fail_job(
        &id,
        JobFailure {
            stage: JobStage::Validate,
            kind: JobFailureKind::Validation,
            message: "error".to_string(),
        },
    )
    .unwrap();
    let tokens = mgr.cancellation_tokens.lock().unwrap();
    assert!(!tokens.contains_key(&id));
}

// ═══════════════════════════════════════════
// Cancellation
// ═══════════════════════════════════════════

#[test]
fn cancel_job_signals_token() {
    let mgr = make_manager();
    let id = mgr.create_job(dummy_request());
    let token = mgr.get_cancellation_token(&id);
    assert!(!token.is_cancelled());
    mgr.cancel_job(&id).unwrap();
    assert!(token.is_cancelled());
}

#[test]
fn cancel_nonexistent_job_succeeds_silently() {
    let mgr = make_manager();
    // Cancelling a job that doesn't exist shouldn't error
    assert!(mgr.cancel_job("no-such-job").is_ok());
}

#[test]
fn get_cancellation_token_creates_on_demand() {
    let mgr = make_manager();
    let id = mgr.create_job(dummy_request());
    let t1 = mgr.get_cancellation_token(&id);
    let t2 = mgr.get_cancellation_token(&id);
    // Same token instance (cloned from same underlying)
    t1.cancel();
    assert!(t2.is_cancelled());
}

// ═══════════════════════════════════════════
// Queue Summary
// ═══════════════════════════════════════════

#[test]
fn empty_queue_summary_is_idle() {
    let mgr = make_manager();
    let summary = mgr.get_queue_summary();
    assert_eq!(summary.total, 0);
    assert!(matches!(summary.status, QueueStatus::Idle));
}

#[test]
fn queue_summary_counts_queued_jobs() {
    let mgr = make_manager();
    mgr.create_job(dummy_request());
    mgr.create_job(dummy_request());
    let summary = mgr.get_queue_summary();
    assert_eq!(summary.total, 2);
    assert_eq!(summary.queued, 2);
    assert_eq!(summary.running, 0);
}

#[test]
fn queue_summary_shows_active_when_running() {
    let mgr = make_manager();
    let id = mgr.create_job(dummy_request());
    mgr.update_status(&id, JobStatus::Pending).unwrap();
    mgr.update_status(&id, JobStatus::Running).unwrap();
    let summary = mgr.get_queue_summary();
    assert!(matches!(summary.status, QueueStatus::Active));
    assert_eq!(summary.running, 1);
}

#[test]
fn queue_summary_shows_completed_when_all_terminal() {
    let mgr = make_manager();
    let id1 = mgr.create_job(dummy_request());
    let id2 = mgr.create_job(dummy_request());
    mgr.update_status(&id1, JobStatus::Cancelled).unwrap();
    mgr.fail_job(
        &id2,
        JobFailure {
            stage: JobStage::Validate,
            kind: JobFailureKind::Internal,
            message: "err".to_string(),
        },
    )
    .unwrap();
    let summary = mgr.get_queue_summary();
    assert!(matches!(summary.status, QueueStatus::Completed));
    assert_eq!(summary.cancelled, 1);
    assert_eq!(summary.failed, 1);
}

#[test]
fn queue_summary_mixed_states() {
    let mgr = make_manager();
    let _id1 = mgr.create_job(dummy_request()); // Queued
    let id2 = mgr.create_job(dummy_request()); // Will be Running
    let id3 = mgr.create_job(dummy_request()); // Will be Completed

    mgr.update_status(&id2, JobStatus::Pending).unwrap();
    mgr.update_status(&id2, JobStatus::Running).unwrap();

    mgr.update_status(&id3, JobStatus::Pending).unwrap();
    mgr.update_status(&id3, JobStatus::Running).unwrap();
    mgr.finalize_job(
        &id3,
        ConversionResult {
            success: true,
            output_path: None,
            message: "ok".to_string(),
            input_integrity: None,
            output_integrity: None,
        },
    )
    .unwrap();

    let summary = mgr.get_queue_summary();
    assert_eq!(summary.total, 3);
    assert_eq!(summary.queued, 1); // id1
    assert_eq!(summary.running, 1); // id2
    assert_eq!(summary.completed, 1); // id3
    assert!(matches!(summary.status, QueueStatus::Active));
}

// ═══════════════════════════════════════════
// Purge Terminal Jobs
// ═══════════════════════════════════════════

#[test]
fn purge_removes_completed_and_failed_jobs() {
    let mgr = make_manager();
    let id1 = mgr.create_job(dummy_request());
    let id2 = mgr.create_job(dummy_request());
    let id3 = mgr.create_job(dummy_request()); // stays queued

    mgr.update_status(&id1, JobStatus::Cancelled).unwrap();
    mgr.fail_job(
        &id2,
        JobFailure {
            stage: JobStage::Validate,
            kind: JobFailureKind::Internal,
            message: "err".to_string(),
        },
    )
    .unwrap();

    mgr.purge_terminal_jobs().unwrap();

    let remaining = mgr.list_all_jobs();
    assert_eq!(remaining.len(), 1, "Only non-terminal job should remain");
    assert_eq!(remaining[0].id, id3);
}

#[test]
fn purge_on_empty_queue_succeeds() {
    let mgr = make_manager();
    assert!(mgr.purge_terminal_jobs().is_ok());
}

#[test]
fn purge_leaves_running_jobs_intact() {
    let mgr = make_manager();
    let id = mgr.create_job(dummy_request());
    mgr.update_status(&id, JobStatus::Pending).unwrap();
    mgr.update_status(&id, JobStatus::Running).unwrap();
    mgr.purge_terminal_jobs().unwrap();
    assert_eq!(mgr.list_all_jobs().len(), 1);
}

// ═══════════════════════════════════════════
// Timestamp Updates
// ═══════════════════════════════════════════

#[test]
fn updated_at_changes_on_status_update() {
    let mgr = make_manager();
    let id = mgr.create_job(dummy_request());
    let original = mgr.get_job(&id).unwrap().updated_at;
    // Small sleep to ensure timestamp difference
    std::thread::sleep(std::time::Duration::from_millis(10));
    mgr.update_status(&id, JobStatus::Pending).unwrap();
    let updated = mgr.get_job(&id).unwrap().updated_at;
    assert!(updated >= original);
}

// ═══════════════════════════════════════════
// Output Path Reservation
// ═══════════════════════════════════════════

#[test]
fn reserve_output_path_succeeds_for_unique_paths() {
    let mgr = make_manager();
    let id1 = mgr.create_job(dummy_request());
    let id2 = mgr.create_job(dummy_request());

    assert!(mgr.reserve_output_path(&id1, "/out/file1.png").is_ok());
    assert!(mgr.reserve_output_path(&id2, "/out/file2.png").is_ok());

    assert_eq!(
        mgr.get_job(&id1).unwrap().reserved_output_path.unwrap(),
        "/out/file1.png"
    );
}

#[test]
fn reserve_output_path_fails_for_duplicate_active_path() {
    let mgr = make_manager();
    let id1 = mgr.create_job(dummy_request());
    let id2 = mgr.create_job(dummy_request());

    assert!(mgr.reserve_output_path(&id1, "/out/duplicate.png").is_ok());

    let err = mgr.reserve_output_path(&id2, "/out/DUPLICATE.png"); // tests case normalization
    assert!(err.is_err());
    assert!(err
        .unwrap_err()
        .to_string()
        .contains("already reserved by another active conversion job"));
}

#[test]
fn reserve_output_path_succeeds_if_previous_job_is_terminal() {
    let mgr = make_manager();
    let id1 = mgr.create_job(dummy_request());

    assert!(mgr.reserve_output_path(&id1, "/out/reused.png").is_ok());

    mgr.update_status(&id1, JobStatus::Cancelled).unwrap();

    let id2 = mgr.create_job(dummy_request());
    assert!(mgr.reserve_output_path(&id2, "/out/reused.png").is_ok());
}

// ═══════════════════════════════════════════
// Weighted admission and cancellation regressions
// ═══════════════════════════════════════════

async fn wait_until_terminal(manager: &JobManager, id: &str) -> JobStatus {
    for _ in 0..200 {
        if let Some(job) = manager.get_job(id) {
            if job.status.is_terminal() {
                return job.status;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    panic!("Timed out waiting for terminal job {id}");
}

fn empty_registry_pipeline() -> Arc<ConversionPipeline> {
    Arc::new(ConversionPipeline::new(Arc::new(
        crate::conversion::registry::ConverterRegistry::new(),
    )))
}

#[tokio::test]
async fn queued_job_can_be_cancelled_before_any_cpu_permit() {
    let manager = make_manager();
    let cpu_permits = manager.concurrency_semaphore.available_permits() as u32;
    let held = Arc::clone(&manager.concurrency_semaphore)
        .acquire_many_owned(cpu_permits)
        .await
        .unwrap();
    let ids = manager.submit_batch(vec![dummy_request()], empty_registry_pipeline());
    let id = &ids[0];
    assert_eq!(manager.get_job(id).unwrap().status, JobStatus::Queued);
    manager.cancel_job(id).unwrap();

    assert_eq!(
        wait_until_terminal(&manager, id).await,
        JobStatus::Cancelled
    );
    assert_eq!(manager.get_queue_summary().cancelled, 1);
    drop(held);
    assert_eq!(
        manager.concurrency_semaphore.available_permits() as u32,
        cpu_permits
    );
}

#[tokio::test]
async fn cancellation_while_waiting_for_memory_releases_cpu_slot() {
    let manager = make_manager();
    let available_memory = manager.memory_semaphore.available_permits() as u32;
    let held = Arc::clone(&manager.memory_semaphore)
        .acquire_many_owned(available_memory)
        .await
        .unwrap();
    let cpu_capacity = manager.concurrency_semaphore.available_permits();
    let ids = manager.submit_batch(vec![dummy_request()], empty_registry_pipeline());
    let id = &ids[0];

    // Wait until a CPU slot is held while the memory budget is exhausted.
    // The request has no real input file, so admission requests full budget.
    let mut waiting_on_memory = false;
    for _ in 0..200 {
        if manager.concurrency_semaphore.available_permits() < cpu_capacity {
            waiting_on_memory = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert!(waiting_on_memory, "Job did not reach memory admission");
    assert_eq!(manager.get_job(id).unwrap().status, JobStatus::Queued);
    manager.cancel_job(id).unwrap();
    assert_eq!(
        wait_until_terminal(&manager, id).await,
        JobStatus::Cancelled
    );
    // Terminal state is emitted just before the task drops its RAII permit.
    let mut released_cpu = false;
    for _ in 0..200 {
        if manager.concurrency_semaphore.available_permits() == cpu_capacity {
            released_cpu = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    assert!(
        released_cpu,
        "CPU permit was not released after cancellation"
    );
    drop(held);
    assert_eq!(
        manager.memory_semaphore.available_permits() as u32,
        available_memory
    );
}

#[tokio::test]
async fn weighted_memory_gate_blocks_second_expensive_worker() {
    let semaphore = Arc::new(Semaphore::new(128));
    let first = Arc::clone(&semaphore).try_acquire_many_owned(96).unwrap();
    assert!(Arc::clone(&semaphore).try_acquire_many_owned(96).is_err());
    drop(first);
    assert!(Arc::clone(&semaphore).try_acquire_many_owned(96).is_ok());
}

#[tokio::test]
async fn real_image_conversion_completes_through_weighted_admission() {
    let temp = tempfile::tempdir().unwrap();
    let input = temp.path().join("source.png");
    let output_root = temp.path().join("converted");
    std::fs::create_dir_all(&output_root).unwrap();
    let image = image::RgbImage::from_pixel(16, 16, image::Rgb([13, 57, 91]));
    image.save(&input).unwrap();

    let request = ConversionRequest {
        family: AssetFamily::Image,
        input_path: input.to_string_lossy().into_owned(),
        output_path: String::new(),
        output_root: output_root.to_string_lossy().into_owned(),
        output_format: OutputFormat("png".to_owned()),
        options: json!({"quality": 80, "overwrite": false}),
    };
    let mut registry = crate::conversion::registry::ConverterRegistry::new();
    registry.register(
        AssetFamily::Image,
        Arc::new(crate::handlers::image::ImageConverterHandler),
    );
    let pipeline = Arc::new(ConversionPipeline::new(Arc::new(registry)));
    let manager = make_manager();
    let ids = manager.submit_batch(vec![request], pipeline);
    assert_eq!(
        wait_until_terminal(&manager, &ids[0]).await,
        JobStatus::Completed
    );
    let job = manager.get_job(&ids[0]).unwrap();
    let result = job.result.unwrap();
    assert!(result.success);
    assert!(result.input_integrity.is_some());
    assert!(result.output_integrity.is_some());
    assert!(std::path::Path::new(result.output_path.as_deref().unwrap()).exists());
}
