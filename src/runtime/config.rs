//! Runtime configuration, loaded from `rhp.toml` or CLI flags, governing all
//! RHPM process tiers, resource limits, and tuning knobs.

use std::time::Duration;

/// Top-level RHPM runtime configuration.
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    // ── Network ─────────────────────────────────────────────────────────────
    /// TCP host to bind (default: `127.0.0.1`).
    pub host: String,
    /// TCP port to listen on (default: `8080`).
    pub port: u16,

    // ── Process Counts ───────────────────────────────────────────────────────
    /// Number of Reactor threads in the Master process (default: logical CPU count).
    pub reactor_threads: usize,
    /// Number of standard Worker processes (default: physical CPU core count).
    pub worker_count: usize,
    /// Number of dedicated Task Worker processes for heavy background jobs (default: 2).
    pub task_worker_count: usize,

    // ── Worker Recycling ─────────────────────────────────────────────────────
    /// Maximum requests a worker handles before graceful recycling (default: 10 000).
    pub max_requests_per_worker: u64,
    /// Maximum worker RSS memory in bytes before graceful recycling (default: 256 MB).
    pub max_worker_rss_bytes: usize,

    // ── Concurrency & Admission ──────────────────────────────────────────────
    /// Maximum concurrent active fibers per Worker process (default: 2 048).
    pub max_active_fibers: usize,
    /// Maximum pending IPC request queue depth before Master sheds load (default: 8 192).
    pub max_queued_requests: usize,

    // ── Resource Limits (per request) ────────────────────────────────────────
    /// Fixed per-request memory cap in bytes (arena ceiling, default: 16 MB).
    pub arena_memory_cap: usize,
    /// Fixed per-request wall-clock I/O timeout (default: 30 s).
    pub io_timeout: Duration,

    // ── Fiber Stack ──────────────────────────────────────────────────────────
    /// corosensei fiber stack size in bytes (default: 128 KB).
    pub fiber_stack_size: usize,
    /// Maximum idle stacks retained in the per-worker recycling pool (default: 512).
    pub max_idle_fiber_stacks: usize,

    // ── Logging ──────────────────────────────────────────────────────────────
    /// Size of the per-worker non-blocking log ring buffer (default: 4 096 entries).
    pub log_ring_size: usize,
    /// Batch flush interval for the master log collector (default: 5 ms).
    pub log_flush_interval: Duration,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        let cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        Self {
            host: "127.0.0.1".to_string(),
            port: 8080,
            reactor_threads: cpus,
            worker_count: cpus,
            task_worker_count: 2,
            max_requests_per_worker: 10_000,
            max_worker_rss_bytes: 256 * 1024 * 1024,
            max_active_fibers: 2048,
            max_queued_requests: 8192,
            arena_memory_cap: 16 * 1024 * 1024,
            io_timeout: Duration::from_secs(30),
            fiber_stack_size: 128 * 1024,
            max_idle_fiber_stacks: 512,
            log_ring_size: 4096,
            log_flush_interval: Duration::from_millis(5),
        }
    }
}

impl RuntimeConfig {
    /// Builder: set host.
    pub fn with_host(mut self, host: impl Into<String>) -> Self {
        self.host = host.into();
        self
    }

    /// Builder: set port.
    pub fn with_port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    /// Builder: set standard worker count.
    pub fn with_workers(mut self, n: usize) -> Self {
        self.worker_count = n.max(1);
        self
    }

    /// Builder: set task worker count.
    pub fn with_task_workers(mut self, n: usize) -> Self {
        self.task_worker_count = n;
        self
    }

    /// Builder: set reactor thread count.
    pub fn with_reactors(mut self, n: usize) -> Self {
        self.reactor_threads = n.max(1);
        self
    }

    /// Builder: set per-request arena memory cap.
    pub fn with_arena_cap(mut self, bytes: usize) -> Self {
        self.arena_memory_cap = bytes;
        self
    }

    /// Builder: set per-request I/O timeout.
    pub fn with_io_timeout(mut self, d: Duration) -> Self {
        self.io_timeout = d;
        self
    }

    /// Socket bind address string.
    pub fn bind_addr(&self) -> String {
        format!("{}:{}", self.host, self.port)
    }
}
