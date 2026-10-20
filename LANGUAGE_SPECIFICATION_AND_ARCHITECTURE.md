# Language Specification & Architecture Document

## Table of Contents
- [Executive Summary & Design Decisions](#executive-summary--design-decisions)
  - [1. Type System Strategy: Medium (Gradual / Surface-Level Contracts)](#1-type-system-strategy-medium-gradual--surface-level-contracts)
  - [2. Type System & Numeric Datatypes](#2-type-system--numeric-datatypes)
- [Technical Architecture (Performance-First Paradigm)](#technical-architecture-performance-first-paradigm)
  - [Process & Threading Architecture](#process--threading-architecture)
  - [1. Compiler Pipeline & Execution Engine](#1-compiler-pipeline--execution-engine)
  - [2. Standard Library & Explicit Subtype System](#2-standard-library--explicit-subtype-system)
  - [Facade Library Pattern](#facade-library-pattern)
  - [Facade Tiering Architecture](#facade-tiering-architecture)
  - [Facade Extensibility: Runtime Macro Registration vs. Class Inheritance](#facade-extensibility-runtime-macro-registration-vs-class-inheritance)
- [Facades & Utility Helpers](#facades--utility-helpers)
  - [1. Arr Facade](#1-arr-facade)
  - [2. Str Facade](#2-str-facade)
  - [3. HTTP Facade](#3-http-facade)
  - [4. File Facade](#4-file-facade)
  - [5. Session Facade](#5-session-facade)
  - [6. Image Facade](#6-image-facade)
  - [7. URL Facade](#7-url-facade)
  - [8. Trace & Telemetry Facades (OpenTelemetry Observability)](#8-trace--telemetry-facades-opentelemetry-observability)
- [Asynchronous Non-Blocking Concurrent Core](#asynchronous-non-blocking-concurrent-core)
- [Process Model & Database Pooling](#process-model--database-pooling)
- [Dual-Engine Execution Modes (Development vs Production Mode)](#dual-engine-execution-modes-development-vs-production-mode)
  - [Overview & Execution Modes Strategy](#overview--execution-modes-strategy)
  - [Developer Experience Framework (`rhp run dev`)](#developer-experience-framework-rhp-run-dev)
  - [Implementation Code Sketch](#implementation-code-sketch)
  - [Execution Matrix](#execution-matrix)
- [Technical Language Construction Architecture (20 Technical Points)](#technical-language-construction-architecture-20-technical-points)

---

## Executive Summary & Design Decisions

### 1. Type System Strategy: Medium (Gradual / Surface-Level Contracts)

- **Function & Method Boundaries:** Enforces strict parameter and return types across signatures to guarantee API predictability, IDE intelligence, and compile-time boundary safety.
- **Function Bodies:** Evaluated using dynamic inference and assignment, preserving rapid prototyping, clean syntax, and low syntactic overhead.
- **Reference Mechanics:** Explicit pass-by-reference (`&$var`) is removed in favor of explicit returns and mutable collection wrappers. Objects pass by reference handle, while arrays/strings utilize Copy-On-Write (COW) semantics.
- **Strict Type Safety & No Implicit Type-Shifting:** RHP enforces strict type boundaries akin to Rust. Once a variable holds a specific declared type (e.g. `int $x = 0;`), you cannot implicitly reassign it to an incompatible type (`$x = 'hello';` throws a Rust-style `VariableTypeMismatch` compile/runtime error).

### 2. Type System, Type Hints & Casting Syntax

| Feature | Syntax | Architectural Rationale |
| :--- | :--- | :--- |
| **Type Casting** | `(int)$x`, `(string)$x` | Matches native PHP casting; avoids confusion with object instantiation (`new Class()`) or function invocations. |
| **Scalar Type Hints** | `int $x`, `string $x`, `float $x`, `bool $x` | Strictly lowercase primitive keywords for function boundaries. Capitalized variants (e.g., `String`, `Integer`) are strictly rejected at parse and check time. |
| **Collection Type Hints** | `List $arr`, `Dict $arr`, `Set $arr`, `HashMap $arr` (or lowercase `list`, `dict`, `set`, `hashmap`) | Explicit high-performance collection subtypes backed by dedicated Rust heap structures (`Box<Vec<PhpValue>>`, `Box<IndexMap>`). Enforces strict type boundaries. |
| **Permissive Parent Type** | `array $arr = [1, 2, 3];`<br/>`function myFunc(): array` | Permissive parent collection type for backward compatibility. Accepts all collection variants (`List`, `Dict`, `Set`, `HashMap`) interchangeably behind a memory-safe pointer. |

- **Dynamic Typing via PhpValue & Flexible Reassignment:** Untyped variables (or variables initialized with standard PHP array syntax) store values dynamically under the hood using a unified type enum (`PhpValue`). Developers can easily reassign an untyped or `array`-hinted variable from a sequential list structure to a dictionary or map structure without throwing a type error:
  ```php
  $arr = [1, 2, 3];
  $arr = ['name' => 'yogesh']; // Seamless flexible reassignment

  array $arr = [1, 2, 3];
  $arr = ['name' => 'yogesh']; // Supported via permissive parent array type
  ```
- **Strict Type Bounds vs. Dynamic Variables:** While dynamic flexibility applies to untyped and generic `array`-hinted variables, once a variable or parameter is explicitly type-hinted with a strict collection subtype (like `list $arr` or `dict $arr`) or a primitive scalar (like `int $x`), RHP enforces strict type boundaries and throws a Rust-style value/type error if assigned an incompatible type.
- **Default Integers and Floats:** Default/standard integer and float types use `i32` (`Int` and `Float` wrappers/primitives).
- **Extended Rust Numeric Types & Fluent APIs:** Developers can also use all other native Rust numeric types in our language (such as `i64`, `f64`, `u32`, etc.). They can be constructed and manipulated fluently via type wrappers, e.g.:

```php
Int::i64()->random()->add()->subtract()

Float::i64()->toDecimal(2)
```

---

## Technical Architecture (Performance-First Paradigm)

### Process & Threading Architecture

```mermaid
flowchart TD
    subgraph Parent["PARENT PROCESS"]
        direction TB
        P1["- Bootstraps Framework & Application Code"]
        P2["- Holds Immutable Global State & Routing Tables"]
        P3["- Manages Shared Connection Pools (sqlx / Diesel)"]
        P4["- Centralized Async Logging Manager"]
    end

    Parent --> Bridge["Shared Memory / Arc  |  IPC / Direct Thread Dispatch"]

    Bridge --> W1["WORKER THREAD 1<br/>(Tokio Fiber Execution)"]
    Bridge --> W2["WORKER THREAD 2<br/>(Tokio Fiber Execution)"]
    Bridge --> W3["WORKER THREAD 3<br/>(Tokio Fiber Execution)"]
```

```text
┌─────────────────────────────────────────────────────────┐
│                     PARENT PROCESS                      │
│ - Bootstraps Framework & Application Code               │
│ - Holds Immutable Global State & Routing Tables         │
│ - Manages Shared Connection Pools (sqlx / Diesel)       │
│ - Centralized Async Logging Manager                     │
└───────────────────────────┬─────────────────────────────┘
                            │
┌───────────────────────────┴─────────────────────────────┐
│ Shared Memory / Arc       │ IPC / Direct Thread Dispatch │
└───────────────────────────┬─────────────────────────────┘
              ┌─────────────┼─────────────┐
              ▼             ▼             ▼
┌───────────────────────┐ ┌───────────────────────┐ ┌───────────────────────┐
│    WORKER THREAD 1    │ │    WORKER THREAD 2    │ │    WORKER THREAD 3    │
│(Tokio Fiber Execution)│ │(Tokio Fiber Execution)│ │(Tokio Fiber Execution)│
└───────────────────────┘ └───────────────────────┘ └───────────────────────┘
```

### 1. Compiler Pipeline & Execution Engine

- **Lexer:** Powered by `logos` for ultra-fast, deterministic finite automata (DFA) tokenization.
- **Parser:** Custom Pratt Parser combined with expression combinators to generate a typed Abstract Syntax Tree (AST).
- **Execution Model:** Lowers AST into a linear Intermediate Representation (IR) opcode stream executed by a register-based Bytecode VM written natively in Rust. Hot paths can be conditionally JIT-compiled via Cranelift.
- **Reference:** Take reference from [https://github.com/tagua-vm/tagua-vm](https://github.com/tagua-vm/tagua-vm)

### 2. Standard Library & Explicit Subtype System

Standard arrays are broken down into four underlying high-performance Rust types. To keep the parent array handle size small on the stack and avoid inline copy overhead, the inner array variants/representations utilize ref-counted interior mutability with Copy-On-Write (COW) semantics:

| Surface Type | Underlying Rust Representation | Primary Use Case |
| :--- | :--- | :--- |
| `Arr::list()` | `Rc<RefCell<Vec<PhpValue>>>` | Sequential indexed data, maximum CPU cache locality |
| `Arr::dict()` | `Rc<RefCell<IndexMap<String, PhpValue>>>` | Key-value map retaining strict insertion order |
| `Arr::set()` | `Rc<RefCell<HashSet<PhpValue>>>` | Unique collections with $O(1)$ membership and set algebra |
| `Arr::hashMap()` / `Arr::map()` | `Rc<RefCell<HashMap<PhpValue, PhpValue>>>` | Universal key-value map supporting complex keys |

### Facade Library Pattern

Global helper functions are eliminated in favor of object-oriented class facades (e.g., `Str::contains()`, `Arr::list()`, `Sys::env()`).

### Facade Tiering Architecture

- **Tier 1 (Built-in Native Facades):** Ship core facades (`Str`, `Arr`, `Future`, `Memory`, `Sys`, `DB`) pre-compiled inside the engine core.
- **Tier 2 (Custom Rust Plugins & Direct `.rs` File Imports):** Allows importing `.rs` files directly (`import "./Cache.rs";` or `use "./Cache.rs" as Cache;`), exposing high-performance helper classes (such as `Cache`, `Log`, `RateLimiter`, and custom extensions) without polluting the core language with non-core dependencies. These are compiled AOT via `rustc --crate-type cdylib` with SHA-256 caching or executed via the thread-safe in-process engine.
- **Tier 3 (Script-land Facades):** Allow standard facades written in your class static function for simple, non-performance-critical user utilities.

### Facade Extensibility: Runtime Macro Registration vs. Class Inheritance

#### Why Traditional Class Extension (`extends Arr`) is Forbidden

1. **Tight Coupling to Rust Internals:** Core helper classes like `Arr` and `Str` are not regular user-land classes—they are static facades wrapping optimized Rust heap allocations (like `Box<Vec<PhpValue>>` or UTF-8 string buffers). Allowing inheritance (`class CustomArr extends Arr`) would expose these internal memory boundaries and complicate the VM's optimization pipeline.
2. **Stateless Design Violations:** `Arr` and `Str` are intentionally designed as stateless utility wrappers and fluent pipeline builders. Subclassing them introduces state management confusion and thread-safety risks in a multi-threaded Tokio execution environment.

Attempting to extend a static facade results in a compile-time fatal error:
```text
Fatal Error: Cannot extend static facade class 'Arr'. Use 'Arr::macro()' to register custom methods dynamically.
```

#### Runtime Macro Registration (`::macro`)

Instead of extending the class via inheritance, developers can register custom methods dynamically at runtime:

```php
// Define a custom macro on the Arr facade
Arr::macro('toCsv', function ($delimiter = ',') {
    // Custom logic accessing the underlying collection
    return implode($delimiter, $this->all());
});

// Usage anywhere in your application
$csv = Arr::list([1, 2, 3])->toCsv();
```

#### Why Macros Work Perfectly for RHP

- **Zero Performance Cost on Core Types:** Macros are stored in a global runtime registry rather than altering the core class definition or Rust struct layouts.
- **Maintains Familiarity:** Matches the exact developer ergonomics of Laravel (`Macroable`), making migration seamless for teams upgrading high-traffic apps.
- **Thread-Safe Registration:** Macros can be registered once during the initial application bootstrap phase in the parent process, making them safely accessible across all concurrent worker threads.

### Global Helpers & Backward-Compatible AST Lowering (`Sys::*`, `phpFun::*`, & `Glob::customHelper`)

To preserve strict object-oriented facade consistency while supporting developer-friendly shorthand syntax, legacy PHP standard library functions, and user-defined global helpers, the **Pratt Parser** performs static symbol resolution and AST lowering during expression parsing:

1. **Why Not the Lexer?** The `logos` DFA lexer is strictly responsible for zero-copy tokenization and lacks grammatical context to distinguish variable names, class names, keywords, or function invocations.
2. **Pratt Parser Symbol Resolution & AST Rewriting:** When the Pratt parser encounters a function call expression (`callee(args)`), it checks:
   - **Reserved Intrinsics (`env`, `print`, `println`, `echo`, `dd`):** Rewritten directly into `Sys::env(...)`, `Sys::print(...)`, `Sys::echo(...)`, and `Sys::dd(...)` static facade calls (`ExprKind::StaticCall`), ensuring zero-copy engine optimizations and exact source span tracking.
   - **Declared User Functions:** Preserved as standard user function call nodes (`ExprKind::Call`).
   - **Legacy PHP Standard Functions (`phpFun::old_php_function()`):** Recognized via a static key-value mapping registry (`LEGACY_PHP_FUNCTIONS` / `php_function_signature_map()`) pairing legacy PHP function names (such as `array_merge`, `implode`, `strlen`, `file_get_contents`) with their canonical RHP facade call signatures (e.g., `"array_merge" => "Arr::merge(array ...$arrays): array"`). The parser rewrites calls like `array_merge($a, $b)` into `phpFun::array_merge($a, $b)` which bridges directly to `Arr::merge($a, $b)`.
   - **Unrecognized Global Helpers (Backward Support):** Automatically intercepted and lowered into `Glob::customHelper('helper_name', ...args)`, routing legacy or dynamically registered custom helpers through the `Glob` namespace container without polluting the global namespace or adding runtime lookup overhead in Development or Production modes.

```rust
let call_node = if is_reserved_intrinsic(callee_name) {
    // Rewrites to Sys::* (Sys::env(), Sys::print(), Sys::echo(), Sys::dd())
    let method = match callee_name {
        "println" => "print",
        other => other,
    };
    Expr::static_call("Sys", method, args, callee_span, span)
} else if self.is_user_defined_function(callee_name) {
    // Calls standard user function
    Expr::user_function_call(callee_name, args, callee_span, span)
} else if is_legacy_php_function(callee_name) {
    // Rewrites legacy PHP function (e.g. array_merge) to phpFun::array_merge() -> Arr::merge()
    Expr::static_call("phpFun", callee_name, args, callee_span, span)
} else {
    // Fallback: Transforms legacy custom helper into Glob::customHelper()
    Expr::static_call("Glob", "customHelper", custom_args, callee_span, span)
};
```

---

## Facades & Utility Helpers

The runtime replaces global procedural functions with expressive, static facade helpers inspired by Laravel syntax. These facades wrap optimized Rust implementations with clean fluent APIs.

### 1. Arr Facade

Provides explicit collection instantiation and chaining helpers. Standard arrays map to four high-performance collection subtypes:

- **Subtypes:** `Arr::list()` (sequential indexed list), `Arr::dict()` (insertion-ordered string-key map), `Arr::set()` (unique value set), and `Arr::hashMap()` / `Arr::map()` (universal complex key-value map).
- **Arr::list Behavior:** Designed for contiguous numerical index operations, providing seamless method-chaining and high CPU cache locality. Includes array helper methods such as `list`, `map`, `filter`, `reduce`, `reject`, and `find`.
- **Arr::dict Behavior:** Key-value map backed by `IndexMap<String, PhpValue>` retaining deterministic insertion order and $O(1)$ key lookups. Provides fluent helpers like `get`, `put`, `pull`, `has`, `keys`, `values`, `sortBy`, `diff`, and `intersect`.
- **Arr::set Behavior (Step 10):** Unique collection directly mapped to Rust `HashSet<PhpValue>`. Automatically deduplicates elements upon construction, push (`$set[] = $v`), or `$set->add($v)`. Bracket indexing (`$set[$val]`) provides $O(1)$ boolean membership checking, while `$set[$val] = true` and `$set[$val] = false` insert and remove elements. Includes set algebra (`diff`, `intersect`, `union`), higher-order methods (`map`, `filter`, `reject`, `reduce`), and native `foreach` iteration.
- **Arr::hashMap Behavior (Step 11):** Universal key-value dictionary directly mapped to Rust `HashMap<PhpValue, PhpValue>`. Supports arbitrary complex keys including arrays/lists, objects/class instances, sets, and scalars. Supports dynamic indexing (`$map[$complexKey] = $val`), fluent operations (`get`, `put`, `pull`, `has`, `remove`, `keys`, `values`, `entries`, `diff`, `intersect`, `merge`), higher-order transforms (`map`, `mapWithKeys`, `filter`, `reject`, `reduce`), and native `foreach ($map as $key => $val)` yielding complex keys.
- **Immutable Helper Methods by Default:** Calling array helper methods (such as `$my_array->sort()` or `Arr::list($my_array)->sort()`) **never mutates the original array in place**. Every transformation returns a brand-new array instance, keeping data flows clean and free of hidden side effects.
- **Explicit Variable Reassignment:** If a developer wants to update the variable with the newly sorted array, they use explicit reassignment:
  ```php
  $my_array = $my_array->sort();
  // or
  $my_array = Arr::list($my_array)->sort();
  ```
  Under the hood, this leverages RHP's internal **Copy-On-Write (COW)** and pointer mechanics, making variable replacement lightning-fast without unnecessary memory duplication or garbage collection overhead.
- **Custom Macros (`Arr::macro`):** Supports dynamic method registration bound to collection instances (`list`, `dict`, `set`, `hashMap`) without subclassing `Arr`.

#### Method Chaining & Collection Subtype Examples:

```php
// Arr::list sequential chaining
$numbers = Arr::list([1, 2, 3, 4, 5])
    ->map(fn($n) => $n * 2)
    ->filter(fn($n) => $n > 4);

// Arr::set unique collection and set algebra
$unique = Arr::set([1, 2, 2, 3, 3])
    ->add(4)
    ->diff(Arr::set([2, 3])); // {1, 4}

// Arr::hashMap with complex keys
$coordsMap = Arr::hashMap();
$coordsMap[[10, 20]] = 'Alpha Base';
$coordsMap[[30, 40]] = 'Beta Outpost';
$loc = $coordsMap[[10, 20]]; // 'Alpha Base'
```

### 2. Str Facade & Binary-Safe String Architecture

In RHP, strings are fundamentally **binary-safe raw byte structures** (`RhpString` backed by an inline 23-byte small string buffer or a reference-counted heap vector `Rc<Vec<u8>>`), matching Zend Engine's binary safety while retaining Rust's memory safety:

#### 1. Binary Safety & Raw Byte Storage
- **Arbitrary Payloads:** Reads binary files, images, network buffers, or arbitrary payloads without engine panics or UTF-8 validation rejections.
- **Legacy Japanese Multi-Byte Support:** Safely stores legacy Japanese encodings such as **Shift-JIS (SJIS / CP932)**, **EUC-JP**, and **ISO-2022-JP (JIS)** without triggering Rust UTF-8 panics.

#### 2. Utilizing the `Str` Facade & Explicit Constructors
Following RHP's design philosophy—where developers do not instantiate helper classes with `new Str()`—developers use **static factory methods and explicit encoding facades** to handle text safely:

```php
// 1. Creating a standard string (treated as raw bytes / UTF-8 safe)
$greeting = "こんにちは世界"; // Stored natively as raw UTF-8 bytes in Vec<u8>

// 2. Explicitly declaring or converting from a specific encoding (e.g., Shift-JIS)
$jisText = Str::fromEncoding($rawBytes, 'Shift-JIS');

// 3. Ensuring UTF-8 compliance via explicit facade constructors
$safeText = Str::utf8($userInput);

// 4. Converting back to raw encoding bytes
$rawJis = Str::toEncoding($greeting, 'Shift-JIS');
```

#### 3. Immutable Helpers & Explicit Reassignment
Consistent with array and collection design rules, string helper methods are **immutable by default** (returning a new instance rather than mutating buffers in place), ensuring clean data flows. Developers update variables via **explicit reassignment** leveraging RHP’s Copy-On-Write (COW) and pointer semantics:

```php
$text = "  hello world  ";

// Method chaining returns a new instance; original $text is untouched
$trimmed = $text->trim()->upper(); 

// Explicit reassignment to update the variable in place cleanly
$text = $text->trim()->upper(); 
```

#### 4. Summary of String Architecture

| Feature | Implementation Strategy | Benefit |
| :--- | :--- | :--- |
| **Internal Storage** | `Vec<u8>` with 23-byte SSO inside `PhpValue::String` | Binary safe; handles arbitrary bytes, images, network streams, and legacy Japanese encodings natively without panics. |
| **Instantiation** | Static facades (`Str::utf8()`, `Str::fromEncoding()`) | Avoids `new Str()` object header overhead; keeps syntax clean and performant. |
| **Mutations** | Immutable helpers + explicit reassignment (`$str = $str->method()`) | Predictable side-effect-free data manipulation combined with zero-garbage memory replacement. |

#### 5. Key Facade & Instance Methods
- **Inspection & Validation:** `Str::contains()`, `Str::startsWith()`, `Str::endsWith()`, `Str::isUtf8()`, `Str::byteLength()`, `Str::encoding()`.
- **Transformation:** `Str::slug()`, `Str::upper()`, `Str::lower()`, `Str::trim()`, `Str::ltrim()`, `Str::rtrim()`, `Str::replace()`, `Str::substr()`, `Str::reverse()`, `Str::repeat()`.
- **Encoding & Conversion:** `Str::fromEncoding()`, `Str::toEncoding()`, `Str::utf8()`, `Str::fromBytes()`, `Str::bytes()`.
- **Dynamic Macros:** `Str::macro('method', fn() => ...)` binds custom helper methods to all string instances.

### 3. HTTP Facade

Fluent HTTP client wrappers built on non-blocking async network tasks:

- **Key Utilities:** `HTTP::get()`, `HTTP::post()`, `HTTP::withHeaders()`, `HTTP::async()`.

```php
$response = HTTP::withHeaders(['Authorization' => 'Bearer token'])
    ->post('https://api.example.com/data', ['name' => 'Runtime']);
```

### 4. File Facade

High-level file I/O interface supporting both synchronous fibers and non-blocking asynchronous execution models:

- **Operations:** `File::read()`, `File::write()`, `File::exists()`, `File::delete()`.
- **Execution Models:** Standard call `File::read('app.log')` blocks the local fiber transparently. Using `Future::run(fn() => File::read(...))` spawns a background task for non-blocking concurrent execution.

### 5. Session Facade

Session state management interface across requests:

- **Key Methods:** `Session::get()`, `Session::put()`, `Session::has()`, `Session::forget()`.

### 6. Image Facade

Fluent image manipulation utilities leveraging native underlying graphics libraries:

- **Key Methods:** `Image::make()`, `Image::resize()`, `Image::crop()`, `Image::save()`.

```php
Image::make('photo.jpg')
    ->resize(300, 200)
    ->save('thumbnail.jpg');
```

### 7. URL Facade

URL generation, route inspection, and request URL analysis:

- **Key Methods:** `URL::to()`, `URL::route()`, `URL::current()`, `URL::full()`.

### 8. Trace & Telemetry Facades (OpenTelemetry Observability)

Native Tier 1 static facades providing enterprise-grade, zero-overhead distributed tracing and metrics conforming strictly to the OpenTelemetry specifications and W3C Distributed Tracing standards:

- **W3C Distributed Trace Context:** Automatic extraction and injection of W3C `traceparent` (`00-{trace_id}-{span_id}-{flags}`) and `baggage` headers across incoming worker requests and outbound HTTP client calls.
- **Hierarchical Span Lifecycle:**
  - `Trace::startSpan(string $name, ?array $options = null)`: Starts a new active span with kind (`SERVER`, `CLIENT`, `INTERNAL`, `PRODUCER`, `CONSUMER`), attributes, and parent linking.
  - `Trace::endSpan(?array $status = null)`: Closes the current active span, calculates nanosecond duration, and moves it to the completed spans store.
  - `Trace::span(string $name, callable $fn, ?array $attributes = null)`: Helper closure wrapper that automatically starts a span, enters it as active, executes `$fn`, records uncaught exceptions or panics as semantic error events, sets status to `OK` or `ERROR`, and ends the span.
  - `Trace::setAttribute(string $key, mixed $value)`: Associates string, numeric, boolean, or array attributes to the active span.
  - `Trace::addEvent(string $name, ?array $attributes = null)`: Records a timestamped event with metadata on the active span.
  - `Trace::recordException(string $type, string $message, ?string $stackTrace = null)`: Records a semantic exception event and marks the span status as `ERROR`.
  - `Trace::traceId()`, `Trace::spanId()`, `Trace::parentSpanId()`, `Trace::isRecording()`: Contextual distributed ID inspection.
- **High-Precision Metrics Engine:**
  - `Trace::counter(string $name, float $delta = 1.0, ?array $attributes = null)`: Monotonically increasing or decreasable counters.
  - `Trace::gauge(string $name, float $value, ?array $attributes = null)`: Instantaneous point-in-time value tracking (e.g. queue depth, memory usage).
  - `Trace::histogram(string $name, float $value, ?array $attributes = null)`: High-precision distribution tracking computing count, sum, min, max, avg, and exact percentiles ($p50$, $p90$, $p95$, $p99$).
  - `Trace::metricSummary(string $name)`: Returns a structured summary dict with calculated statistics and percentiles.
- **OpenTelemetry Protocol (OTLP) Exporters:**
  - `Trace::exportOtlp()`: Canonical OTLP/HTTP JSON `/v1/traces` export.
  - `Trace::exportOtlpMetrics()`: Canonical OTLP/HTTP JSON `/v1/metrics` export.
  - Direct compatibility with OpenTelemetry Collector, Jaeger, Honeycomb, Datadog, and New Relic.
- **Native Sentry & Datadog APM Bridges:**
  - `Trace::exportSentry(string $type, string $message)`: Emits Sentry event JSON including exception metadata, stack frames, and active trace context.
  - `Trace::exportDatadog()`: Emits Datadog APM trace JSON format.
- **Auto-Instrumentation Hooks:**
  - Incoming requests: `WorkerRequest` extracts `traceparent` and starts a root `SERVER` span, recording status and populating `WorkerResponse::trace_id` and `span_id`.
  - Outgoing HTTP client: `HTTP::get` and `HTTP::post` auto-instrument `CLIENT` spans and inject the active W3C `traceparent` header.
  - Database queries: `DB::*` auto-instruments `CLIENT` spans with semantic attributes.

---

## Asynchronous Non-Blocking Concurrent Core

- **Synchronous-By-Default Execution Model:** Helpers and standard library calls execute line-by-line synchronously on local execution fibers without blocking underlying OS worker threads.
- **Dart-Style Future Facade API (`Future` class):**
  - `Future::run(callable $fn)`: Spawns a background task on the Tokio worker pool without holding up subsequent local code.
  - `Future::delayed(int $milliseconds, callable $fn = null)`: Non-blocking yield/delay using Tokio timers (`tokio::time::sleep`). If a closure is provided, executes and returns its result after the delay.
  - `Future::value(mixed $value)`: Immediately wraps a value into a completed `Future` instance synchronously.
  - `Future::error(mixed $error)`: Immediately returns a rejected/failed `Future` wrapping an error or exception.
  - `Future::wait(array $tasks)`: Concurrently runs an array of closures (or Futures) and directly evaluates and returns an array of resolved values without requiring an explicit `->wait()` chain.
  - `Future::any(array $tasks)`: Concurrently runs an array of closures (or Futures) and directly evaluates and returns the result of whichever task completes first without requiring an explicit `->wait()` chain.
  - `Future::defer(callable $fn)`: Registers a post-response callback in the request queue executed after the HTTP response payload is flushed to the client (replacing stand-alone `defer()`).

#### Concurrent execution using `Future::wait()`:

```php
[$user, $posts] = Future::wait([
    fn() => DB::table('users')->find($id),
    fn() => HTTP::get("https://api.example.com/posts?user={$id}")->json()
]);
```

#### First-to-complete resolution using `Future::any()`:

```php
$fastestResponse = Future::any([
    fn() => HTTP::get('https://primary-mirror.com/data')->json(),
    fn() => HTTP::get('https://secondary-mirror.com/data')->json()
]);
```

#### Non-blocking delay using `Future::delayed()`:

```php
$delayedResult = Future::delayed(500, fn() => 'Execution resumed after 500ms');
```

#### Register post-response hook with `Future::defer()`:

```php
Future::defer(fn() => Log::info('Request processed successfully'));
```

---

## Process Model & Database Pooling

### Persistent Parent Process Architecture (`Arc<EngineState>`)

- **Persistent Memory Model:** The parent process parses and compiles the framework and application code once into an immutable state (`Arc<EngineState>`), bypassing per-request bootstrapping.
- **Singleton Process Services:** If a developer needs to run something as part of the parent process, they first declare a class with the `singleton` keyword (e.g. `singleton class DatabasePool { ... }` or `#[Singleton]`).
- **Singleton Invocation Invariants:**
  - **Within Other Singleton Classes:** Developers can use **both static and non-static methods** of a singleton class (e.g., `$pool->query()`, `DatabasePool::status()`). Singletons can also instantiate other singletons directly (`new ConfigManager()`).
  - **Inside Non-Singleton Classes:** Developers can **only use static methods** of a singleton class (e.g., `DatabasePool::getConnection()`). Attempting to call non-static methods (or instantiate with `new SingletonClass()`, or extend `class Child extends SingletonClass`) inside a non-singleton class results in a compile-time `TypeError` and runtime `RuntimeError` (`NonStaticSingletonMethodCallForbidden`).
- **Zero Bootstrap Overhead for Workers:** Worker tasks or threads hold `Arc<EngineState>` and instantiate lightweight request VMs (`engine_state.create_worker_vm()`) or evaluate request scripts (`engine_state.execute_request(req)`) with zero re-parsing or re-compilation of framework and application code.
- **Connection Pooling:** The parent manages a database pool (e.g., `sqlx`). Worker tasks check out connections instantly during active requests/transactions without creating socket handshake bottlenecks.
- **Centralized Logging (`Log::info`):** Log invocations push structured log payloads over a non-blocking multi-producer single-consumer (`mpsc`) channel directly to the parent process logging worker.

---

## Dual-Engine Execution Modes (Development vs Production Mode)

### Overview & Execution Modes Strategy

- **Development Mode (`rhp run dev`):** Direct AST Execution via Tree-Walking Interpreter. Evaluates AST nodes directly in memory using recursive pattern matching without emitting bytecode. Delivers instant cold startup times and sub-millisecond hot reloading.
- **Production / Build Mode (`rhp run build`):** Register-based Bytecode VM with optional JIT (Cranelift) compilation. Lowers AST into linear IR opcode streams executed within a Tokio multi-worker execution pool for maximum throughput.
- **Entrypoint & Target File Resolution:**
  - When running `rhp run dev` or `rhp run build` without a file argument, the CLI automatically picks `index.rhp` in the current working directory (`pwd`).
  - An explicit script file can be passed via the `--file` flag (e.g., `rhp run build --file="/your-path/server.rhp"`), `-f <path>`, or as a positional path argument.

### Developer Experience Framework (`rhp run dev`)

- **Hot Reloading:** File-watching engine powered by the `notify` crate. Re-parses changed files on the fly via Logos/Pratt parser, invalidating in-memory AST cache nodes without restarting persistent server state.
- **Exact Stack Traces & Source Maps:** Precise source span tracking (`Span { file_id, start, end, line, col }`) attached directly across AST nodes. Translates runtime errors directly to source file locations without requiring IP-to-bytecode line-table offset translation.
- **Interactive Debugging (DAP Protocol):** Integrated Debug Adapter Protocol (`dap-rs`) server listening on a configurable port. Execution traps checked via atomic execution flags in the evaluator loop for breakpoints and local environment scope inspection.
- **Integrated Code Coverage Engine:** AST execution counter tracking executed nodes during test runs, exported directly to LCOV (`coverage.info`) format.

### Implementation Code Sketch

```rust
// CLI Dispatch Logic

pub enum RunMode {
    Dev { hot_reload: bool, dap_port: Option<u16> },
    Build { jit_enabled: bool, opt_level: u8 },
}

pub fn execute(mode: RunMode, ast: &ASTNode, env: &mut Environment) -> Result<Value, RuntimeError> {
    match mode {
        RunMode::Dev { .. } => eval_expr(ast, env),
        RunMode::Build { jit_enabled, .. } => {
            let bytecode = compile_to_bytecode(ast)?;
            VM::new(bytecode, jit_enabled).run()
        }
    }
}

// Direct Recursive AST Evaluator (Dev Mode)

pub fn eval_expr(node: &ASTNode, env: &mut Environment) -> Result<Value, RuntimeError> {
    check_dap_breakpoints(&node.span)?;
    track_coverage(&node.span.id);

    match &node.kind {
        ASTKind::BinaryOp { op, left, right } => {
            let lhs = eval_expr(left, env)?;
            let rhs = eval_expr(right, env)?;
            apply_binary_op(op, lhs, rhs)
        }
        ASTKind::FunctionCall { callee, args } => {
            invoke_func(callee, args, env)
        }
        ASTKind::Literal(val) => Ok(val.clone()),
    }
}
```

### Execution Matrix

| Feature / Dimension | Development Mode (`rhp run dev`) | Production Mode (`rhp run build`) |
| :--- | :--- | :--- |
| **Execution Pipeline** | Direct Tree-Walking AST Interpreter | Register Bytecode VM + Cranelift JIT |
| **Reload Strategy** | Instant AST Node Invalidation & Re-parse | AOT / Compiled Bytecode Artifacts |
| **Startup Latency** | Near-Zero Cold Start (<5ms) | Requires Compilation Pass |
| **Throughput** | Optimized for Rapid Feedback | Maximum Ops/Sec & Cache Locality |
| **Developer Ergonomics** | DAP protocol, exact source maps, LCOV coverage | Production logging & telemetry |

---

## Technical Language Construction Architecture (20 Technical Points)

1. **Frontend Lexical Analysis Engine**: Implement a deterministic finite automaton (DFA) based lexer using the `logos` crate to tokenize source code files with zero-copy string slicing.
2. **Grammar & Expression Parsing**: Build a custom Pratt parser combined with combinator parsing to construct a strongly-typed Abstract Syntax Tree (AST) representing the program's logical structure.
3. **Type System Strategy (Medium / Gradual)**: Enforce strict type signatures across function and method boundaries while utilizing dynamic type inference within function bodies to preserve clean developer ergonomics.
4. **Intermediate Representation (IR) Lowering**: Translate the typed AST into flat, compact linear bytecode instructions optimized for execution within a custom virtual machine.
5. **Virtual Machine Execution Engine**: Design a native Rust register-based bytecode VM loop to achieve fast startup times and efficient low-level instruction dispatching.
6. **Just-In-Time (JIT) Compilation Pipeline**: Integrate the `Cranelift` compiler backend to conditionally compile hot execution loops and long-lived functions into native machine code at runtime.
7. **Dynamic Value Enum Representation**: Define a unified core `PhpValue` Rust enum capable of representing primitive and complex types, utilizing copy-on-write semantics for strings and arrays.
8. **Subtyped Collection Architecture (Arr::list)**: Map sequential lists directly to underlying Rust `Vec<PhpValue>` structures to maximize CPU cache locality during iteration.
9. **Subtyped Collection Architecture (Arr::dict)**: Map key-value dictionaries with strict insertion ordering to Rust `IndexMap<String, PhpValue>` structures.
10. **Subtyped Collection Architecture (Arr::set)**: Map unique collections directly to Rust `HashSet<PhpValue>` structures.
11. **Subtyped Collection Architecture (Arr::map)**: Implement universal general key-value dictionaries supporting complex keys using Rust `HashMap<PhpValue, PhpValue>` structures.
12. **Three-Tier Facade Architecture & Instantiation Guard**: Establish an object-oriented facade standard library organized into **Tier 1** (built-in native engine facades like `Str`, `Arr`, `Future`, `Log`, `DB`, `Memory`), **Tier 2** (custom Rust plugins/crates included via AOT builds), and **Tier 3** (script-land static facades), protected by an engine-level `is_static_only` flag that halts illegal instantiation attempts (e.g., `new Int()`) with a fatal error.
13. **Persistent Parent Process Architecture**: Boot, parse, and compile application code inside a long-lived parent process into an immutable memory state (`Arc<EngineState>`) to avoid per-request bootstrap overhead.
14. **Worker Process Isolation & Concurrency**: Spawn multi-threaded worker tasks or use process-level copy-on-write models to handle incoming requests concurrently against the shared parent state.
15. **Database Connection Pooling**: Manage persistent database connection pools centrally within the parent process, allowing workers to check out connections instantly during active requests.
16. **Synchronous-by-Default Execution Model**: Design local execution fibers to run standard helpers and I/O calls (`File::read()`, `DB::table()`) sequentially and blocking by default for predictable, linear developer ergonomics.
17. **Asynchronous Non-Blocking Logging Channel**: Decouple `Log::info()` calls by pushing structured log payloads into an in-memory lock-free multi-producer single-consumer (`mpsc`) channel consumed by a background worker.
18. **Post-Response Defer Execution Hooks (`Future::defer`)**: Implement `Future::defer()` to register post-response callbacks in a dedicated request queue executed strictly *after* the HTTP response payload has been flushed to the client, preventing background tasks from impacting request latency.
19. **Dart-Style Future Concurrency Core (`Future` Facade)**: Provide non-blocking background task execution via the `Future` class (`Future::run`, `Future::delayed`, `Future::wait`, `Future::any`) which accepts closures directly to evaluate parallel task joins and races cleanly without redundant `->wait()` chains.
20. **Memory Ownership & Safety Enforcement**: Leverage Rust’s compile-time borrow checker and ownership model (`Send + Sync`) to guarantee absolute memory safety and eliminate data races across concurrent worker threads without relying on a traditional tracing garbage collector.

### Deep Dive: Memory Ownership & Safety Enforcement (Technical Point #20)

#### 1. Zero Tracing Garbage Collector & Deterministic RAII
- **Zero GC Overhead & Pause Times:** Unlike legacy runtimes (PHP, JVM, Node.js, Go) that incur stop-the-world pauses and background cyclic GC sweeps, RHP operates 100% free of a tracing garbage collector (`Memory::isGcFree() == true`, `Memory::gcRuns() == 0`, `Memory::gcPauseMs() == 0.0`).
- **Deterministic Scoped Destruction ($O(1)$ Cleanup):** Variables, collections, and call frame registers are deallocated immediately upon exiting their lexical block, fiber, or worker request via Rust's RAII drop semantics.
- **Memory Scopes (`Memory::scope`):** User code can execute intensive sub-routines inside `Memory::scope(fn() => ...)`; intermediate values allocated within the scope are dropped instantly upon return, returning only the desired result.

#### 2. Copy-On-Write (COW) Memory Architecture
- **Immutable Sharing:** Assigning collections (`$copy = $orig;`) shares the underlying heap buffer (`Rc<RefCell<...>>`) with zero allocation overhead.
- **Detachment on Mutation:** Hot mutations (`$copy[0] = 99;` or `$copy[] = 100;`) detect `strong_count > 1` and transparently detach into an independent buffer, recording telemetry (`Memory::cowDetachments()`).
- **Small-String-Optimization (SSO):** Strings up to 23 bytes (`RhpString::Inline`) are stored directly on the stack without heap allocation.

#### 3. Compile-Time Data Race Elimination Across Worker Threads
- **Thread Boundary Isolation:** Active execution values (`PhpValue`) are strictly `!Send` and `!Sync` due to thread-local reference counting. The Rust compiler statically forbids sending active VM values directly across threads.
- **Immutable Snapshots (`FrozenValue`):** Cross-thread data sharing (e.g., `WorkerRequest`, `Future::run`, `ParentProcess` state) requires conversion to `FrozenValue` (`Send + Sync + 'static'`), eliminating data races at compile time.
- **Worker Isolation:** Each worker thread in `WorkerPool` executes an isolated VM with thread-local CoW states (`CoWState`). Mutations to static properties or globals are scoped to the individual request and never leak across threads.

#### 4. Prevention of Cyclic Leaks at the Language Boundary
- Pass-by-reference (`&$param`) and closure capture-by-reference (`use (&$var)`) are strictly forbidden at the type checker level (`PassByReferenceForbidden`, `ClosureReferenceCaptureForbidden`).
- By eliminating reference variables and mutable closure upvalues, cyclic reference graphs between closures and collections are impossible, removing the primary cause of memory leaks in dynamic languages without requiring a cyclic GC.

#### 5. Tier 1 Native Static Facade: `Memory`
| Method | Return Type | Description |
| :--- | :--- | :--- |
| `Memory::usage()` | `int` | Current estimated allocated memory in bytes |
| `Memory::peak()` | `int` | Peak allocated memory in bytes |
| `Memory::isGcFree()` | `bool` | Returns `true`, verifying complete absence of a tracing GC |
| `Memory::gcRuns()` | `int` | Always returns `0` (zero GC cycles) |
| `Memory::gcPauseMs()` | `float` | Always returns `0.0` (zero GC pause time) |
| `Memory::cowDetachments()` | `int` | Total number of Copy-On-Write detachments executed |
| `Memory::stats()` | `dict` | Comprehensive dictionary of memory, ownership, and GC metrics |
| `Memory::scope(callable)` | `mixed` | Executes closure in a scoped arena, reclaiming intermediate allocations immediately |
| `Memory::assertThreadSafe($val)` | `bool` | Asserts that `$val` satisfies cross-thread boundary safety without data races |
| `Memory::resetPeak()` | `void` | Resets peak memory tracking to current usage |
| `Memory::leakCheck()` | `dict` | Asserts zero memory leaks in the current scope |
| `Memory::arenaStats()` | `dict` | Telemetry on request arena allocation, capacity, reset count, and zero-GC invariant |
| `Memory::resetArena()` | `dict` | Explicitly drops and resets the request arena, running any pending DropList resource hooks |

---

### Deep Dive: Compile-Time Cycle Prevention & Request-Scoped Arena Allocation

#### 1. Compile-Time AST Linting Against Cyclic References
- **Closure Capture Inspection (`CyclicClosureCaptureForbidden`):** During AST type checking and static analysis, the compiler inspects closures and arrow functions. If a closure assigned to an object property captures `$this` (either explicitly via `use ($this)` or implicitly via `$this->...` property access in arrow functions), the compiler flags a static error (`CyclicClosureCaptureForbidden`).
- **Static Closures Allowed:** Closures declared with `static fn(...) => ...` or `static function(...) { ... }` cannot capture `$this` and are safely permitted as property callbacks.
- **Explicit Weak References for Circular Links (`CyclicReferenceRisk`):** Bidirectional relationships (e.g. parent-child tree nodes, listener lists) cannot directly store strong owning references back to their parent. Direct assignments like `$child->parent = $this;` trigger `CyclicReferenceRisk` at compile time.
- **Tier 1 Native `Weak` Facade:** To form valid bidirectional links, developers wrap the target using `Weak::to($obj)`.
  - `Weak::to($obj)`: Wraps an object in an ephemeral `WeakReference` backed by Rust's `std::rc::Weak<RefCell<VmObject>>`, incrementing the weak reference count without incrementing the strong reference count.
  - `Weak::get($ref)`: Rehydrates the target object if still allocated, or returns `null` if dropped.
  - `Weak::isValid($ref)`: Fast boolean check returning whether the referenced object is still alive.

#### 2. Request-Scoped Arena Allocation (`RequestArena`)
- **Fast Bump Allocation:** Temporary per-request allocations (byte buffers, slices, intermediate strings) are allocated sequentially from continuous pre-allocated memory chunks via bump pointers with 8-byte alignment.
- **Instantaneous $O(1)$ Bulk Drop:** When a request completes in a child worker, the entire arena buffer is reset in $O(1)$ time (`finish_request()`), resetting the bump pointer to 0 without individual object traversal.
- **Chunk Retention & Zero Fragmentation:** Chunks allocated during high-water requests are retained across requests, avoiding expensive OS heap re-allocations (`mmap`/`malloc`) and heap fragmentation.
- **Deterministic Non-Memory Resource Cleanup (`DropList`):** Non-memory OS resources (database transactions, open file descriptors, network sockets) are registered in the arena's `DropList` and cleanly closed upon request termination before memory pointers are reset.

#### 3. Parent vs. Child Worker Process Tiering
- **Immutable Parent Framework State:** The parent process boots, parses, and compiles framework code once into immutable `Arc<EngineState>` (bytecode IR, type hierarchy, class definitions, and constants).
- **Zero-GC Worker Execution:** Child worker VMs inherit the pre-compiled parent state and execute requests using a fresh `RequestArena`. Cleanup at request completion is instantaneous and zero-overhead:
  - `gc_runs`: strictly 0
  - `gc_pause_ms`: strictly 0.0
  - `zero_gc_drop`: strictly true
  - Telemetry is automatically captured in `WorkerResponse::arena_drop()`.
