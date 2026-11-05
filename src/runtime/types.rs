//! Shared data types used across all RHPM runtime modules.

use std::sync::atomic::{AtomicU64, Ordering};

/// Globally unique, monotonically increasing request identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RequestId(pub u64);

static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

impl RequestId {
    /// Allocate a new unique [`RequestId`].
    pub fn next() -> Self {
        Self(NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed))
    }
}

impl std::fmt::Display for RequestId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "req#{}", self.0)
    }
}

/// Discriminates between standard (latency-critical) and task (heavy-work) worker processes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerKind {
    /// Handles fast, interactive HTTP requests via stackful coroutines.
    Standard,
    /// Handles heavy/long-running background tasks and `Future::defer` callbacks.
    Task,
}

/// Reason a request coroutine was cancelled before completion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CancelReason {
    /// Client disconnected before the response was sent.
    ClientDisconnect,
    /// Wall-clock execution deadline exceeded.
    Timeout,
    /// Worker is being gracefully recycled (`max_requests` or RSS ceiling reached).
    WorkerRecycle,
    /// Manager requested worker shutdown.
    Shutdown,
}

impl std::fmt::Display for CancelReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ClientDisconnect => write!(f, "client disconnected"),
            Self::Timeout => write!(f, "request timeout exceeded"),
            Self::WorkerRecycle => write!(f, "worker recycling"),
            Self::Shutdown => write!(f, "graceful shutdown"),
        }
    }
}

/// Messages exchanged over the IPC control channel between process tiers.
#[derive(Debug, Clone)]
pub enum IpcMessage {
    /// Master → Worker: a new HTTP request is ready to dispatch.
    DispatchRequest {
        request_id: RequestId,
        method: String,
        uri: String,
        headers: Vec<(String, String)>,
        /// Body bytes for small payloads (<64 KB). `None` signals the worker to
        /// read from the memfd descriptor passed alongside this message.
        body: Option<Vec<u8>>,
    },
    /// Master → Worker: cancel an in-flight request (client disconnect / timeout).
    CancelRequest {
        request_id: RequestId,
        reason: CancelReason,
    },
    /// Worker → Master: HTTP response ready to flush.
    ResponseReady {
        request_id: RequestId,
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    },
    /// Worker → Master/Manager: append a batch of log records.
    LogBatch(Vec<LogRecord>),
    /// Worker → Manager: worker is about to exit (recycled / crashed).
    WorkerExit { worker_id: u32, reason: String },
    /// Manager → Worker: begin graceful drain and exit after active fibers complete.
    GracefulShutdown,
    /// Manager → Worker: diagnostic dump request (`rhp status --dump`).
    DiagnosticDump,
    /// Worker → Manager: response to a diagnostic dump.
    DiagnosticDumpResponse(Vec<ActiveFiberEntry>),
}

/// Minimal log record forwarded from a worker's ring buffer to the master log collector.
#[derive(Debug, Clone)]
pub struct LogRecord {
    pub worker_id: u32,
    pub request_id: RequestId,
    pub level: LogLevel,
    pub timestamp_micros: u64,
    pub message: String,
}

/// Log severity levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Debug = 0,
    Info = 1,
    Warn = 2,
    Error = 3,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Debug => write!(f, "DEBUG"),
            Self::Info  => write!(f, "INFO"),
            Self::Warn  => write!(f, "WARN"),
            Self::Error => write!(f, "ERROR"),
        }
    }
}

/// Snapshot of a single active request fiber for diagnostic introspection.
#[derive(Debug, Clone)]
pub struct ActiveFiberEntry {
    pub request_id: RequestId,
    pub worker_id: u32,
    pub method: String,
    pub uri: String,
    pub elapsed_ms: u64,
    pub state: FiberDiagState,
}

/// Simplified fiber state for diagnostic output.
#[derive(Debug, Clone)]
pub enum FiberDiagState {
    Running,
    WaitingIo { kind: String, elapsed_ms: u64 },
    Suspended,
}
