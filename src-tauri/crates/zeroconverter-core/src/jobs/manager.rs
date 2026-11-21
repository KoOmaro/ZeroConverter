// ==============================================================================
// OWNERSHIP:
//   Tracks conversion job state, lifecycle transitions, progress updates, event broadcasting, and output path reservations.
//
// DO NOT ADD:
//   - Conversion format logic
//   - Direct disk/file output writes
//   - Frontend DTO mapping
//   - UI state concerns
//
// NOTES:
//   Reservation conflicts use typed OutputReservationAttempt results, not string matching.
// ==============================================================================
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::app_error::AppError;
use crate::conversion::pipeline::ConversionPipeline;
use crate::conversion::types::{ConversionRequest, ConversionResult};
use crate::jobs::admission;
use crate::jobs::events::{CoreJobEvent, JobBroadcaster};
use crate::jobs::types::{
    ConversionJob, JobFailure, JobFailureKind, JobProgress, JobStage, JobStatus,
    OutputReservationAttempt, QueueStatus, QueueSummary,
};

/// Keep no more than one active job per logical core (minimum two).
/// Codec implementations may themselves use threads; this is an outer bound,
/// not a guarantee against all internal codec oversubscription.
fn default_job_concurrency() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .max(2)
}

pub struct JobManager {
    jobs: Mutex<HashMap<String, ConversionJob>>,
    cancellation_tokens: Mutex<HashMap<String, CancellationToken>>,
    /// Each active job's input, held so nothing can replace it meanwhile.
    input_guards: Mutex<HashMap<String, zero_fs::InputGuard>>,
    concurrency_semaphore: Arc<Semaphore>,
    memory_semaphore: Arc<Semaphore>,
    memory_budget_mib: u32,
    broadcaster: Arc<JobBroadcaster>,
}

impl JobManager {
    pub fn new(broadcaster: Arc<JobBroadcaster>) -> Self {
        let memory_budget_mib = admission::configured_memory_mib();
        Self {
            jobs: Mutex::new(HashMap::new()),
            cancellation_tokens: Mutex::new(HashMap::new()),
            input_guards: Mutex::new(HashMap::new()),
            concurrency_semaphore: Arc::new(Semaphore::new(default_job_concurrency())),
            memory_semaphore: Arc::new(Semaphore::new(memory_budget_mib as usize)),
            memory_budget_mib,
            broadcaster,
        }
    }

    pub fn add_listener(&self, listener: crate::jobs::events::Listener) {
        self.broadcaster.add_listener(listener);
    }

    pub fn submit_batch(
        self: &Arc<Self>,
        requests: Vec<ConversionRequest>,
        pipeline: Arc<ConversionPipeline>,
    ) -> Vec<String> {
        let mut ids = Vec::new();
        for req in requests {
            let id = self.create_job(req.clone());
            ids.push(id.clone());

            let manager = Arc::clone(self);
            let pipeline = Arc::clone(&pipeline);
            let semaphore = Arc::clone(&self.concurrency_semaphore);
            let job_id = id.clone();

            tokio::spawn(async move {
                // Tokens exist from creation, so a queued job can be cancelled
                // while waiting for either semaphore. No permit leaks on cancel.
                let token = manager.get_cancellation_token(&job_id);
                let _cpu_permit = tokio::select! {
                    biased;
                    _ = token.cancelled() => {
                        let _ = manager.update_status(&job_id, JobStatus::Cancelled);
                        return;
                    }
                    slot = semaphore.acquire() => match slot {
                        Ok(slot) => slot,
                        Err(_) => {
                            let _ = manager.fail_job(&job_id, JobFailure {
                                stage: JobStage::Validate,
                                kind: JobFailureKind::Internal,
                                message: "CPU admission semaphore closed".to_owned(),
                            });
                            return;
                        }
                    },
                };

                let req_data = {
                    let jobs = manager.jobs.lock().unwrap();
                    jobs.get(&job_id).map(|j| j.request.clone())
                };

                if let Some(request) = req_data {
                    // Header-only probing runs off the async runtime. An
                    // inaccessible or suspicious input consumes the whole
                    // budget until pipeline validation rejects it.
                    let budget = manager.memory_budget_mib;
                    let admission_request = request.clone();
                    let cost = tokio::task::spawn_blocking(move || {
                        admission::estimate_permits(&admission_request, budget)
                    })
                    .await
                    .unwrap_or(budget);

                    let _memory_permit = tokio::select! {
                        biased;
                        _ = token.cancelled() => {
                            let _ = manager.update_status(&job_id, JobStatus::Cancelled);
                            return;
                        }
                        slot = Arc::clone(&manager.memory_semaphore)
                            .acquire_many_owned(cost) => match slot {
                            Ok(slot) => slot,
                            Err(_) => {
                                let _ = manager.fail_job(&job_id, JobFailure {
                                    stage: JobStage::Validate,
                                    kind: JobFailureKind::Internal,
                                    message: "Memory admission semaphore closed".to_owned(),
                                });
                                return;
                            }
                        },
                    };

                    if token.is_cancelled() {
                        let _ = manager.update_status(&job_id, JobStatus::Cancelled);
                        return;
                    }
                    if manager.update_status(&job_id, JobStatus::Pending).is_err() {
                        return;
                    }
                    // Both permits stay live through integrity / manifest work.
                    let _ = pipeline.run(request, job_id, manager, token).await;
                }
            });
        }

        self.broadcast_summary();
        ids
    }

    pub fn create_job(&self, request: ConversionRequest) -> String {
        let id = Uuid::new_v4().to_string();
        if let Some(guard) = zero_fs::InputGuard::hold(std::path::Path::new(&request.input_path)) {
            self.input_guards.lock().unwrap().insert(id.clone(), guard);
        }
        let job = ConversionJob::new(id.clone(), request);
        let mut jobs = self.jobs.lock().unwrap();
        jobs.insert(id.clone(), job);
        // Create before submit_batch spawns: cancel_job must work in Queued.
        self.cancellation_tokens
            .lock()
            .unwrap()
            .insert(id.clone(), CancellationToken::new());
        id
    }

    pub fn get_job(&self, id: &str) -> Option<ConversionJob> {
        let jobs = self.jobs.lock().unwrap();
        jobs.get(id).cloned()
    }

    pub fn list_all_jobs(&self) -> Vec<ConversionJob> {
        let jobs = self.jobs.lock().unwrap();
        let mut list: Vec<_> = jobs.values().cloned().collect();
        list.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        list
    }

    pub fn get_queue_summary(&self) -> QueueSummary {
        let jobs = self.jobs.lock().unwrap();
        let mut summary = QueueSummary {
            status: QueueStatus::Idle,
            total: jobs.len(),
            queued: 0,
            running: 0,
            completed: 0,
            failed: 0,
            cancelled: 0,
        };

        for job in jobs.values() {
            match job.status {
                JobStatus::Queued => summary.queued += 1,
                JobStatus::Pending | JobStatus::Running => summary.running += 1,
                JobStatus::Completed => summary.completed += 1,
                JobStatus::Failed => summary.failed += 1,
                JobStatus::Cancelled => summary.cancelled += 1,
            }
        }

        if summary.running > 0 {
            summary.status = QueueStatus::Active;
        } else if summary.total > 0
            && summary.total == (summary.completed + summary.failed + summary.cancelled)
        {
            summary.status = QueueStatus::Completed;
        }

        summary
    }

    pub fn broadcast_summary(&self) {
        let summary = self.get_queue_summary();
        self.broadcaster
            .broadcast(CoreJobEvent::QueueSummaryChanged { summary });
    }

    pub fn try_reserve_output_path(
        &self,
        id: &str,
        path: &str,
    ) -> Result<OutputReservationAttempt, AppError> {
        let key = std::path::Path::new(path)
            .to_string_lossy()
            .to_lowercase()
            .replace("\\", "/");

        let mut jobs = self.jobs.lock().unwrap();

        for job in jobs.values() {
            if job.id != id && !job.status.is_terminal() {
                let input = job.request.input_path.to_lowercase().replace('\\', "/");
                if input == key {
                    return Ok(OutputReservationAttempt::Protected);
                }
                if let Some(ref res) = job.reserved_output_path {
                    if res == &key {
                        return Ok(OutputReservationAttempt::AlreadyReserved);
                    }
                }
            }
        }

        if let Some(job) = jobs.get_mut(id) {
            job.reserved_output_path = Some(key);
            Ok(OutputReservationAttempt::Reserved)
        } else {
            Err(AppError::Execute(format!("Job {} not found", id)))
        }
    }

    pub fn reserve_output_path(&self, id: &str, path: &str) -> Result<(), AppError> {
        match self.try_reserve_output_path(id, path)? {
            OutputReservationAttempt::Reserved => Ok(()),
            OutputReservationAttempt::AlreadyReserved => Err(AppError::Validate(format!(
                "Output path is already reserved by another active conversion job: {}",
                path
            ))),
            OutputReservationAttempt::Protected => Err(AppError::Validate(format!(
                "Output path is the input of another conversion job: {}",
                path
            ))),
        }
    }

    pub fn update_planned_request(
        &self,
        id: &str,
        request: &ConversionRequest,
    ) -> Result<(), AppError> {
        let mut jobs = self.jobs.lock().unwrap();
        if let Some(job) = jobs.get_mut(id) {
            job.request = request.clone();
            Ok(())
        } else {
            Err(AppError::Execute(format!("Job {} not found", id)))
        }
    }

    pub fn update_status(&self, id: &str, status: JobStatus) -> Result<(), AppError> {
        {
            let mut jobs = self.jobs.lock().unwrap();
            if let Some(job) = jobs.get_mut(id) {
                if job.status.can_transition_to(status) {
                    job.status = status;
                    job.updated_at = self.now();

                    if status.is_terminal() {
                        job.reserved_output_path = None;
                        let mut tokens = self.cancellation_tokens.lock().unwrap();
                        tokens.remove(id);
                        self.input_guards.lock().unwrap().remove(id);
                    }
                } else {
                    return Err(AppError::Execute(format!(
                        "Invalid state transition from {:?} to {:?}",
                        job.status, status
                    )));
                }
            } else {
                return Err(AppError::Execute(format!("Job {} not found", id)));
            }
        }

        // Broadcast status change
        self.broadcaster.broadcast(CoreJobEvent::StatusChanged {
            id: id.to_string(),
            status: format!("{:?}", status),
        });

        if status == JobStatus::Running {
            self.broadcaster
                .broadcast(CoreJobEvent::JobStarted { id: id.to_string() });
        }

        self.broadcast_summary();
        Ok(())
    }

    pub fn update_progress(&self, id: &str, progress: JobProgress) -> Result<(), AppError> {
        let mut jobs = self.jobs.lock().unwrap();
        if let Some(job) = jobs.get_mut(id) {
            if !job.status.is_terminal() {
                job.progress = progress.clone();
                job.updated_at = self.now();

                self.broadcaster.broadcast(CoreJobEvent::ProgressUpdated {
                    id: id.to_string(),
                    progress,
                });

                Ok(())
            } else {
                Err(AppError::Execute(format!(
                    "Cannot update progress of terminal job {}",
                    id
                )))
            }
        } else {
            Err(AppError::Execute(format!("Job {} not found", id)))
        }
    }

    pub fn finalize_job(&self, id: &str, result: ConversionResult) -> Result<(), AppError> {
        let status = if result.success {
            JobStatus::Completed
        } else {
            JobStatus::Failed
        };
        {
            let mut jobs = self.jobs.lock().unwrap();
            if let Some(job) = jobs.get_mut(id) {
                job.status = status;
                job.result = Some(result.clone());
                job.failure = None; // Reset failure state on success or finalization
                job.reserved_output_path = None;
                job.updated_at = self.now();

                let mut tokens = self.cancellation_tokens.lock().unwrap();
                tokens.remove(id);
                self.input_guards.lock().unwrap().remove(id);
            } else {
                return Err(AppError::Execute(format!("Job {} not found", id)));
            }
        }

        // Broadcast completion
        self.broadcaster.broadcast(CoreJobEvent::Completed {
            id: id.to_string(),
            result,
        });

        self.broadcast_summary();
        Ok(())
    }

    pub fn fail_job(&self, id: &str, failure: JobFailure) -> Result<(), AppError> {
        {
            let mut jobs = self.jobs.lock().unwrap();
            if let Some(job) = jobs.get_mut(id) {
                job.status = JobStatus::Failed;
                job.failure = Some(failure.clone());
                job.result = Some(ConversionResult {
                    success: false,
                    output_path: None,
                    message: failure.message.clone(),
                    input_integrity: None,
                    output_integrity: None,
                });
                job.reserved_output_path = None;
                job.updated_at = self.now();

                let mut tokens = self.cancellation_tokens.lock().unwrap();
                tokens.remove(id);
                self.input_guards.lock().unwrap().remove(id);
            } else {
                return Err(AppError::Execute(format!("Job {} not found", id)));
            }
        }

        // Broadcast failure
        self.broadcaster.broadcast(CoreJobEvent::Failed {
            id: id.to_string(),
            failure,
        });

        self.broadcast_summary();
        Ok(())
    }

    pub fn get_cancellation_token(&self, id: &str) -> CancellationToken {
        let mut tokens = self.cancellation_tokens.lock().unwrap();
        tokens.entry(id.to_string()).or_default().clone()
    }

    pub fn cancel_job(&self, id: &str) -> Result<(), AppError> {
        let tokens = self.cancellation_tokens.lock().unwrap();
        if let Some(token) = tokens.get(id) {
            token.cancel();
            Ok(())
        } else {
            Ok(())
        }
    }

    pub fn purge_terminal_jobs(&self) -> Result<(), AppError> {
        let mut jobs = self.jobs.lock().unwrap();
        let mut tokens = self.cancellation_tokens.lock().unwrap();

        let terminal_ids: Vec<String> = jobs
            .values()
            .filter(|j| j.status.is_terminal())
            .map(|j| j.id.clone())
            .collect();

        for id in terminal_ids {
            jobs.remove(&id);
            tokens.remove(&id);
            self.input_guards.lock().unwrap().remove(&id);
        }

        drop(jobs);
        drop(tokens);

        self.broadcast_summary();
        Ok(())
    }

    fn now(&self) -> u64 {
        SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    }
}

#[cfg(test)]
#[path = "manager_tests.rs"]
mod tests;
