# RHP Engineering Roadmap & Architectural Implementation Plan (TODO)

This document outlines the engineering roadmap and architectural improvements for **RHP**, incorporating high-performance design patterns inspired by the **Bun Rust codebase** (`oven-sh/bun`: `bun_js_parser`, `bun_ast`, `bun_collections`, `bun_jsc`, and `bun_alloc`).

---

## 1. Syntax & Core Language Rules

- [ ] **Native Script Syntax (No Opening/Closing Tags):**
  - `.rhp` files are treated as direct source code files.
  - Files **do not** start with `<?php` and **do not** close with `?>`. Parsing begins immediately with the first token of the source file.
  - Any occurrence of `<?php` or `?>` should be rejected or treated as an invalid token / syntax error.

- [ ] **Strict Subtyped Collection Invariants (`Arr::list`):**
  - `Arr::list` is strictly sequential and indexed by contiguous integers.
  - Attempting to set or append a string/custom key (e.g. `$arr["foo"] = 1;` or `$arr['key'] = $val;`) on an `Arr::list` must fail with an explicit runtime/compile error:
    ```text
    cannot use custom key for list. use dict or hashMap instead
    ```
  - Disallow automatic implicit type coercion or promotion from `Arr::list` to `dict` during key assignment unless using the generic permissive parent `array` type.

- [x] **Prefix Variable Type Declarations Only (`int $x = 10;`):**
  - Typed variable declarations support **prefix syntax only**: `int $x = 10;`, `string $name = "Alice";`, `float $pi = 3.14;`, `bool $flag = true;`, or `int $x;`.
  - Colon-based type declarations (`$x: int = 10;` or `$x: int;`) are **explicitly disallowed** and produce a compile-time parse error:
    ```text
    Colon type syntax '$x: Type' is not supported. Use prefix type syntax 'Type $x' instead (e.g., 'int $x = 10;').
    ```
  - Applies consistently across local variables, class properties (`public int $x = 1;`), and parameter lists (`function foo(int $x)`).

- [x] **Functional Type Conversions Only (`int($x)`, `string($x)`):**
  - Type conversions must use functional conversion syntax: `int($x)`, `string($x)`, `float($x)`, `bool($x)`.
  - C-style / PHP-style casts (`(int)$x`, `(string)$x`, `(float)$x`, `(bool)$x`) are **not supported** and produce a compile-time parse error:
    ```text
    Type cast syntax '(int)' is not supported in RHP. Use functional conversion 'int(...)' instead (e.g. 'int($x)').
    ```

---

## 2. Phase-by-Phase Implementation Roadmap

### Phase 1: Frontend Lexer & AST Memory Allocation (Inspired by `bun_js_parser` & `bun_alloc`)

- [ ] **Handwritten SIMD Lexer with `memchr` (Replacing `logos` for Core Scanning):**
  - Transition from table DFA `logos` to a handwritten cursor scanner.
  - Use `memchr` / `memchr2` for SIMD-accelerated skipping of whitespace, newlines, and comments.
  - Implement precomputed ASCII lookup tables for identifier character validation (`ID_START`, `ID_CONTINUE`).
  - Seamlessly handle stateful tokenization (string interpolation, multi-line strings, heredocs) without `logos` callback overhead.

- [ ] **Arena-Based AST Allocation (`bumpalo` / `StoreSlice`):**
  - Implement bump-pointer arena allocation (matching Bun's `bun_alloc::Arena` and `oxc_allocator`).
  - Store AST node slices using arena-backed storage (`StoreSlice<T>`, `StoreStr`) instead of heap-scattered `Box<Expr>` and `Vec<Stmt>`.
  - Eliminate lifetime parameter `'a` propagation across AST structures.
  - Enable instant $O(1)$ bulk deallocation of the entire AST upon compilation completion.

- [ ] **String Interning (Atoms):**
  - Introduce an identifier interner (e.g. `lasso` or `string_cache`) for variable names, property names, and keywords.
  - Represent identifiers as compact 32-bit `Atom` (`u32`) handles for fast $O(1)$ comparisons.

- [ ] **Handwritten Recursive Descent + Pratt Expression Parser:**
  - Standardize on Recursive Descent for statements and class/function declarations.
  - Retain Top-Down Operator Precedence (Pratt) exclusively for arithmetic, logical, and ternary expressions.
  - Remove combinator parsing abstractions (`nom`/`combine` style) to eliminate closure allocation and call-stack bloat.

---

### Phase 2: Runtime Value Representation — 64-bit NaN Boxing (Inspired by `bun_jsc::JSValue`)

- [ ] **Replace 40-Byte `PhpValue` Enum with 64-Bit NaN-Boxed `Value(u64)`:**
  - Replace the current multi-word `PhpValue` enum with a single 8-byte IEEE-754 quiet-NaN encoded word (`#[repr(transparent)] struct Value(pub u64)`), identical to Bun's `bun_jsc::JSValue`.
  - **Bit-level encoding layout:**
    - **Double / Floats (`f64`):** Stored as raw IEEE-754 doubles.
    - **Standard Integers (`i32`):** `0xFFFE_0000_0000_0000 | (val as u32 as u64)`.
    - **Booleans (`bool`):** Tagged constants (`TRUE = 0x07`, `FALSE = 0x06`).
    - **Null / Undefined:** Dedicated bit patterns (`NULL = 0x02`, `UNDEFINED = 0x0A`).
    - **Heap Pointers (Strings, Objects, Collections):** 48-bit pointer packed into the low 48 bits with `NOT_CELL_MASK = 0xFFFE_0000_0000_0002`.
  - **Memory benefits:**
    - Reduces value size from 40 bytes to 8 bytes.
    - Fits 8 values per 64-byte CPU cache line (up from 1.6).
    - Register moves and parameter passing compile to a single CPU instruction (`mov rax, rbx`).
  - Represent 128-bit numbers (`i128`, `u128`) via boxed heap cells so the core value remains 8 bytes.

---

### Phase 3: High-Performance Subtyped Collections (Inspired by `bun_collections`)

- [ ] **Strict Collection Subtype Invariants:**
  - `Arr::list()`: Backed by a contiguous flat vector.
    - Throw `"cannot use custom key for list. use dict or hashMap instead"` when indexing with string keys.
  - `Arr::dict()`: Key-value dictionary retaining strict insertion ordering.
  - `Arr::set()`: Unique set collection with $O(1)$ membership checks.
  - `Arr::map()`: Universal dictionary supporting arbitrary complex keys.

- [ ] **Eliminate `RefCell` Overhead:**
  - Replace `Rc<RefCell<...>>` in collections with single-owner or explicit Copy-On-Write (COW) reference counters.
  - Eliminate runtime borrow counter checks on every hot-path read/write.

- [ ] **Fast Hash Tables with `hashbrown` & `raw-entry`:**
  - Switch from `std::collections::HashMap` (SipHash) to `hashbrown` with `AHash` / `wyhash`.
  - Utilize `raw-entry` probing (`get_hashed`, `put_hashed`) to hash strings once and reuse the hash across repeated lookups.

- [ ] **Packed Array Optimization:**
  - For sequential indexed collections, bypass hash calculation and store elements in flat contiguous memory.

---

### Phase 4: Intermediate Representation (IR) & SSA Lowering

- [ ] **Compact 32-Bit Instruction Encoding:**
  - Pack bytecode instructions into dense 32-bit words (e.g. `OP: 8 bits | DST: 8 bits | SRC1: 8 bits | SRC2: 8 bits`) or 64-bit fixed words.
  - Avoid large Rust enum payloads in the bytecode array to maximize L1 instruction cache ($L1i$) hit rates.

- [ ] **Out-of-Band Debug Span Line Tables:**
  - Remove `span` lookups from the hot execution path in `chunk.instructions`.
  - Maintain a separate out-of-band table (`Vec<(u32, Span)>`) queried exclusively on traps, errors, exceptions, and debugger stops.

- [ ] **Local SSA Optimization & Type Specialization:**
  - Lower AST to basic blocks with Static Single Assignment (SSA).
  - Statically infer unboxed operations for local variables that do not change type within function bodies (e.g. emit `AddI32` instead of polymorphic `AddVal`).

---

### Phase 5: Virtual Machine Execution Engine

- [ ] **Inner Loop Optimization & Pointer Pinning:**
  - Cache the active frame's instruction pointer (`*const OpCode`) and register window base (`*mut Value`) directly in local CPU registers.
  - Eliminate per-instruction frame array indexings (`self.frames[frame_idx]`).

- [ ] **Dense Match Jump Table Dispatch:**
  - Annotate `OpCode` with `#[repr(u8)]` and verify LLVM compiles the dispatch loop to an indirect jump table (`jmp [table + rdx*8]`).

- [ ] **Instruction Quickening & Inline Caches (IC):**
  - Dynamically rewrite polymorphic property/method access opcodes to specialized slot lookups (e.g. `GetProp` -> `GetPropSlot { offset }`) upon the first shape resolution.

---

### Phase 6: Just-In-Time (JIT) Pipeline with Cranelift

- [ ] **Calibrated Compilation Thresholds:**
  - Prevent premature compilation of short loops. Increase the Cranelift compilation threshold (e.g., 5,000–10,000 invocations) to ensure only long-lived hotspots are handed to the Cranelift backend.

- [ ] **On-Stack Replacement (OSR Entry & Exit):**
  - Implement OSR loop headers to transition execution from the bytecode interpreter into native Cranelift code mid-loop without returning.
  - Implement deoptimization bailout points (OSR Exit) that safely restore interpreter registers if dynamic runtime type assumptions fail.

---

### Phase 7: Gradual Type System & Boundaries

- [ ] **Strict Function & Method Boundary Contracts:**
  - Enforce explicit type signatures across public function and method boundaries.
  - Generate monomorphic calling convention stubs between typed functions to bypass value boxing/unboxing overhead.

- [ ] **Dynamic & Permissive Typing in Function Bodies:**
  - Retain ergonomic dynamic typing for untyped local variables within function bodies.
  - Prevent implicit type-shifting once a variable is explicitly annotated with a strict scalar or collection type hint.

---

### Phase 8: Concurrency, Parent Process & Facades

- [x] **Persistent Parent Process Architecture & Singleton Process Services:**
  - Parse and compile framework code once into immutable state (`Arc<EngineState>`), eliminating per-request bootstrap overhead.
  - Require `singleton` keyword for classes declared to run as part of the parent process.
  - Enforce invocation invariants: one can use both static and non-static methods of a singleton class within other singleton classes, but can only use static methods of a singleton class inside non-singleton classes.
  - Workers execute requests concurrently against shared immutable `Arc<EngineState>` without per-request bootstrap overhead.

- [x] **Worker Process Isolation & Concurrency (Step 14):**
  - Spawn multi-threaded worker tasks (`WorkerModel::MultiThreaded`) or process-level copy-on-write workers (`WorkerModel::ProcessCow` via Unix `fork()` and anonymous pipe IPC).
  - Guarantee Copy-on-Write (CoW) state isolation: mutations to static properties, globals, or superglobals (`$_GET`, `$_POST`, `$_SERVER`, `$_COOKIE`) inside a request VM do not leak to the parent or concurrent/subsequent requests.
  - Implement fault isolation and panic boundaries: unhandled runtime errors, division by zero, and panics are safely caught and return isolated error responses without crashing the worker pool or parent server.
  - Support high-throughput pre-compiled function calls (`WorkerPayload::Call`) with zero parsing overhead.
  - Worker pool builder, batch execution, throughput metrics (`WorkerPoolStats`), and graceful shutdown.

- [x] **Synchronous-by-Default Execution Model & Cooperative Fiber Concurrency (Step 16):**
  - Standard helpers and I/O calls (`File::read()`, `DB::table()`, `HTTP::get()`) run sequentially and blocking by default for linear developer ergonomics.
  - Local execution fibers (`Fiber`) track execution lifecycle (`Created`, `Running`, `Yielded`, `WaitingIo`, `Completed`, `Failed`) with high-resolution CPU and I/O duration metrics.
  - Native `Fiber` Tier 1 facade provides `Fiber::current()`, `Fiber::isFiber()`, `Fiber::yield()`, `Fiber::stats()`, `Fiber::context()`, and `Fiber::all()`.
  - Cooperative `FiberScheduler` coordinates multi-task fiber scheduling and non-blocking timer/IO yields without blocking underlying OS worker threads.
  - Worker pool requests automatically execute within isolated request fibers, populating `WorkerResponse::fiber_stats()`.

- [ ] **Epoll-Based Event Loop & Fiber I/O Scheduling:**
  - Implement OS kernel event-driven scheduling leveraging Linux `epoll` (`epoll_create1`, `epoll_ctl`, `epoll_wait`) with `kqueue` support for macOS/BSD.
  - Register network sockets, file descriptors, `timerfd`, and `eventfd` directly into an `epoll` interest list (`EPOLLIN`, `EPOLLOUT`, `EPOLLET`).
  - Park fibers in `WaitingIo` state without busy-polling or CPU spinning, immediately waking and resuming execution upon receiving kernel readiness notifications.
  - Seamlessly integrate with non-blocking I/O operations (`HTTP`, sockets, pipes, IPC) to achieve ultra-high concurrency with minimal memory and context switch overhead.

- [ ] **Lock-Free Asynchronous Logging:**
  - Offload `Log::info()` payloads over a lock-free `mpsc` ring buffer to a dedicated background logging worker.

- [ ] **Post-Response Defer Execution Hooks:**
  - Execute per-request `defer()` hooks strictly after the client response payload is flushed.

- [x] **Static Facades & Dynamic Macros:**
  - Enforce static class facades (`Str`, `Arr`, `File`, `HTTP`, `Log`, `DB`, `Int`, `Float`) with forbidden inheritance (`extends Arr`), allowing extension via dynamic macro registration (`Arr::macro(...)`).
  - Three-Tier Facade Architecture: Tier 1 (Native Engine Facades), Tier 2 (Custom Rust Plugins/Crates via AOT), and Tier 3 (Script-land Static Facades).
  - Engine-level `is_static_only` flag halting illegal instantiation attempts (`new Int()`, dynamic `new $cls()`) with fatal error at compile-time and runtime.

- [x] **Dart-Style Future Concurrency Core (`Future` Facade) (Step 19):**
  - Provide non-blocking background task execution via the native `Future` class (`Future::run`, `Future::delayed`, `Future::wait`, `Future::any`, `Future::value`, `Future::error`, `Future::defer`).
  - Directly accept closures (`fn() => ...`, `function() { ... }`) to evaluate parallel task joins (`Future::wait`) and races (`Future::any`) cleanly without redundant `->wait()` chains.
  - Full Dart-style fluent instance methods: `->wait()`, `->then()`, `->catch()`, `->isCompleted()`, `->hasError()`, `->value()`, `->error()`, and `->timeout()`.
  - Multi-threaded task execution with isolated VM sandbox, panic boundaries, upvalue capture, and fiber timer telemetry integration.

- [x] **Memory Ownership & Safety Enforcement (Step 20):**
  - Leverage Rust’s compile-time borrow checker and ownership model (`Send + Sync`) to guarantee absolute memory safety and eliminate data races across concurrent worker threads without relying on a traditional tracing garbage collector.
  - Zero tracing GC overhead: 0 stop-the-world pauses (`gc_pause_ms: 0.0`), 0 cyclic mark-and-sweep sweeps (`gc_runs: 0`), and zero GC-induced latency jitter (`isGcFree() == true`).
  - Scoped RAII and deterministic deallocation (`Memory::scope(...)` immediately drops local frames and intermediate allocations upon closure exit).
  - Copy-On-Write (COW) buffer detachment and atomic telemetry tracking (`Memory::cowDetachments()`).
  - Strict thread-safety boundary enforcement: active execution values (`PhpValue`) are `!Send + !Sync`, while cross-thread transfers strictly use immutable snapshots (`FrozenValue`) or channel message passing.
  - Tier 1 native static facade (`Memory`) exposing `Memory::usage()`, `Memory::peak()`, `Memory::stats()`, `Memory::isGcFree()`, `Memory::gcRuns()`, `Memory::gcPauseMs()`, `Memory::assertThreadSafe()`, and `Memory::leakCheck()`.

- [x] **Binary-Safe String Architecture & Legacy Japanese Multi-Byte Support:**
  - Back `PhpValue::String` with binary-safe byte structure (`RhpString` storing up to 23 bytes inline via SSO and `Rc<Vec<u8>>` for heap strings).
  - Solve the binary safety problem: read arbitrary binary payloads, image streams, and null bytes without engine panics.
  - Accommodate legacy Japanese character sets (**Shift-JIS**, **EUC-JP**, **ISO-2022-JP / JIS**, and UTF-8) natively.
  - Implement static facade constructors on `Str` (`Str::fromEncoding`, `Str::toEncoding`, `Str::utf8`, `Str::fromBytes`, `Str::bytes`, `Str::byteLength`, `Str::isUtf8`).
  - Immutable fluent string helper methods with explicit reassignment (`$str = $str->trim()->upper()`) with COW semantics.

- [x] **Compile-Time AST Cycle Prevention & Request-Scoped Arena Allocation (Step 21):**
  - **Compile-Time Static AST Linting:** Prevent cyclic references before code runs. The AST checker inspects closures and arrow functions; if a closure assigned to an object property captures `$this` (either explicitly via `use ($this)` or implicitly via property access/arrow functions), the compiler rejects it with `CyclicClosureCaptureForbidden`. Static closures (`static fn(...) => ...`, `static function(...) { ... }`) are allowed.
  - **Explicit Weak References for Circular Links:** Disallow assigning parent or self references directly into child properties without explicit weak reference wrapping (`CyclicReferenceRisk`). Provide native Tier 1 `Weak` facade (`Weak::to($obj)`, `Weak::get($ref)`, `Weak::isValid($ref)`) backed by `std::rc::Weak<RefCell<VmObject>>` with zero strong reference count increment.
  - **Request-Scoped Arena Allocation (`RequestArena`):** Fast bump pointer allocator tied to worker request lifecycles. Instantaneous $O(1)$ bulk drop/reset when request completes. Pre-allocated chunk buffers are retained across requests, eliminating heap churn and memory fragmentation.
  - **DropList Resource Hooks:** Non-memory OS handles (database transactions, open file descriptors, network sockets) are registered in the arena's `DropList` and cleanly closed upon request termination before memory pointers are reset.
  - **Parent vs. Child Process Tiering:** Immutable parent bytecode/AST in `Arc<EngineState>` shared across workers; worker VM executes requests using `RequestArena` with guaranteed 0 tracing GC runs (`gc_runs: 0`, `gc_pause_ms: 0.0`), reporting telemetry via `WorkerResponse::arena_drop()`.
  - Expose telemetry through `Memory::arenaStats()` and manual rollback through `Memory::resetArena()`.

---

### Phase 9: Regex Engine & PCRE2 Parity (Laravel/Symfony Compatibility)

- [ ] **PCRE2 / `fancy-regex` Engine Integration:**
  - Rust's standard `regex` crate uses linear-time finite automata and does not support lookaround assertions (`(?=...)`, `(?!...)`, `(?<=...)`, `(?<!...)`), backreferences (`\1`, `\k<name>`), atomic groups, or recursion.
  - Laravel and Symfony routing routers, validation rules, Blade directive parsers, and string manipulation libraries rely heavily on Perl-Compatible Regular Expressions (PCRE).
  - Integrate `pcre2` (via `pcre2-sys` C bindings) or a hybrid engine with `fancy-regex` to ensure full PCRE feature parity.
  - Support standard PCRE modifiers (`/u` UTF-8, `/i` case-insensitive, `/s` dotall, `/m` multiline, `/x` extended).
  - Provide an in-memory LRU pattern compilation cache for compiled PCRE regex objects to avoid repeated compilation across requests.
  - Implement standard PHP regex APIs: `preg_match()`, `preg_match_all()`, `preg_replace()`, `preg_replace_callback()`, `preg_split()`, and `preg_grep()`.

---

### Phase 10: Built-in High-Performance HTTP Server Engine (`hyper`, TLS, HTTP/2, WebSockets)

- [ ] **Embedded Async HTTP Engine with `hyper` & `tokio`:**
  - Build a high-performance, embedded Rust HTTP server using `hyper` (v1) and `tokio`, eliminating the mandatory requirement for an external web server (Nginx/Apache + FPM).
  - Multi-protocol support: Native HTTP/1.1 and HTTP/2 (via `h2`) negotiation with ALPN.
  - Native TLS termination: Integrate `rustls` (zero OpenSSL C-dependency overhead) or `native-tls` with support for dynamic certificate reloading.

- [ ] **Streaming Responses & Server-Sent Events (SSE):**
  - Implement async chunked transfer encoding and streaming response bodies (`Stream<Item = Bytes>`), allowing instant data flushing without buffering full payloads in RAM.
  - First-class Server-Sent Events (SSE) primitives for real-time notifications and streaming endpoints.

- [ ] **WebSockets & Real-Time Protocol Support (Laravel Reverb Compatibility):**
  - Built-in RFC 6455 WebSocket upgrade pipeline using `tokio-tungstenite` / `fastwebsockets`.
  - Provide full compatibility with Laravel Reverb and custom WebSocket handlers on the same HTTP port.

- [ ] **Zero-Copy Multipart File Uploads:**
  - Streaming multipart form-data parser (e.g. `multer-rs`) that spools uploaded files directly to temporary disk storage to prevent Out-Of-Memory (OOM) vulnerabilities.
  - Configurable upload size thresholds and file limits.
  - Seamlessly populate request file objects and standard `$_FILES` structures.
  - Automatic cleanup of temporary upload files at the end of the request lifecycle.

---

### Phase 11: Web Request Lifecycle, State, Superglobals & Output Buffering

- [ ] **Session & Cookie Management Subsystem:**
  - Built-in session store abstraction with pluggable storage backends (In-memory, File, Redis).
  - Cryptographically secure session ID generation, cookie binding, lifetime tracking, and garbage collection.
  - Comprehensive cookie parser/serializer supporting `SameSite` (Lax, Strict, None), `HttpOnly`, `Secure`, and domain/path scoping.
  - Built-in CSRF token generation and validation middleware.

- [ ] **Superglobals CoW Emulation (`$_GET`, `$_POST`, `$_SERVER`, `$_COOKIE`, `$_FILES`, `$_SESSION`, `$_ENV`):**
  - Map incoming HTTP request headers, query parameters, parsed bodies, cookies, and environment variables into isolated request-scoped superglobals.
  - Enforce Copy-on-Write (CoW) state isolation: mutations within a request fiber do not bleed into concurrent or subsequent requests.

- [ ] **Nested Output Buffering Subsystem:**
  - Implement full PHP output buffering semantics: `ob_start()`, `ob_get_clean()`, `ob_get_contents()`, `ob_flush()`, `ob_end_clean()`, and `ob_end_flush()`.
  - Support nested buffer levels, chunk size triggers, and custom output filtering callbacks.
  - Automatically flush and send active output buffers upon normal request completion.

- [ ] **Clean Control-Flow Unwinding on `exit()` / `die()`:**
  - Calling `exit()` or `die()` in script-land must NOT terminate the host operating system process or worker thread.
  - Implement `exit()` / `die()` as a controlled unwinding exception/panic boundary (`ExitUnwind`), safely triggering output buffer flushes, executing shutdown functions and post-response `defer` hooks, and returning the HTTP response cleanly.

---

### Phase 12: Module System, Composer Interoperability & Modern PHP OOP

- [ ] **Composer Ecosystem Interoperability & Autoloading (PSR-4 / PSR-0):**
  - Native runtime parser for `composer.json` and `composer.lock`.
  - Built-in high-performance PSR-4 and PSR-0 class loader with an in-memory cached path resolution map, eliminating hot-path filesystem `stat()` calls.
  - Enable 100% compatibility with Composer packages in `vendor/` without requiring modifications to package code.

- [ ] **Modern PHP OOP Constructs:**
  - **Namespaces:** Full support for namespaces, aliases (`use Foo\Bar as Baz`), and namespace resolution rules.
  - [x] **Traits:** Trait composition, conflict resolution (`insteadof`), method aliasing (`as`), and property precedence.
  - **Enums:** Pure enums and Backed enums (`enum Status: string`), backed value casting, methods on enums, and `UnitEnum` / `BackedEnum` interfaces.
  - **Attributes:** First-class PHP 8+ attributes (`#[Attribute]`, `#[Route('/api')]`), reflection metadata API, and compile-time attribute extraction.
  - **Readonly Properties & Classes:** Enforce `readonly` immutability invariants at compile time and runtime initialization.
  - **Constructor Property Promotion:** Parsing and lowering of `public function __construct(private string $name)`.

- [ ] **Function-Local Static Variables Lifecycle:**
  - Implement `static $var = initial;` semantics with deterministic scoping.
  - Isolate static variable persistence across repeated fiber calls vs. request-scoped lifecycle resets.

---

### Phase 13: Framework & CLI Runtime (Artisan, Queue, Horizon, Scheduler, Migrations)

- [ ] **Artisan CLI Runner & Console Subsystem:**
  - Native CLI entrypoint (`rhp artisan ...`) capable of bootstrapping the framework and running Symfony Console / Laravel Artisan commands without code changes.
  - Support interactive prompts, ANSI color formatting, arguments, and options parsing.

- [ ] **Long-Running Queue Workers & Laravel Horizon Compatibility:**
  - Robust execution loop for queue workers (`rhp artisan queue:work`).
  - Process isolation and worker recycling after processing $N$ jobs or exceeding memory thresholds.
  - Signal handling (`SIGTERM`, `SIGINT`, `SIGUSR2`, `SIGHUP`) for graceful worker stopping, active job completion, and Horizon supervisor communication.
  - Job timeout timers with preemptive fiber cancellation.

- [ ] **Artisan Task Scheduler Daemon:**
  - Built-in cron runner for `schedule:run` and `schedule:work` executing background tasks concurrently without blocking the main scheduler process.

- [ ] **Database Migrations Subsystem:**
  - Full compatibility with Laravel database migrations, schema builders, Blueprint DSL, foreign keys, and migration rollbacks.

---

### Phase 14: Blade Template Engine & Dynamic Eval Pipeline

- [ ] **Dynamic String Compilation & `eval()` Support:**
  - Blade templates compile dynamically into PHP code containing embedded echo statements, control flow (`@if`, `@foreach`), and component directives.
  - Support on-the-fly string compilation and execution (`eval()` / `Engine::eval_string()`) through the bytecode compiler, with cached AST/bytecode reuse.

- [ ] **Blade AOT Pre-Compilation & Bytecode Caching:**
  - CLI pre-compilation command (`rhp blade:compile`) to parse and lower all `.blade.php` templates into native RHP bytecode or AST chunks at build time.
  - Store compiled Blade artifacts in the persistent parent process cache (`Arc<EngineState>`), eliminating disk I/O and runtime evaluation overhead entirely in production.

---

### Phase 15: Testing, Tooling & Developer Ergonomics (PHPUnit, Pest, Debugger, Profiler, LSP, PHPStan)

- [ ] **PHPUnit & Pest Testing Framework Support:**
  - Ensure full compatibility to run PHPUnit and Pest test suites on the RHP engine.
  - Support test mocking, data providers, test lifecycle hooks, and process isolation runner modes.

- [ ] **Accurate Stack Traces & Source Mapping:**
  - Generate precise stack traces with line numbers, column offsets, and source code previews for both interpreter and Cranelift JIT-compiled frames.
  - Source-map translation for AOT pre-compiled templates (e.g. Blade) back to original template line numbers.

- [ ] **Native Debugger Protocol (Xdebug Equivalent / DAP):**
  - Implement a Debug Adapter Protocol (DAP) or DBGp engine listener directly inside the VM.
  - Support line breakpoints, conditional breakpoints, step-in/step-over/step-out, call stack inspection, and runtime variable watches in VS Code and PhpStorm.

- [ ] **Integrated Low-Overhead Sampling Profiler:**
  - Low-overhead sampling profiler tracking CPU execution time and heap memory allocations per function/fiber.
  - Export profile dumps in standard Speedscope and pprof flamegraph formats for immediate performance diagnosis.

- [ ] **Language Server Protocol (LSP) & Static Analysis Tooling:**
  - Standalone `rhp-lsp` binary providing language intelligence: auto-completion, hover definitions, go-to-definition, and real-time syntax/type diagnostics.
  - Compatibility with PHPStan and Larastan: Expose AST and reflection metadata or provide a drop-in static analyzer engine.

---

### Phase 16: Native Observability & OpenTelemetry Integration

- [x] **OpenTelemetry (OTel) Distributed Tracing & Metrics Engine:**
  - High-performance native zero-dependency OpenTelemetry core built in Rust for RHP VM fibers and request lifecycles.
  - Automatic W3C Distributed Trace Context propagation (`traceparent` format `00-{trace_id}-{span_id}-{flags}` and `baggage`).
  - Hierarchical span model (`SpanContext`, `SpanId` 64-bit, `TraceId` 128-bit, `SpanKind::Server/Client/Internal/Producer/Consumer`, `SpanStatus`, `SpanEvent`, nanosecond timestamps, and `AttributeValue`).
  - Thread-local active span stack supporting arbitrary nesting, parent-child linking, and completed spans buffer.
  - Native Tier 1 static facades `Trace` and `Telemetry` with `startSpan`, `endSpan`, `span` (closure with automatic lifecycle and error capture), `setAttribute`, `addEvent`, `recordException`, `traceId`, `spanId`, `parentSpanId`, `isRecording`.
  - Auto-instrumentation hooks for incoming worker requests (`WorkerRequest` / `WorkerResponse`), outbound `HTTP` client calls, and `DB` database operations.
  - Real-time canonical OTLP/HTTP JSON export for `/v1/traces` and `/v1/metrics` (compatible with OTel Collector, Jaeger, Honeycomb, Datadog, New Relic).

- [x] **Native Error Tracking & Exception Reporting (Sentry / Datadog Bridges):**
  - Native in-memory exception capture pipeline dispatching crash reports and stack traces directly to Sentry (Sentry event JSON payload with exception type, value, and stack frames) and Datadog APM trace exporter (`export_datadog_traces`).
  - High-precision runtime metrics registry: `Counter`, `Gauge`, and `Histogram` with count, sum, min, max, average, and exact percentiles ($p50$, $p90$, $p95$, $p99$).
  - Auto-instrumented request latency tracking (`rhp.request.duration_ms`, `rhp.request.total`, `rhp.request.success`, `rhp.request.error`).

---

### Phase 17: Process Failure Isolation, Crash Protection & JIT Sandboxing

- [ ] **Process-Level Failure Isolation (Defending Against Panics & Segfaults):**
  - In a purely multi-threaded model, a single fatal native crash (e.g. a segfault in a Tier 2 C/Rust native plugin, or an unsafe pointer bug in Cranelift JIT codegen) kills the entire process and drops all in-flight requests across all threads.
  - In contrast, PHP-FPM provides process isolation where a worker crash only affects a single request.
  - Architect a resilient multi-process supervisor model (`WorkerModel::ProcessCow` / Supervisor-Worker topology):
    - Long-lived parent supervisor maintains shared immutable application state (`Arc<EngineState>`) and manages a pool of worker processes.
    - If a worker process segfaults or panics, the supervisor immediately logs the failure, restarts the worker in milliseconds, and returns a clean 500 error to that single client, leaving all other concurrent worker processes completely unharmed.

- [ ] **JIT Memory Safety & RCE Protection:**
  - JIT compiler bugs can potentially produce invalid memory accesses or Remote Code Execution (RCE) vulnerabilities.
  - Enforce strict $W \oplus X$ (Write XOR Execute) memory protections on Cranelift code pages (pages are never simultaneously writable and executable).
  - Implement JIT execution bounds validation, memory limit enforcement on native codegen buffers, and fallback to interpreter upon native trap detection.

---

### Phase 18: Production Operations, Resource Limiting & Zero-Downtime Reload

- [ ] **Per-Request Resource Limiting (Memory & Execution Time):**
  - Preemptive execution time limits (`max_execution_time`) enforced via fiber cancellation timers that abort runaway loops without thread freezing.
  - Per-request memory allocation limits (`memory_limit`) tracking fiber allocations and aborting requests before they trigger process-wide Out-Of-Memory (OOM) kills.

- [ ] **Backpressure & Overload Protection:**
  - Bounded concurrency and queue limits: reject incoming requests with HTTP 503 Service Unavailable or HTTP 429 Too Many Requests when worker capacity is saturated.
  - Adaptive rate limiting and request shedder to maintain low latency under traffic spikes.

- [ ] **Graceful Shutdown & Zero-Downtime Reloading:**
  - Handle `SIGTERM` and `SIGINT` signals gracefully: immediately stop accepting new HTTP connections, allow in-flight requests a configurable grace period (e.g. 30 seconds) to complete, and flush all deferred logs and metrics.
  - Zero-downtime rolling reload: Socket file-descriptor handoff (`SO_REUSEPORT` / Unix domain socket passing), enabling seamless binary updates and config changes without dropping a single active TCP connection.

- [ ] **Development Hot Reload (`rhp run dev`):**
  - Integrated file watcher (`notify` crate) monitoring codebase directories, performing incremental recompilation and instant worker pool recycling upon file save.

---

### Phase 19: Tier 2 AOT Native Plugins: Compilation, Distribution & ABI

- [x] **Direct Rust File Imports & Helper Class Dynamic Loading (`import "./Cache.rs"`)**:
  - Support direct import of `.rs` files in RHP scripts using `import "./Cache.rs";`, `import "./Cache.rs" as Cache;`, or `use "./Cache.rs" as Cache;`.
  - Expose imported Rust files as Tier 2 Static Facades with native performance without core language bloat (`Cache::set()`, `Cache::get()`, `Log::info()`).
  - Automatic compilation to dynamic shared libraries (`cdylib` / `.so`) via `rustc` with SHA-256 caching and POSIX `dlopen` symbol loading.
  - Built-in thread-safe in-process native execution fallback with `FrozenValue` state boundary isolation.

- [ ] **Plugin ABI & Dynamic Library Loading (`cdylib`):**
  - In a static-only AOT architecture, adding or modifying a plugin requires full recompilation of the entire RHP binary.
  - Define a stable C-ABI / dynamic plugin interface (`rhp_plugin_api`) allowing Tier 2 plugins to be compiled into dynamic shared libraries (`.so`, `.dylib`, `.dll`) loaded at runtime via `libloading`.
  - ABI version verification: Prevent crashes by verifying engine and plugin version hashes at load time.

- [ ] **Cross-Compilation & Binary Distribution Tooling:**
  - Eliminate the requirement for end-users to have a full Rust toolchain installed.
  - Implement `rhp plugin install <vendor/package>` CLI command to fetch pre-compiled native binaries from a registry for the host platform (Linux x86_64/aarch64, macOS, Windows).

- [ ] **Sandboxed WebAssembly (Wasm) Plugin Alternative:**
  - Support high-performance WebAssembly (Wasm/WASI via `wasmtime`) as an isolated, portable Tier 2 plugin target, providing near-native execution speed with zero native compilation risk and zero process crash risk.

---

### Phase 20: Future Cancellation Mechanics & In-Flight Cleanup (`Future::any`)

- [ ] **Cancellation Token Architecture:**
  - When `Future::any([$f1, $f2])` resolves with the earliest winner, the remaining in-flight futures must be cancelled immediately to prevent runaway CPU waste and resource leaks.
  - Introduce thread-safe `CancellationToken` hierarchies propagated through every `Future` and Fiber task.

- [ ] **Cooperative Cancellation Points:**
  - Insert low-overhead cancellation checkpoints at loop headers, function call entries, and async fiber yields (`fiber.check_cancelled()`).
  - If a cancellation token is tripped, the fiber aborts execution and unwinds cleanly.

- [ ] **In-Flight Query & I/O Abort Cleanup:**
  - When a `Future` is cancelled, immediately trigger abort signals on underlying network connections (e.g. closing HTTP client sockets).
  - Issue asynchronous SQL query cancellation packets (`pg_cancel_backend` / connection drop) to database servers to abort long-running queries on the database server.
  - Deterministically execute `finally` blocks and RAII cleanup handlers to avoid leaking file descriptors, memory locks, or connections.

---

### Phase 21: RHPM Architecture & Process Manager (Supervision, PM2-like Lifecycle & Slow Route Diversion)

- [ ] **Separation into `rhp` and `rhpm`:**
  - Decouple the codebase into two dedicated roles and binaries:
    - `rhp`: Core script execution engine and worker instance.
    - `rhpm` (RHP Manager): Gateway and process supervisor managing HTTP requests and the full request/response lifecycle.

- [ ] **PM2 & Supervisor-Style Process Lifecycle Management:**
  - Run application worker processes like PM2, assigning incoming HTTP requests to managed worker processes.
  - Supervisor-style crash recovery: continuously monitor process health and automatically reinitiate/restart crashed or failed processes.

- [ ] **Route Telemetry & SLA Tracking:**
  - Monitor route execution metrics in real-time, detecting routes taking more than 3 seconds or overutilizing CPU and memory.
  - Sliding-window tracking: detect anomalies such as 2 slow requests out of 20 for a given route.

- [ ] **Dynamic Traffic Diversion to Secondary Process & Failback:**
  - **Diverting Slow Routes:** When 2 slow requests out of 20 are detected on a route, automatically divert incoming requests for that route to a secondary process running a second pre-warmed instance of the application, isolating the primary worker pool from degradation.
  - **Automatic Failback:** Continuously evaluate the redirected route's metrics in the secondary process; once execution times and resource usage return to normal levels, divert traffic back to the main process.

---

## 3. Progress Tracking

| Subsystem | Planned Improvement | Status | Priority |
| :--- | :--- | :--- | :--- |
| **Syntax** | Remove `<?php` and `?>` tag requirement | Pending | **P0** |
| **Collections** | Strict `Arr::list` string key error (`cannot use custom key for list...`) | Pending | **P0** |
| **Value Model** | 64-bit NaN-boxed `Value(u64)` (Bun `JSValue` pattern) | Pending | **P1** |
| **Lexer** | Handwritten SIMD scanner with `memchr` | Pending | **P1** |
| **Parser / AST** | Arena allocation (`bumpalo`) without lifetime contamination | Pending | **P1** |
| **VM Dispatch** | Pin register/IP pointers; out-of-band debug spans | Pending | **P1** |
| **Collections** | `hashbrown` with `raw-entry`, `wyhash`, COW without `RefCell` | Pending | **P2** |
| **IR Lowering** | Compact 32-bit packed instruction format | Pending | **P2** |
| **JIT Engine** | High threshold + OSR entry/exit bailout | Pending | **P2** |
| **Type System** | SSA-based local type specialization & monomorphic call stubs | Pending | **P2** |
| **Regex** | PCRE2 / `fancy-regex` with lookaround, backreferences, & `preg_*` parity | Pending | **P0** |
| **HTTP Server** | Built-in async server with `hyper`, TLS, HTTP/2, SSE, WebSockets, & file uploads | Pending | **P0** |
| **Web Lifecycle** | Sessions, cookies, CSRF, superglobals CoW, `exit`/`die` unwinding, output buffering | Pending | **P0** |
| **Modules & Composer** | PSR-4 autoloading, namespaces, traits, enums, attributes, readonly, static vars | Pending | **P0** |
| **CLI & Ecosystem** | Artisan CLI, Queue workers, Horizon signals, Task Scheduler, Migrations | Pending | **P1** |
| **Blade Engine** | In-memory dynamic `eval()` / string compilation & Blade AOT pre-compiler | Pending | **P1** |
| **Testing & Tooling** | PHPUnit/Pest compatibility, source-mapped stack traces, DAP debugger, profiler, LSP | Pending | **P1** |
| **Observability** | Native OpenTelemetry (OTel) traces/metrics/logs & Sentry/Datadog bridges | Pending | **P1** |
| **Failure Isolation** | Process supervisor (`ProcessCow`), panic boundaries, JIT memory safety ($W \oplus X$) | Pending | **P0** |
| **Operations** | Per-request memory/time limits, backpressure, graceful shutdown, zero-downtime reload | Pending | **P1** |
| **Plugin System** | Direct `.rs` file imports (`import "./Cache.rs"`), Tier 2 dynamic cdylib compilation & in-process fallback | Completed | **P1** |
| **Concurrency** | `Future::any` cancellation tokens, cooperative cancel points, & in-flight query aborts | Pending | **P1** |
| **Fiber & I/O Scheduling** | Epoll-based event loop (`epoll_wait`, timerfd, eventfd) for fibers and IPC readiness | Pending | **P1** |
| **Process Manager (RHPM)** | Separate `rhp`/`rhpm`, PM2/supervisor lifecycle & crash recovery, >3s/resource tracking & secondary route diversion | Pending | **P0** |
