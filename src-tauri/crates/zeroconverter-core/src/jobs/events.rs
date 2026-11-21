use crate::conversion::types::ConversionResult;
use crate::jobs::types::{JobFailure, JobProgress, QueueSummary};
use std::sync::Mutex;

/// Receives each event as transport JSON. Returns false once it is gone
/// (a closed window), and is then dropped.
pub type Listener = Box<dyn Fn(&serde_json::Value) -> bool + Send + Sync>;

/// Core-domain job event. This is the source of truth.
/// API layer converts these to transport DTOs at the boundary.
#[derive(Debug, Clone)]
pub enum CoreJobEvent {
    JobStarted {
        id: String,
    },
    StatusChanged {
        id: String,
        status: String,
    },
    ProgressUpdated {
        id: String,
        progress: JobProgress,
    },
    Completed {
        id: String,
        result: ConversionResult,
    },
    Failed {
        id: String,
        failure: JobFailure,
    },
    QueueSummaryChanged {
        summary: QueueSummary,
    },
}

/// Hands job events to whoever listens (the app's windows). `core` does not
/// know how they are delivered: the API layer registers a listener that
/// sends on a Tauri channel. Events go out through a mapper set at
/// construction, which turns them into transport DTOs.
pub struct JobBroadcaster {
    subscribers: Mutex<Vec<Listener>>,
    mapper: Box<dyn Fn(&CoreJobEvent) -> serde_json::Value + Send + Sync>,
}

impl JobBroadcaster {
    pub fn new(mapper: Box<dyn Fn(&CoreJobEvent) -> serde_json::Value + Send + Sync>) -> Self {
        Self {
            subscribers: Mutex::new(Vec::new()),
            mapper,
        }
    }

    /// Attaches a live event listener.
    pub fn add_listener(&self, listener: Listener) {
        self.subscribers.lock().unwrap().push(listener);
    }

    /// Broadcasts a CoreJobEvent to all subscribers.
    /// The mapper converts it to transport format at the boundary.
    /// Resilient: Failed sends trigger immediate pruning of stale/closed channels.
    pub fn broadcast(&self, event: CoreJobEvent) {
        let payload = (self.mapper)(&event);
        // Listeners that report themselves gone are dropped.
        self.subscribers
            .lock()
            .unwrap()
            .retain(|listen| listen(&payload));
    }
}
