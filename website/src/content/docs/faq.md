---
title: "Frequently Asked Questions"
description: "Answers to common questions regarding RHP, compatibility with traditional PHP, Octane vs RHP, and performance trade-offs."
section: "Community"
order: 3
---

## What is RHP in one sentence?

RHP is a high-performance, drop-in syntax programming language designed to supercharge PHP and Laravel-style applications with the raw speed, zero-GC pauses, and memory safety of Rust.

---

## How does RHP compare to Laravel Octane or Swoole?

While Laravel Octane (backed by FrankenPHP, RoadRunner, or Swoole) keeps PHP framework boots in memory, it remains constrained by:
1. **Zend Engine Architecture:** Traditional PHP data structures (Zvals and hashtables) still incur significant heap allocation overhead and memory fragmentation.
2. **Cyclic Garbage Collection:** PHP's cyclic GC sweeps occasionally freeze worker threads, causing p99 latency spikes.
3. **C Extension Fragility:** Debugging memory leaks or native crashes in Swoole extensions can be notoriously difficult.

RHP is written natively in Rust from the ground up:
- Application framework code is compiled into an immutable `Arc<EngineState>`.
- Collections map directly to cache-friendly Rust heap vectors (`Vec`, `IndexMap`).
- There is zero tracing garbage collection (`Memory::gcPauseMs() == 0.0`).
- Request memory is reset instantly in $O(1)$ time via pre-allocated bump arenas.

---

## Is RHP drop-in compatible with standard PHP or Laravel?

**No. RHP is not PHP-compatible.** Running unmodified PHP or Laravel applications is **explicitly out of scope for v1**.

Instead, existing PHP applications and frameworks are ported or rewritten using the `rhp migrate` tool. RHP eliminates PHP's legacy dynamic quirks—such as pass-by-reference (`&$var`), variable variables (`$$var`), runtime `eval()`, and loose equality (`==`)—in order to guarantee memory safety, unboxed fast paths, and zero-GC request execution.

---

## What is the RHP runtime model?

RHP runs as **one process with many worker threads**, hosted by a native Rust engine (Tokio for HTTP):
- **`server.rhp` runs once at boot:** It sets up environment, config, routing tables, and the dependency container, and freezes the compiled classes into an immutable boot heap (`Arc<EngineState>`), shared read-only across all threads.
- **`index.rhp` runs once per request:** Each request gets an isolated bump allocation arena (`RequestArena`). Everything allocated during the request is dropped in one $O(1)$ step at the end. There is no fork and no IPC between boot and request code.


---

## Why is pass-by-reference (`&$var`) removed?

Pass-by-reference variables and closure captures by mutable reference (`use (&$var)`) create cyclic reference graphs. In dynamic languages, resolving these cyclic graphs requires a stop-the-world tracing garbage collector.

By eliminating reference variables and enforcing immutable collections with Copy-On-Write (COW) semantics, RHP eliminates cyclic leaks at compile time, allowing the runtime to operate 100% GC-free with deterministic RAII deallocation.

---

## What file extension should I use?

RHP officially supports both:
- `.rhp`: Standard extension for modern RHP projects.
- `.php`: Supported for drop-in migration workflows and editor compatibility.
