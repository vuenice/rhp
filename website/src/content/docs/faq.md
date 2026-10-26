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

## How does RHP handle the borrow checker?

RHP eliminates the cognitive burden of Rust's borrow checker for application developers while preserving Rust's memory safety guarantees:

- **For script developers:** You do not write manual lifetimes (`'a`) or borrow annotations. Memory safety and Zero-GC execution are achieved through language constraints (forbidding pass-by-reference `&$var`), automatic Copy-On-Write (CoW) collection detachment, and $O(1)$ request bump arenas.
- **For the runtime engine:** The underlying VM loop is built in Rust using scoped interior mutability (`Rc<RefCell<...>>`) and statically enforces thread safety by requiring cross-thread values to be converted into immutable `FrozenValue` (`Send + Sync`).

For full architectural details, see the dedicated section in [Memory Model: Borrow Checker and Memory Safety](/docs/advanced/memory-model#borrow-checker).

---

## What file extension should I use?

RHP officially supports both:
- `.rhp`: Standard extension for modern RHP projects.
- `.php`: Supported for drop-in migration workflows and editor compatibility.

---

## How do typed variable declarations and type casting work in RHP?

Unlike TypeScript, Hack, or legacy PHP, RHP enforces **prefix type declarations** matching C/Rust conventions:
```php
int $x = 10;          // Supported prefix syntax
$x: int = 10;         // Rejected: ParseError (colon syntax not supported)
```

Similarly, RHP replaces legacy PHP / C-style parenthesized casts with **functional conversions**:
```php
$id = int($raw);      // Supported functional syntax
$id = (int)$raw;      // Rejected: ParseError (parenthesized cast not supported)
$str = string($id);   // Supported functional syntax
```

Both rules ensure consistent parsing, avoid grammar ambiguities with grouping parentheses and object instantiation, and provide immediate, helpful compiler diagnostics if old syntax is encountered.

---

## Why are superglobals like `$_ENV`, `$_FILES`, `$_SESSION`, `$_COOKIE`, and `$GLOBALS` not supported, and how do I handle them?

Traditional PHP superglobals are mutable global arrays that lead to state pollution and race conditions across concurrent requests in multi-threaded worker pools, while lacking strict type safety.

In RHP:
- **`env()`**: **`$_ENV` is not supported**. Read system environment variables via `env('KEY', 'default')` or `Sys::env('KEY')`.
- **`File::fromRequest()`**: **`$_FILES` is not supported**. Uploaded files from multi-part requests are retrieved as structured file instances via `File::fromRequest('upload_field')`.
- **`Session::get()`**: **`$_SESSION` is not supported**. Session data is managed through the thread-isolated `Session` facade: `Session::get('user_id')`, `Session::set('user_id', 42)`, `Session::has('user_id')`, and `Session::clear()`.
- **`Cookie::get()`**: **`$_COOKIE` is not supported**. Read and set HTTP cookies via `Cookie::get('theme', 'light')` and `Cookie::set('theme', 'dark')`.
- **`Glob::`**: **`$GLOBALS` is also not supported**. Dynamic helper calls fallback to `Glob::customHelper()`, and global variables are declared lexically using `global $var;`.
- **`Request::get()`**: `$_GET` and `$_POST` are deprecated in favor of `Request::get('key')`, `Request::all()`, and `Request::only()`.

### Request Type Formatting with `Request::format()`
All incoming query and body request values are strings by default. Use `Request::format()` to validate and convert parameter types on the request object:

```php
Request::format([
    'page' => 'int',
    'active' => bool,
]);

int $page = Request::get('page');     // 1
bool $active = Request::get('active'); // true
```

Both quoted strings (`'int'`, `'bool'`) and bare type keywords (`int`, `bool`) are supported in `Request::format()`. The parser deterministically resolves bare keywords in expression positions without ambiguity against statement-level typed variable declarations (`int $x = 10;`).

---

## How does RHP resolve the script file when running `rhp run dev` or `rhp run build`?

When executing `rhp run dev` or `rhp run build`:
1. **Default (`pwd`):** If no file is specified, RHP automatically looks for `index.rhp` in the current working directory (`pwd`).
2. **Explicit `--file` Argument:** If `--file` is provided (e.g. `rhp run build --file="/your-path/server.rhp"` or `rhp run dev --file="server.rhp"`), RHP executes that specific file. The short flag `-f` is also supported.
3. **Positional Argument:** You can also pass the script path directly as a positional argument (e.g. `rhp run dev server.rhp`).

---

## Why is extending core facades (`class CustomArr extends Arr`) forbidden, and how do macros work?

In traditional PHP, developers frequently use class inheritance to extend helper utilities (`class CustomArr extends Arr`). In RHP, **extending core static facades via inheritance is strictly forbidden at compile time**:

1. **Tight Coupling to Rust Internals:** Core helper classes like `Arr` and `Str` are static facades wrapping optimized Rust heap allocations (such as `Box<Vec<PhpValue>>`, `Box<IndexMap<String, PhpValue>>`, or UTF-8 string buffers). Allowing inheritance would expose these internal memory boundaries and complicate the VM's Cranelift JIT optimization pipeline.
2. **Stateless Design Violations:** `Arr` and `Str` are intentionally designed as stateless utility wrappers and fluent pipeline builders. Subclassing them introduces state management confusion and severe thread-safety risks in a multi-threaded Tokio execution environment.

Attempting to extend `Arr` results in a fatal compile error:
```text
Fatal Error: Cannot extend static facade class 'Arr'. Use 'Arr::macro()' to register custom methods dynamically.
```

Instead of extending the class via inheritance, developers register custom methods dynamically via `::macro()`:
```php
// Define a custom macro on the Arr facade
Arr::macro('toCsv', function ($delimiter = ',') {
    // Custom logic accessing the underlying collection
    return implode($delimiter, $this->all());
});

// Usage anywhere in your application
$csv = Arr::list([1, 2, 3])->toCsv();
```

**Why Macros Work Perfectly for RHP:**
* **Zero Performance Cost on Core Types:** Macros are stored in a global thread-safe runtime registry rather than altering the core class definition or Rust struct layouts.
* **Maintains Familiarity:** Matches the exact developer ergonomics of Laravel's `Macroable` trait, making migration seamless for teams upgrading high-traffic apps.
* **Thread-Safe Registration:** Macros can be registered once during application bootstrap (`server.rhp`) in the parent process, making them safely accessible across all concurrent worker threads with zero mutex contention on request paths.


