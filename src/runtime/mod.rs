//! # RHPM: RHP Runtime Process Manager
//!
//! Implements the Swoole-grade hybrid Multi-Process + Multi-Thread + Stackful Coroutine
//! runtime engine for the RHP language.
//!
//! ## Architecture Tiers
//! - **Tier 1 (Master Process):** Reactor threads handle raw TCP/TLS and HTTP framing.
//! - **Tier 2 (Manager Process):** Supervises worker and task-worker process pools.
//! - **Tier 3 (Worker Processes):** Each worker runs a single-threaded Tokio LocalSet with
//!   corosensei stackful fibers — one fiber per active HTTP request.
//!
//! Standard I/O helpers (`DB::*`, `File::*`, `Http::*`) transparently suspend the calling
//! fiber and resume it when the Tokio future resolves, giving developers synchronous syntax
//! with non-blocking execution. `ADB::*`, `AFile::*`, `AHttp::*` fire-and-forget without
//! pausing the current fiber.

pub mod arena_pool;
pub mod config;
pub mod ipc;
pub mod log_pool;
pub mod manager;
pub mod reactor;
pub mod task;
pub mod types;
pub mod worker;

// Re-export the most used surface area
pub use arena_pool::{acquire_arena, release_arena, arena_pool_stats, ArenaPoolStats};
pub use config::RuntimeConfig;
pub use ipc::{ipc_channel, IpcReceiver, IpcSender};
pub use log_pool::{LogBatchSender, LogCollector, LogRingBuffer};
pub use manager::{Manager, ManagerHandle};
pub use reactor::{parse_http_request, run_reactor_simple, ParsedRequest, ReactorHandle, WorkerRouter};
pub use task::{TaskId, TaskJob, TaskKind, TaskQueueSender, TaskWorker};
pub use types::{
    ActiveFiberEntry, CancelReason, FiberDiagState, IpcMessage, LogLevel, LogRecord, RequestId,
    WorkerKind,
};
pub use worker::{fiber_yield_io, run_worker, ResumeSignal, YieldCause};

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_request_id_generation() {
        let r1 = RequestId::next();
        let r2 = RequestId::next();
        assert!(r2.0 > r1.0);
        assert_eq!(format!("{}", r1), format!("req#{}", r1.0));
    }

    #[test]
    fn test_cancel_reasons() {
        assert_eq!(format!("{}", CancelReason::ClientDisconnect), "client disconnected");
        assert_eq!(format!("{}", CancelReason::Timeout), "request timeout exceeded");
        assert_eq!(format!("{}", CancelReason::WorkerRecycle), "worker recycling");
        assert_eq!(format!("{}", CancelReason::Shutdown), "graceful shutdown");
    }

    #[test]
    fn test_runtime_config_defaults_and_builder() {
        let cfg = RuntimeConfig::default()
            .with_host("0.0.0.0")
            .with_port(9000)
            .with_workers(8)
            .with_task_workers(4)
            .with_reactors(4)
            .with_arena_cap(32 * 1024 * 1024)
            .with_io_timeout(Duration::from_secs(45));

        assert_eq!(cfg.host, "0.0.0.0");
        assert_eq!(cfg.port, 9000);
        assert_eq!(cfg.worker_count, 8);
        assert_eq!(cfg.task_worker_count, 4);
        assert_eq!(cfg.reactor_threads, 4);
        assert_eq!(cfg.arena_memory_cap, 32 * 1024 * 1024);
        assert_eq!(cfg.io_timeout, Duration::from_secs(45));
        assert_eq!(cfg.bind_addr(), "0.0.0.0:9000");
    }

    #[tokio::test]
    async fn test_ipc_channel_exchange() {
        let (tx, mut rx) = ipc_channel();
        let req_id = RequestId::next();

        let msg = IpcMessage::DispatchRequest {
            request_id: req_id,
            method: "GET".to_string(),
            uri: "/test".to_string(),
            headers: vec![("accept".to_string(), "*/*".to_string())],
            body: Some(b"hello".to_vec()),
        };

        assert!(tx.send(msg).is_ok());

        let received = rx.recv().await.expect("Should receive message");
        match received {
            IpcMessage::DispatchRequest { request_id, method, uri, body, .. } => {
                assert_eq!(request_id, req_id);
                assert_eq!(method, "GET");
                assert_eq!(uri, "/test");
                assert_eq!(body.unwrap(), b"hello");
            }
            _ => panic!("Unexpected message variant received"),
        }
    }

    #[test]
    fn test_log_ring_buffer_operations() {
        let (flush_tx, mut flush_rx) = tokio::sync::mpsc::unbounded_channel();
        let ring = LogRingBuffer::new(1, 16, flush_tx);
        let req_id = RequestId::next();

        // Enqueue Info
        assert!(ring.try_enqueue(req_id, LogLevel::Info, "Normal log".to_string()));
        // Flush channel shouldn't have fired for Info
        assert!(flush_rx.try_recv().is_err());

        // Enqueue Warn (triggers flush signal)
        assert!(ring.try_enqueue(req_id, LogLevel::Warn, "Warning log".to_string()));
        assert!(flush_rx.try_recv().is_ok());

        // Drain batch
        let batch = ring.drain_batch(10);
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0].message, "Normal log");
        assert_eq!(batch[1].message, "Warning log");
    }

    #[test]
    fn test_arena_pool_acquire_and_release() {
        let stats_before = arena_pool_stats();
        let mut arena = acquire_arena();

        // Perform bump allocation
        let ptr = arena.alloc(128, 8);
        assert!(!ptr.is_null());
        assert!(arena.allocated_bytes() >= 128);

        // Release back to pool
        release_arena(arena);

        // Reacquire - should be warm and reset
        let arena2 = acquire_arena();
        assert_eq!(arena2.allocated_bytes(), 0);
        release_arena(arena2);

        let stats_after = arena_pool_stats();
        assert_eq!(stats_after.max_cached, stats_before.max_cached);
    }

    #[test]
    fn test_http_request_parser() {
        let raw_get = b"GET /api/v1/users?page=1 HTTP/1.1\r\nHost: localhost:8080\r\nAccept: application/json\r\n\r\n";
        let parsed = parse_http_request(raw_get).expect("Failed to parse valid GET");
        assert_eq!(parsed.method, "GET");
        assert_eq!(parsed.uri, "/api/v1/users?page=1");
        assert_eq!(parsed.version, "HTTP/1.1");
        assert_eq!(parsed.body.len(), 0);

        let raw_post = b"POST /submit HTTP/1.1\r\nHost: example.com\r\nContent-Length: 11\r\n\r\nhello world";
        let parsed_post = parse_http_request(raw_post).expect("Failed to parse valid POST");
        assert_eq!(parsed_post.method, "POST");
        assert_eq!(parsed_post.uri, "/submit");
        assert_eq!(parsed_post.body, b"hello world");

        // Incomplete POST body
        let raw_incomplete = b"POST /submit HTTP/1.1\r\nContent-Length: 20\r\n\r\nshort";
        assert!(parse_http_request(raw_incomplete).is_none());
    }

    #[test]
    fn test_worker_router_least_loaded() {
        let (tx1, _rx1) = ipc_channel();
        let (tx2, _rx2) = ipc_channel();

        let router = WorkerRouter::new(vec![tx1, tx2]);
        let msg = IpcMessage::GracefulShutdown;

        // Route first message
        assert!(router.route(msg.clone()).is_ok());

        // Route second message - should route to slot with lower load
        assert!(router.route(msg).is_ok());

        // Slot 0 completed
        router.complete_slot_index(0);
    }

    #[test]
    fn test_corosensei_stackful_fiber_lifecycle() {
        use corosensei::{Coroutine, CoroutineResult, Yielder};

        let stack = corosensei::stack::DefaultStack::new(64 * 1024).expect("Alloc stack");
        let mut coroutine = Coroutine::with_stack(stack, |yielder: &Yielder<u32, u32>, first_val| {
            assert_eq!(first_val, 10);
            let resume_val = yielder.suspend(42);
            assert_eq!(resume_val, 100);
            "fiber done"
        });

        // First run -> yields 42
        match coroutine.resume(10) {
            CoroutineResult::Yield(v) => assert_eq!(v, 42),
            CoroutineResult::Return(_) => panic!("Expected yield"),
        }

        // Resume -> returns final value
        match coroutine.resume(100) {
            CoroutineResult::Yield(_) => panic!("Expected return"),
            CoroutineResult::Return(r) => assert_eq!(r, "fiber done"),
        }
    }
}
