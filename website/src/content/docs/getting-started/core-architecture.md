---
title: "Core Architecture"
description: "Core architecture of RHP: master supervisor process, pre-forked worker pool, single index.rhp entrypoint, zero-GC bump arenas, and strict language rules."
section: "Getting Started"
order: 3
---

## What RHP Is

<p align="center" class="my-6">
  <img
    src="/rhp-banner.png"
    alt="RHP - High-Performance Programming Language Combining Rust Speed & PHP Ergonomics"
    class="w-full h-auto rounded-2xl shadow-xl border border-slate-200 dark:border-slate-800"
  />
</p>

RHP is a **strict, gradually typed, PHP-style language with a Rust host runtime**. It is built from the ground up for blazing fast, memory-safe web requests. 

- **Not PHP-compatible:** RHP deliberately breaks backward compatibility with PHP's legacy baggage, dynamic quirks, and loose typing.
- **Automated migration:** Existing PHP applications are ported using the dedicated `rhp migrate` tool rather than by trying to run legacy PHP code unmodified.

---

## The Three-Layer Separation of Concerns

To preserve architectural purity and eliminate leaks between responsibilities, the ecosystem is strictly divided into three distinct layers:

| Layer | Primary Entity | Owns | Litmus Test |
|---|---|---|---|
| **Layer 1: Language** | `rhp` Compiler & VM | Everything that defines **what code means** | **The CLI Test:** Can run `rhp script.rhp` with zero servers, zero workers, and no framework. |
| **Layer 2: Runtime** | `rhpm` + Host Server | Everything about **processes, I/O, and lifecycle** | Manages OS sockets, processes, forks, and network events without dictating application structure. |
| **Layer 3: Framework** | Laravel Rocket | Everything about **how an app is structured** | Provides application conventions, routing DSL, DI container, and controllers on top of language primitives. |

For deep dives, see [Language Layer & Semantics](/docs/advanced/language-layer) and [Runtime & Process Architecture](/docs/advanced/runtime-architecture).

---

## Runtime Model

```text
┌────────────────────────────────────────────────────────────────────────┐
│                   RHP SUPERVISOR (Master Process)                      │
│                                                                        │
│  - Reads rhp.toml (port, workers, timeouts, watch rules)               │
│  - Executes index.rhp Boot Phase (Sync, strictly single-threaded)      │
│  - Registers routes, service container, pre-compiles bytecode/JIT       │
│  - Binds shared listening socket                                       │
│  - File watcher (hot-reloading in dev mode)                            │
└───────────────────┬────────────────────────────────┬───────────────────┘
                    │ fork() (Pre-forked pool at boot)│ fork()
                    ▼                                ▼
       ┌────────────────────────┐       ┌────────────────────────┐
       │   WORKER PROCESS 1     │       │   WORKER PROCESS 2     │
       │                        │       │                        │
       │  Post-fork init:       │       │  Post-fork init:       │
       │  - Starts Tokio/Async  │       │  - Starts Tokio/Async  │
       │  - Initializes DB Pool │       │  - Initializes DB Pool │
       │                        │       │                        │
       │  Request Loop:         │       │  Request Loop:         │
       │  - index.rhp (Request) │       │  - index.rhp (Request) │
       │  - Dedicated Bump Arena│       │  - Dedicated Bump Arena│
       │  - O(1) Bulk Drop      │       │  - O(1) Bulk Drop      │
       └────────────────────────┘       └────────────────────────┘
```

- **Runtime Owns the Server:** The runtime (`rhp run dev`, `rhp run build`, `rhpm`) handles the socket, process supervision, and worker management. User code only needs a single **`index.rhp`** file, while infrastructure settings live in **`rhp.toml`**.
- **Two Lifecycle Phases via Hooks:**
  - **Boot Phase (Master, runs ONCE):** Evaluates top-level registrations, config, container bindings, and compiles classes. The master remains strictly single-threaded (no Tokio runtime, no background threads, no database connections before forking) to prevent POSIX `fork()` deadlocks.
  - **Request Phase (Workers, repeated):** The master pre-forks $N$ worker processes. Each worker initializes its Tokio runtime and database connection pools post-fork.
- **Request Bump Arenas:** Each incoming request within a worker is given its own bump allocation arena (`RequestArena`). Every object, array, or string created during the request is allocated inside this arena. When the request finishes, the arena is dropped in one step ($O(1)$), guaranteeing **zero garbage collection** and **zero memory leaks** between requests.
- **Fault Containment & Hot Reload:** If a worker panics, crashes, or breaches memory limits, the master supervisor catches `SIGCHLD` and immediately respawns a clean worker without impacting active traffic. In development mode, the master watches source files and issues rolling restarts to workers.
- **Deadline checks in the VM:** The virtual machine inspects a time limit at loop iterations and function calls. A per-arena memory cap terminates runaway requests before they consume excess server RAM.

---

## Memory and State Rules

- **Boot data is strictly read-only to request code:** Any attempt by request code to mutate global or boot-time data structures throws a fatal error. Shared mutable state across requests must go through explicit native APIs.
- **`Cache::get/set` takes plain data only:** The cache accepts only primitive scalars and array structures—never live object references or closures. Backing cache drivers include file, database, or Redis, with an optional in-memory L1 cache in front.
- **Everything finishes before the arena drops:** Post-response deferred work (`Future::defer`) executes completely before the request arena is dropped. Queued jobs are serialized into the queue prior to teardown.
- **Resources belong to the host:** Operating system resources—including log file descriptors, database connections, and network sockets—live in host-owned pools. Request code only holds lightweight handles. Borrowed connections are automatically reset upon return and reclaimed on timeout.
- **Borrow Checker & Zero-GC Guarantees:** Developers are freed from manual borrow checking or lifetime annotations; the runtime enforces memory safety through pass-by-value/CoW semantics, request bump arenas, and compile-time cycle elimination. See [Memory Model: Borrow Checker and Memory Safety](/docs/advanced/memory-model#borrow-checker).

---

## Language Design

- **Gradual static types:** Statically typed code takes an optimized fast path with unboxed values compiled via Cranelift. Untyped code continues to work dynamically via boxed values, but runs slower.
- **Strict semantics:** RHP completely eliminates:
  - No pass-by-reference (`&$var` is forbidden).
  - No variable variables (`$$var` is forbidden).
  - No runtime `eval()`.
  - No loose equality (`==` behaves strictly without implicit type juggling).
- **Rust escape hatch:** Developers can import native Rust files directly as helper classes, giving instant access to high-performance systems code without complex C FFI wrappers.

---

## Explicitly Out of Scope for v1

To ensure rapid delivery, architectural purity, and uncompromised speed, the following are explicitly out of scope for v1:

- **Concurrency inside a single request:** Requests execute sequentially and deterministically on their assigned worker thread.
- **Running unmodified PHP or Laravel:** The framework and application code are rewritten or ported via migration tools.
- **A full standard library:** The standard library remains lean and focused, expanding only after benchmarks validate the core engine design.

---

## First Milestone

The initial milestone delivers a razor-thin, highly optimized vertical slice:
1. A lightweight bytecode interpreter and Cranelift JIT pipeline.
2. Single `index.rhp` lifecycle (master boot phase + worker request bump arenas).
3. A host-managed logger and connection pool.
4. A fast static router.

**Benchmark Target:** Measure a JSON endpoint executing one database query against Laravel + Octane on **requests per second per MB of RAM**.
