//! # RHPM Task Worker Subsystem
//!
//! Dedicated Task Workers handle heavy, non-latency-critical background work
//! such as asynchronous database operations (`ADB::*`), background file writes (`AFile::*`),
//! outbound background HTTP queries (`AHttp::*`), and `Future::defer` jobs.
//!
//! Because Task Workers are decoupled from the standard HTTP request-response
//! path, prolonged CPU bursts or heavy operations do not degrade latency or
//! stall the corosensei stackful fibers serving HTTP traffic.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::mpsc;

/// Unique identifier for a background task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskId(pub u64);

static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(1);

impl TaskId {
    pub fn next() -> Self {
        Self(NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed))
    }
}

impl std::fmt::Display for TaskId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "task#{}", self.0)
    }
}

/// Category of background task.
#[derive(Debug, Clone)]
pub enum TaskKind {
    /// Asynchronous database query (`ADB::*`).
    Database { query: String },
    /// Asynchronous file operation (`AFile::*`).
    File { path: String, operation: String },
    /// Asynchronous HTTP request (`AHttp::*`).
    Http { url: String, method: String },
    /// Generic deferred closure or job.
    Custom { name: String, payload: Vec<u8> },
}

/// Task unit dispatched to a Task Worker.
pub struct TaskJob {
    pub task_id: TaskId,
    pub kind: TaskKind,
    pub queued_at: Instant,
}

/// Sender handle to enqueue jobs into the Task Worker queue.
#[derive(Clone)]
pub struct TaskQueueSender {
    sender: mpsc::UnboundedSender<TaskJob>,
}

impl TaskQueueSender {
    /// Dispatch a job to the background task queue without blocking.
    pub fn dispatch(&self, kind: TaskKind) -> TaskId {
        let task_id = TaskId::next();
        let job = TaskJob {
            task_id,
            kind,
            queued_at: Instant::now(),
        };
        let _ = self.sender.send(job);
        task_id
    }
}

/// Task Worker executor loop.
pub struct TaskWorker {
    pub worker_id: u32,
    pub receiver: mpsc::UnboundedReceiver<TaskJob>,
    pub shutdown: Arc<AtomicBool>,
}

impl TaskWorker {
    /// Create a new Task Worker and its sender handle.
    pub fn new(worker_id: u32, shutdown: Arc<AtomicBool>) -> (TaskQueueSender, Self) {
        let (sender, receiver) = mpsc::unbounded_channel();
        (TaskQueueSender { sender }, Self {
            worker_id,
            receiver,
            shutdown,
        })
    }

    /// Run the Task Worker event loop.
    pub async fn run(mut self) {
        while let Some(job) = self.receiver.recv().await {
            if self.shutdown.load(Ordering::Relaxed) {
                break;
            }
            // Execute background job
            match job.kind {
                TaskKind::Database { query } => {
                    let _ = query;
                    // Async DB execution hook (e.g. background logging / audit trails)
                }
                TaskKind::File { path, operation } => {
                    let _ = (path, operation);
                    // Async file I/O hook
                }
                TaskKind::Http { url, method } => {
                    let _ = (url, method);
                    // Async webhook / telemetry ping
                }
                TaskKind::Custom { name, payload } => {
                    let _ = (name, payload);
                }
            }
        }
    }
}
