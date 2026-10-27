# RHP Architecture: The Three-Layer Separation of Concerns

> **Architectural Principle:** The system is divided into three distinct layers: **Language**, **Runtime**, and **Framework**. Each layer has strict ownership boundaries. Userland code should never see process management, and the language layer must never be coupled to HTTP or web concepts.

---

## 1. The Tri-Layer Architecture Overview

| Layer | Primary Entity | Owns | Litmus Test |
|---|---|---|---|
| **Layer 1: Language** | `rhp` Compiler & VM | Everything that defines **what code means** | Can execute a script via CLI (`rhp script.rhp`) with zero servers, zero workers, and no framework? |
| **Layer 2: Runtime** | `rhpm` + Built-in Host Server | Everything about **processes, I/O, and lifecycle** | Manages OS sockets, processes, forks, and network events without dictating application structure. |
| **Layer 3: Framework** | Laravel Rocket | Everything about **how an application is structured** | Provides application conventions, routing DSL, DI container, and controllers on top of language primitives. |

```text
┌────────────────────────────────────────────────────────────────────────┐
│                   LAYER 3: FRAMEWORK (Laravel Rocket)                  │
│  - Service Container & Providers       - Routing DSL & Controllers     │
│  - Middleware Pipeline                 - Fluent Query Builder (ORM)    │
│  - Facades (DB, Route, Log)            - boot() vs handle() Conventions│
└───────────────────────────────────┬────────────────────────────────────┘
                                    │ Runs on top of
┌───────────────────────────────────▼────────────────────────────────────┐
│                    LAYER 2: RUNTIME (rhpm & Host Server)               │
│  - Master Supervisor Process           - Pre-forked Worker Pool        │
│  - Socket Binding (SO_REUSEPORT)       - HTTP Accept & Parsing Loop    │
│  - Post-Fork Tokio / sqlx Init         - Request Dispatch & Arenas     │
│  - Future::defer Flush Execution       - Worker Respawn & Hot Reload   │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │ Executes via
┌───────────────────────────────────▼────────────────────────────────────┐
│                    LAYER 1: LANGUAGE (rhp Compiler & VM)               │
│  - Lexer, Pratt Parser, Type Checker   - Register Bytecode VM / JIT    │
│  - Types: PhpValue, Int, Float, Collections (list, dict, set)          │
│  - Primitive stdlib: Str, Arr, DB handle- Future primitive (run, defer)│
│  - Rust FFI & Crate Interop            - Memory Safety & Crash Boundary│
└────────────────────────────────────────────────────────────────────────┘
```

---

## 2. Layer 1: The Language (`rhp` Compiler & VM)

The Language layer is responsible for the syntax, type system, intermediate representation, bytecode execution, and language-level memory safety.

### Responsibilities & Capabilities
- **Lexing, Parsing & AST:** Deterministic Logos DFA tokenization and recursive Pratt parser generating typed AST nodes.
- **Type System & Gradual Typing:**
  - Fast-path unboxed scalar types (`int`, `float`, `bool`, `string`) lowered directly to Cranelift IR.
  - Subtyped collections: `list` (dense vector), `dict` (insertion-ordered IndexMap), and `set` (unique dense collection).
  - Mandatory function/method boundary signatures.
- **Register Bytecode VM & Cranelift JIT:**
  - Linear IR module lowering with 3-operand register allocation.
  - Direct execution loop with runtime boundary contract verification.
- **Primitive Standard Library:**
  - `Str`: Binary-safe UTF-8 string manipulation.
  - `Arr`: Immutable collection pipelines with Copy-on-Write (CoW) detachment.
  - `Future`: Primitive asynchronous primitives (`Future::run`, `Future::defer`).
  - Low-level database handle interface.
- **Rust FFI & Native Extension Interop:** Direct compilation and linking of `.rs` files or static Rust helpers with zero C-FFI overhead.
- **Fault Containment Guarantee:** A panic, arithmetic failure (e.g. division by zero), or uncaught exception aborts only the current evaluation scope/request, never crashing the host engine.

### The CLI Test (Litmus Test for Language Purity)
> **The Test:** You must be able to run a pure computation script from the CLI with zero workers, no HTTP server, and no framework:
> ```bash
> rhp compute.rhp
> ```
> If `compute.rhp` requires an active server, expects HTTP superglobals, or fails because no process manager is running, an abstraction from Layer 2 or Layer 3 has contaminated the Language layer.

---

## 3. Layer 2: The Runtime (`rhpm` + Host Server)

The Runtime layer owns operating system resources, processes, network concurrency, and execution scheduling. It acts as the process supervisor and I/O host.

### Responsibilities & Capabilities
- **Master Process Lifecycle:**
  - Boots once: parses configuration (`rhp.toml`), binds listening TCP sockets, and pre-compiles application bytecode.
  - Enforces the **Pre-Fork Single-Thread Invariant**: strictly single-threaded before forking (no Tokio runtime, thread pools, or database connections before fork) to prevent POSIX `fork()` deadlocks.
- **Pre-forked Worker Pool:**
  - Forks $N$ persistent worker processes at startup (leveraging kernel MMU Copy-on-Write).
  - Catches `SIGCHLD` when a worker terminates or hits a memory limit and immediately respawns a clean replacement.
- **Post-Fork Resource Initialization:**
  - Inside each child worker: spins up its independent Tokio async runtime and initializes its connection pools (`sqlx`, Redis).
- **HTTP I/O & Request Dispatching:**
  - Accepts connections on the shared socket, parses incoming HTTP wire protocols, and assigns each request to an isolated request bump arena (`RequestArena`).
  - Guarantees instant $O(1)$ arena reclamation upon request completion.
- **Hot Reloading:**
  - File-watching engine monitors source files; issues rolling restarts to workers (spawning new workers and sending `SIGQUIT`/`SIGTERM` to drain existing ones).
- **Process Management (`rhpm`):**
  - Manages multiple instances, background daemon workers, and routes slow, CPU-intensive, or memory-heavy endpoints to dedicated sidecar processes.
- **Post-Response Defer Execution:**
  - Tracks tasks queued via `Future::defer`. Flushes the HTTP response to the client socket *first*, then executes the deferred task queue inside the worker before resetting the request arena.

---

## 4. Layer 3: The Framework (Laravel Rocket)

The Framework layer provides the developer-facing application architecture, design patterns, conventions, and high-level ergonomic APIs.

### Responsibilities & Capabilities
- **Inversion of Control (IoC) Container & Service Providers:**
  - Service registration, dependency injection, and contract-to-implementation binding.
  - Lifecycle providers that configure packages and singletons.
- **Routing DSL & Controllers:**
  - Declarative route declarations (`Route::get('/', [UserController::class, 'index'])`).
  - Named routes, route parameters, grouping, and resource controllers.
- **Middleware Pipeline:**
  - Onion-architecture request/response filters (Authentication, CSRF verification, CORS, rate limiting).
- **High-Level Facades:**
  - Expressive static facades (`DB::table('users')`, `Route::post()`, `Cache::remember()`) proxying to container services and low-level drivers.
- **Fluent Query Builder & ORM:**
  - Active-record or data-mapper models built on top of the language's raw database handle.
- **Lifecycle Conventions (`boot()` vs `handle()`):**
  - Defines the organizational conventions for what code belongs in the boot phase (configuration, route registration, container bindings) versus the request phase (controllers, middleware, query execution).

---

## 5. Gray Areas & Deliberate Architectural Decisions

When designing high-performance systems, certain capabilities sit on the boundary between layers. RHP resolves these with strict contracts:

### 1. `Future::defer` Contract
* **The Problem:** The language provides the syntax (`Future::defer(fn() => ...)`), but only the runtime knows when the HTTP socket has flushed its bytes to the client.
* **The Contract:**
  1. **Language Layer:** Exposes `Future::defer(callable $fn): void`. The VM appends the callable to the current request context's defer queue.
  2. **Runtime Layer:** Owns the execution trigger. The runtime flushes the HTTP response over the wire, then drains the defer queue sequentially.
  3. **Fault Semantics:** `Future::defer` is **at-most-once**. If a worker process experiences an abrupt power loss or SIGKILL while executing deferred work, the task is not retried. Work that requires transactional persistence must be queued to Redis/database via a background job queue.
  4. **Shutdown Draining:** On graceful shutdown (`SIGTERM`), workers pause accepting new requests and drain all pending deferred queues before terminating.

### 2. Request & Response Objects
* **The Problem:** Tying request/response classes to the compiler ties the programming language directly to HTTP.
* **The Contract:**
  1. **Language/Runtime:** Operates on raw, low-level data structures:
     - `RawRequest { method: String, uri: String, headers: IndexMap, body: Vec<u8> }`
     - `RawResponse { status: u16, headers: IndexMap, body: Vec<u8> }`
  2. **Framework (Laravel Rocket):** Wraps raw buffers into rich, ergonomic PSR-style or Laravel objects:
     - `$request->validated()`, `$request->user()`, `$request->file('avatar')`
     - `response()->json([...])`, `redirect()->back()`

### 3. Process Model Visibility (Zero Forking Concepts in Userland)
* **The Problem:** If userland code ever references `pid`, `fork()`, "master", "child", or "worker", the abstraction has leaked.
* **The Contract:**
  1. The language exposes **only lifecycle phase hooks**:
     - **Boot Phase:** Global scope of `index.rhp` and `Provider::register()`.
     - **Request Phase:** Inside route handlers, controllers, and middleware.
  2. The runtime maps these phases to processes (Master Supervisor vs Pre-forked Workers). Userland never knows whether code is running in a thread, a forked worker, or a single-shot CLI process.

---

## 6. Request Lifecycle Flow Across the Three Layers

```text
1. [Runtime]   Master supervisor boots, loads index.rhp, binds TCP:8080.
2. [Language]  Pratt parser & compiler build typed AST/bytecode. Master stays single-threaded.
3. [Framework] Boot phase executes: Service providers register routes, bindings, and middleware.
4. [Runtime]   Master forks N persistent worker processes.
5. [Runtime]   (Post-Fork) Worker initializes Tokio runtime and sqlx connection pool.
6. [Runtime]   Worker accepts TCP connection; parses HTTP into RawRequest.
7. [Runtime]   Worker initializes RequestArena (bump pointer = 0).
8. [Framework] Wraps RawRequest into Request object; dispatches through Middleware pipeline.
9. [Framework] Controller executes; runs query via DB facade; registers Future::defer hook.
10. [Framework] Returns Response object; lowered into RawResponse bytes.
11. [Runtime]  Worker writes HTTP response to client socket and flushes.
12. [Runtime]  Worker executes registered Future::defer callbacks (e.g. Log::info).
13. [Runtime]  Worker drops RequestArena in O(1) bulk reset (zero-GC).
```

---

## 7. Anti-Patterns & Boundary Violations

| Violation | Why it Fails | Correct Separation |
|---|---|---|
| **Compiler understands HTTP headers** | Compilers should only understand types, ASTs, and bytecodes. Coupling to HTTP breaks CLI tools, queue consumers, and desktop apps. | Runtime parses HTTP headers into raw maps; framework exposes `$request->header()`. |
| **Parent process connects to DB at boot** | POSIX `fork()` copies only the calling thread. Mutexes locked in the parent by Tokio or sqlx pool cause permanent child deadlocks. | Language forbids `DB::*` in boot phase; runtime initializes DB pool post-fork in each worker. |
| **Framework writes the socket accept loop** | Forces the application developer to maintain low-level network loops (like Swoole/Workerman scripts in legacy PHP). | Runtime owns the socket and server lifecycle; framework only writes request handlers. |
| **Language exposes `isWorker()` / `getPid()`** | Couples application logic to a specific OS process model, preventing future serverless, WebAssembly, or embedded targets. | Framework uses lifecycle conventions (`boot()` vs request handlers); runtime maps them to workers. |
