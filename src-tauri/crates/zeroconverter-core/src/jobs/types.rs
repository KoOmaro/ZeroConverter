use crate::conversion::types::{ConversionRequest, ConversionResult};
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    Queued,
    Pending,
    Running,
    Completed,
    Failed,
    Cancelled,
}

impl JobStatus {
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled
        )
    }

    pub fn can_transition_to(&self, next: JobStatus) -> bool {
        if self.is_terminal() {
            return false;
        }

        matches!(
            (self, next),
            (JobStatus::Queued, JobStatus::Pending | JobStatus::Cancelled)
                | (
                    JobStatus::Pending,
                    JobStatus::Running | JobStatus::Cancelled
                )
                | (
                    JobStatus::Running,
                    JobStatus::Completed | JobStatus::Failed | JobStatus::Cancelled
                )
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum QueueStatus {
    Idle,
    Active,
    Draining,
    Completed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueueSummary {
    pub status: QueueStatus,
    pub total: usize,
    pub queued: usize,
    pub running: usize,
    pub completed: usize,
    pub failed: usize,
    pub cancelled: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputReservationAttempt {
    Reserved,
    AlreadyReserved,
    /// The path is the input of another job: never replace it, pick a new
    /// name instead (even with overwrite on).
    Protected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStage {
    Validate,
    Plan,
    Execute,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobFailureKind {
    Io,
    Format,
    Decode,
    Validation,
    Internal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobFailure {
    pub stage: JobStage,
    pub kind: JobFailureKind,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobProgress {
    pub stage: String,
    pub message: String,
    pub percent: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConversionJob {
    pub id: String,
    pub request: ConversionRequest,
    pub status: JobStatus,
    pub progress: JobProgress,
    pub failure: Option<JobFailure>,
    pub result: Option<ConversionResult>,
    pub reserved_output_path: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

impl ConversionJob {
    pub fn new(id: String, request: ConversionRequest) -> Self {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            id,
            request,
            status: JobStatus::Queued,
            progress: JobProgress {
                stage: "Queued".to_string(),
                message: "Waiting in queue".to_string(),
                percent: None,
            },
            failure: None,
            result: None,
            reserved_output_path: None,
            created_at: now,
            updated_at: now,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::conversion::types::{AssetFamily, OutputFormat};
    use serde_json::json;

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
    // Terminal State Detection
    // ═══════════════════════════════════════════

    #[test]
    fn completed_is_terminal() {
        assert!(JobStatus::Completed.is_terminal());
    }

    #[test]
    fn failed_is_terminal() {
        assert!(JobStatus::Failed.is_terminal());
    }

    #[test]
    fn cancelled_is_terminal() {
        assert!(JobStatus::Cancelled.is_terminal());
    }

    #[test]
    fn queued_is_not_terminal() {
        assert!(!JobStatus::Queued.is_terminal());
    }

    #[test]
    fn pending_is_not_terminal() {
        assert!(!JobStatus::Pending.is_terminal());
    }

    #[test]
    fn running_is_not_terminal() {
        assert!(!JobStatus::Running.is_terminal());
    }

    // ═══════════════════════════════════════════
    // Valid State Transitions
    // ═══════════════════════════════════════════

    #[test]
    fn queued_can_transition_to_pending() {
        assert!(JobStatus::Queued.can_transition_to(JobStatus::Pending));
    }

    #[test]
    fn queued_can_transition_to_cancelled() {
        assert!(JobStatus::Queued.can_transition_to(JobStatus::Cancelled));
    }

    #[test]
    fn pending_can_transition_to_running() {
        assert!(JobStatus::Pending.can_transition_to(JobStatus::Running));
    }

    #[test]
    fn pending_can_transition_to_cancelled() {
        assert!(JobStatus::Pending.can_transition_to(JobStatus::Cancelled));
    }

    #[test]
    fn running_can_transition_to_completed() {
        assert!(JobStatus::Running.can_transition_to(JobStatus::Completed));
    }

    #[test]
    fn running_can_transition_to_failed() {
        assert!(JobStatus::Running.can_transition_to(JobStatus::Failed));
    }

    #[test]
    fn running_can_transition_to_cancelled() {
        assert!(JobStatus::Running.can_transition_to(JobStatus::Cancelled));
    }

    // ═══════════════════════════════════════════
    // Invalid State Transitions
    // ═══════════════════════════════════════════

    #[test]
    fn queued_cannot_transition_to_running() {
        // Must go through Pending first
        assert!(!JobStatus::Queued.can_transition_to(JobStatus::Running));
    }

    #[test]
    fn queued_cannot_transition_to_completed() {
        assert!(!JobStatus::Queued.can_transition_to(JobStatus::Completed));
    }

    #[test]
    fn queued_cannot_transition_to_failed() {
        assert!(!JobStatus::Queued.can_transition_to(JobStatus::Failed));
    }

    #[test]
    fn queued_cannot_transition_to_queued() {
        assert!(!JobStatus::Queued.can_transition_to(JobStatus::Queued));
    }

    #[test]
    fn pending_cannot_transition_to_completed() {
        // Must go through Running first
        assert!(!JobStatus::Pending.can_transition_to(JobStatus::Completed));
    }

    #[test]
    fn pending_cannot_transition_to_failed() {
        assert!(!JobStatus::Pending.can_transition_to(JobStatus::Failed));
    }

    #[test]
    fn pending_cannot_transition_to_queued() {
        assert!(!JobStatus::Pending.can_transition_to(JobStatus::Queued));
    }

    #[test]
    fn pending_cannot_transition_to_pending() {
        assert!(!JobStatus::Pending.can_transition_to(JobStatus::Pending));
    }

    #[test]
    fn running_cannot_transition_to_queued() {
        assert!(!JobStatus::Running.can_transition_to(JobStatus::Queued));
    }

    #[test]
    fn running_cannot_transition_to_pending() {
        assert!(!JobStatus::Running.can_transition_to(JobStatus::Pending));
    }

    #[test]
    fn running_cannot_transition_to_running() {
        assert!(!JobStatus::Running.can_transition_to(JobStatus::Running));
    }

    // ═══════════════════════════════════════════
    // Terminal States Block All Transitions
    // ═══════════════════════════════════════════

    #[test]
    fn completed_cannot_transition_to_anything() {
        assert!(!JobStatus::Completed.can_transition_to(JobStatus::Queued));
        assert!(!JobStatus::Completed.can_transition_to(JobStatus::Pending));
        assert!(!JobStatus::Completed.can_transition_to(JobStatus::Running));
        assert!(!JobStatus::Completed.can_transition_to(JobStatus::Failed));
        assert!(!JobStatus::Completed.can_transition_to(JobStatus::Cancelled));
        assert!(!JobStatus::Completed.can_transition_to(JobStatus::Completed));
    }

    #[test]
    fn failed_cannot_transition_to_anything() {
        assert!(!JobStatus::Failed.can_transition_to(JobStatus::Queued));
        assert!(!JobStatus::Failed.can_transition_to(JobStatus::Pending));
        assert!(!JobStatus::Failed.can_transition_to(JobStatus::Running));
        assert!(!JobStatus::Failed.can_transition_to(JobStatus::Completed));
        assert!(!JobStatus::Failed.can_transition_to(JobStatus::Cancelled));
        assert!(!JobStatus::Failed.can_transition_to(JobStatus::Failed));
    }

    #[test]
    fn cancelled_cannot_transition_to_anything() {
        assert!(!JobStatus::Cancelled.can_transition_to(JobStatus::Queued));
        assert!(!JobStatus::Cancelled.can_transition_to(JobStatus::Pending));
        assert!(!JobStatus::Cancelled.can_transition_to(JobStatus::Running));
        assert!(!JobStatus::Cancelled.can_transition_to(JobStatus::Completed));
        assert!(!JobStatus::Cancelled.can_transition_to(JobStatus::Failed));
        assert!(!JobStatus::Cancelled.can_transition_to(JobStatus::Cancelled));
    }

    // ═══════════════════════════════════════════
    // ConversionJob Initialization
    // ═══════════════════════════════════════════

    #[test]
    fn new_job_starts_in_queued_state() {
        let job = ConversionJob::new("test-1".to_string(), dummy_request());
        assert_eq!(job.status, JobStatus::Queued);
    }

    #[test]
    fn new_job_has_no_result() {
        let job = ConversionJob::new("test-1".to_string(), dummy_request());
        assert!(job.result.is_none());
    }

    #[test]
    fn new_job_progress_shows_queued() {
        let job = ConversionJob::new("test-1".to_string(), dummy_request());
        assert_eq!(job.progress.stage, "Queued");
        assert!(job.progress.percent.is_none());
    }

    #[test]
    fn new_job_preserves_request() {
        let req = dummy_request();
        let job = ConversionJob::new("test-1".to_string(), req.clone());
        assert_eq!(job.request.input_path, req.input_path);
        assert_eq!(job.request.output_path, req.output_path);
    }

    #[test]
    fn new_job_has_valid_timestamps() {
        let job = ConversionJob::new("test-1".to_string(), dummy_request());
        assert!(job.created_at > 0);
        assert_eq!(job.created_at, job.updated_at);
    }

    #[test]
    fn new_job_preserves_id() {
        let job = ConversionJob::new("custom-id-42".to_string(), dummy_request());
        assert_eq!(job.id, "custom-id-42");
    }

    // ═══════════════════════════════════════════
    // QueueStatus Serialization
    // ═══════════════════════════════════════════

    #[test]
    fn queue_summary_defaults() {
        let summary = QueueSummary {
            status: QueueStatus::Idle,
            total: 0,
            queued: 0,
            running: 0,
            completed: 0,
            failed: 0,
            cancelled: 0,
        };
        assert_eq!(summary.total, 0);
    }
}
