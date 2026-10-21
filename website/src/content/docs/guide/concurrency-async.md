---
title: "Concurrency and Async"
description: "Master Tokio fiber concurrency, Dart-style Future APIs, parallel resolution, and post-response defer hooks in RHP."
section: "Language Guide"
order: 8
---

## Concurrency Architecture: Synchronous by Default & No `await` Keyword

RHP adopts a **synchronous-by-default execution model**:

- Standard I/O operations (such as `File::read()` or `DB::table()->find()`) execute sequentially on local fibers without callback hell or tedious `await` prefixes everywhere.
- **No `await` Keyword (No Function Coloring):** RHP deliberately avoids the `await` keyword. In languages with `async/await`, functions become "colored"—calling an async function forces the caller to become async, spreading throughout the entire codebase. In RHP:
  - If you want simple linear execution, use synchronous facades directly (`File`, `DB`, `HTTP`).
  - If you initiate non-blocking background tasks, you receive a `Future` handle and resolve it by calling `$future->wait()` or composing tasks with `Future::wait([ ... ])`.
  - If a developer uses the `await` keyword, the parser emits a compile-time error:
    > *`Parse Error: The 'await' keyword is not supported in RHP. Use synchronous facades (File, DB, HTTP) to get results synchronously, or call '->wait()' on Future objects.`*

---

## Asynchronous Tokio Facades: `AFile`, `AHTTP`, and `ADB`

When you need non-blocking I/O tasks executing concurrently in the background without blocking the current worker thread, RHP provides dedicated async facades backed by a multi-threaded Rust Tokio runtime:

| Facade | Execution Engine | Return Type | Common Operations |
| :--- | :--- | :--- | :--- |
| **`AFile`** | Tokio thread pool (`tokio::fs`) | `Future` | `read()`, `write()`, `exists()`, `delete()`, `append()`, `size()` |
| **`AHTTP`** | Tokio non-blocking HTTP (`reqwest` / async) | `Future` | `get()`, `post()`, `put()`, `delete()`, `patch()`, `withHeaders()` |
| **`ADB`** | Tokio non-blocking DB pool | `Future` | `table()`, `find()`, `get()`, `select()`, `insert()`, `update()`, `delete()`, `query()`, `raw()` |

These facades return an RHP `Future` immediately without blocking execution:

```php
// Dispatch non-blocking Tokio tasks in parallel
$futFile = AFile::read('large_payload.json');
$futHttp = AHTTP::get('https://api.example.com/v1/metrics');
$futDb   = ADB::table('users')->where('active', 1)->get();

// Both AFile, AHTTP, and ADB return Future instances immediately!
```

---

## Future State Model (Rust-Like `Poll::Pending` / `Poll::Ready`)

RHP's `Future` object tracks background Tokio execution state with semantics matching Rust's `Poll` enum:

- **State Values:** `"Pending"` (task is executing on Tokio) and `"Ready"` (task has completed).
- **Inspection Properties & Methods:**
  - `$future->state` / `$future->getState()`: returns string `"Pending"` or `"Ready"`.
  - `$future->isPending()`: returns `true` while the background task is running.
  - `$future->isReady()` / `$future->isCompleted()`: returns `true` once resolved.
  - `$future->poll()`: non-blocking probe returning `null` if still pending, or the resolved value if ready.
  - `$future->wait()`: synchronizes the caller, waiting until ready and returning the resolved value.

```php
$fut = AFile::read('config.json');

// Check state without blocking
if ($fut->isPending()) {
    Sys::print("Task is running in background, state={}", $fut->state); // "Pending"
}

// Synchronously wait for completion when the value is needed:
string $content = $fut->wait();

Sys::print("State after completion: {}", $fut->state); // "Ready"
Sys::print("Is completed: {}", $fut->isReady());       // true
```

---

## Dart-Style `Future` Facade API

Unlike JavaScript Promises which eagerly execute on creation, or Go goroutines which require explicit waitgroups and channel plumbing, RHP provides a clean Dart-style `Future` facade:

### 1. Spawning Tasks with `Future::run`
Spawns an asynchronous task on the Tokio background thread pool:

```php
$future = Future::run(function(): string {
    return File::read('report.csv');
});

// Explicitly resolve the result when needed
string $data = $future->wait();
```

---

### 2. Parallel Join with `Future::wait`
Evaluates an array of closures (or Futures) concurrently and returns an array of resolved values:

```php
// Concurrently wait for multiple asynchronous operations
[$fileData, $httpResp, $users] = Future::wait([
    AFile::read('report.csv'),
    AHTTP::get('https://analytics.internal/health'),
    ADB::table('users')->find(42)
]);
```

All tasks run concurrently on separate Tokio worker fibers. Total latency equals the duration of the slowest single task, not the sum of all tasks.

---

### 3. First-to-Complete Resolution with `Future::any`
Returns the result of whichever task finishes first:

```php
$fastestMirror = Future::any([
    AHTTP::get('https://mirror-us.cdn.org/data'),
    AHTTP::get('https://mirror-eu.cdn.org/data')
]);
```

---

### 4. Non-Blocking Delays with `Future::delayed`
Yields the fiber using Tokio timers (`tokio::time::sleep`) without blocking OS worker threads:

```php
$delayedResult = Future::delayed(500, fn() => 'Resumed after 500ms');
```

---

## Post-Response Defer Hooks: `Future::defer`

In high-throughput web applications, user response latency must not be delayed by analytics logging, metrics collection, or cache warming.

`Future::defer()` registers a hook in the request queue that executes **strictly after** the HTTP response payload has been flushed to the network socket, but **before** the request arena drops:

```php
// Flush user response immediately, then run background task before arena drop
Future::defer(function() use ($userId) {
    Log::info("Request served for user {$userId}");
    Analytics::recordHit("/dashboard");
});
```

---

## Concurrency Scope for v1

In line with RHP's v1 core architecture decisions:
- **Single-request execution is strictly sequential by default:** Fine-grained concurrency inside an individual HTTP request is explicitly managed via Tokio async facades (`AFile`, `AHTTP`, `ADB`) or `Future::wait()`, keeping code free of function coloring.
- **High-throughput request concurrency:** Concurrency is achieved at the host level, where the Rust runtime executes many concurrent HTTP requests across a thread pool against the frozen immutable boot heap (`Arc<EngineState>`).


