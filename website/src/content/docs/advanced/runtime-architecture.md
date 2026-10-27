---
title: "Runtime & Process Architecture"
description: "Deep dive into the RHP host runtime: supervisor process, pre-forked worker pool, socket ownership, Tokio/sqlx post-fork init, hot reload, and rhpm."
section: "Advanced"
order: 2
---

## What the Runtime Layer Owns

The **Runtime Layer** (`rhpm` + built-in host server) owns **everything about processes, I/O, sockets, and lifecycle**. It bridges pure language code to operating system facilities without dictating high-level application structure.

```text
┌────────────────────────────────────────────────────────────────────────┐
│                    LAYER 2: RUNTIME (rhpm & Host Server)               │
│                                                                        │
│  - Master Supervisor Process           - Pre-forked Worker Pool        │
│  - Socket Binding (SO_REUSEPORT)       - HTTP Accept & Parsing Loop    │
│  - Post-Fork Tokio / sqlx Init         - Request Dispatch & Arenas     │
│  - Future::defer Flush Execution       - Worker Respawn & Hot Reload   │
└────────────────────────────────────────────────────────────────────────┘
```

---

## 1. Master Supervisor Process

The runtime entry point (`rhp run dev`, `rhp run build`, or `rhpm`) starts as a lightweight, persistent master supervisor process:

1. **Loads Configuration:** Parses `rhp.toml` for port assignments, worker limits, timeouts, and file-watching rules.
2. **Evaluates Application Boot Phase:** Runs top-level statements and service registrations in `index.rhp` once, pre-compiling bytecode and freezing immutable definitions into an immutable state (`Arc<EngineState>`).
3. **Binds TCP Socket:** Binds the server socket (`SO_REUSEPORT` or shared file descriptor) before workers are spawned.
4. **Enforces the POSIX Fork / Threading Invariant:**
   > **The Rule:** The master process remains strictly single-threaded during the boot phase.
   > Under POSIX, `fork()` copies *only* the calling thread. If the parent process were to initialize a multi-threaded Tokio runtime, spawn background worker threads, or open database pools prior to forking, child processes would instantly deadlock on inherited mutexes. The runtime enforces this at startup—any boot code accessing `DB::*` or async threads raises a fatal `BootPhaseViolation`.

---

## 2. Pre-Forked Worker Pool

Rather than incurring the heavy cost of fork-per-request (copy-on-write page faults and lost JIT caches), the master supervisor pre-forks $N$ persistent worker processes at startup:

```text
┌────────────────────────────────────────────────────────────────────────┐
│                   RHP SUPERVISOR (Master Process)                      │
│ - Reads rhp.toml (port, workers, timeouts, watch rules)                │
│ - Executes index.rhp Boot Phase (Sync, strictly single-threaded)       │
│ - Binds shared TCP listening socket                                    │
│ - File watcher (hot reloading) & process supervisor (SIGCHLD / respawn)│
└───────────────────┬────────────────────────────────┬───────────────────┘
                    │ fork() (Pre-forked pool at boot)│ fork()
                    ▼                                ▼
       ┌────────────────────────┐       ┌────────────────────────┐
       │    WORKER PROCESS 1    │       │    WORKER PROCESS 2    │
       │                        │       │                        │
       │ Post-fork init:        │       │ Post-fork init:        │
       │ - Starts Tokio/Async   │       │ - Starts Tokio/Async   │
       │ - Inits DB Pool (sqlx) │       │ - Inits DB Pool (sqlx) │
       │                        │       │                        │
       │ Request Loop:          │       │ Request Loop:          │
       │ - Accepts connections  │       │ - Accepts connections  │
       │ - RequestArena (O(1))  │       │ - RequestArena (O(1))  │
       │ - Executes defer queue │       │ - Executes defer queue │
       └────────────────────────┘       └────────────────────────┘
```

### Fault Isolation & Automatic Worker Respawn
Each worker runs in its own kernel-isolated virtual memory address space:
- If a worker process encounters an unrecoverable failure (e.g. native segfault, unexpected abort, or memory cap kill), the master supervisor catches `SIGCHLD`.
- The supervisor logs the crash telemetry and immediately forks a clean replacement worker.
- Other concurrent workers and in-flight client connections are 100% unaffected.

---

## 3. Post-Fork Worker Initialization

Immediately following the `fork()` syscall, each worker executes its isolated startup sequence:

1. **Spins up Tokio Runtime:** Instantiates a multi-threaded or current-thread Tokio asynchronous event loop inside the child process.
2. **Initializes Database Pools:** Creates native connection pools (`sqlx`, Redis, PostgreSQL, SQLite) without cross-process locking or IPC bottlenecks.
3. **Enters the Request Accept Loop:** Attaches to the shared socket descriptor and begins serving incoming HTTP requests.

---

## 4. HTTP Accept Loop & Request-Scoped Bump Arenas

When an incoming connection arrives:

1. **HTTP Parsing:** The worker reads and parses incoming HTTP wire headers into a `RawRequest` tuple (method, URI, headers, body bytes).
2. **Dedicated Bump Arena (`RequestArena`):** The worker initializes a pre-allocated bump allocation chunk. Every object, array, or string created during request processing is allocated sequentially using an 8-byte aligned bump pointer.
3. **Execution:** Dispatches the request through application routes and handlers.
4. **Instant $O(1)$ Teardown:** When the response is sent, the entire request arena's bump pointer is reset to 0 in a single step ($O(1)$). This eliminates tracing garbage collection sweeps and guarantees zero memory leaks across requests.

---

## 5. `Future::defer` Post-Response Flush Execution

A crucial responsibility of the runtime is managing the execution lifecycle of deferred tasks:

```php
Route::post('/orders', function(Request $req): Response {
    $order = Order::create($req->json());

    // Queued to run AFTER response is delivered to the client
    Future::defer(function() use ($order) {
        Analytics::track('order.created', $order->id);
        Log::info("Order {$order->id} completed");
    });

    return Response::json(['status' => 'created']);
});
```

### Execution Lifecycle:
1. **Response Flush First:** The runtime writes the HTTP response status, headers, and body bytes to the client socket and flushes immediately.
2. **Deferred Queue Drain:** Once the client receives its response, the worker sequentially executes all callbacks registered in `Future::defer`.
3. **Arena Reset:** After all deferred tasks finish, the request arena is dropped and reset.
4. **Shutdown Draining:** On receiving `SIGTERM` / `SIGQUIT`, workers stop accepting new connections, finish all active requests, drain their pending deferred queues, and exit cleanly without dropping background jobs.

---

## 6. Development Mode Hot Reloading

In development mode (`rhp run dev` or `rhp dev`):
- The master process integrates a file watcher powered by the `notify` crate, monitoring `*.rhp` and `rhp.toml` files.
- When changes are detected on disk, the master:
  1. Re-executes the boot phase in memory.
  2. Forks fresh worker processes with the updated code.
  3. Sends `SIGQUIT` to older workers, allowing them to drain in-flight requests gracefully before exiting.
  4. Achieves sub-millisecond, zero-downtime hot reloading.

---

## 7. `rhpm` Multi-Process & Sidecar Management

In large-scale production deployments, `rhpm` acts as the cluster and process manager:
- **Multiple Service Instances:** Scales worker pools across multiple host cores or CPU affinity masks.
- **Sidecar Isolation:** Routes heavy, slow, or memory-intensive endpoints (e.g. bulk CSV exports, image processing, heavy AI embeddings) to dedicated sidecar worker pools, preventing them from monopolizing the primary HTTP request workers.
- **Queue Worker Daemons:** Manages long-running background CLI consumers and scheduler cron processes.

---

## 8. Runtime Configuration (`rhp.toml`)

All runtime options are managed declaratively in `rhp.toml` rather than hardcoded in PHP scripts:

```toml
[server]
host = "127.0.0.1"
port = 8080
workers = "auto" # "auto" detects available CPU cores, or integer (e.g. 8)

[limits]
max_arena_mb = 32 # Terminates requests exceeding memory limit
timeout_ms = 5000 # Kills runaway requests exceeding 5s deadline

[dev]
hot_reload = true
watch = ["src/", "index.rhp"]
```
