# RHP Asynchronous Architecture & API Surface Unification

> **Status:** Specification & Design Plan  
> **Subsystem:** Language Standard Library, AST, IR Lowering & Coroutine Runtime  
> **Target Version:** RHP 1.0  

---

## 1. Executive Summary & Problem Diagnosis

### 1.1 The "A* Twins" Anti-Pattern

In previous drafts and early implementations, asynchronous non-blocking I/O was exposed via parallel twin facades:

* `File` vs. `AFile`
* `DB` vs. `ADB` (and `DBQuery` vs. `ADBQuery`)
* `Http` vs. `AHttp` (or `AHTTP`)
* `Redis` vs. `ARedis`
* `Kafka` vs. `AKafka`

This dual-facade strategy created severe architectural liabilities:

1. **Doubled API Surface Area:** Every driver author had to implement and maintain two identical sets of methods, fluent builders, docstrings, type stubs in `src/type_system/env.rs`, and lint checks in `src/lint/casing.rs`.
2. **Fractured Developer Ergonomics:** Developers had to constantly choose between `DB::table(...)` and `ADB::table(...)`, causing cognitive friction and inconsistent coding patterns.
3. **Query Builder Duplication:** The fluent query builder was duplicated into `DBQuery` (synchronous/fiber-suspending) and `ADBQuery` (returning Tokio `VmFuture` handles).

### 1.2 The Unbounded Connection Pool Leak (The Statement-Level Trap)

The most critical issue with `A*` twins was how they interacted with statement lowering in the compiler.

When a developer wrote an unassigned database query:
```php
ADB::table('users')->where('id', $id)->update(['active' => 1]);
```

The AST lowered this as a standard `StmtKind::Expr`:
```text
t0 = FACADE_CALL "ADB", "table", args=1
t1 = METHOD_CALL t0, "where", args=2
t2 = METHOD_CALL t1, "update", args=1
; Statement terminates here!
; Register t2 (holding a VmFuture) is immediately discarded.
; NO Await instruction is emitted!
```

At runtime, `ADB` invoked `tokio_runtime.spawn(async move { ... })`. Because the statement did not assign or await `t2`:
* The `VmFuture` handle was discarded immediately.
* The spawned Tokio task ran **detached** in the background.
* Under heavy load or inside loops, hundreds of detached queries spawned concurrently, checking out connections from the database pool.
* With no caller waiting on them and no structured lifecycle boundaries, database connection pools were rapidly exhausted, leading to connection starvation and deadlocks.

---

## 2. The Unified Concurrency Model

RHP replaces the dual `A*` facade model with **single canonical drivers** and **two distinct, purpose-built concurrency primitives**:

```text
┌──────────────────────────────────────────────────────────────────────────────────┐
│                             CANONICAL DRIVER APIS                                │
│          DB::*   │   File::*   │   Http::*   │   Redis::*   │   Kafka::*         │
└──────────────────────────────────────┬───────────────────────────────────────────┘
                                       │
            ┌──────────────────────────┴──────────────────────────┐
            ▼                                                     ▼
┌──────────────────────────────────────┐  ┌──────────────────────────────────────┐
│     REQUEST-SCOPED CONCURRENCY       │  │    OFFLOADED BACKGROUND WORKERS      │
│  async() / Future::async()           │  │  task() / Task::dispatch()           │
│  Future::concurrent()                │  │                                      │
├──────────────────────────────────────┤  ├──────────────────────────────────────┤
│ • In-request cooperative execution   │  │ • Dedicated Task Worker Process Pool │
│ • Returns Future<T> handle           │  │ • Returns void (strictly non-future) │
│ • Resolved via $fut->wait() or await │  │ • Offloaded CPU/Heavy I/O work       │
│ • Runs within RequestArena (Zero-GC) │  │ • Isolated connection pool           │
│ • Pure closure optimizations applied │  │ • Structured backpressure & queue    │
│ • Zero state-freezing in-request     │  │ • State frozen across processes      │
└──────────────────────────────────────┘  └──────────────────────────────────────┘
```

### 2.1 Canonical Drivers (Single API Surface)
All drivers expose only one API: `DB`, `File`, `Http`, `Redis`, `Kafka`.
When called directly in linear code:
```php
$user = DB::table('users')->find($id);
$data = File::read('report.csv');
$resp = Http::get('https://api.internal/v1/health');
```
Under stackful coroutines (`corosensei`), these operations transparently suspend the current request fiber and await Tokio completion without blocking the OS worker thread.

### 2.2 `Future::async()` & `async()` (Request-Scoped Concurrency)
For fine-grained concurrency within an HTTP request:

```php
// Core API
$future = Future::async(callable $fn): Future;

// Sugar Syntax Helper
$future = async(callable $fn): Future;
```

* **Semantics:** Spawns a cooperative, non-blocking asynchronous task within the current request context.
* **Return Type:** Always returns a typed `Future<T>` instance.
* **Resolution:** Synchronized either via `$future->wait()` or `await $future`.

```php
$userFut = async(fn() => DB::table('users')->find($id));
$ordersFut = async(fn() => DB::table('orders')->where('user_id', $id)->get());

// Resolve when ready
$user = $userFut->wait();
$orders = $ordersFut->wait();
```

### 2.3 `Future::concurrent()` (Parallel Task Join Barrier)
Replaces `Future::all()` and `Future::wait()`:

```php
$future = Future::concurrent(array $tasks): Future;
```

* **Semantics:** Evaluates an array of callables or `Future` instances concurrently on the worker's event loop.
* **Return Type:** Returns a `Future<array>` containing the ordered results.
* **Ergonomics:** Integrates cleanly with destructuring and `$future->wait()`:

```php
// Concurrently fetch profile, orders, and notifications
[$user, $orders, $notifications] = Future::concurrent([
    fn() => DB::table('users')->find($userId),
    fn() => DB::table('orders')->where('user_id', $userId)->get(),
    fn() => Redis::get("notifications:{$userId}"),
])->wait();
```

### 2.4 `Task::dispatch()` & `task()` (Offloaded Background Work)
For fire-and-forget background jobs, heavy computations, or operations that must outlive the HTTP request:

```php
// Core API
Task::dispatch(callable $fn, ...$args): void;

// Sugar Syntax Helper
task(callable $fn, ...$args): void;
```

* **Return Type:** **`void`**. It cannot be awaited.
* **Execution Target:** Dispatched to the dedicated **Task Worker Process Pool** via high-speed, non-blocking IPC channels.
* **Resource Safety:** Task Workers run in isolated processes with their own independent connection pools and memory pools. Fire-and-forget background tasks **cannot starve or leak** the request worker's database pool.

```php
// Fast HTTP response; heavy task processed in background
task(function() use ($orderId, $customerEmail) {
    $pdf = ReportGenerator::generateInvoice($orderId);
    Mailer::send($customerEmail, $pdf);
});
```

---

## 3. Interaction with Stackful Coroutines (`corosensei`)

RHP's execution model is powered by stackful fibers (`corosensei`), giving it Swoole-like performance without function coloring. The unified concurrency primitives map natively onto this fiber architecture:

### 3.1 Transparent Fiber I/O Hook (`DB::*`, `File::*`, `Http::*`)
```text
[Request Fiber Stack] ──► DB::find('users', 1)
                                │
                                ▼
               Yielder::suspend(WaitingIo(DbQuery))
                                │
     ┌──────────────────────────┴──────────────────────────┐
     ▼ (Register Swap: Saves Fiber Stack)                  ▼
[Suspended Fiber Stack]                           [Worker Tokio Event Loop]
• Call frames & local variables preserved        • Tokio executes non-blocking query on epoll
• Zero OS thread sleep                           • Worker thread services other requests
     │                                                     │
     │ (Tokio Query Completes)                             │
     └──────────────────────────┬──────────────────────────┘
                                ▼
               Coroutine::resume(IoComplete(row))
                                │
                                ▼
                     `$user` assigned & returned!
```

### 3.2 `Future::concurrent()` Under `corosensei` (Zero-OS-Blocking Join)
When `Future::concurrent()` is invoked:
1. The worker scheduler creates cooperative sub-fibers for each callable on the **same worker thread's event loop**.
2. Sub-fiber 1 begins executing and suspends on its first I/O call (`DB::find`).
3. Sub-fiber 2 immediately executes on the event loop and suspends on its I/O call (`DB::get`).
4. Both I/O operations are registered on Tokio's `epoll` reactor simultaneously.
5. The parent request fiber calls `Yielder::suspend(WaitingConcurrentJoin)`.
6. Total elapsed wall-clock time is $\max(T_1, T_2)$ instead of $T_1 + T_2$.
7. **Zero OS threads are blocked**; other incoming HTTP requests continue processing concurrently.

### 3.3 `Task::dispatch()` Under `corosensei` ($O(1)$ IPC Push)
When `task()` or `Task::dispatch()` is called from a fiber:
1. The closure and arguments are serialized into an immutable message.
2. The message is pushed onto the worker's lock-free channel queue connected to the Task Worker process pool.
3. The queue push is non-blocking ($O(1)$).
4. **The fiber never yields or suspends**, continuing execution instantly.

---

## 4. Pure Closure Optimization & State Freezing Elision

A major advantage of unifying around `async()` and `Future::concurrent()` is how they leverage RHP's **Pure Closure Optimization**:

```text
Closure Purity Analysis (src/expand/pure_closure.rs)
├── PureNonCapturing   (0 upvalues, no $this)  -> Direct Cranelift function pointer
├── PureThisOnly       (captures only $this)   -> 16B inline struct
├── PureValueSmall(N)  (captures <= 2 values)  -> 24-32B inline struct (no heap alloc)
└── ImpureReference(N) (use (&$var))           -> Explicit heap cell
```

### 4.1 In-Request Concurrency: Eliding `FrozenValue`
Previously, `Future::run()` spawned an OS thread and required deep state conversion:
`PhpValue` $\to$ `FrozenValue` $\to$ `PhpValue`.

Under the unified stackful coroutine model:
* **Same-Thread Execution:** `async()` and `Future::concurrent()` execute sub-fibers on the **same worker process and thread**.
* **Shared Request Arena:** All sub-fibers allocate within the calling request's `RequestArena`.
* **Zero State Freezing:** Closures in `Future::concurrent()` and `async()` do **not** need to convert values to `FrozenValue`. They read captured variables directly from the arena, eliminating serialization overhead and memory copying.

### 4.2 Cross-Process Tasks: Retaining `FrozenValue`
* `Task::dispatch()` sends jobs across process boundaries to the Task Worker pool.
* Here, immutable state conversion via `FrozenValue` (or compact binary IPC serialization) is preserved, ensuring complete memory isolation and zero data races between worker processes.

---

## 5. AST & IR Lowering Transformations

### 5.1 First-Class `async()` Lowering
In `src/ir/lower.rs`:
```rust
// AST Call to `async($fn)` desugars directly to Future::async($fn)
if clean_fn == "async" && args.len() == 1 {
    let arg_count = self.evaluate_and_push_args(b, args)?;
    let facade_const = b.intern_string("Future");
    let method_const = b.intern_string("async");
    b.emit(
        OpCode::FacadeCall {
            dst,
            facade: facade_const,
            method: method_const,
            arg_count,
        },
        expr.span,
    );
    return Ok(dst);
}
```

### 5.2 Enabling the `await` Keyword
In `src/parser/pratt.rs`, the artificial error rejecting `await` is lifted:
```rust
Token::Await => {
    let inner = self.parse_expr(binding_power::PREFIX)?;
    let span = spanned.span.merge(inner.span);
    Ok(Expr::new(ExprKind::Await(Box::new(inner)), span))
}
```
`ExprKind::Await` lowers directly to `OpCode::Await { dst, future }`, which in turn delegates to the fiber scheduler or `VmFuture::wait_result()`.

### 5.3 Statement-Level Future Discard Detection (Lint & Safety)
To permanently eliminate the silent connection leak bug:
When lowering [`StmtKind::Expr(expr)`](file:///home/yogesh/code/rhp/src/ir/lower.rs#L462):
1. The compiler checks if `expr` has inferred type `RhpType::Future(_)`.
2. If an expression produces a `Future` in statement position without `await` or assignment, the compiler emits a diagnostic:
   > **Warning (`UnawaitedFuture`):** Statement creates a `Future` that is immediately discarded. This will not execute asynchronously as fire-and-forget. Use `await` or `->wait()` to synchronize, or use `task(fn() => ...)` to offload to background workers.

---

## 6. Deprecation & Migration Roadmap for `A*` Twins

To ensure backward compatibility while completing the unification:

1. **Phase 1 (Desugaring & Deprecation Warning):**
   * Calls to `ADB::*`, `AFile::*`, `AHTTP::*` continue to compile, but the compiler desugars them:
     * `ADB::table('users')->get()` $\to$ `async(fn() => DB::table('users')->get())`
     * `AFile::read($path)` $\to$ `async(fn() => File::read($path))`
   * The linter emits a deprecation notice pointing developers to canonical facades with `async()` or `Future::concurrent()`.
2. **Phase 2 (Query Builder Unification):**
   * Merge `ADBQuery` methods into `DBQuery`.
   * Standardize all query operations to return values synchronously (or suspend fibers), delegating concurrency entirely to `async()` and `Future::concurrent()`.
3. **Phase 3 (Removal):**
   * Remove `AFile`, `AHTTP`, `ADB`, `ARedis`, `AKafka` from the standard facade registry.
   * Total driver API surface cut in half.

---

## 7. API Summary Matrix

| API | Syntax | Return Type | Fiber Action | Process/Thread Target | State Freezing? |
|---|---|:---:|---|---|:---:|
| **Standard I/O** | `DB::find(...)` | `mixed` | Suspends fiber until I/O ready | Local worker event loop (Tokio epoll) | No |
| **Async Spawn** | `async(fn() => ...)` | `Future<T>` | Returns future immediately | Local worker sub-fiber | No (RequestArena) |
| **Concurrent Join**| `Future::concurrent([...])` | `Future<list>`| Suspends until all tasks ready | Local worker sub-fibers | No (RequestArena) |
| **Future Await** | `$fut->wait()` / `await $fut` | `T` | Suspends until future completes | Local worker event loop | No |
| **Task Dispatch** | `task(fn() => ...)` | `void` | Never suspends ($O(1)$ IPC push) | Task Worker Process Pool | Yes (`FrozenValue`) |
