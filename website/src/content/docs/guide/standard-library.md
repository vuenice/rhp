---
title: "Standard Library Overview"
description: "Overview of Tier 1 native static facades: Arr, Str, Future, Memory, File, HTTP, Log, and Sys."
section: "Language Guide"
order: 9
---

## Facade Architecture Philosophy

RHP replaces procedural legacy functions (`array_map`, `strlen`, `var_dump`) with expressive, static class facades wrapping optimized Rust heap primitives.

---

## Tier 1 Native Facades Summary

| Facade | Responsibility | Key Methods |
| :--- | :--- | :--- |
| `Request` | HTTP input parameters, filtering & in-place type formatting | `get()`, `all()`, `only()`, `format()`, `has()`, `set()`, `merge()` |
| `Session` | Session state management across requests | `get()`, `set()`, `put()`, `has()`, `all()`, `forget()`, `clear()` |
| `Cookie` | HTTP cookie reading and setting | `get()`, `set()`, `put()`, `has()`, `all()`, `forget()`, `clear()` |
| `Arr` | Subtyped collection creation & fluent operations | `list()`, `dict()`, `set()`, `hashMap()`, `macro()` |
| `Str` | Binary-safe raw byte & UTF-8 string manipulation | `slug()`, `contains()`, `upper()`, `fromEncoding()` |
| `Future` | Tokio fiber concurrency & async coordination | `run()`, `wait()`, `any()`, `delayed()`, `defer()` |
| `Memory` | Zero-GC telemetry & scoped request arena inspection | `usage()`, `peak()`, `isGcFree()`, `gcPauseMs()`, `scope()` |
| `File` | Synchronous filesystem I/O operations | `read()`, `write()`, `exists()`, `delete()` |
| `AFile` | Non-blocking async filesystem I/O returning `Future` | `read()`, `write()`, `exists()`, `delete()`, `append()`, `size()` |
| `HTTP` | Synchronous HTTP client and server kernel | `get()`, `post()`, `withHeaders()`, `serve()` |
| `AHTTP` | Non-blocking async HTTP client returning `Future` | `get()`, `post()`, `put()`, `delete()`, `patch()`, `withHeaders()` |
| `DB` | Synchronous database queries and transactions | `table()`, `find()`, `get()`, `select()`, `insert()` |
| `ADB` | Non-blocking async database queries returning `Future` | `table()`, `find()`, `get()`, `select()`, `insert()`, `update()`, `delete()` |
| `Log` | Lock-free mpsc async structured logger | `info()`, `warning()`, `error()`, `debug()` |
| `Sys` | System intrinsics, formatted output & process environment | `env()`, `echo()`, `print()`, `dd()`, `version()`, `timestamp()` |
| `Weak` | Ephemeral weak references for cycle prevention | `to()`, `get()`, `isValid()` |
| `Trace` / `Telemetry` | Native OpenTelemetry distributed tracing, metrics & APM bridges | `startSpan()`, `endSpan()`, `span()`, `counter()`, `gauge()`, `histogram()`, `exportOtlp()`, `exportSentry()` |

---

## Web State Facades (`Request`, `Session`, `Cookie`)

In RHP, legacy PHP superglobals (`$_GET`, `$_POST`, `$_SESSION`, `$_COOKIE`) are deprecated. Instead, use static facades:

### 1. `Request` Facade & Type Formatting

All incoming request parameters are strings by default. You can inspect, filter, or cast them on the request object using `Request::format()`:

```php
// Retrieve raw string values by default
$pageStr = Request::get('page'); // e.g. "1"

// Format and cast types in-place on the request object:
// Both quoted types ('int', 'bool') and bare type keywords (int, bool) are supported!
Request::format([
    'page'   => 'int',
    'active' => bool,
    'rate'   => float,
]);

int $page = Request::get('page');     // 1
bool $active = Request::get('active'); // true

// Filter specific keys via array list or variadic args:
dict $public = Request::only(['page', 'active']);
dict $pageOnly = Request::only('page');
```

> **Why bare type keywords work without ambiguity:**
> In RHP's Pratt parser, type keywords in statement positions (`int $x = 10;`) are validated by the presence of a variable identifier `$var`. When a type keyword appears as a value in an array literal (`'active' => bool`), it parses cleanly as a value token resolving to the type name string without any grammar ambiguity.

### 2. `Session` Facade

```php
// Store session state
Session::set('user_id', 42);

// Retrieve session value with fallback default
$userId = Session::get('user_id', 0);
$hasUser = Session::has('user_id');

// Clear specific key or all session data
Session::forget('user_id');
Session::clear();
```

### 3. `Cookie` Facade

```php
// Set a cookie (stored as string)
Cookie::set('theme', 'dark');

// Read cookie with fallback default
string $theme = Cookie::get('theme', 'light');

// Check and remove cookie
bool $hasTheme = Cookie::has('theme');
Cookie::forget('theme');
```

---

## Str Facade & Binary-Safe String Architecture

In RHP, strings are fundamentally **binary-safe raw byte buffers** (`RhpString` storing $\le 23$ bytes inline on the stack via Small-String-Optimization (SSO) and larger buffers via `Rc<Vec<u8>>`). This design matches the Zend Engine's binary safety while maintaining Rust's strict memory safety:

### 1. Underlying Representation: `PhpValue::String(Vec<u8>)`
* **Binary Safety:** Safely store and process arbitrary binary payloads, network buffers, images (e.g. PNG/JPEG byte headers), and strings containing null bytes (`\0`) without triggering Rust UTF-8 panics or validation errors.
* **Legacy Japanese Multi-Byte Support:** Safely accommodates legacy multi-byte Japanese encodings like **Shift-JIS (SJIS / CP932)**, **EUC-JP**, and **ISO-2022-JP (JIS)** natively as raw bytes without crashing.

### 2. Static Facade Constructors (No `new Str()`)
Following RHP's design philosophy—where developers do not instantiate helper classes with `new Str()`—developers use **static factory methods and explicit encoding facades** to handle text safely:

```php
// 1. Creating a standard string (treated as raw bytes / UTF-8 safe)
$greeting = "こんにちは世界"; // Stored natively as raw UTF-8 bytes in Vec<u8>

// 2. Explicitly declaring or converting from a specific encoding (e.g., Shift-JIS)
$jisText = Str::fromEncoding($rawBytes, 'Shift-JIS');

// 3. Ensuring UTF-8 compliance via explicit facade constructors
$safeText = Str::utf8($userInput);

// 4. Converting back to raw encoding bytes (e.g. Shift-JIS)
$rawJis = Str::toEncoding($greeting, 'Shift-JIS');
```

### 3. Immutable Helpers & Explicit Variable Reassignment
Consistent with array and collection design rules, string helper methods are **immutable by default** (returning a new instance rather than mutating buffers in place), ensuring clean data flows. Developers update variables via **explicit reassignment** leveraging RHP’s Copy-On-Write (COW) and pointer semantics:

```php
$text = "  hello world  ";

// Method chaining returns a new instance; original $text is untouched
$trimmed = $text->trim()->upper(); 

// Explicit reassignment to update the variable in place cleanly
$text = $text->trim()->upper(); 
```

### 4. Summary of String Architecture

| Feature | Implementation Strategy | Benefit |
| :--- | :--- | :--- |
| **Internal Storage** | `Vec<u8>` with 23-byte SSO inside `PhpValue::String` | Binary safe; handles arbitrary bytes, images, network streams, and legacy Japanese encodings natively without panics. |
| **Instantiation** | Static facades (`Str::utf8()`, `Str::fromEncoding()`) | Avoids `new Str()` object header overhead; keeps syntax clean and performant. |
| **Mutations** | Immutable helpers + explicit reassignment (`$str = $str->method()`) | Predictable side-effect-free data manipulation combined with zero-garbage memory replacement. |

---

## Sys Facade: Formatted Output (`echo` & `print`)

`Sys` contains two primary output functions that compile directly to native Rust `print!` and `println!` functions with function parameters passed as-is:
- **`Sys::echo(...)`**: Prints on the same line without a trailing newline (identical to Rust's `print!()`).
- **`Sys::print(...)`**: Prints and automatically adds a trailing newline `\n` after the line (identical to Rust's `println!()`).

Both functions work identically with format strings and arguments. RHP does **not** support C-language style formatting (such as `%d`, `%s`, `%f`, or `%x`). All templates use native Rust formatting parameters directly:

| Format Specifier | Description | Example | Output |
| :--- | :--- | :--- | :--- |
| `{}` | Value placeholder | `Sys::print("Age: {}, Name: {}", 30, "Bob")` | `Age: 30, Name: Bob\n` |
| `{:.2}` | Floating-point precision | `Sys::print("{:.2}", 3.141592)` | `3.14\n` |
| `{:05}` | Integer padding with zeros | `Sys::print("{:05}", 42)` | `00042\n` |
| `{:x}` | Hexadecimal lowercase | `Sys::print("{:x}", 255)` | `ff\n` |
| `{:b}` | Binary formatting | `Sys::print("{:b}", 5)` | `101\n` |

```php
// 1. Value placeholders
Sys::print("Age: {}, Name: {}", 30, "Bob"); // Outputs: Age: 30, Name: Bob\n

// 2. Floating-point precision
Sys::print("{:.2}", 3.141592); // Outputs: 3.14\n

// 3. Integer padding with zeros
Sys::print("{:05}", 42); // Outputs: 00042\n

// 4. Hexadecimal lowercase
Sys::print("{:x}", 255); // Outputs: ff\n

// 5. Binary formatting
Sys::print("{:b}", 5); // Outputs: 101\n

// Same-line printing with Sys::echo (no newline)
Sys::echo("Age: {}, Name: {}", 30, "Bob"); // Outputs: Age: 30, Name: Bob (same line)
Sys::echo(" | Binary: {:b}", 5);            // Outputs:  | Binary: 101 (same line)
Sys::print(" [done]");                     // Outputs:  [done]\n
```

> **Important: There is no `Sys::println` function.**
> In RHP, only `echo` and `print` exist (both as language constructs `echo ...` / `print(...)` and via `Sys::echo(...)` / `Sys::print(...)`). There is **no** `Sys::println` function. Calling `Sys::println(...)` will result in an `UndefinedFacadeMethod` error.

---

## Synchronous Facades (`File`, `HTTP`, `DB`) vs Async Facades (`AFile`, `AHTTP`, `ADB`)

RHP eliminates function coloring by providing pairs of synchronous and asynchronous facades rather than requiring `async/await` keywords:

- **Use Synchronous Facades (`File`, `HTTP`, `DB`)** for straightforward, sequential blocking execution:
  ```php
  string $data = File::read('report.csv');
  $response = HTTP::get('https://api.example.com/status');
  $user = DB::table('users')->find(10);
  ```
- **Use Async Facades (`AFile`, `AHTTP`, `ADB`)** to launch non-blocking background tasks on Rust Tokio, returning `Future` handles that can be checked with `$fut->isPending()` or resolved via `$fut->wait()`:
  ```php
  $fut1 = AFile::read('report.csv');
  $fut2 = AHTTP::get('https://api.example.com/status');
  $fut3 = ADB::table('users')->find(10);

  // Synchronize results without 'await':
  $data = $fut1->wait();
  $response = $fut2->wait();
  $user = $fut3->wait();
  ```

---

## Observability & Distributed Tracing (`Trace` & `Telemetry`)

RHP embeds a zero-cost OpenTelemetry-compliant tracing and metrics engine directly into the runtime, replacing heavy Zend C extensions:

```php
// Automatic closure-based span with metrics and exception capture
$result = Trace::span('order.process', function() {
    Trace::setAttribute('order.amount', 149.99);
    Trace::counter('orders.processed', 1);

    // Outbound HTTP call automatically injects W3C traceparent header
    $res = HTTP::post('https://api.payment.internal/charge', ['amount' => 149.99]);

    Trace::addEvent('payment_settled', Arr::dict(['provider' => 'stripe']));
    return $res->status();
});

// Record high-precision latency percentiles (p50, p90, p95, p99)
Trace::histogram('db.query_latency_ms', 4.2);
$stats = Trace::metricSummary('db.query_latency_ms');

// Export canonical OpenTelemetry Protocol (OTLP/HTTP JSON)
$tracesJson = Trace::exportOtlp();
$metricsJson = Trace::exportOtlpMetrics();
```

For full architecture details, W3C context propagation, Sentry/Datadog bridges, and worker auto-instrumentation, see the [Observability & OpenTelemetry Guide](/docs/guide/observability-opentelemetry).


---

## Tier 2 Native Helper Classes (`Cache`, `Log`) via Rust File Imports

While Tier 1 facades are pre-compiled into the RHP binary, framework-level helper utilities like `Cache` or high-performance `Log` are designed as **Tier 2 Native Helper Classes**.

### Why Tier 2 Rust Imports?
1. **Core Language Minimalism:** Helper classes are not required in the core programming language syntax, keeping RHP lean, fast to parse, and architecturally uncluttered.
2. **Framework-Level Speed:** Frameworks need high-speed caching with TTLs, atomics, and structured logging. By implementing these as native Rust files (`.rs`) imported directly (`import "./Cache.rs"`), applications achieve bare-metal Rust performance without engine forks or C FFI overhead.
3. **Thread Safety & Multi-Worker Concurrency:** Native helper stores use `FrozenValue` boundaries, ensuring lock-free or read-optimized thread safety across all Tokio worker threads with zero GC pauses.

```php
import "./Cache.rs";
import "./Log.rs" as Logger;

// Cache operations (in-memory atomic L1 store with TTL)
Cache::set("session:user_10", "Alice", 3600);
if (Cache::has("session:user_10")) {
    $user = Cache::get("session:user_10");
}
Cache::increment("hits", 1);
int $hitCount = Cache::get("hits");

// Structured logging
Logger::info("User session verified", ["user_id" => 10]);
Logger::warn("Rate limit threshold nearing", ["ip" => "127.0.0.1"]);
```

---

## Example: Composing Facades


```php
// Read input, transform collection, and log telemetry
string $rawContent = File::read('metrics.json');
dict $data = json_decode($rawContent, true);

list $activeUsers = Arr::list($data->get('users'))
    ->filter(fn($u) => $u['is_active'] === true)
    ->map(fn($u) => Str::slug($u['username']));

Log::info("Found " . (string)$activeUsers->count() . " active users");
Sys::print("Active users: {}, Peak Memory: {} bytes", $activeUsers->count(), Memory::peak());
```
