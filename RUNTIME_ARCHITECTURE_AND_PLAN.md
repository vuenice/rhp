# RHP Runtime Architecture Specification & Implementation Plan

> **Document Version:** 3.1.0 (Swoole-Grade Hybrid Multi-Process, Multi-Thread, Stackful Fiber Architecture with RHPM Specifications)  
> **Target Subsystem:** `rhp::runtime` (Layer 2 of RHP Tri-Layer Architecture)  
> **Status:** Approved Technical Design & Execution Roadmap  
> **Binary Strategy:** Unified Single Binary (`rhp`)

---

## 1. Executive Summary & Swoole-Grade Hybrid Topology

The RHP Runtime implements a production-grade **hybrid Multi-Process + Multi-Thread + Stackful Coroutine (Fiber)** architecture inspired by **Swoole**, engineered in Rust with **Stackful Fibers (`corosensei`)**, **Tokio event multiplexing**, and **Request-Scoped Zero-GC Bump Arenas (`RequestArena`)**.

It operates across three distinct execution tiers:
1. **Master Process (Multi-Threaded Network Tier):** Runs Reactor Threads that own non-blocking OS I/O multiplexing (`epoll`/`kqueue`), TCP connection handling, TLS handshakes, and HTTP wire framing.
2. **Manager Process (Process Supervision Tier):** Manages worker process pools (Standard Workers + Task Workers), reaps crashed processes (`SIGCHLD`), manages worker recycling (`max_requests`), and coordinates rolling zero-downtime reloads (`SIGUSR1`).
3. **Worker Processes (Coroutine Execution Tier):** Each Worker runs an event loop and wraps every HTTP request in a **Stackful Fiber (`corosensei`)**.
   - **Transparent Synchronous I/O (`DB`, `File`, `Http`):** Hooks standard I/O calls to transparently suspend the current request fiber (under the hood running Rust `await` on Tokio), switching CPU instantly to other fibers.
   - **Asynchronous Non-Pausing Primitives (`ADB`, `AFile`, `AHttp`):** When developers do *not* want to pause the current request, `ADB`, `AFile`, and `AHttp` dispatch detached background futures without yielding the calling fiber.
   - **Singleton vs. Request Scoping:** Classes marked `singleton` and singleton variables persist across requests; all request globals, non-singleton statics, and memory arenas are wiped in $O(1)$ upon request completion.

```text
                     [ Incoming Client HTTP Connections ]
                                      │
                                      ▼
┌────────────────────────────────────────────────────────────────────────┐
│ MASTER PROCESS (Network Level: OS Threads)                             │
│  • Main Thread: Signal management, control socket, CLI interface       │
│  • Reactor Thread Pool (OS Threads, e.g. 4-16 threads):                │
│    - Non-blocking epoll/kqueue network I/O                             │
│    - TLS handshake & termination (Zero PHP/RHP overhead)               │
│    - HTTP wire protocol parsing & streaming buffer                     │
│    - Dispatches parsed requests to Worker Processes via Unix IPC / fd  │
└─────────────────────────────────┬──────────────────────────────────────┘
                                  │
                                  ▼
┌────────────────────────────────────────────────────────────────────────┐
│ MANAGER PROCESS (Supervision Level: OS Process)                        │
│  • Forked by Master process; strictly manages the worker pools         │
│  • Supervises Standard Workers & Task Workers                          │
│  • Auto-restarts crashed workers (SIGCHLD) without dropping master TCP │
│  • Enforces worker recycling (max_requests & memory ceilings)          │
│  • Orchestrates rolling zero-downtime reloads (SIGUSR1)                │
└─────────────────┬──────────────────────────────────────┬───────────────┘
                  │                                      │
                  ▼                                      ▼
┌──────────────────────────────────────┐  ┌──────────────────────────────┐
│ WORKER PROCESSES (Execution Level)   │  │ TASK WORKER PROCESSES        │
│ (N Worker Processes = CPU Cores)     │  │ (Async & Heavy Task Pool)    │
│                                      │  │                              │
│ Each Worker Process runs:            │  │ • Dedicated to offloaded     │
│ • 1 Single-Threaded Tokio Event Loop │  │   background computations    │
│ • corosensei Stackful Fiber per Req  │  │ • Post-response defer jobs   │
│ • Request-Scoped Bump Arena (Zero-GC)│  │   (Future::defer)            │
│ • Transparent I/O Hooks:             │  │ • Heavy exports / PDF jobs   │
│   - DB::*, File::*, Http::*          │  │ • Dispatched via Task::queue │
│     -> Suspend fiber & Tokio await   │  │ • Completely isolated from   │
│ • Non-Pausing Primitives:            │  │   latency-critical workers   │
│   - ADB::*, AFile::*, AHttp::*       │  │                              │
│     -> Never pause current fiber!    │  │                              │
│ • State Reset: Clear globals/statics;│  │                              │
│   Retain `singleton` classes/vars.   │  │                              │
└──────────────────────────────────────┘  └──────────────────────────────┘
```

---

## 2. Architectural Audit: What is Added, Subtracted & Retained

Comparing the previous draft against the Swoole-grade stackful coroutine architecture:

### 2.1 Subtracted / Removed Items

| Subtracted Feature | Why It Is Removed |
|---|---|
| **Direct `SO_REUSEPORT` Worker Accept** | Subtracted. Workers should not handle raw TCP handshakes or slow HTTP clients. The **Master Process Reactor Threads** handle connection acceptance, TLS, and framing, shielding workers from Slowloris attacks. |
| **Two-Thread-Per-Request Model** | Subtracted. Replaced entirely by **Stackful Fibers (`corosensei`)**. A single worker process runs thousands of concurrent request fibers cooperatively on one OS thread with zero context-switching syscall overhead. |
| **502 Quota Breaches & Next-Hit Routing** | Subtracted. In Swoole, because I/O is non-blocking and automatically yields the fiber, slow DB queries or network calls do *not* block the worker or other requests. There is no need to abort with 502 or push routes to a sidecar! |
| **Dynamic Fair-Share Quota (`max_memory / requests`)** | Subtracted. Unstable and prone to cascading failure feedback loops. Replaced by fixed per-request limits and process-level admission control. |
| **Mid-Flight Socket Transfers (`SCM_RIGHTS`)** | Subtracted. Never migrate running request stacks mid-flight. |
| **Stackless VM State Machines** | Subtracted. RHP code contains deep call stacks and recursion. Converting to stackless async state machines is complex and slow; `corosensei` gives true native stackful switching. |

---

### 2.2 Retained Core Features

| Retained Feature | Technical Value & Rationale |
|---|---|
| **RequestArena with $O(1)$ Reset** | Instantaneous bulk deallocation of request data with zero GC pauses. Retained with a chunk cache limit (`MAX_CACHED_CHUNKS = 32`) to prevent memory bloat. |
| **Unified Single Binary (`rhp`)** | Master, Manager, Worker, and Task Worker processes all spawn via `current_exe()` re-exec, guaranteeing identical JIT compilation flags and ABI matching. |
| **Native `\rhp\http\Request` & `Response`** | High-performance C/Rust FFI primitives backed directly by the request arena, avoiding PSR-7 marshalling bottlenecks. |
| **Centralized Log Pool** | Non-blocking ring buffer in workers draining to a dedicated log collector in the Master process. |
| **Worker Recycling (`max_requests`)** | Proven insurance from Swoole and RoadRunner against native memory fragmentation and memory leaks. |

---

### 2.3 Added Swoole Architectural Features

| Added Feature | Technical Specification & Implementation |
|---|---|
| **3-Tier Hierarchy (Master $\to$ Manager $\to$ Workers)** | Master owns Reactor Threads; Manager owns process lifecycle and crash recovery; Workers execute RHP code. |
| **Stackful Fibers (`corosensei`)** | Every HTTP request and async task runs on its own isolated stackful fiber. Instant register swap on yield; call stack preserved across I/O suspension. |
| **Transparent Synchronous I/O Hooks** | `DB::*`, `File::*`, `Http::*`, `sleep()`: Standard synchronous syntax in PHP/RHP code automatically suspends the current fiber back to Tokio, running Rust `await` under the hood. |
| **Asynchronous Non-Pausing Primitives (`ADB`, `AFile`, `AHttp`)** | Dedicated helper APIs that do **not** pause the calling request fiber. They run as background Tokio futures or Task Worker jobs. |
| **Singleton Lifecycle Boundaries** | Classes and variables declared with `singleton` persist across requests in `EngineState`. All standard globals, non-singleton static properties, and `RequestArena` are wiped clean in $O(1)$ after every request. |
| **Dedicated Task Worker Pool** | Swoole Task Worker pool for heavy CPU-bound tasks, post-response `Future::defer` draining, and explicit background job offloading. |

---

## 3. Deep-Dive: Execution Engine & Coroutine Architecture

### 3.1 The Three Process Tiers

#### Tier 1: Master Process (Network Level)
* Spawns a pool of **Reactor Threads** (heavy OS threads running Tokio multi-threaded runtime or raw `epoll`/`kqueue`).
* Listens on the public TCP socket (e.g. `:8080`), accepts incoming connections, handles TLS encryption/decryption, and parses HTTP/1.1 and HTTP/2 frames.
* Buffers incoming payloads and passes complete request descriptors down to Worker Processes via Unix domain sockets / shared memory channels.
* Passes outgoing response buffers from Workers back to clients.

#### Tier 2: Manager Process (Supervision Level)
* Forked by the Master process before any threads are spawned (maintaining POSIX fork safety).
* Holds zero client sockets and processes zero HTTP requests.
* Supervises $N$ Worker processes and $M$ Task Worker processes:
  - If a Worker crashes or hits a panic, Manager reaps it via `SIGCHLD` and immediately forks a clean replacement.
  - On `SIGUSR1`, conducts rolling zero-downtime worker reloads.
  - Monitors worker memory (RSS) and lifetime request count for bounded recycling.

#### Tier 3: Worker Processes (PHP/RHP Coroutine Level)
* Runs a single-threaded Tokio event loop (`current_thread` + `LocalSet`).
* Manages thousands of concurrent request coroutines using **`corosensei` Stackful Fibers**.
* Houses the RHP Register Bytecode VM and Cranelift JIT engine.

---

### 3.2 Stackful Fibers (`corosensei`) for Every HTTP Request

Unlike stackless async/await, stackful fibers allow RHP code to yield from **arbitrary nested function calls** without modifying method signatures or contaminating the language with `async`/`await` keywords:

```rust
use corosensei::{Coroutine, Yielder};

/// Represents an active HTTP request fiber inside a worker process.
pub struct RequestFiber {
    pub id: u64,
    pub arena: RequestArena,
    pub coroutine: Coroutine<ResumeSignal, YieldReason, ()>,
}

/// Signal sent to resume a suspended fiber.
pub enum ResumeSignal {
    IoComplete(Result<PhpValue, RuntimeError>),
    YieldTick,
}

/// Reason why a fiber surrendered CPU back to the worker scheduler.
pub enum YieldReason {
    WaitingIo(IoOperation),
    CooperativeYield,
    Completed(RawResponse),
    Failed(RuntimeError),
}
```

```text
[HTTP Request Arrives] ──► Worker Scheduler spawns `corosensei` Fiber
                                 │
                                 ▼
                     [Fiber Call Stack]
                     handleRequest()
                       └─► authenticateMiddleware()
                             └─► UserController::show()
                                   └─► DB::find('users', 42)  <-- Transparent I/O Hook
                                         │
                                         ▼
                     Yielder::suspend(WaitingIo(DbQuery))
                                         │
              ┌──────────────────────────┴──────────────────────────┐
              ▼ (CPU Context Switch: Save $SP, Restore Scheduler)   ▼
    [Fiber Stack Suspended]                                [Worker Event Loop]
    • Preserves local variables & call frames             • Schedules other requests
    • Memory untouched in RequestArena                    • Tokio executes async DB query
              │                                                     │
              │ (Tokio DB Query Completes)                          │
              └──────────────────────────┬──────────────────────────┘
                                         ▼
                     Scheduler calls Coroutine::resume(IoComplete(row))
                                         │
                                         ▼
                     [Fiber Call Stack Resumes Instantly]
                     `$user` assigned; response rendered; 200 OK flushed!
```

---

### 3.3 Transparent I/O Hooks vs. Non-Pausing Primitives

RHP introduces a clean, deliberate separation between **automatic fiber suspension** and **non-pausing background asynchronous execution**:

```text
┌────────────────────────────────────────────────────────────────────────┐
│ STANDARD I/O (PAUSES CURRENT FIBER)                                    │
│ Syntax:   $user = DB::find('users', 1);                                │
│           $html = File::read('template.html');                         │
│           $res  = Http::get('https://api.internal/service');           │
│ Behavior: Transparently suspends current fiber via `corosensei`.       │
│           Runs Rust Tokio `await` under the hood.                      │
│           Worker switches to other requests; resumes fiber when done.  │
│           Linear, sequential, zero async/await syntax needed.          │
├────────────────────────────────────────────────────────────────────────┤
│ ASYNCHRONOUS PRIMITIVES (NEVER PAUSES CURRENT FIBER)                   │
│ Syntax:   $future = ADB::find('users', 1);                             │
│           AFile::write('audit.log', $payload);                         │
│           $httpFuture = AHttp::post('https://analytics.com', $event);  │
│ Behavior: Current request fiber does NOT pause!                        │
│           Dispatches a non-blocking detached Tokio task or queues to a │
│           Task Worker immediately. Calling code continues to run.      │
│           Can be awaited later via `await($future)` or left fire-and-  │
│           forget / queued for post-response defer execution.           │
└────────────────────────────────────────────────────────────────────────┘
```

---

### 3.4 Singleton Persistence vs. Per-Request Memory Cleanup

RHP strictly preserves memory safety while supporting high-performance persistent services:

```text
Incoming Request ──► Worker grabs RequestArena from BoundedArenaPool
                           │
       ┌───────────────────┴───────────────────┐
       ▼                                       ▼
[Singleton Services]                   [Per-Request Execution]
• Declared via `singleton` keyword     • Request superglobals ($_GET, $_POST)
• Stored in immutable `EngineState`    • Local variables & controller instances
• Retained across all requests         • Request-scoped allocations
• Static connections & pools           • Non-singleton static properties
       │                                       │
       │                                       ▼
       │                              [Response Flushed to Socket]
       │                                       │
       │                              [Future::defer Queue Executed]
       │                                       │
       │                              [O(1) RequestArena Bulk Reset]
       │                              • Zero GC tracing
       │                              • Bulk pointer reset to 0
       ▼                              • Non-singleton statics wiped
[Retained Pristine for Next Request]
```

1. **`singleton` Classes & Variables:**
   - Classes declared with `singleton class AppService { ... }` and variables registered in the global container survive across requests.
   - Initialized during the boot phase and stored in `Arc<EngineState>`.
2. **Per-Request Isolation:**
   - Standard global variables, non-singleton static properties, route parameters, and request objects are allocated directly inside the request's `RequestArena`.
   - Upon response flush and `Future::defer` draining, the `RequestArena` resets in a single CPU instruction ($O(1)$). Non-singleton statics revert to their pristine state. Zero memory leaks across millions of requests.

---

### 3.5 Dedicated Task Worker Pool

Following Swoole's Task Worker model:
* **Separation of Concerns:**
  - **Standard Workers:** Dedicated strictly to fast, interactive HTTP requests running on stackful coroutines.
  - **Task Workers:** Separate OS processes dedicated to heavy, blocking, or asynchronous background jobs.
* **Dispatch Mechanics:**
  - Standard workers can offload heavy jobs via `Task::dispatch($callable, $data)`.
  - The Task Worker executes the task in the background, freeing the Standard Worker immediately.
  - Standard Worker can choose to wait for the task result via coroutine suspension or let it run completely detached.
* **Post-Response Defer Draining:**
  - Callbacks registered via `Future::defer(fn() => ...)` (e.g. audit logging, email dispatch) can either run immediately after response flush in the worker or be handed off to Task Workers to achieve minimum TTFB (Time to First Byte).

---

### 3.6 Fiber Cancellation & Client Disconnect Unwinding (The Safe Unwind Protocol)

In a stackful coroutine environment, forcefully destroying a suspended fiber externally is strictly prohibited because it causes memory leaks, abandoned native locks, and unclosed file descriptors. RHPM implements a deterministic cancellation unwinding protocol:

1. **Disconnect & Timeout Detection (Master Reactor):**
   - The Master Reactor thread detects early client disconnection (`POLLHUP`, `read() -> 0`, or connection reset) or when the per-request deadline timer expires.
   - Master dispatches a high-priority cancellation frame over IPC to the assigned Worker: `IpcMessage::CancelRequest { request_id, reason }`.
2. **Deterministic Fiber Resumption:**
   - The Worker's fiber scheduler aborts the pending Tokio I/O future (`DB`, `File`, `Http`) and immediately resumes the suspended `corosensei` fiber with `ResumeSignal::Cancelled(reason)`.
3. **Structured Stack Unwinding:**
   - Inside `suspend_for_io`, the runtime raises a special `CancellationException`.
   - The RHP call stack unwinds normally: all local variables are dropped, and all userland `finally` and `defer` cleanup blocks execute.
   - Once unwinding hits the fiber root, the fiber terminates cleanly, returning the coroutine to the thread-local stack recycling pool and resetting the `RequestArena` in $O(1)$.

```text
[Client Disconnect / Timeout]
             │
             ▼
[Master Reactor detects POLLHUP] ──► Dispatches CancelRequest { id } via IPC
                                                    │
                                                    ▼
                                    [Worker Event Loop receives Cancel]
                                    • Aborts pending Tokio future
                                    • Resumes fiber with ResumeSignal::Cancelled
                                                    │
                                                    ▼
                                    [suspend_for_io converts to CancellationException]
                                    • Unwinds fiber call frames
                                    • Executes `defer` & `finally` handlers
                                    • Native drops run cleanly
                                                    │
                                                    ▼
                                    [Fiber terminates at root -> Stack recycled to pool]
```

---

### 3.7 Master-Worker Zero-Copy IPC & Large Body Streaming (`memfd` / Channels)

Standard Unix Domain Sockets (UDS) introduce a severe double-buffering bottleneck for large file uploads, video processing, or high-volume downloads (e.g., streaming 100MB files through `Client -> Master -> Worker -> Master -> Client`). RHPM introduces a hybrid two-tier IPC transfer pipeline:

1. **Small Payloads ($< 64\text{ KB}$):**
   - Transferred directly over non-blocking Unix Domain Socket datagrams/stream channels along with framed HTTP metadata.
2. **Large Payloads ($\ge 64\text{ KB}$):**
   - The Master Reactor buffers the request body directly into an anonymous Linux memory file (`memfd_create("rhp_payload", MFD_CLOEXEC)`).
   - Once populated, the `memfd` file descriptor is passed to the Worker process via `SCM_RIGHTS` over the control socket.
   - The Worker maps the memory descriptor directly into its virtual memory address space (zero-copy memory mapping), eliminating inter-process buffer copying.
3. **Streaming Responses (SSE / Chunked Downloads):**
   - For chunked streaming, the Worker writes directly to a shared memory ring buffer or passes outbound stream chunks through an IPC streaming pipe, allowing backpressure to propagate smoothly from client socket to worker fiber.

---

### 3.8 Long-Lived Protocol Arenas (WebSockets & Server-Sent Events)

The default RHP memory model assumes single-turn requests: `Request -> Process -> Flush -> O(1) Bulk Reset of RequestArena`. For long-lived connections (WebSockets, Server-Sent Events) that remain active for hours, resetting memory only upon disconnection causes severe unbounded memory bloat.

RHPM implements a **Two-Tier Arena Architecture** for long-lived protocols:

```text
[Long-Lived Connection: WebSocket / SSE]
                 │
  ┌──────────────┴──────────────┐
  ▼                             ▼
[Connection Arena]             [Message / Frame Arena]
• Allocated once on handshake  • Allocated per WebSocket frame or SSE tick
• Holds socket state, user     • Holds incoming frame buffer, decoded JSON,
  session, subscribed channels   and controller call allocations
• Persists for connection life • RESET IN O(1) IMMEDIATELY AFTER FRAME DISPATCH!
• Bounded footprint (< 8 KB)   • Zero accumulated memory bloat over 24+ hours
```

1. **Connection Arena (Session Scope):**
   - Allocated when the HTTP 101 Switching Protocols handshake succeeds.
   - Holds the persistent `WebSocketConnection` handle, authentication context, and channel subscription state.
2. **Message Arena (Frame Scope):**
   - Allocated per incoming WebSocket frame or SSE push iteration.
   - Request-scoped objects, parsed frame payloads, and temporary variables allocate into this arena.
   - Upon completing the frame handler (`onMessage`), the Message Arena is reset to 0 in $O(1)$, ensuring steady-state RSS memory over millions of messages.

---

### 3.9 Fiber Stack Virtual Memory Limits & Thread-Local Stack Recycling Pool

Spawning thousands of `corosensei` fibers with unbounded default 2MB OS stacks creates massive virtual memory bloat ($10,000 \times 2\text{ MB} = 20\text{ GB}$) and frequent `mmap`/`munmap` syscall overhead. RHPM regulates fiber stacks via a high-performance stack recycling pool:

1. **Bounded Stack Sizing:**
   - Default fiber stack size is capped at **128 KB – 256 KB** (configured via `runtime.fiber_stack_size_kb = 128` in `rhp.toml`).
   - A single OS guard page (`mprotect(PROT_NONE)`) is established at the bottom of each stack to immediately catch recursion overflows without corrupting adjacent memory.
2. **Thread-Local `FiberStackPool`:**
   - Stacks are **never** deallocated with `munmap` after every request.
   - Completed fibers return their allocated stacks to a thread-local pool (`Vec<Coroutine>`).
   - Subsequent requests pop a warm stack from the pool, eliminating kernel memory mapping calls and preserving L1/L2 CPU cache locality.
   - Pool capacity is bounded (e.g. `MAX_IDLE_STACKS = 512`); excess idle stacks are released to avoid holding inactive virtual address space.

---

### 3.10 Overload Admission Control & Backpressure

To prevent cascading collapse under sudden traffic surges, RHPM implements a two-tier admission control and early load shedding mechanism:

1. **Master Reactor Admission Gate (Early Shedding):**
   - The Master Process tracks in-flight requests and the pending IPC queue depth.
   - If `pending_requests >= max_queued_requests` (e.g., 8,192), Master sheds load immediately at the network edge:
     - Rejects incoming HTTP requests with **`503 Service Unavailable`** (or **`429 Too Many Requests`**) and a `Retry-After: 5` header.
     - Drops connection gracefully without touching the Worker processes or allocating worker memory.
2. **Worker Concurrency Ceiling (`max_active_fibers`):**
   - Each Worker process enforces a strict concurrency ceiling (`max_active_fibers`, default: 2,048).
   - When active fibers reach the limit, the Worker temporarily unregisters its IPC socket from its local Tokio `epoll` interest list.
   - This applies immediate backpressure to Master's IPC dispatch queue, signaling Master to route new requests to other workers or engage early shedding.

---

### 3.11 Runtime Introspection & Diagnostic Fiber Dump (`rhp status --dump`)

Debugging deadlocks, connection leaks, or hung requests across thousands of concurrent suspended fibers is impossible with external tools like `gdb` or `strace`. RHPM builds an in-memory live introspection registry into every worker:

1. **Lock-Free Fiber Registry:**
   - Every active fiber registers in a thread-local array upon spawn:
     ```rust
     pub struct ActiveFiberEntry {
         pub request_id: u64,
         pub uri: CompactString,
         pub started_at: Instant,
         pub current_state: FiberState, // Running | WaitingIo(kind, started_at) | Suspended
     }
     ```
2. **Diagnostic IPC Command:**
   - The Manager and Master processes expose a local Unix control socket (`/run/rhp.sock` or `rhp status --dump`).
   - When requested via CLI, Manager queries all Workers for their active fiber registry.
   - Produces an instantaneous structured JSON or ASCII table dump:
     ```text
     WORKER #1 [PID: 41203] (Active Fibers: 342)
     ├── Req #10921 | GET /api/v1/orders | Elapsed: 4.82s | WAITING: DB::query (postgres://...) [elapsed: 4.80s]
     ├── Req #10984 | POST /checkout     | Elapsed: 0.12s | RUNNING (Bytecode instruction 0x48FA)
     └── Req #11002 | GET /health        | Elapsed: 0.01s | WAITING: LocalTimer (sleep)
     ```
   - Instantly exposes stuck queries, hung external HTTP endpoints, or infinite loops in production.

---

## 4. Rust Implementation Architecture & Module Layout

### 4.1 Module Structure (`src/runtime/`)

```text
src/
└── runtime/
    ├── mod.rs                   // Runtime bootstrap, CLI hooks, public exports
    ├── config.rs                // RuntimeConfig (reactors, workers, task_workers, limits)
    │
    ├── master/                  // Tier 1: Master Process (Network Level)
    │   ├── mod.rs               // Master supervisor loop & signal orchestration
    │   ├── reactor.rs           // Reactor Thread pool (OS threads, epoll/kqueue)
    │   ├── tls.rs               // Non-blocking TLS termination
    │   ├── admission.rs         // Overload admission gate (queue depth, 503/429 shedding)
    │   └── ipc_dispatcher.rs    // Request distribution to Worker processes (UDS + memfd)
    │
    ├── manager/                 // Tier 2: Manager Process (Supervision Level)
    │   ├── mod.rs               // Manager process loop, SIGCHLD reaper
    │   ├── supervisor.rs        // Worker monitoring, health checks, auto-restart
    │   ├── reloader.rs          // Zero-downtime rolling reload (SIGUSR1)
    │   └── diagnostics.rs       // Master diagnostic aggregator (CLI rhp status/dump)
    │
    ├── worker/                  // Tier 3: Worker Processes (Coroutine Level)
    │   ├── mod.rs               // Worker process bootstrap & IPC listener
    │   ├── event_loop.rs        // Single-threaded Tokio LocalSet runner
    │   ├── fiber/               // Stackful Fiber Subsystem
    │   │   ├── mod.rs           // corosensei scheduler & context switching
    │   │   ├── pool.rs          // Thread-local stack recycling pool (FiberStackPool)
    │   │   ├── cancellation.rs  // ResumeSignal::Cancelled & stack unwinding protocol
    │   │   └── registry.rs      // Live fiber tracking for status/dump introspection
    │   ├── hooks.rs             // Transparent I/O hooks (DB, File, Http yielders)
    │   ├── async_helpers.rs     // Non-pausing ADB, AFile, AHttp primitives
    │   ├── singleton.rs         // Singleton retention vs per-request state cleaner
    │   └── recycling.rs         // max_requests & memory ceiling recycling
    │
    ├── task/                    // Task Worker Process Pool
    │   ├── mod.rs               // Task worker process bootstrap
    │   ├── queue.rs             // IPC task distribution queue
    │   └── executor.rs          // Background task & Future::defer execution
    │
    ├── arena/                   // Memory Subsystem
    │   ├── mod.rs               // RequestArena definition
    │   ├── pool.rs              // BoundedArenaPool with MAX_CACHED_CHUNKS cap
    │   └── long_lived.rs        // Two-Tier Arena: ConnectionArena + FrameArena (WS/SSE)
    │
    ├── logger/                  // Centralized Logging
    │   ├── mod.rs               // Worker ring buffer
    │   └── collector.rs         // Master process log collector & batch writer
    │
    └── http/                    // Native HTTP Engine
        ├── parser.rs            // Zero-copy HTTP framing (httparse)
        ├── request.rs           // Native \rhp\http\Request class bindings
        └── response.rs          // Native \rhp\http\Response class bindings
```

---

### 4.2 Stackful Coroutine Hook Implementation (`worker/fiber.rs` & `worker/hooks.rs`)

```rust
use corosensei::{Coroutine, Yielder};
use std::cell::RefCell;
use std::rc::Rc;

pub type FiberYielder = Yielder<ResumeSignal, YieldReason>;

thread_local! {
    static CURRENT_YIELDER: RefCell<Option<*mut FiberYielder>> = RefCell::new(None);
}

/// Helper to execute an I/O operation by suspending the current stackful fiber.
pub fn suspend_for_io<T: 'static, F>(io_kind: IoKind, async_op: F) -> Result<T, RuntimeError>
where
    F: std::future::Future<Output = Result<T, RuntimeError>> + 'static,
{
    CURRENT_YIELDER.with(|cell| {
        let yielder_ptr = cell.borrow().expect("suspend_for_io called outside a Fiber context");
        let yielder = unsafe { &mut *yielder_ptr };

        // 1. Surrender CPU back to worker scheduler with the I/O descriptor
        let resume_signal = yielder.suspend(YieldReason::WaitingIo(Box::new(async_op)));

        // 2. Scheduler resumes fiber once Tokio future resolves or is cancelled
        match resume_signal {
            ResumeSignal::IoComplete(boxed_val) => {
                let downcasted = boxed_val.downcast::<T>().map_err(|_| {
                    RuntimeError::custom("Type mismatch on fiber resumption")
                })?;
                Ok(*downcasted)
            }
            ResumeSignal::Cancelled(reason) => {
                // Initiates safe stack unwinding; userland defer/finally blocks will execute
                Err(RuntimeError::RequestCancelled(reason))
            }
            ResumeSignal::Abort(err) => Err(err),
            _ => Err(RuntimeError::custom("Unexpected resume signal")),
        }
    })
}
```

---

### 4.3 Non-Pausing Primitives (`worker/async_helpers.rs`)

```rust
/// Implementation of ADB, AFile, AHttp helpers that do NOT suspend the fiber.
pub struct AsyncHelpers;

impl AsyncHelpers {
    /// ADB::query: Spawns an async query on the worker's local event loop without suspending!
    pub fn adb_query(sql: String, params: Vec<PhpValue>) -> PhpValue {
        let (tx, rx) = tokio::sync::oneshot::channel();
        
        // Spawn background task onto the local single-threaded Tokio event loop
        tokio::task::spawn_local(async move {
            let res = execute_db_query(sql, params).await;
            let _ = tx.send(res);
        });

        // Returns an RHP Future object immediately to the calling script
        PhpValue::Object(VmObject::from_future(rx))
    }

    /// AFile::write: Dispatches fire-and-forget file write without pausing!
    pub fn afile_write(path: String, content: Vec<u8>) {
        tokio::task::spawn_local(async move {
            let _ = tokio::fs::write(path, content).await;
        });
    }
}
```

---

## 5. Phased Implementation Roadmap

```text
┌────────────────────────────────────────────────────────────────────────┐
│                        IMPLEMENTATION ROADMAP                          │
├────────────────────────────────────────────────────────────────────────┤
│ PHASE 1: STACKFUL FIBER CORE SPIKE & corosensei INTEGRATION            │
│ • Add corosensei to Cargo.toml                                         │
│ • Implement coroutine yielder context (suspend_for_io / resume)        │
│ • Build FiberStackPool thread-local stack recycler (128KB guard-pages) │
│ • Implement ResumeSignal::Cancelled structured stack unwinding         │
│ • Benchmark fiber switch latency (target: < 25 nanoseconds)            │
├────────────────────────────────────────────────────────────────────────┤
│ PHASE 2: WORKER COROUTINE EVENT LOOP & I/O HOOKS                       │
│ • Wire single-threaded Tokio LocalSet event loop into Worker Process   │
│ • Implement transparent I/O hooks: DB::query(), File::read(), Http::get│
│ • Implement non-pausing ADB, AFile, AHttp helpers                      │
│ • Integrate RequestArena per-request allocation & O(1) bulk reset      │
│ • Integrate Two-Tier Arenas (ConnectionArena vs FrameArena for WS/SSE) │
│ • Wire in-memory ActiveFiberEntry registry for live status tracking    │
├────────────────────────────────────────────────────────────────────────┤
│ PHASE 3: MASTER PROCESS REACTOR THREADS & PROTOCOL FRAMING             │
│ • Build Master Process with multi-threaded Reactor Thread pool         │
│ • Implement non-blocking epoll/kqueue network accept & TLS             │
│ • Implement httparse zero-copy request framing                         │
│ • Build Master Admission Gate (queue limits, early 503/429 shedding)   │
│ • Setup UDS control IPC + memfd SCM_RIGHTS zero-copy body transfer     │
├────────────────────────────────────────────────────────────────────────┤
│ PHASE 4: MANAGER PROCESS & TASK WORKER POOL                            │
│ • Implement Manager process for supervision and SIGCHLD auto-restart   │
│ • Implement Task Worker process pool for background jobs               │
│ • Implement worker recycling (max_requests & RSS memory ceilings)      │
│ • Implement rolling zero-downtime reloads (SIGUSR1)                    │
│ • Implement CLI diagnostic socket: `rhp status --dump`                 │
├────────────────────────────────────────────────────────────────────────┤
│ PHASE 5: SINGLETON PERSISTENCE, BENCHMARKS & HARDENING                 │
│ • Enforce singleton retention vs per-request state wiping              │
│ • Benchmark against Swoole and FrankenPHP under high concurrency       │
│ • Wire into unified CLI: `rhp serve [SCRIPT] --workers=N --reactors=M` │
└────────────────────────────────────────────────────────────────────────┘
```

---

## 6. Verification Gates

1. **Fiber Switching & Depth Test:**
   - Execute a 1,000-deep recursive RHP function that executes a `DB::query()` at the deepest stack frame.
   - Verify `corosensei` suspends and resumes cleanly with 100% stack frame preservation and zero memory corruption.
2. **Transparent vs. Non-Pausing Benchmark:**
   - Script A runs 10 sequential `DB::query()` calls $\to$ verifies fiber yields 10 times, event loop serves other requests, total time equals sum of queries.
   - Script B runs 10 `ADB::query()` calls $\to$ verifies fiber does *not* pause, all 10 queries fire concurrently, total time equals max of single query.
3. **Client Disconnect & Cancellation Unwinding Test:**
   - Client sends request, script suspends on a slow DB query, client abruptly drops TCP socket.
   - Master detects `POLLHUP`, signals Worker $\to$ verify Worker resumes fiber with `ResumeSignal::Cancelled`, triggers all `finally`/`defer` blocks, unwinds stack, recycles fiber to `FiberStackPool`, and causes zero resource leaks.
4. **Zero-Copy Payload Transfer Benchmark:**
   - Upload 100MB file via multipart POST $\to$ verify Master uses `memfd` and passes fd to Worker without intermediate socket buffer copying, maintaining $< 10\text{ms}$ handoff latency.
5. **Overload Shedding Gate Test:**
   - Saturate server past `max_queued_requests` $\to$ verify Master immediately returns `503 Service Unavailable` with `Retry-After: 5` without creating worker fibers or elevating worker RSS.
6. **Long-Lived Protocol Memory Leak Test:**
   - Maintain a WebSocket connection for 100,000 messages $\to$ verify MessageArena resets after each frame and worker RSS remains completely flat.
7. **Diagnostic Introspection Dump Test:**
   - Execute `rhp status --dump` while requests are suspended on DB queries $\to$ verify output displays in-flight fibers, elapsed durations, and exact wait states.
8. **Singleton Retention vs. State Leak Test:**
   - Request 1 mutates a `singleton` class property and a standard global variable `$_SERVER['TEST']`.
   - Request 2 verifies the `singleton` mutation is preserved, while `$_SERVER['TEST']` and the `RequestArena` are completely reset.
9. **Crash Recovery & Zero Downtime Reload:**
   - Inject a panic/segfault in a Worker process $\to$ verify Manager reaps it and forks a replacement in $< 5\text{ms}$ without disconnecting clients on the Master Reactor.

