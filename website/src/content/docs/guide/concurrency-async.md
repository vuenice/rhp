---
title: "Concurrency and Async"
description: "Master Tokio fiber concurrency, Dart-style Future APIs, parallel resolution, and post-response defer hooks in RHP."
section: "Language Guide"
order: 8
---

## Concurrency Architecture: Synchronous by Default

RHP adopts a **synchronous-by-default execution model**:

- Standard I/O operations (such as `File::read()` or `DB::table()->find()`) execute sequentially on local fibers without callback hell or tedious `await` prefixes everywhere.
- When you need true parallelism, you explicitly offload tasks onto the multi-threaded Tokio worker pool using the native `Future` facade.

---

## Dart-Style `Future` Facade API

Unlike JavaScript Promises which eagerly execute on creation, or Go goroutines which require explicit waitgroups and channel plumbing, RHP provides a clean Dart-style `Future` facade:

### 1. Spawning Tasks with `Future::run`
Spawns an asynchronous task on the Tokio background thread pool:

```php
$future = Future::run(function(): string {
    return File::read('report.csv');
});

// Explicitly await the result when needed
string $data = $future->wait();
```

---

### 2. Parallel Join with `Future::wait`
Evaluates an array of closures (or Futures) concurrently and returns an array of resolved values:

```php
[$user, $orders, $metrics] = Future::wait([
    fn() => DB::table('users')->find(42),
    fn() => DB::table('orders')->where('user_id', 42)->get(),
    fn() => HTTP::get('https://analytics.internal/health')->json()
]);
```

All three tasks run concurrently on separate Tokio worker fibers. Total latency equals the duration of the slowest single task, not the sum of all three.

---

### 3. First-to-Complete Resolution with `Future::any`
Returns the result of whichever task finishes first:

```php
$fastestMirror = Future::any([
    fn() => HTTP::get('https://mirror-us.cdn.org/data')->json(),
    fn() => HTTP::get('https://mirror-eu.cdn.org/data')->json()
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
- **Single-request execution is strictly sequential:** Fine-grained concurrency inside an individual HTTP request is explicitly out of scope for v1.
- **High-throughput request concurrency:** Concurrency is achieved at the host level, where the Rust runtime executes many concurrent HTTP requests across a thread pool against the frozen immutable boot heap (`Arc<EngineState>`).

