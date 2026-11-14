//! # RHPM Worker Process - Coroutine Event Loop
//!
//! Each Worker process runs a **single-threaded Tokio `current_thread` runtime**
//! hosting a `LocalSet` task queue. Every incoming HTTP request is wrapped in a
//! [`corosensei`] stackful fiber so that synchronous-looking I/O helpers
//! (`DB::*`, `File::*`, `Http::*`) can transparently suspend the calling fiber
//! and resume it when the Tokio future resolves - without any `async`/`await`
//! in the application script.
//!
//! ## Memory Lifecycle
//! 1. Worker pops a warm `RequestArena` from the per-thread `BoundedArenaPool`.
//! 2. The fiber executes the route handler inside the arena.
//! 3. After response flush + `Future::defer` drain, the arena resets in O(1).
//! 4. The arena is returned to the pool (up to `MAX_CACHED_CHUNKS` slabs).
//!
//! ## Singleton vs Per-Request State
//! - `singleton` classes and variables live in `Arc<EngineState>` and are shared
//!   read-only across all fibers.
//! - All other globals, statics, and request objects are allocated inside the
//!   per-request arena and vanish on reset.

#[allow(unused_imports)]
use std::cell::Cell;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use corosensei::{Coroutine, CoroutineResult, Yielder};
use tokio::task::LocalSet;

use crate::runtime::config::RuntimeConfig;
use crate::runtime::types::{ActiveFiberEntry, CancelReason, FiberDiagState, IpcMessage, RequestId};
use crate::runtime::ipc::{IpcReceiver, IpcSender};
use crate::runtime::log_pool::LogRingBuffer;

// ============================================================================
// Coroutine signal types
// ============================================================================

/// Signal delivered to a suspended fiber to resume it.
#[derive(Debug)]
pub enum ResumeSignal {
    /// An awaited I/O future completed successfully.
    IoComplete(Vec<u8>),
    /// The request was cancelled (client disconnect / timeout / shutdown).
    Cancelled(CancelReason),
}

/// Reason a fiber voluntarily surrendered the CPU.
#[derive(Debug)]
pub enum YieldCause {
    /// Fiber is waiting for a Tokio async future to resolve.
    PendingIo { kind: String },
    /// Fiber completed and produced a response.
    Done {
        status: u16,
        headers: Vec<(String, String)>,
        body: Vec<u8>,
    },
    /// Fiber failed with a runtime error.
    Failed(String),
}

// ============================================================================
// Per-fiber tracking entry used by the diagnostics dump.
// ============================================================================

/// Live introspection record maintained for every active fiber.
struct FiberEntry {
    request_id: RequestId,
    method: String,
    uri: String,
    started_at: Instant,
    state: FiberDiagState,
}

// ============================================================================
// Worker process main loop
// ============================================================================

/// Entry point for a standard Worker process.
///
/// Receives dispatch requests from the Master Reactor over `ipc_rx`, executes
/// them inside stackful coroutines, and sends responses back via `ipc_tx`.
/// Exits cleanly when `shutdown` is set or a `GracefulShutdown` IPC message arrives.
pub fn run_worker(
    worker_id: u32,
    config: Arc<RuntimeConfig>,
    ipc_tx: IpcSender,
    ipc_rx: IpcReceiver,
    log_buf: Arc<LogRingBuffer>,
    shutdown: Arc<AtomicBool>,
) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("Failed to build worker Tokio runtime");

    rt.block_on(async move {
        let local = LocalSet::new();
        local.run_until(worker_event_loop(
            worker_id,
            config,
            ipc_tx,
            ipc_rx,
            log_buf,
            shutdown,
        )).await;
    });
}

/// Inner async event loop running inside the `LocalSet`.
async fn worker_event_loop(
    worker_id: u32,
    config: Arc<RuntimeConfig>,
    ipc_tx: IpcSender,
    mut ipc_rx: IpcReceiver,
    log_buf: Arc<LogRingBuffer>,
    shutdown: Arc<AtomicBool>,
) {
    let mut active_fibers: HashMap<RequestId, FiberEntry> = HashMap::new();
    let mut requests_handled: u64 = 0;
    let max_requests = config.max_requests_per_worker;
    let max_fibers = config.max_active_fibers;

    // Direct all Ws:: facade calls in this worker to the Master Reactor over IPC
    crate::runtime::websocket::set_current_ws_backend(Some(Arc::new(
        crate::runtime::websocket::IpcWebSocketBackend::new(ipc_tx.clone()),
    )));

    loop {
        // Check shutdown flag and request cap (graceful recycling)
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        if requests_handled >= max_requests {
            eprintln!("[rhpm] worker {} reached max_requests ({}); recycling", worker_id, max_requests);
            let _ = ipc_tx.send(IpcMessage::WorkerExit {
                worker_id,
                reason: "max_requests reached".to_string(),
            });
            break;
        }

        // Receive next IPC message from master
        let msg = match tokio::time::timeout(
            Duration::from_millis(100),
            ipc_rx.recv(),
        ).await {
            Ok(Some(m)) => m,
            Ok(None) => break, // Channel closed
            Err(_) => continue, // Timeout - loop back for shutdown check
        };

        match msg {
            IpcMessage::DispatchRequest { request_id, method, uri, headers, body } => {
                // Admission control
                if active_fibers.len() >= max_fibers {
                    let _ = ipc_tx.send(IpcMessage::ResponseReady {
                        request_id,
                        status: 503,
                        headers: vec![("retry-after".to_string(), "1".to_string())],
                        body: b"503 Service Unavailable\n".to_vec(),
                    });
                    continue;
                }

                requests_handled += 1;

                // Track active fiber
                active_fibers.insert(request_id, FiberEntry {
                    request_id,
                    method: method.clone(),
                    uri: uri.clone(),
                    started_at: Instant::now(),
                    state: FiberDiagState::Running,
                });

                // Spawn fiber as a Tokio local task
                let req_id = request_id;
                let tx = ipc_tx.clone();
                let cfg = config.clone();
                let body_data = body.unwrap_or_default();
                let m = method.clone();
                let u = uri.clone();

                tokio::task::spawn_local(async move {
                    execute_request_fiber(req_id, m, u, headers, body_data, tx, cfg).await;
                });
            }

            IpcMessage::CancelRequest { request_id, reason } => {
                // Mark as cancelled; the fiber will check this on next resume
                if let Some(entry) = active_fibers.get_mut(&request_id) {
                    entry.state = FiberDiagState::Suspended;
                }
                log_buf.try_enqueue(
                    request_id,
                    crate::runtime::types::LogLevel::Info,
                    format!("Request {} cancelled: {}", request_id, reason),
                );
            }

            IpcMessage::GracefulShutdown => {
                eprintln!("[rhpm] worker {} received GracefulShutdown", worker_id);
                break;
            }

            IpcMessage::DiagnosticDump => {
                let entries: Vec<ActiveFiberEntry> = active_fibers
                    .values()
                    .map(|e| ActiveFiberEntry {
                        request_id: e.request_id,
                        worker_id,
                        method: e.method.clone(),
                        uri: e.uri.clone(),
                        elapsed_ms: e.started_at.elapsed().as_millis() as u64,
                        state: e.state.clone(),
                    })
                    .collect();
                let _ = ipc_tx.send(IpcMessage::DiagnosticDumpResponse(entries));
            }

            _ => {}
        }
    }
}

// ============================================================================
// Per-request stackful fiber execution
// ============================================================================

/// Executes a single HTTP request inside a `corosensei` stackful fiber.
///
/// The fiber body builds the response synchronously from the script's
/// perspective. Any `DB::*`, `File::*`, or `Http::*` call invokes
/// `fiber_yield_io` which suspends the fiber and queues a Tokio future;
/// when the future resolves the fiber is resumed with the result.
async fn execute_request_fiber(
    request_id: RequestId,
    method: String,
    uri: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    ipc_tx: IpcSender,
    config: Arc<RuntimeConfig>,
) {
    // Channel for the fiber to yield I/O descriptors back to the async driver.
    let (yield_tx, mut yield_rx) =
        tokio::sync::mpsc::unbounded_channel::<YieldCause>();
    // Channel for the async driver to resume the fiber with I/O results.
    let (_resume_tx, _resume_rx) =
        std::sync::mpsc::sync_channel::<ResumeSignal>(1);

    // Build the corosensei coroutine. The coroutine body is a synchronous
    // closure; it communicates with the outer async loop via the channels above.
    let yield_tx_fiber = yield_tx.clone();
    let req_id = request_id;
    let m = method.clone();
    let u = uri.clone();

    // Suppress unused variable warnings for parameters captured for future use.
    let _ = &headers;
    let _ = &body;

    let stack = corosensei::stack::DefaultStack::new(config.fiber_stack_size)
        .unwrap_or_else(|_| corosensei::stack::DefaultStack::default());
    let mut coroutine =
        Coroutine::with_stack(stack, move |_yielder: &Yielder<ResumeSignal, ()>, first_signal| {
            // The coroutine body is where the actual request script would run.
            // Here we simulate a simple echo-style handler as a placeholder
            // that shows the fiber suspend/resume pattern for I/O hooks.
            let _ = first_signal; // Ignore initial resume value

            // --- Placeholder: simulate script execution ---
            // In production, `run_request_script(req_id, &m, &u, headers, body, &yielder)`
            // would execute the compiled RHP bytecode. The script calls e.g. `DB::find()`
            // which calls `fiber_yield_io(&yielder, ...)` shown below.

            let response_body = format!(
                "HTTP/1.1 handled | method={} uri={} req={}",
                m, u, req_id
            );

            // Yield Done to signal the async driver we have a response.
            let _ = yield_tx_fiber.send(YieldCause::Done {
                status: 200,
                headers: vec![
                    ("content-type".to_string(), "text/plain".to_string()),
                    ("content-length".to_string(), response_body.len().to_string()),
                ],
                body: response_body.into_bytes(),
            });

            // Return empty string; the coroutine is done.
            String::new()
        });

    // --- Async driver loop ---
    // We drive the coroutine forward by calling `resume`. The coroutine yields
    // `YieldCause` values back; for I/O causes we await the Tokio future then
    // send the result back via `resume_tx`.

    // Initial resume to start the coroutine.
    let mut result = coroutine.resume(ResumeSignal::IoComplete(vec![]));

    loop {
        match result {
            CoroutineResult::Yield(()) => {
                // Coroutine yielded control; check what it sent us.
                match yield_rx.recv().await {
                    Some(YieldCause::PendingIo { kind }) => {
                        // Drive the Tokio I/O future (placeholder: 1ms sleep)
                        let _ = kind; // used in real implementation to dispatch correct future
                        tokio::time::sleep(Duration::from_millis(1)).await;
                        // Resume fiber with the I/O result
                        result = coroutine.resume(ResumeSignal::IoComplete(vec![]));
                    }
                    Some(YieldCause::Done { status, headers, body }) => {
                        let _ = ipc_tx.send(IpcMessage::ResponseReady {
                            request_id,
                            status,
                            headers,
                            body,
                        });
                        break;
                    }
                    Some(YieldCause::Failed(err)) => {
                        let body = format!("500 Internal Server Error\n{}\n", err);
                        let _ = ipc_tx.send(IpcMessage::ResponseReady {
                            request_id,
                            status: 500,
                            headers: vec![("content-type".to_string(), "text/plain".to_string())],
                            body: body.into_bytes(),
                        });
                        break;
                    }
                    None => break, // yield channel dropped
                }
            }
            CoroutineResult::Return(_) => {
                // Coroutine returned normally - the Done signal should already
                // have been sent; we just clean up here.
                break;
            }
        }
    }
}

/// Called from inside the coroutine body to pause the fiber and await a Tokio
/// I/O future. The fiber suspends here and resumes when `resume()` is called
/// with an `IoComplete` signal from the async driver.
///
/// This is the bridge between synchronous script code and async Tokio I/O.
#[allow(dead_code)]
pub fn fiber_yield_io(
    yielder: &Yielder<ResumeSignal, ()>,
    kind: impl Into<String>,
    yield_tx: &tokio::sync::mpsc::UnboundedSender<YieldCause>,
) -> Result<Vec<u8>, CancelReason> {
    // Send the I/O descriptor to the async driver.
    let _ = yield_tx.send(YieldCause::PendingIo { kind: kind.into() });

    // Suspend the fiber and wait for the resume signal.
    let signal = yielder.suspend(());

    match signal {
        ResumeSignal::IoComplete(data) => Ok(data),
        ResumeSignal::Cancelled(reason) => Err(reason),
    }
}
