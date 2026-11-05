---
title: "RHPM: Runtime Process & Coroutine Architecture"
description: "Comprehensive guide to RHPM: Swoole-grade 3-tier hybrid process model, corosensei stackful coroutines, Reactor threads, Manager supervisor, and zero-overhead memory arenas."
section: "Advanced"
order: 2
---

## Overview: The Swoole-Grade Hybrid Concurrency Model

RHP replaces naive single-threaded request handling and heavy fork-per-request models with **RHPM (RHP Process Manager)**: an enterprise-grade hybrid **Multi-Process + Multi-Thread + Stackful Coroutine** architecture inspired by Swoole, RoadRunner, and Erlang/BEAM.

```text
                  [ Incoming HTTP Requests ]
                             │
                             ▼
  Master Process ──► [ Reactor Threads (OS Threads) ]  <-- Non-blocking I/O multiplexing & HTTP framing
                             │
                             ▼ (Least-Loaded IPC Dispatch)
  Manager Process ──► [ Worker Processes (OS Processes) ]
                             │
                             ▼
                 [ Coroutines (corosensei Fibers) ]   <-- 1 Stackful Fiber per active request
                             │
                             ├─► Standard I/O (DB, File, Http) ──► Transparent fiber suspend & Tokio resume
                             └─► Async I/O (ADB, AFile, AHttp)  ──► Dispatched to dedicated Task Workers
```

Instead of forcing PHP-family code into single-threaded asynchronous callback hell or spawning expensive OS processes per request, RHPM combines three complementary tiers:
1. **Master Process (Reactor OS Threads):** High-speed non-blocking socket multiplexing and HTTP protocol framing.
2. **Manager Process (Supervisor & Orchestrator):** Lifecycle management, worker health monitoring, automatic restart on crash, and zero-downtime rolling recycling.
3. **Worker Processes (Tokio LocalSet + Stackful Fibers):** Each worker runs a single-threaded Tokio event loop hosting thousands of lightweight `corosensei` stackful fibers simultaneously.
4. **Task Worker Pool (Dedicated Background Workers):** Heavy computation, background telemetry, and deferred jobs are isolated from the latency-sensitive HTTP request path.

---

## 1. Master Process & Reactor Threads (Tier 1)

The Master Process owns the primary listening sockets (`SO_REUSEPORT`) and manages $N$ **Reactor OS Threads** (defaulting to the host's logical CPU count).

### Responsibilities:
- **TCP & TLS Multiplexing:** Accepts client connections using epoll/kqueue event loops.
- **HTTP/1.1 & HTTP/2 Framing:** Decodes incoming byte streams into structured request envelopes (`method`, `uri`, `headers`, `body`).
- **Least-Loaded Load Balancing:** Routes requests across healthy Worker processes via low-latency IPC channels using an $O(N)$ least-pending dispatch algorithm.
- **Overload Admission & Load Shedding:** When the queue reaches capacity (`max_queued_requests = 8192`), the Reactor immediately sheds load with `503 Service Unavailable` before worker memory is exhausted.
- **Client Response Flushing:** Receives completed response buffers from Workers and flushes them directly to the client socket without blocking.

---

## 2. Manager Process: Supervision & Zero-Downtime Reload (Tier 2)

The Manager Process acts as the cluster supervisor, keeping workers healthy without being on the request data path:

- **Fault Containment & Auto-Respawn:** Monitors worker OS processes via `SIGCHLD`. If a worker terminates abnormally (e.g., segfault in a native C extension or Out-Of-Memory abort), the Manager immediately forks a clean replacement. Concurrent workers are 100% unaffected.
- **Proactive Rolling Recycling:** Workers track total requests handled and resident memory footprint (RSS). When reaching configured thresholds (`max_requests_per_worker = 10000` or `max_worker_rss_bytes = 256MB`), the worker gracefully drains active fibers and exits cleanly, prompting the Manager to spawn a pristine successor.
- **Zero-Downtime Hot Code Reload:** Upon code modification or `SIGUSR1`, the Manager spawns fresh workers with the updated application state and signals older workers to drain in-flight requests before exiting.

---

## 3. Worker Processes & `corosensei` Stackful Coroutines (Tier 3)

Every Worker process runs a single-threaded Tokio `current_thread` runtime with a `LocalSet`. Every incoming HTTP request is wrapped in an independent **Stackful Fiber** powered by [`corosensei`].

```text
┌────────────────────────────────────────────────────────────────────────┐
│                        WORKER PROCESS (OS Process)                     │
│                                                                        │
│  Tokio LocalSet Event Loop                                             │
│  ┌───────────────────────┐  ┌───────────────────────┐                  │
│  │ Fiber 1 (Req #1001)   │  │ Fiber 2 (Req #1002)   │   ... (Up to    │
│  │ - 128 KB Stack        │  │ - 128 KB Stack        │        2048     │
│  │ - Running PHP Route   │  │ - Suspended on DB     │       Fibers)   │
│  └───────────┬───────────┘  └───────────▲───────────┘                  │
│              │                          │                              │
│              ▼ suspend()                │ resume()                     │
│  ┌──────────────────────────────────────┴───────────────────────────┐  │
│  │                     Tokio Async I/O Futures                      │  │
│  └──────────────────────────────────────────────────────────────────┘  │
│                                                                        │
│  Request Memory Arena Pool                                             │
│  [ Warm Arena 1 ]  [ Warm Arena 2 ] ──► Instantaneous O(1) Slab Reset  │
└────────────────────────────────────────────────────────────────────────┘
```

### Transparent Fiber Suspension vs. Async Helpers

RHP completely eliminates the "function coloring" problem of `async`/`await`:

1. **Standard Facades (`DB::*`, `File::*`, `Http::*`):**
   - Written synchronously in application code: `$user = DB::table('users')->find(42);`
   - Under the hood, the helper invokes `fiber_yield_io()`, which suspends the current `corosensei` stackful fiber and registers a Tokio async future.
   - The OS thread continues executing other fibers.
   - When Tokio completes the I/O operation, the fiber resumes at the exact instruction pointer with zero mental overhead for developers.

2. **Non-Pausing Facades (`ADB::*`, `AFile::*`, `AHttp::*`):**
   - Used when the current HTTP request does not need to wait for the result (e.g. fire-and-forget metrics, asynchronous audit logging).
   - Enqueues the task to the **Task Worker Pool** immediately without pausing the calling fiber.

---

## 4. Dedicated Task Worker Pool

For heavy computation, PDF rendering, bulk emails, or asynchronous database writes, RHPM provides a dedicated **Task Worker Pool**:

- **Decoupled Latency:** Runs as separate OS processes/threads. Heavy CPU bursts or disk writes never steal CPU cycles from the latency-critical HTTP request loop.
- **Non-blocking Dispatch:** Enqueue jobs via `TaskQueueSender` without blocking the HTTP worker fiber.

```php
// Fired into the background Task Worker pool without pausing the HTTP response:
AFile::write('/var/log/audit.log', $auditEntry);
ADB::table('analytics')->insert(['event' => 'page_view', 'ip' => $ip]);
```

---

## 5. Request-Scoped Arena Allocation & `ArenaPool`

RHP bypasses traditional garbage collector pauses through **$O(1)$ Bump Arenas**:

1. **Warm Slab Acquisition:** At the start of a request, the worker acquires a pre-allocated `RequestArena` from the thread-local `ArenaPool`.
2. **Zero GC Allocation:** Memory for strings, arrays, and request objects is bumped sequentially on 8-byte aligned memory slabs.
3. **Instantaneous $O(1)$ Teardown:** Once the HTTP response is flushed to the client and post-response `Future::defer` hooks finish, the arena executes `drop_request()`:
   - External OS handles (uncommitted DB transactions, open file descriptors) in the DropList are cleaned up.
   - Pointers reset to `0` in a single CPU instruction.
   - Pre-allocated virtual memory chunks remain hot in cache and return to the `ArenaPool` for the next request.

### Singleton vs. Request State
- **`singleton` Classes & Global State:** Persist in shared immutable `Arc<EngineState>` across requests.
- **Request State:** Superglobals (`$_GET`, `$_POST`, `$_SERVER`), local variables, and standard static variables belong exclusively to the request arena and are cleared on reset.

---

## 6. High-Throughput Non-Blocking Log Ring Buffer

To prevent slow disk I/O from stalling HTTP workers:
- Each Worker process logs into a bounded non-blocking `LogRingBuffer` (`std::sync::mpsc::sync_channel`).
- `Debug` and `Info` records are enqueued without blocking.
- `Warn` and `Error` records trigger an immediate wake-up signal to the log drainer.
- The Master **`LogCollector`** receives batches asynchronously and performs vectorized writes to stdout or configured file sinks.

---

## 7. Configuration Reference (`rhp.toml`)

All RHPM tuning parameters can be customized declaratively:

```toml
[server]
host = "127.0.0.1"
port = 8080
reactors = "auto"          # Logical CPU count for Master Reactor threads
workers = "auto"           # Physical CPU core count for Worker processes
task_workers = 2           # Dedicated background Task Workers

[limits]
max_requests_per_worker = 10000  # Graceful worker recycling threshold
max_worker_rss_mb = 256          # Maximum resident set size before recycle
max_active_fibers = 2048         # Concurrency admission limit per worker
max_queued_requests = 8192       # Master queue capacity before 503 load shedding
arena_memory_cap_mb = 16         # Per-request bump memory ceiling
io_timeout_secs = 30             # Per-request wall-clock I/O deadline
fiber_stack_kb = 128             # corosensei stack size per fiber

[logging]
log_ring_size = 4096             # Per-worker non-blocking ring buffer slots
flush_interval_ms = 5            # Master log collector batch interval
```

---

## 8. CLI Commands

RHPM is embedded directly into the single unified `rhp` binary:

```bash
# Start the production RHPM server
rhp serve --port 8080 --host 0.0.0.0

# Start in development mode with hot reload
rhp dev

# Inspect active fibers and runtime status
rhp status --dump
```
