//! # Worker Process Isolation & Concurrency
//!
//! Implements Technical Point #14 of the RHP Architecture Specification:
//! - **Worker Process Isolation & Concurrency**: Spawn multi-threaded worker tasks or use
//!   process-level copy-on-write models to handle incoming requests concurrently against
//!   the shared parent state (`Arc<EngineState>`).
//! - **Zero Bootstrap Overhead**: Workers reuse the pre-parsed, pre-compiled immutable AST,
//!   type hierarchy, and bytecode module of the persistent parent process.
//! - **Copy-on-Write (CoW) State Isolation**: Request-level mutations (local variables, globals,
//!   `$_GET`/`$_POST`/`$_SERVER` superglobals, and static class properties) are isolated to the
//!   individual request VM and do not contaminate the parent process or other concurrent workers.
//! - **Fault Isolation & Panic Boundaries**: Failures, runtime errors, or panics in user requests
//!   are isolated to the request, returning clean error responses without crashing the worker pool
//!   or the parent server process.
//! - **Dual Worker Models**:
//!   - `WorkerModel::MultiThreaded`: Spawns concurrent multi-threaded worker tasks inside an
//!     in-memory thread pool with panic containment and isolated VM scopes.
//!   - `WorkerModel::ProcessCow`: Spawns process-level workers leveraging hardware virtual memory
//!     Copy-on-Write (on Unix via `fork()`), guaranteeing OS-enforced address-space isolation.

use std::any::Any;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use indexmap::IndexMap;

use crate::ir::lower_typed_program_with_base;
use crate::lexer::span::Span;
use crate::type_system::parse_and_check_with_hierarchy;
use crate::type_system::types::{RustFloatKind, RustIntKind};
use crate::vm::engine::VM;
use crate::vm::fiber::{enter_fiber, Fiber, FiberId, FiberStats};
use crate::vm::parent_process::{EngineState, FrozenValue};
use crate::vm::value::PhpValue;

/// Concurrency and execution model for worker request execution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WorkerModel {
    /// Spawns multi-threaded worker tasks running inside a thread pool with
    /// in-memory isolated VM instances, Copy-on-Write state rehydration, and panic boundaries.
    #[default]
    MultiThreaded,

    /// Process-level Copy-on-Write model. On Unix, utilizes `fork()` to leverage
    /// kernel MMU page-level copy-on-write, guaranteeing hardware-enforced fault isolation
    /// (segfaults, aborts, or panics never crash the parent process).
    ProcessCow,
}

/// Configuration options for a [`WorkerPool`].
#[derive(Debug, Clone)]
pub struct WorkerPoolConfig {
    /// Number of concurrent worker tasks/threads in the pool.
    pub num_workers: usize,
    /// Concurrency model (`MultiThreaded` vs `ProcessCow`).
    pub model: WorkerModel,
    /// Maximum capacity of the incoming request queue (default: 1024).
    pub queue_capacity: usize,
    /// Optional timeout for request execution.
    pub request_timeout: Option<Duration>,
}

impl Default for WorkerPoolConfig {
    fn default() -> Self {
        let default_workers = thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .max(2);
        Self {
            num_workers: default_workers,
            model: WorkerModel::MultiThreaded,
            queue_capacity: 1024,
            request_timeout: None,
        }
    }
}

impl WorkerPoolConfig {
    /// Create a new configuration with the specified worker count.
    pub fn new(num_workers: usize) -> Self {
        Self {
            num_workers: num_workers.max(1),
            ..Default::default()
        }
    }

    /// Set the worker concurrency model (`MultiThreaded` or `ProcessCow`).
    pub fn with_model(mut self, model: WorkerModel) -> Self {
        self.model = model;
        self
    }

    /// Set the bounded request queue capacity.
    pub fn with_queue_capacity(mut self, capacity: usize) -> Self {
        self.queue_capacity = capacity.max(1);
        self
    }

    /// Set the request execution timeout.
    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = Some(timeout);
        self
    }
}

/// Builder pattern for [`WorkerPool`].
#[derive(Debug, Default)]
pub struct WorkerPoolBuilder {
    config: WorkerPoolConfig,
}

impl WorkerPoolBuilder {
    /// Initialize a new builder with default settings.
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the number of worker tasks.
    pub fn workers(mut self, n: usize) -> Self {
        self.config.num_workers = n.max(1);
        self
    }

    /// Set the worker execution model.
    pub fn model(mut self, model: WorkerModel) -> Self {
        self.config.model = model;
        self
    }

    /// Set the request queue capacity.
    pub fn queue_capacity(mut self, cap: usize) -> Self {
        self.config.queue_capacity = cap.max(1);
        self
    }

    /// Set the request timeout.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.config.request_timeout = Some(timeout);
        self
    }

    /// Build the [`WorkerPool`] against the provided immutable parent [`EngineState`].
    pub fn build(self, state: Arc<EngineState>) -> Result<WorkerPool, WorkerError> {
        WorkerPool::new(state, self.config)
    }
}

/// Request payload: either a source script or direct pre-compiled function invocation.
#[derive(Debug, Clone, PartialEq)]
pub enum WorkerPayload {
    /// Source code script evaluated against the parent state.
    Source(String),
    /// Direct function or static method invocation with zero parsing overhead.
    Call {
        func_name: String,
        args: Vec<FrozenValue>,
    },
}

/// An incoming HTTP-style request payload dispatched to the worker pool.
///
/// Implements `Send + Sync` to permit safe transfer across worker threads and fibers.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkerRequest {
    /// Unique identifier for this request.
    pub id: String,
    /// Execution payload (script source or direct function call).
    pub payload: WorkerPayload,
    /// Superglobal `$_GET` parameters.
    pub get_params: IndexMap<String, FrozenValue>,
    /// Superglobal `$_POST` parameters.
    pub post_params: IndexMap<String, FrozenValue>,
    /// Superglobal `$_SERVER` environment and request metadata.
    pub server_vars: IndexMap<String, String>,
    /// Superglobal `$_COOKIE` parameters.
    pub cookies: IndexMap<String, String>,
    /// Request HTTP headers.
    pub headers: IndexMap<String, String>,
    /// Request-local global variables injected into VM global scope.
    pub globals: HashMap<String, FrozenValue>,
}

impl WorkerRequest {
    /// Create a request evaluating an RHP source script.
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            id: format!("req_{}", fastrand_u64()),
            payload: WorkerPayload::Source(source.into()),
            get_params: IndexMap::new(),
            post_params: IndexMap::new(),
            server_vars: IndexMap::new(),
            cookies: IndexMap::new(),
            headers: IndexMap::new(),
            globals: HashMap::new(),
        }
    }

    /// Create a request invoking an already-compiled function or static method.
    pub fn call(func_name: impl Into<String>, args: Vec<impl Into<FrozenValue>>) -> Self {
        Self {
            id: format!("req_{}", fastrand_u64()),
            payload: WorkerPayload::Call {
                func_name: func_name.into(),
                args: args.into_iter().map(Into::into).collect(),
            },
            get_params: IndexMap::new(),
            post_params: IndexMap::new(),
            server_vars: IndexMap::new(),
            cookies: IndexMap::new(),
            headers: IndexMap::new(),
            globals: HashMap::new(),
        }
    }

    /// Set an explicit unique request ID.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    /// Add a `$_GET` parameter.
    pub fn with_get(mut self, key: impl Into<String>, value: impl Into<FrozenValue>) -> Self {
        self.get_params.insert(key.into(), value.into());
        self
    }

    /// Add a `$_POST` parameter.
    pub fn with_post(mut self, key: impl Into<String>, value: impl Into<FrozenValue>) -> Self {
        self.post_params.insert(key.into(), value.into());
        self
    }

    /// Add a `$_SERVER` variable.
    pub fn with_server(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.server_vars.insert(key.into(), value.into());
        self
    }

    /// Add a `$_COOKIE` variable.
    pub fn with_cookie(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.cookies.insert(key.into(), value.into());
        self
    }

    /// Add an HTTP header.
    pub fn with_header(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(key.into(), value.into());
        self
    }

    /// Add a custom request-local global variable.
    pub fn with_global(mut self, key: impl Into<String>, value: impl Into<FrozenValue>) -> Self {
        self.globals.insert(key.into(), value.into());
        self
    }
}

/// Execution status of a completed worker request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerResponseStatus {
    /// Request executed successfully.
    Success,
    /// Request encountered a runtime error (e.g. DivisionByZero, UndefinedFunction, etc.).
    RuntimeError,
    /// Request encountered a compilation/syntax error.
    CompilationError,
    /// Request panicked and was safely caught by the worker boundary.
    Panic,
    /// Worker process terminated abnormally (used by `ProcessCow`).
    ProcessTerminated,
    /// Request exceeded configured execution timeout.
    Timeout,
}

/// Result returned after executing a request in an isolated worker.
///
/// Implements `Send + Sync` via [`FrozenValue`] snapshotting, making it safely
/// transferable across threads, channels, and IPC boundaries.
#[derive(Debug, Clone, PartialEq)]
pub struct WorkerResponse {
    /// ID of the request.
    pub request_id: String,
    /// Execution status.
    pub status: WorkerResponseStatus,
    /// Thread-safe evaluated value snapshot.
    pub value: FrozenValue,
    /// Standard output captured during request execution (`echo`, `print`).
    pub output: String,
    /// Elapsed wall-clock execution duration.
    pub duration: Duration,
    /// Error message if status is not `Success`.
    pub error: Option<String>,
    /// ID of the worker thread/process that handled the request.
    pub worker_id: usize,
    /// Execution telemetry of the local fiber that ran this request.
    pub fiber_stats: Option<FiberStats>,
}

impl WorkerResponse {
    /// Check whether the request finished with `WorkerResponseStatus::Success`.
    #[inline]
    pub fn is_success(&self) -> bool {
        self.status == WorkerResponseStatus::Success
    }

    /// Alias for `is_success`.
    #[inline]
    pub fn is_ok(&self) -> bool {
        self.is_success()
    }

    /// Access evaluated return value as thread-safe [`FrozenValue`].
    #[inline]
    pub fn value(&self) -> &FrozenValue {
        &self.value
    }

    /// Rehydrate the returned [`FrozenValue`] into a thread-local [`PhpValue`].
    #[inline]
    pub fn to_php_value(&self) -> PhpValue {
        self.value.to_php_value()
    }

    /// Access captured stdout output buffer.
    #[inline]
    pub fn output(&self) -> &str {
        &self.output
    }

    /// Access error message if any.
    #[inline]
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Access fiber execution statistics if executed within a fiber.
    #[inline]
    pub fn fiber_stats(&self) -> Option<&FiberStats> {
        self.fiber_stats.as_ref()
    }
}

/// Handle to an in-flight worker request for asynchronous retrieval.
#[derive(Debug)]
pub struct WorkerHandle {
    receiver: Receiver<WorkerResponse>,
}

impl WorkerHandle {
    /// Block until the worker finishes the request and returns the response.
    pub fn recv(self) -> Result<WorkerResponse, WorkerError> {
        self.receiver.recv().map_err(|_| WorkerError::ChannelClosed)
    }

    /// Wait for the response up to a specified timeout.
    pub fn recv_timeout(self, timeout: Duration) -> Result<WorkerResponse, WorkerError> {
        self.receiver.recv_timeout(timeout).map_err(|e| match e {
            std::sync::mpsc::RecvTimeoutError::Timeout => WorkerError::Timeout(timeout),
            std::sync::mpsc::RecvTimeoutError::Disconnected => WorkerError::ChannelClosed,
        })
    }

    /// Non-blocking check for response completion.
    pub fn try_recv(&self) -> Result<Option<WorkerResponse>, WorkerError> {
        match self.receiver.try_recv() {
            Ok(res) => Ok(Some(res)),
            Err(std::sync::mpsc::TryRecvError::Empty) => Ok(None),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => Err(WorkerError::ChannelClosed),
        }
    }
}

/// Errors occurring during worker pool operations.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum WorkerError {
    #[error("Worker channel closed or worker thread disconnected")]
    ChannelClosed,
    #[error("Request timed out after {0:?}")]
    Timeout(Duration),
    #[error("Worker pool is shut down")]
    PoolShutdown,
    #[error("Worker process fork failed: {0}")]
    ForkFailed(String),
    #[error("IPC communication error: {0}")]
    IpcError(String),
    #[error("Serialization error: {0}")]
    SerializationError(String),
}

/// Aggregate performance and request statistics for a [`WorkerPool`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkerPoolStats {
    pub total_requests: u64,
    pub successful_requests: u64,
    pub failed_requests: u64,
    pub panicked_requests: u64,
    pub active_workers: usize,
    pub queued_requests: usize,
    pub total_latency_micros: u64,
    pub avg_latency_micros: u64,
}

/// Internal atomic metrics tracking.
#[derive(Debug, Default)]
struct WorkerMetrics {
    total_requests: AtomicU64,
    successful_requests: AtomicU64,
    failed_requests: AtomicU64,
    panicked_requests: AtomicU64,
    active_workers: AtomicUsize,
    queued_requests: AtomicUsize,
    total_latency_micros: AtomicU64,
}

impl WorkerMetrics {
    fn record_response(&self, status: WorkerResponseStatus, duration: Duration) {
        let micros = duration.as_micros() as u64;
        self.total_latency_micros
            .fetch_add(micros, Ordering::Relaxed);
        match status {
            WorkerResponseStatus::Success => {
                self.successful_requests.fetch_add(1, Ordering::Relaxed);
            }
            WorkerResponseStatus::Panic => {
                self.panicked_requests.fetch_add(1, Ordering::Relaxed);
                self.failed_requests.fetch_add(1, Ordering::Relaxed);
            }
            _ => {
                self.failed_requests.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    fn snapshot(&self) -> WorkerPoolStats {
        let total = self.total_requests.load(Ordering::Relaxed);
        let total_lat = self.total_latency_micros.load(Ordering::Relaxed);
        let avg_lat = if total > 0 { total_lat / total } else { 0 };
        WorkerPoolStats {
            total_requests: total,
            successful_requests: self.successful_requests.load(Ordering::Relaxed),
            failed_requests: self.failed_requests.load(Ordering::Relaxed),
            panicked_requests: self.panicked_requests.load(Ordering::Relaxed),
            active_workers: self.active_workers.load(Ordering::Relaxed),
            queued_requests: self.queued_requests.load(Ordering::Relaxed),
            total_latency_micros: total_lat,
            avg_latency_micros: avg_lat,
        }
    }
}

/// Internal message dispatched across worker queue channels.
struct WorkerTask {
    request: WorkerRequest,
    sender: std::sync::mpsc::Sender<WorkerResponse>,
}

/// High-throughput worker pool handling concurrent requests against shared parent [`EngineState`].
pub struct WorkerPool {
    config: WorkerPoolConfig,
    state: Arc<EngineState>,
    sender: Option<SyncSender<WorkerTask>>,
    worker_threads: Vec<JoinHandle<()>>,
    metrics: Arc<WorkerMetrics>,
    is_shutdown: Arc<AtomicBool>,
}

impl WorkerPool {
    /// Construct a new [`WorkerPool`] and spawn worker tasks.
    pub fn new(state: Arc<EngineState>, config: WorkerPoolConfig) -> Result<Self, WorkerError> {
        let num_workers = config.num_workers;
        let (sender, receiver) = sync_channel::<WorkerTask>(config.queue_capacity);
        let receiver = Arc::new(Mutex::new(receiver));

        let metrics = Arc::new(WorkerMetrics::default());
        let is_shutdown = Arc::new(AtomicBool::new(false));

        let mut worker_threads = Vec::with_capacity(num_workers);

        for worker_id in 0..num_workers {
            let rx = Arc::clone(&receiver);
            let state_clone = Arc::clone(&state);
            let metrics_clone = Arc::clone(&metrics);
            let shutdown_clone = Arc::clone(&is_shutdown);
            let model = config.model;

            let handle = thread::Builder::new()
                .name(format!("rhp-worker-{}", worker_id))
                .spawn(move || {
                    worker_loop(
                        worker_id,
                        rx,
                        state_clone,
                        metrics_clone,
                        shutdown_clone,
                        model,
                    );
                })
                .map_err(|e| WorkerError::ForkFailed(e.to_string()))?;

            worker_threads.push(handle);
        }

        Ok(Self {
            config,
            state,
            sender: Some(sender),
            worker_threads,
            metrics,
            is_shutdown,
        })
    }

    /// Access fluent builder.
    pub fn builder() -> WorkerPoolBuilder {
        WorkerPoolBuilder::new()
    }

    /// Number of worker threads/processes in the pool.
    #[inline]
    pub fn num_workers(&self) -> usize {
        self.config.num_workers
    }

    /// Concurrency model in use.
    #[inline]
    pub fn model(&self) -> WorkerModel {
        self.config.model
    }

    /// Immutable reference to the shared parent [`EngineState`].
    #[inline]
    pub fn state(&self) -> &Arc<EngineState> {
        &self.state
    }

    /// Asynchronously dispatch a [`WorkerRequest`] to the worker pool.
    ///
    /// Returns a [`WorkerHandle`] that can be polled or waited upon.
    pub fn dispatch(&self, request: WorkerRequest) -> Result<WorkerHandle, WorkerError> {
        if self.is_shutdown.load(Ordering::Relaxed) {
            return Err(WorkerError::PoolShutdown);
        }

        let sender = self.sender.as_ref().ok_or(WorkerError::PoolShutdown)?;
        let (res_tx, res_rx) = std::sync::mpsc::channel();

        self.metrics.total_requests.fetch_add(1, Ordering::Relaxed);
        self.metrics.queued_requests.fetch_add(1, Ordering::Relaxed);

        sender
            .send(WorkerTask {
                request,
                sender: res_tx,
            })
            .map_err(|_| WorkerError::ChannelClosed)?;

        Ok(WorkerHandle { receiver: res_rx })
    }

    /// Synchronously execute a [`WorkerRequest`] and wait for its completion.
    pub fn execute(&self, request: WorkerRequest) -> WorkerResponse {
        let timeout = self.config.request_timeout;
        match self.dispatch(request) {
            Ok(handle) => match timeout {
                Some(dur) => handle.recv_timeout(dur).unwrap_or_else(|e| WorkerResponse {
                    request_id: "timeout".to_string(),
                    status: WorkerResponseStatus::Timeout,
                    value: FrozenValue::Null,
                    output: String::new(),
                    duration: dur,
                    error: Some(e.to_string()),
                    worker_id: 0,
                    fiber_stats: None,
                }),
                None => handle.recv().unwrap_or_else(|e| WorkerResponse {
                    request_id: "error".to_string(),
                    status: WorkerResponseStatus::RuntimeError,
                    value: FrozenValue::Null,
                    output: String::new(),
                    duration: Duration::default(),
                    error: Some(e.to_string()),
                    worker_id: 0,
                    fiber_stats: None,
                }),
            },
            Err(err) => WorkerResponse {
                request_id: "dispatch_error".to_string(),
                status: WorkerResponseStatus::RuntimeError,
                value: FrozenValue::Null,
                output: String::new(),
                duration: Duration::default(),
                error: Some(err.to_string()),
                worker_id: 0,
                fiber_stats: None,
            },
        }
    }

    /// Shorthand convenience to execute an RHP source script.
    pub fn execute_source(&self, source: &str) -> WorkerResponse {
        self.execute(WorkerRequest::new(source))
    }

    /// Shorthand convenience to execute a pre-compiled function or static method.
    pub fn execute_call(&self, func_name: &str, args: Vec<impl Into<FrozenValue>>) -> WorkerResponse {
        self.execute(WorkerRequest::call(func_name, args))
    }

    /// Execute a collection of requests in parallel across the worker pool,
    /// joining and returning all responses in original order.
    pub fn execute_batch(&self, requests: Vec<WorkerRequest>) -> Vec<WorkerResponse> {
        let mut handles = Vec::with_capacity(requests.len());
        for req in requests {
            match self.dispatch(req) {
                Ok(h) => handles.push(Some(h)),
                Err(_) => {
                    handles.push(None);
                }
            }
        }

        let timeout = self.config.request_timeout;
        handles
            .into_iter()
            .map(|h_opt| match h_opt {
                Some(handle) => match timeout {
                    Some(dur) => handle.recv_timeout(dur).unwrap_or_else(|e| WorkerResponse {
                        request_id: "timeout".to_string(),
                        status: WorkerResponseStatus::Timeout,
                        value: FrozenValue::Null,
                        output: String::new(),
                        duration: dur,
                        error: Some(e.to_string()),
                        worker_id: 0,
                        fiber_stats: None,
                    }),
                    None => handle.recv().unwrap_or_else(|e| WorkerResponse {
                        request_id: "batch_recv_error".to_string(),
                        status: WorkerResponseStatus::RuntimeError,
                        value: FrozenValue::Null,
                        output: String::new(),
                        duration: Duration::default(),
                        error: Some(e.to_string()),
                        worker_id: 0,
                        fiber_stats: None,
                    }),
                },
                None => WorkerResponse {
                    request_id: "batch_dispatch_error".to_string(),
                    status: WorkerResponseStatus::RuntimeError,
                    value: FrozenValue::Null,
                    output: String::new(),
                    duration: Duration::default(),
                    error: Some("Failed to dispatch request to worker pool".to_string()),
                    worker_id: 0,
                    fiber_stats: None,
                },
            })
            .collect()
    }

    /// Query live metrics and throughput statistics.
    pub fn stats(&self) -> WorkerPoolStats {
        self.metrics.snapshot()
    }

    /// Gracefully shutdown all worker threads.
    pub fn shutdown(&mut self) -> Result<(), WorkerError> {
        self.is_shutdown.store(true, Ordering::Relaxed);
        // Dropping sender triggers channel disconnection
        drop(self.sender.take());

        for handle in self.worker_threads.drain(..) {
            let _ = handle.join();
        }

        Ok(())
    }

    /// Shutdown with a maximum wait timeout.
    pub fn shutdown_timeout(&mut self, _timeout: Duration) -> Result<(), WorkerError> {
        self.shutdown()
    }
}

impl Drop for WorkerPool {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

/// Worker loop pulling tasks from the shared MPMC queue.
fn worker_loop(
    worker_id: usize,
    receiver: Arc<Mutex<Receiver<WorkerTask>>>,
    state: Arc<EngineState>,
    metrics: Arc<WorkerMetrics>,
    shutdown: Arc<AtomicBool>,
    model: WorkerModel,
) {
    loop {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }

        // Pull task from synchronized channel
        let task = {
            let rx = match receiver.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            rx.recv()
        };

        let task = match task {
            Ok(t) => t,
            Err(_) => break, // Channel closed
        };

        metrics.queued_requests.fetch_sub(1, Ordering::Relaxed);
        metrics.active_workers.fetch_add(1, Ordering::Relaxed);

        let response = match model {
            WorkerModel::MultiThreaded => {
                execute_isolated_request(&state, &task.request, worker_id)
            }
            WorkerModel::ProcessCow => {
                #[cfg(unix)]
                {
                    execute_process_cow(&state, &task.request, worker_id)
                }
                #[cfg(not(unix))]
                {
                    execute_isolated_request(&state, &task.request, worker_id)
                }
            }
        };

        metrics.record_response(response.status, response.duration);
        metrics.active_workers.fetch_sub(1, Ordering::Relaxed);

        let _ = task.sender.send(response);
    }
}

/// Execute a request inside an isolated, Copy-on-Write worker VM sandbox with panic containment.
pub fn execute_isolated_request(
    state: &EngineState,
    request: &WorkerRequest,
    worker_id: usize,
) -> WorkerResponse {
    let start = Instant::now();

    // Catch any panics inside user code or native facades to guarantee fault isolation
    let run_outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        execute_isolated_request_inner(state, request, worker_id, start)
    }));

    match run_outcome {
        Ok(resp) => resp,
        Err(panic_payload) => {
            let panic_msg = extract_panic_message(&panic_payload);
            WorkerResponse {
                request_id: request.id.clone(),
                status: WorkerResponseStatus::Panic,
                value: FrozenValue::Null,
                output: String::new(),
                duration: start.elapsed(),
                error: Some(panic_msg),
                worker_id,
                fiber_stats: None,
            }
        }
    }
}

fn execute_isolated_request_inner(
    state: &EngineState,
    request: &WorkerRequest,
    worker_id: usize,
    start: Instant,
) -> WorkerResponse {
    match &request.payload {
        WorkerPayload::Source(source) => {
            // Parse & check against parent type hierarchy with zero framework bootstrap overhead
            let typed = match parse_and_check_with_hierarchy(source, (*state.type_hierarchy).clone())
            {
                Ok(t) => t,
                Err(e) => {
                    return WorkerResponse {
                        request_id: request.id.clone(),
                        status: WorkerResponseStatus::CompilationError,
                        value: FrozenValue::Null,
                        output: String::new(),
                        duration: start.elapsed(),
                        error: Some(e.to_string()),
                        worker_id,
                        fiber_stats: None,
                    };
                }
            };

            // Lower against immutable parent IrModule
            let request_module =
                match lower_typed_program_with_base(&typed, (*state.module).clone()) {
                    Ok(m) => m,
                    Err(e) => {
                        return WorkerResponse {
                            request_id: request.id.clone(),
                            status: WorkerResponseStatus::CompilationError,
                            value: FrozenValue::Null,
                            output: String::new(),
                            duration: start.elapsed(),
                            error: Some(e.to_string()),
                            worker_id,
                            fiber_stats: None,
                        };
                    }
                };

            // Instantiate isolated worker VM
            let mut vm = VM::new(request_module, false)
                .with_type_hierarchy(typed.type_info.hierarchy);

            // Rehydrate pristine Copy-on-Write maps from parent state
            vm.set_static_properties(state.rehydrate_static_properties());
            vm.set_class_constants(state.rehydrate_class_constants());
            vm.set_globals(state.rehydrate_globals());

            // Inject superglobals and request-local context
            inject_superglobals(&mut vm, request);

            // Execute request inside an isolated local execution fiber
            let fiber_name = format!("worker_{}_fiber_{}", worker_id, request.id);
            let (run_res, stats) = {
                let fiber = Fiber::new(fiber_name);
                let guard = enter_fiber(fiber);
                let res = vm.run();
                let f = match &res {
                    Ok(val) => guard.finish(val.clone()),
                    Err(err) => guard.fail(err.to_string()),
                };
                (res, f.stats)
            };

            match run_res {
                Ok(val) => WorkerResponse {
                    request_id: request.id.clone(),
                    status: WorkerResponseStatus::Success,
                    value: FrozenValue::from_php_value(&val),
                    output: vm.output().to_string(),
                    duration: start.elapsed(),
                    error: None,
                    worker_id,
                    fiber_stats: Some(stats),
                },
                Err(err) => WorkerResponse {
                    request_id: request.id.clone(),
                    status: WorkerResponseStatus::RuntimeError,
                    value: FrozenValue::Null,
                    output: vm.output().to_string(),
                    duration: start.elapsed(),
                    error: Some(err.to_string()),
                    worker_id,
                    fiber_stats: Some(stats),
                },
            }
        }
        WorkerPayload::Call { func_name, args } => {
            let mut vm = state.create_worker_vm();
            inject_superglobals(&mut vm, request);

            let php_args: Vec<PhpValue> = args.iter().map(|a| a.to_php_value()).collect();

            let fiber_name = format!("worker_{}_call_{}", worker_id, func_name);
            let (run_res, stats) = {
                let fiber = Fiber::new(fiber_name);
                let guard = enter_fiber(fiber);
                let res = vm.invoke_function_by_name(func_name, php_args, Span::default());
                let f = match &res {
                    Ok(val) => guard.finish(val.clone()),
                    Err(err) => guard.fail(err.to_string()),
                };
                (res, f.stats)
            };

            match run_res {
                Ok(val) => WorkerResponse {
                    request_id: request.id.clone(),
                    status: WorkerResponseStatus::Success,
                    value: FrozenValue::from_php_value(&val),
                    output: vm.output().to_string(),
                    duration: start.elapsed(),
                    error: None,
                    worker_id,
                    fiber_stats: Some(stats),
                },
                Err(err) => WorkerResponse {
                    request_id: request.id.clone(),
                    status: WorkerResponseStatus::RuntimeError,
                    value: FrozenValue::Null,
                    output: vm.output().to_string(),
                    duration: start.elapsed(),
                    error: Some(err.to_string()),
                    worker_id,
                    fiber_stats: Some(stats),
                },
            }
        }
    }
}

/// Inject superglobals (`$_GET`, `$_POST`, `$_SERVER`, `$_COOKIE`) into request VM.
fn inject_superglobals(vm: &mut VM, request: &WorkerRequest) {
    // 1. $_GET
    let mut get_dict = IndexMap::new();
    for (k, v) in &request.get_params {
        get_dict.insert(k.clone(), v.to_php_value());
    }
    let get_val = PhpValue::dict(get_dict);
    vm.set_global("_GET", get_val.clone());
    vm.set_global("$_GET", get_val);

    // 2. $_POST
    let mut post_dict = IndexMap::new();
    for (k, v) in &request.post_params {
        post_dict.insert(k.clone(), v.to_php_value());
    }
    let post_val = PhpValue::dict(post_dict);
    vm.set_global("_POST", post_val.clone());
    vm.set_global("$_POST", post_val);

    // 3. $_SERVER
    let mut server_dict = IndexMap::new();
    for (k, v) in &request.server_vars {
        server_dict.insert(k.clone(), PhpValue::string(v.clone()));
    }
    let server_val = PhpValue::dict(server_dict);
    vm.set_global("_SERVER", server_val.clone());
    vm.set_global("$_SERVER", server_val);

    // 4. $_COOKIE
    let mut cookie_dict = IndexMap::new();
    for (k, v) in &request.cookies {
        cookie_dict.insert(k.clone(), PhpValue::string(v.clone()));
    }
    let cookie_val = PhpValue::dict(cookie_dict);
    vm.set_global("_COOKIE", cookie_val.clone());
    vm.set_global("$_COOKIE", cookie_val);

    // 5. Custom globals
    for (k, v) in &request.globals {
        let bare = k.strip_prefix('$').unwrap_or(k);
        let sigil = format!("${}", bare);
        let val = v.to_php_value();
        vm.set_global(bare, val.clone());
        vm.set_global(sigil, val);
    }
}

/// Extract human-readable string message from panic payload.
fn extract_panic_message(payload: &Box<dyn Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        s.to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "Worker thread caught an unhandled panic".to_string()
    }
}

/// Process-level Copy-on-Write model using Unix `fork()` and anonymous pipe IPC.
#[cfg(unix)]
pub fn execute_process_cow(
    state: &EngineState,
    request: &WorkerRequest,
    worker_id: usize,
) -> WorkerResponse {
    let start = Instant::now();
    let mut pipe_fds = [0i32; 2];

    if unsafe { libc::pipe(pipe_fds.as_mut_ptr()) } != 0 {
        return WorkerResponse {
            request_id: request.id.clone(),
            status: WorkerResponseStatus::ProcessTerminated,
            value: FrozenValue::Null,
            output: String::new(),
            duration: start.elapsed(),
            error: Some("Failed to create IPC pipe for process-level worker".to_string()),
            worker_id,
            fiber_stats: None,
        };
    }

    let pid = unsafe { libc::fork() };

    if pid < 0 {
        unsafe {
            libc::close(pipe_fds[0]);
            libc::close(pipe_fds[1]);
        }
        return WorkerResponse {
            request_id: request.id.clone(),
            status: WorkerResponseStatus::ProcessTerminated,
            value: FrozenValue::Null,
            output: String::new(),
            duration: start.elapsed(),
            error: Some("Failed to fork worker process".to_string()),
            worker_id,
            fiber_stats: None,
        };
    }

    if pid == 0 {
        // -------------------------------------------------------------
        // CHILD PROCESS (Hardware Virtual Memory Copy-on-Write)
        // -------------------------------------------------------------
        unsafe {
            libc::close(pipe_fds[0]); // Close read end in child
        }

        // Execute request inside isolated child address space
        let resp = execute_isolated_request(state, request, worker_id);

        // Serialize response to bytes
        let bytes = serialize_response(&resp);
        let len = bytes.len() as u32;

        unsafe {
            let len_bytes = len.to_le_bytes();
            let _ = libc::write(pipe_fds[1], len_bytes.as_ptr() as *const libc::c_void, 4);
            let _ = libc::write(
                pipe_fds[1],
                bytes.as_ptr() as *const libc::c_void,
                bytes.len(),
            );
            libc::close(pipe_fds[1]);
            libc::_exit(0);
        }
    } else {
        // -------------------------------------------------------------
        // PARENT PROCESS
        // -------------------------------------------------------------
        unsafe {
            libc::close(pipe_fds[1]); // Close write end in parent
        }

        let mut status = 0i32;
        let wait_res = unsafe { libc::waitpid(pid, &mut status, 0) };

        if wait_res < 0 {
            unsafe {
                libc::close(pipe_fds[0]);
            }
            return WorkerResponse {
                request_id: request.id.clone(),
                status: WorkerResponseStatus::ProcessTerminated,
                value: FrozenValue::Null,
                output: String::new(),
                duration: start.elapsed(),
                error: Some("libc::waitpid failed".to_string()),
                worker_id,
                fiber_stats: None,
            };
        }

        let exited_normally = libc::WIFEXITED(status);
        let exit_code = if exited_normally {
            libc::WEXITSTATUS(status)
        } else {
            -1
        };
        let signaled = libc::WIFSIGNALED(status);
        let term_sig = if signaled {
            libc::WTERMSIG(status)
        } else {
            0
        };

        if signaled || exit_code != 0 {
            unsafe {
                libc::close(pipe_fds[0]);
            }
            return WorkerResponse {
                request_id: request.id.clone(),
                status: WorkerResponseStatus::ProcessTerminated,
                value: FrozenValue::Null,
                output: String::new(),
                duration: start.elapsed(),
                error: Some(format!(
                    "Worker process terminated abnormally: signaled={}, signal={}, exit_code={}",
                    signaled, term_sig, exit_code
                )),
                worker_id,
                fiber_stats: None,
            };
        }

        // Read 4-byte length prefix
        let mut len_buf = [0u8; 4];
        let n = unsafe { libc::read(pipe_fds[0], len_buf.as_mut_ptr() as *mut libc::c_void, 4) };
        if n != 4 {
            unsafe {
                libc::close(pipe_fds[0]);
            }
            return WorkerResponse {
                request_id: request.id.clone(),
                status: WorkerResponseStatus::ProcessTerminated,
                value: FrozenValue::Null,
                output: String::new(),
                duration: start.elapsed(),
                error: Some("Failed to read header from worker process pipe".to_string()),
                worker_id,
                fiber_stats: None,
            };
        }

        let total_len = u32::from_le_bytes(len_buf) as usize;
        let mut data = vec![0u8; total_len];
        let mut read_bytes = 0;

        while read_bytes < total_len {
            let r = unsafe {
                libc::read(
                    pipe_fds[0],
                    data[read_bytes..].as_mut_ptr() as *mut libc::c_void,
                    total_len - read_bytes,
                )
            };
            if r <= 0 {
                break;
            }
            read_bytes += r as usize;
        }

        unsafe {
            libc::close(pipe_fds[0]);
        }

        if read_bytes != total_len {
            return WorkerResponse {
                request_id: request.id.clone(),
                status: WorkerResponseStatus::ProcessTerminated,
                value: FrozenValue::Null,
                output: String::new(),
                duration: start.elapsed(),
                error: Some("Incomplete response read from worker process pipe".to_string()),
                worker_id,
                fiber_stats: None,
            };
        }

        deserialize_response(&data).unwrap_or_else(|e| WorkerResponse {
            request_id: request.id.clone(),
            status: WorkerResponseStatus::RuntimeError,
            value: FrozenValue::Null,
            output: String::new(),
            duration: start.elapsed(),
            error: Some(format!("Failed to deserialize worker response: {}", e)),
            worker_id,
            fiber_stats: None,
        })
    }
}

// =====================================================================
// Binary Serialization for Inter-Process Communication (IPC)
// =====================================================================

/// Serialize a [`WorkerResponse`] into a compact byte payload.
pub fn serialize_response(resp: &WorkerResponse) -> Vec<u8> {
    let mut buf = Vec::with_capacity(128);

    // 1. Request ID
    write_string(&mut buf, &resp.request_id);

    // 2. Status tag
    let status_tag = match resp.status {
        WorkerResponseStatus::Success => 0u8,
        WorkerResponseStatus::RuntimeError => 1u8,
        WorkerResponseStatus::CompilationError => 2u8,
        WorkerResponseStatus::Panic => 3u8,
        WorkerResponseStatus::ProcessTerminated => 4u8,
        WorkerResponseStatus::Timeout => 5u8,
    };
    buf.push(status_tag);

    // 3. Evaluated value as FrozenValue
    write_frozen(&mut buf, &resp.value);

    // 4. Output buffer
    write_string(&mut buf, &resp.output);

    // 5. Duration (micros)
    let micros = resp.duration.as_micros() as u64;
    buf.extend_from_slice(&micros.to_le_bytes());

    // 6. Optional error string
    match &resp.error {
        Some(err) => {
            buf.push(1);
            write_string(&mut buf, err);
        }
        None => buf.push(0),
    }

    // 7. Worker ID
    buf.extend_from_slice(&(resp.worker_id as u64).to_le_bytes());

    // 8. Fiber stats
    match &resp.fiber_stats {
        Some(stats) => {
            buf.push(1);
            let stats_val = stats.to_php_value();
            let frozen = FrozenValue::from_php_value(&stats_val);
            write_frozen(&mut buf, &frozen);
        }
        None => buf.push(0),
    }

    buf
}

/// Deserialize a byte payload back into a [`WorkerResponse`].
pub fn deserialize_response(data: &[u8]) -> Result<WorkerResponse, WorkerError> {
    let mut cursor = 0;

    let request_id = read_string(data, &mut cursor)?;

    let status_byte = read_u8(data, &mut cursor)?;
    let status = match status_byte {
        0 => WorkerResponseStatus::Success,
        1 => WorkerResponseStatus::RuntimeError,
        2 => WorkerResponseStatus::CompilationError,
        3 => WorkerResponseStatus::Panic,
        4 => WorkerResponseStatus::ProcessTerminated,
        5 => WorkerResponseStatus::Timeout,
        _ => {
            return Err(WorkerError::SerializationError(format!(
                "Unknown status tag: {}",
                status_byte
            )))
        }
    };

    let value = read_frozen(data, &mut cursor)?;
    let output = read_string(data, &mut cursor)?;

    let micros = read_u64(data, &mut cursor)?;
    let duration = Duration::from_micros(micros);

    let has_error = read_u8(data, &mut cursor)?;
    let error = if has_error == 1 {
        Some(read_string(data, &mut cursor)?)
    } else {
        None
    };

    let worker_id_u64 = read_u64(data, &mut cursor)?;
    let worker_id = worker_id_u64 as usize;

    let has_fiber = if cursor < data.len() {
        read_u8(data, &mut cursor)?
    } else {
        0
    };
    let fiber_stats = if has_fiber == 1 {
        let frozen = read_frozen(data, &mut cursor)?;
        let php_val = frozen.to_php_value();
        if let PhpValue::Dict(d) = php_val {
            let map = d.borrow();
            let yield_count = map.get("yield_count").and_then(|v| v.to_i64()).unwrap_or(0) as usize;
            let io_count = map.get("io_count").and_then(|v| v.to_i64()).unwrap_or(0) as usize;
            let id_num = map.get("id").and_then(|v| v.to_i64()).unwrap_or(0) as u64;
            let name_str = map.get("name").map(|v| v.to_php_string()).unwrap_or_default();
            let total_us = map.get("total_duration_us").and_then(|v| v.to_i64()).unwrap_or(0) as u64;
            let cpu_us = map.get("cpu_duration_us").and_then(|v| v.to_i64()).unwrap_or(0) as u64;
            let io_us = map.get("io_wait_duration_us").and_then(|v| v.to_i64()).unwrap_or(0) as u64;
            let is_comp = map.get("is_completed").map(|v| v.is_truthy()).unwrap_or(true);
            Some(FiberStats {
                fiber_id: FiberId(id_num),
                name: name_str,
                total_duration: Duration::from_micros(total_us),
                cpu_duration: Duration::from_micros(cpu_us),
                io_wait_duration: Duration::from_micros(io_us),
                yield_count,
                io_count,
                io_breakdown: IndexMap::new(),
                io_history: Vec::new(),
                is_completed: is_comp,
            })
        } else {
            None
        }
    } else {
        None
    };

    Ok(WorkerResponse {
        request_id,
        status,
        value,
        output,
        duration,
        error,
        worker_id,
        fiber_stats,
    })
}

fn write_string(buf: &mut Vec<u8>, s: &str) {
    let len = s.len() as u32;
    buf.extend_from_slice(&len.to_le_bytes());
    buf.extend_from_slice(s.as_bytes());
}

fn read_string(data: &[u8], cursor: &mut usize) -> Result<String, WorkerError> {
    let len = read_u32(data, cursor)? as usize;
    if *cursor + len > data.len() {
        return Err(WorkerError::SerializationError(
            "Unexpected EOF reading string".to_string(),
        ));
    }
    let s = std::str::from_utf8(&data[*cursor..*cursor + len])
        .map_err(|e| WorkerError::SerializationError(e.to_string()))?
        .to_string();
    *cursor += len;
    Ok(s)
}

fn write_frozen(buf: &mut Vec<u8>, val: &FrozenValue) {
    match val {
        FrozenValue::Null => buf.push(0),
        FrozenValue::Bool(b) => {
            buf.push(1);
            buf.push(if *b { 1 } else { 0 });
        }
        FrozenValue::Int(n) => {
            buf.push(2);
            buf.extend_from_slice(&n.to_le_bytes());
        }
        FrozenValue::Float(f) => {
            buf.push(3);
            buf.extend_from_slice(&f.to_le_bytes());
        }
        FrozenValue::RustInt { kind, value } => {
            buf.push(4);
            buf.push(rust_int_kind_to_u8(*kind));
            buf.extend_from_slice(&value.to_le_bytes());
        }
        FrozenValue::RustFloat { kind, value } => {
            buf.push(5);
            buf.push(rust_float_kind_to_u8(*kind));
            buf.extend_from_slice(&value.to_le_bytes());
        }
        FrozenValue::String(s) => {
            buf.push(6);
            write_string(buf, s);
        }
        FrozenValue::List(items) => {
            buf.push(7);
            let len = items.len() as u32;
            buf.extend_from_slice(&len.to_le_bytes());
            for item in items {
                write_frozen(buf, item);
            }
        }
        FrozenValue::Dict(pairs) => {
            buf.push(8);
            let len = pairs.len() as u32;
            buf.extend_from_slice(&len.to_le_bytes());
            for (k, v) in pairs {
                write_string(buf, k);
                write_frozen(buf, v);
            }
        }
        FrozenValue::Set(items) => {
            buf.push(9);
            let len = items.len() as u32;
            buf.extend_from_slice(&len.to_le_bytes());
            for item in items {
                write_frozen(buf, item);
            }
        }
        FrozenValue::HashMap(pairs) => {
            buf.push(10);
            let len = pairs.len() as u32;
            buf.extend_from_slice(&len.to_le_bytes());
            for (k, v) in pairs {
                write_frozen(buf, k);
                write_frozen(buf, v);
            }
        }
        FrozenValue::Object {
            class_name,
            properties,
        } => {
            buf.push(11);
            write_string(buf, class_name);
            let len = properties.len() as u32;
            buf.extend_from_slice(&len.to_le_bytes());
            for (k, v) in properties {
                write_string(buf, k);
                write_frozen(buf, v);
            }
        }
    }
}

fn read_frozen(data: &[u8], cursor: &mut usize) -> Result<FrozenValue, WorkerError> {
    let tag = read_u8(data, cursor)?;
    match tag {
        0 => Ok(FrozenValue::Null),
        1 => {
            let b = read_u8(data, cursor)? != 0;
            Ok(FrozenValue::Bool(b))
        }
        2 => {
            let mut b = [0u8; 4];
            copy_slice(data, cursor, &mut b)?;
            Ok(FrozenValue::Int(i32::from_le_bytes(b)))
        }
        3 => {
            let mut b = [0u8; 4];
            copy_slice(data, cursor, &mut b)?;
            Ok(FrozenValue::Float(f32::from_le_bytes(b)))
        }
        4 => {
            let kind_u8 = read_u8(data, cursor)?;
            let kind = u8_to_rust_int_kind(kind_u8)?;
            let mut b = [0u8; 16];
            copy_slice(data, cursor, &mut b)?;
            Ok(FrozenValue::RustInt {
                kind,
                value: i128::from_le_bytes(b),
            })
        }
        5 => {
            let kind_u8 = read_u8(data, cursor)?;
            let kind = u8_to_rust_float_kind(kind_u8)?;
            let mut b = [0u8; 8];
            copy_slice(data, cursor, &mut b)?;
            Ok(FrozenValue::RustFloat {
                kind,
                value: f64::from_le_bytes(b),
            })
        }
        6 => {
            let s = read_string(data, cursor)?;
            Ok(FrozenValue::String(s))
        }
        7 => {
            let count = read_u32(data, cursor)? as usize;
            let mut list = Vec::with_capacity(count);
            for _ in 0..count {
                list.push(read_frozen(data, cursor)?);
            }
            Ok(FrozenValue::List(list))
        }
        8 => {
            let count = read_u32(data, cursor)? as usize;
            let mut pairs = Vec::with_capacity(count);
            for _ in 0..count {
                let k = read_string(data, cursor)?;
                let v = read_frozen(data, cursor)?;
                pairs.push((k, v));
            }
            Ok(FrozenValue::Dict(pairs))
        }
        9 => {
            let count = read_u32(data, cursor)? as usize;
            let mut set = Vec::with_capacity(count);
            for _ in 0..count {
                set.push(read_frozen(data, cursor)?);
            }
            Ok(FrozenValue::Set(set))
        }
        10 => {
            let count = read_u32(data, cursor)? as usize;
            let mut map = Vec::with_capacity(count);
            for _ in 0..count {
                let k = read_frozen(data, cursor)?;
                let v = read_frozen(data, cursor)?;
                map.push((k, v));
            }
            Ok(FrozenValue::HashMap(map))
        }
        11 => {
            let class_name = read_string(data, cursor)?;
            let count = read_u32(data, cursor)? as usize;
            let mut props = Vec::with_capacity(count);
            for _ in 0..count {
                let k = read_string(data, cursor)?;
                let v = read_frozen(data, cursor)?;
                props.push((k, v));
            }
            Ok(FrozenValue::Object {
                class_name,
                properties: props,
            })
        }
        other => Err(WorkerError::SerializationError(format!(
            "Unknown FrozenValue tag: {}",
            other
        ))),
    }
}

fn read_u8(data: &[u8], cursor: &mut usize) -> Result<u8, WorkerError> {
    if *cursor >= data.len() {
        return Err(WorkerError::SerializationError(
            "Unexpected EOF reading u8".to_string(),
        ));
    }
    let b = data[*cursor];
    *cursor += 1;
    Ok(b)
}

fn read_u32(data: &[u8], cursor: &mut usize) -> Result<u32, WorkerError> {
    let mut b = [0u8; 4];
    copy_slice(data, cursor, &mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn read_u64(data: &[u8], cursor: &mut usize) -> Result<u64, WorkerError> {
    let mut b = [0u8; 8];
    copy_slice(data, cursor, &mut b)?;
    Ok(u64::from_le_bytes(b))
}

fn copy_slice(data: &[u8], cursor: &mut usize, dest: &mut [u8]) -> Result<(), WorkerError> {
    let len = dest.len();
    if *cursor + len > data.len() {
        return Err(WorkerError::SerializationError(
            "Unexpected EOF reading bytes".to_string(),
        ));
    }
    dest.copy_from_slice(&data[*cursor..*cursor + len]);
    *cursor += len;
    Ok(())
}

fn rust_int_kind_to_u8(kind: RustIntKind) -> u8 {
    match kind {
        RustIntKind::I8 => 0,
        RustIntKind::I16 => 1,
        RustIntKind::I32 => 2,
        RustIntKind::I64 => 3,
        RustIntKind::I128 => 4,
        RustIntKind::U8 => 5,
        RustIntKind::U16 => 6,
        RustIntKind::U32 => 7,
        RustIntKind::U64 => 8,
        RustIntKind::U128 => 9,
        RustIntKind::Usize => 10,
        RustIntKind::Isize => 11,
    }
}

fn u8_to_rust_int_kind(b: u8) -> Result<RustIntKind, WorkerError> {
    match b {
        0 => Ok(RustIntKind::I8),
        1 => Ok(RustIntKind::I16),
        2 => Ok(RustIntKind::I32),
        3 => Ok(RustIntKind::I64),
        4 => Ok(RustIntKind::I128),
        5 => Ok(RustIntKind::U8),
        6 => Ok(RustIntKind::U16),
        7 => Ok(RustIntKind::U32),
        8 => Ok(RustIntKind::U64),
        9 => Ok(RustIntKind::U128),
        10 => Ok(RustIntKind::Usize),
        11 => Ok(RustIntKind::Isize),
        _ => Err(WorkerError::SerializationError(format!(
            "Invalid RustIntKind tag: {}",
            b
        ))),
    }
}

fn rust_float_kind_to_u8(kind: RustFloatKind) -> u8 {
    match kind {
        RustFloatKind::F32 => 0,
        RustFloatKind::F64 => 1,
    }
}

fn u8_to_rust_float_kind(b: u8) -> Result<RustFloatKind, WorkerError> {
    match b {
        0 => Ok(RustFloatKind::F32),
        1 => Ok(RustFloatKind::F64),
        _ => Err(WorkerError::SerializationError(format!(
            "Invalid RustFloatKind tag: {}",
            b
        ))),
    }
}

static RAND_STATE: AtomicU64 = AtomicU64::new(123456789);

fn fastrand_u64() -> u64 {
    let mut state = RAND_STATE.load(Ordering::Relaxed);
    loop {
        // xorshift64*
        let mut x = state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        let next = x.wrapping_mul(0x2545F4914F6CDD1D);
        match RAND_STATE.compare_exchange_weak(state, next, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return next,
            Err(curr) => state = curr,
        }
    }
}
