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
  - If you initiate non-blocking background tasks, you receive a `Future` handle and resolve it by calling `$future->wait()` or composing tasks with `Future::all([ ... ])`.
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

### 2. Parallel Join with `Future::all` / `Future::join`
Evaluates an array of closures (or Futures) concurrently and returns an array of resolved values. In alignment with Rust/Tokio conventions, `Future::join([...])` and the global `join([...])` helper are available:

```php
// Concurrently wait for multiple asynchronous operations
[$fileData, $httpResp, $users] = Future::join([
    fn() => File::read('report.csv'),
    fn() => HTTP::get('https://analytics.internal/health'),
    fn() => DB::table('users')->find(42)
])->wait();

// Or using global join sugar
[$users, $stats] = join([
    fn() => DB::table('users')->get(),
    fn() => DB::table('stats')->get(),
])->wait();
```

All tasks run concurrently on separate Tokio worker fibers. Total latency equals the duration of the slowest single task, not the sum of all tasks.

---

### 3. First-to-Complete Race with `Future::select` / `Future::any`
Races multiple concurrent tasks and resolves immediately with the result of whichever completes first (Tokio's `select!` pattern):

```php
$winner = Future::select([
    function() {
        Future::sleep(500)->wait();
        return "slow";
    },
    fn() => "fast",
])->wait(); // "fast"

// Global select sugar
$fastestMirror = select([
    fn() => HTTP::get('https://mirror-us.cdn.org/data'),
    fn() => HTTP::get('https://mirror-eu.cdn.org/data'),
])->wait();
```

---

### 4. Timeouts & Fallback with `Future::timeout`
Guards operations against hung network requests or slow database queries by enforcing a maximum elapsed deadline in milliseconds:

```php
// Static helper with fallback
$result = Future::timeout(
    HTTP::get('https://external-service.com/api'),
    250, // 250ms deadline
    ['cached' => true] // fallback value if timed out
)->wait();

// Fluent instance method
$result = $future->timeout(500, "fallback_response")->wait();
```

---

### 5. Non-Blocking Timers with `Future::sleep`
Yields the fiber non-blockingly using Tokio timers (`tokio::time::sleep`) without occupying OS worker threads:

```php
// Yield fiber for 100 milliseconds
Future::sleep(100)->wait();
```

---

## CPU-Heavy & Blocking Operations: `Task::blocking`

Because RHP's fiber runtime relies on **cooperative scheduling**, executing a long CPU loop or blocking C library function on a request fiber starves the worker event loop.

To prevent starvation, RHP provides `Task::blocking()` (equivalent to Tokio's `tokio::task::spawn_blocking`):

```php
// Offload CPU-heavy computation or blocking C tasks to a dedicated thread pool
$hash = Task::blocking(function() use ($largePayload): string {
    return Crypt::hash("sha256", $largePayload);
})->wait();
```

---

## Offloaded Background Workers: `Task::dispatch` & `task()`

For fire-and-forget background jobs that must run independently without holding database connections or delaying the HTTP response:

```php
// Dispatch to dedicated task worker pool (returns void, non-future)
task(fn() => DB::table('audit_logs')->insert(['event' => 'order_placed']));

// Facade equivalent
Task::dispatch(fn() => File::write('analytics.log', 'event'));
```

---

## Concurrency Rate Limiting: `Semaphore`

Exposing Tokio's `tokio::sync::Semaphore` directly to prevent overwhelming upstream APIs or database connection pools:

```php
// Create a permit coordinator with 5 concurrent slots
$sem = Semaphore::create(5);

// Check available permits
Sys::print("Available: {}", $sem->available());

// Scoped execution with automatic permit release
$response = $sem->run(fn() => HTTP::get('https://rate-limited-api.com/query'));

// Explicit acquire / release
if ($sem->tryAcquire()) {
    try {
        doSomething();
    } finally {
        $sem->release();
    }
}

// Scoped one-liner
$data = Semaphore::with(10, fn() => DB::table('reports')->get());
```

---

## Post-Response Defer Hooks: `Future::defer` & `defer()`

In high-throughput web applications, user response latency must not be delayed by analytics logging, metrics collection, or cache warming.

`Future::defer()` (or global helper `defer()`) registers a hook in the request queue that executes **strictly after** the HTTP response payload has been flushed to the network socket, but **before** the request arena drops:

```php
// Flush user response immediately, then run background task before arena drop
defer(function() use ($userId) {
    Log::info("Request served for user {$userId}");
    Analytics::recordHit("/dashboard");
});
```

---

## Compiler Safety: The `unawaited-future` Warning

To prevent silent database connection pool exhaustion and dangling asynchronous operations:
- When an expression in statement position returns a `Future` that is immediately discarded without being awaited (`await` / `->wait()`) or assigned, the compiler emits:
  > `[unawaited-future] Statement creates a Future that is immediately discarded without being awaited or joined. Cooperative tasks will not advance as fire-and-forget; use 'await', '->wait()', 'Future::join()', or 'Task::dispatch()' instead.`

---

## Rust & Tokio Mental Model Alignment

| Rust / Tokio Concept | RHP Equivalent | Explanation |
|---|---|---|
| **I/O-Bound Work** | Request fibers + Tokio Reactor | Non-blocking socket I/O suspends fibers, freeing OS threads to process other requests. |
| **CPU-Bound Work** | `Task::blocking(fn)` | Offloads heavy CPU calculation to dedicated OS thread pools, avoiding event loop starvation. |
| **Concurrency vs Parallelism** | Fibers (Concurrent) vs Worker Pool (Parallel) | Cooperative fiber interleaving within threads; multi-process parallel execution across cores. |
| **Work-Stealing** | Tokio Multi-Thread Runtime | Worker threads automatically steal ready tasks to eliminate idle core imbalances. |
| **Cooperative Yield Points** | Non-blocking I/O, `sleep`, `wait` | Fibers yield explicitly at I/O and wait points; loops without yield points starve threads. |
| **Cancellation Safety** | `Future::timeout($fut, $ms, $fallback)` | Operations with timeouts can safely abort or return fallbacks without leaking resources. |
| **`Send` / Thread Safety** | `FrozenValue` | Cross-thread tasks pass deeply frozen immutable data, making data races impossible. |



