---
title: "Core Architecture"
description: "Decided core architecture of RHP: one-process multi-threaded runtime, server.rhp/index.rhp lifecycle, zero-GC bump arenas, and strict language rules."
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

## Architectural Goals (Priority Order)

1. **Performance:** Sub-millisecond cold starts, zero garbage collection pauses during web requests, and maximum requests per second per MB of RAM.
2. **Upgrade Path for Old PHP Apps:** A clean, automated transition path for large legacy codebases to gain modern memory safety and speed.
3. **Teaching Rust-Level Concepts to PHP Developers:** Introducing ownership, immutability, unboxed types, and thread-safety invariants through familiar PHP-like syntax.

---

## Runtime Model

```text
┌────────────────────────────────────────────────────────────────────────┐
│                   RUST HOST RUNTIME (One Process)                      │
│                                                                        │
│  server.rhp (Runs ONCE at boot)                                        │
│  - Builds config, env, routes, container, and compiled classes         │
│  - Frozen into an immutable Boot Heap (Arc<EngineState>)               │
└───────────────────────────────────┬────────────────────────────────────┘
                                    │ Shared Read-Only by all threads
            ┌───────────────────────┼───────────────────────┐
            ▼                       ▼                       ▼
 ┌─────────────────────┐ ┌─────────────────────┐ ┌─────────────────────┐
 │   WORKER THREAD 1   │ │   WORKER THREAD 2   │ │   WORKER THREAD 3   │
 │                     │ │                     │ │                     │
 │ index.rhp (Request) │ │ index.rhp (Request) │ │ index.rhp (Request) │
 │ - Dedicated Arena   │ │ - Dedicated Arena   │ │ - Dedicated Arena   │
 │ - O(1) Bulk Drop    │ │ - O(1) Bulk Drop    │ │ - O(1) Bulk Drop    │
 └─────────────────────┘ └─────────────────────┘ └─────────────────────┘
```

- **One Process, Many Threads:** A high-performance Rust host (Tokio for HTTP) runs the entire service. There is no `fork()` and no IPC overhead between boot and request code.
- **`server.rhp` runs once at boot:** It parses configuration, reads environment variables, builds routing tables, bootstraps the service container, and pre-compiles classes. The resulting state is frozen into an immutable boot heap (`Arc<EngineState>`), shared read-only across all worker threads.
- **`index.rhp` runs once per request:** Each incoming request is dispatched to a worker thread and given its own bump allocation arena (`RequestArena`). Every object, array, or string the request creates is allocated inside this arena. When the request finishes, the arena is dropped in one step ($O(1)$), guaranteeing **zero garbage collection** and **zero memory leaks** between requests.
- **Deadline checks in the VM:** The virtual machine inspects a time limit at loop iterations and function calls. A per-arena memory cap terminates runaway requests before they consume excess server RAM.

---

## Memory and State Rules

- **Boot data is strictly read-only to request code:** Any attempt by request code to mutate global or boot-time data structures throws a fatal error. Shared mutable state across requests must go through explicit native APIs.
- **`Cache::get/set` takes plain data only:** The cache accepts only primitive scalars and array structures—never live object references or closures. Backing cache drivers include file, database, or Redis, with an optional in-memory L1 cache in front.
- **Everything finishes before the arena drops:** Post-response deferred work (`Future::defer`) executes completely before the request arena is dropped. Queued jobs are serialized into the queue prior to teardown.
- **Resources belong to the host:** Operating system resources—including log file descriptors, database connections, and network sockets—live in host-owned pools. Request code only holds lightweight handles. Borrowed connections are automatically reset upon return and reclaimed on timeout.

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
2. The `server.rhp` (boot) and `index.rhp` (request) split with request bump arenas.
3. A host-managed logger and connection pool.
4. A fast static router.

**Benchmark Target:** Measure a JSON endpoint executing one database query against Laravel + Octane on **requests per second per MB of RAM**.
