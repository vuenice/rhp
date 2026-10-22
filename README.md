# RHP (Rust-Powered High-Performance PHP)

Welcome to RHP—a high-performance, drop-in syntax programming language designed to supercharge PHP and Laravel-style applications with the raw speed and memory safety of Rust.

---

## 🚀 Why We Created RHP

Most applications do not require extreme performance from day one, but businesses must plan for future migration costs as traffic scales. RHP was built to solve the core performance and architectural limitations of traditional PHP without sacrificing developer velocity or requiring complete code rewrites:

- **Bridging the Performance Gap:** Today, traditional PHP and Laravel benchmarks lag behind modern runtimes like Node.js, Bun, Go, and Java. RHP makes Laravel-style frameworks competitive with these high-performance alternatives.
- **Avoiding Fragile Workarounds:** While solutions like Laravel Octane and Swoole improve performance, supporting them can be technically difficult, and they still cannot match the deep, foundational performance of a core programming language written in Rust.
- **Eliminating Per-Request Bootstrapping:** Traditional PHP-FPM boots up the entire framework, parses code, and loads dependency injection containers on every single HTTP request, only to discard it immediately. RHP keeps your framework code permanently loaded in memory across requests.
- **Cost-Effective Scaling:** Upgrade high-traffic or resource-heavy Laravel applications by retaining familiar syntax while reaping massive performance gains, saving businesses time and migration costs.

---

## ⚙️ How It Works

RHP combines a dynamic, developer-friendly interface with a robust, multi-threaded Rust execution backend.

- **Execution & Tooling Modes:**
  - **Interactive Desktop Playground (`rhp-play`):** Instant desktop GUI playground with real-time syntax highlighting, multiple workspace/project connection manager, and split-pane execution output.
  - **Development Mode (`rhp run dev` or `rhp dev`):** Direct tree-walking AST interpreter for sub-millisecond hot-reloading, instant cold starts, and immediate stack traces. When executing a web application (e.g. `examples/todo`), it **automatically launches the interactive HTTP server on port 8080**.
  - **Production Mode (`rhp run build` or `rhp build`):** Lowers code into a register-based Bytecode VM with optional Cranelift JIT compilation, running on a multi-worker Tokio execution pool for maximum throughput.
  - **HTTP Web Server (`rhp serve`):** Dedicated HTTP server routing requests into isolated zero-bootstrap worker VMs with standard superglobals (`$_SERVER`, `$_GET`, `$_POST`, `HTTP_HX_REQUEST`).
  - **Lint & Auto-Format Mode (`rhp run lint`):** Auto-applies standard 4-space indentation, formats operators/braces (inspired by Carthage Mago), automatically infers and adds variable type signatures (`$x = '';` -> `string $x = '';`), runs the static type checker, and outputs diagnostic issues in JSON format.
- **Asynchronous & Synchronous Flexibility:** RHP is dynamically typed and supports both synchronous and asynchronous execution. Most database queries and file reads execute sequentially in local fibers without extra boilerplate, but you can explicitly offload tasks when needed.
- **Rust-Powered Reliability:** RHP is built on Rust to guarantee memory safety, eliminate crashes from isolated user requests, and provide top-tier request handling speeds.

---

## ⚡ Quick Start: Running the Reactive TODO App

RHP includes a complete, full-stack reactive TODO application in [`examples/todo`](examples/todo) proving that the compiler, type system, register VM, JSON persistence, and built-in HTTP server work seamlessly with **HTMX** and **Alpine.js**:

```bash
# 1. Install RHP via Homebrew (macOS & Linux):
brew install vuenice/rhp/rhp

# (Or build from source via Cargo: cargo install --path .)

# 2. Run the TODO app in Development Mode:
rhp run dev examples/todo
# or shorthand:
rhp dev examples/todo

# 3. Or run in Production Mode with JIT:
rhp run build examples/todo

# 4. Open in your browser:
# http://localhost:8080
```

> **CLI vs Web Mode:** If you want to dump the rendered HTML to stdout as a one-shot CLI script, simply pass `--cli`:
> `rhp run dev examples/todo --cli`


---

## 📝 Syntax & Core Features

RHP replaces legacy global procedural functions with expressive, object-oriented class facades inspired by Laravel, wrapping optimized Rust implementations.

### 1. Arr Facade & Explicit Subtypes

Standard arrays map to four high-performance Rust-backed collection types to optimize memory and CPU cache locality:

- `Arr::list()`: Sequential indexed lists (`Box<Vec<PhpValue>>`).
- `Arr::dict()`: Key-value maps retaining insertion order (`Box<IndexMap>`).
- `Arr::set()`: Unique collections with $O(1)$ lookups (`Box<IndexSet>`).
- `Arr::hashMap()`: Universal key-value maps for complex keys (`Box<HashMap>`).

#### Method Chaining Example:

```php
$numbers = Arr::list([1, 2, 3, 4, 5])
    ->map(fn($n) => $n * 2)
    ->filter(fn($n) => $n > 4);
```

#### Runtime Macro Registration (`Arr::macro`):

Core facades (`Arr`, `Str`, etc.) wrap optimized Rust allocations and cannot be subclassed (`extends Arr` is forbidden). Instead, register custom methods dynamically via `::macro()`:

```php
// Define a custom macro on the Arr facade
Arr::macro('toCsv', function ($delimiter = ',') {
    // Custom logic accessing the underlying collection
    return implode($delimiter, $this->all());
});

// Usage anywhere in your application
$csv = Arr::list([1, 2, 3])->toCsv();
```

### 2. Str Facade & Binary-Safe String Architecture

RHP strings are fundamentally **binary-safe raw byte structures** (`RhpString` storing $\le 23$ bytes inline on the stack via Small-String-Optimization (SSO) and larger buffers via `Rc<Vec<u8>>`), matching Zend Engine's binary safety while retaining Rust's memory safety:

* **Binary Safety:** Safely reads binary files, network streams, images, and raw byte arrays with null bytes (`\0`) without triggering Rust UTF-8 panics.
* **Legacy Japanese Multi-Byte Support:** Natively accommodates **Shift-JIS (SJIS / CP932)**, **EUC-JP**, and **ISO-2022-JP (JIS)** without panics.
* **Static Constructors & Facades:** Avoids `new Str()` object overhead in favor of static facade factories:
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
* **Immutable Helpers & Explicit Variable Reassignment:** Consistent with collection rules, string helpers are **immutable by default** (returning a new instance rather than mutating in place). Developers use explicit reassignment, leveraging Copy-On-Write (COW) and pointer semantics:
  ```php
  $text = "  hello world  ";

  // Method chaining returns a new instance; original $text is untouched
  $trimmed = $text->trim()->upper(); 

  // Explicit reassignment to update the variable in place cleanly
  $text = $text->trim()->upper(); 
  ```

| Feature | Implementation Strategy | Benefit |
| :--- | :--- | :--- |
| **Internal Storage** | `Vec<u8>` with 23-byte SSO inside `PhpValue::String` | Binary safe; handles arbitrary bytes, images, network streams, and legacy Japanese encodings natively. |
| **Instantiation** | Static facades (`Str::utf8()`, `Str::fromEncoding()`) | Avoids `new Str()` object header overhead; keeps syntax clean and performant. |
| **Mutations** | Immutable helpers + explicit reassignment (`$str = $str->method()`) | Predictable side-effect-free data manipulation combined with zero-garbage memory replacement. |

### 3. Asynchronous Execution (`async` / `defer`)

- **Use `async()` to spawn background tasks and return immediate futures:**

```php
$fileFuture = async(File::read('data.csv'));
$data = $fileFuture->wait();
```

- **Use `defer()` to execute post-response hooks after the payload has been flushed to the client, ensuring background logging or cleanup tasks never delay user response times.**

### 4. Typed Variable Declarations & Functional Type Conversions

- **Prefix Variable Declarations (`int $x = 10;`):**
  RHP enforces prefix type declarations for local variables, class properties, and parameters. Colon syntax (`$x: int = 10;`) is strictly forbidden and produces a compile-time parse error.
  ```php
  int $x = 10;
  string $name = "Alice";
  float $price = 99.95;
  bool $isActive = true;
  int $counter; // Uninitialized typed declaration
  ```

- **Functional Type Conversions (`int($x)`, `string($x)`):**
  Type conversions use functional notation. C-style / PHP-style casts (`(int)$x`, `(string)$x`) are not supported and trigger an informative compile-time parse error:
  ```php
  $id = int($rawInput);
  $text = string($id);
  $ratio = float("0.75");
  $flag = bool(1);
  ```

### 5. Interactive Desktop Playground (`rhp-play`)

RHP includes a minimalist, high-performance desktop playground built with **Tauri v2**, **Vanilla TypeScript**, and **CodeMirror 6** (inspired by TweakPHP) featuring real-time syntax highlighting, local workspace connection management, and split-pane execution output:

```bash
# Run in development mode (with hot-reload):
cd playground
npm install
npm run tauri dev

# Or build the frontend and compile the native release binary:
npm run build
cargo build --release
./target/release/rhp-play

# Or install globally to ~/.cargo/bin:
cargo install --path src-tauri
rhp-play
```

### 6. Executing Scripts (`rhp run dev` & `rhp run build`)

Execute RHP programs in either Development Mode (direct AST interpretation with sub-millisecond startup) or Production Mode (register bytecode VM with Cranelift JIT compilation).

Both commands automatically pick `index.rhp` in the current working directory (`pwd`) by default, or an explicit file passed via the `--file` argument:

```bash
# Execute index.rhp in current working directory (default)
rhp run dev
rhp run build

# Execute specific script via --file argument
rhp run dev --file="/your-path/server.rhp"
rhp run build --file="/your-path/server.rhp"

# Execute specific script via positional path
rhp run dev server.rhp
rhp run build server.rhp
```

### 7. Code Formatting, Auto-Typing & Linting (`rhp run lint`)

Inspired by Carthage Software Mago, `rhp run lint` combines an AST-guided auto-formatter, static type annotator, and boundary type checker into a single command:

```bash
# Auto-format, infer types, enforce casing, and output diagnostics
rhp run lint [PATH]

# Dry run: check and report issues in JSON without writing files
rhp run lint [PATH] --check
# or
rhp run lint [PATH] --no-fix

# Skip naming convention transformations:
rhp run lint [PATH] --no-case-change
```

- **Naming Conventions:**
  - Variables to `snake_case` (`$myVar` $\rightarrow$ `$my_var`, preserving `$this` and superglobals `$_GET`, `$_POST`, etc.).
  - Classes, interfaces, traits, and enums to `PascalCase` (`class user_account` $\rightarrow$ `class UserAccount`, `new user_account()` $\rightarrow$ `new UserAccount()`).
  - Functions and methods to `camelCase` (`function calculate_tax()` $\rightarrow$ `function calculateTax()`).
  - **Test Function Exception:** Any function or method starting with `test` is kept/converted to `snake_case` (`function testUserLogin()` $\rightarrow$ `function test_user_login()`).
- **Formatting:** Enforces standard 4-space indentation, normalized brace positioning, and operator spacing while preserving comments and strings verbatim.
- **Auto-Typing:**
  - Infers scalar primitives (`string $x = '';`, `int $y = 1;`, `float $f = 3.14;`, `bool $b = true;`).
  - Classifies array literals declared and initialized with values:
    - Sequential values (`[1, 2, 3]`) $\rightarrow$ `list`
    - String-keyed pairs (`['name' => 'Alice']`) $\rightarrow$ `dict`
    - Non-string keys (`[1 => 'val']`) $\rightarrow$ `hashMap`
    - Empty arrays (`[]`) $\rightarrow$ `array`
  - Infers types for function/method parameters with default values (`function foo(int $x = 1)`).
  - Preserves already written type annotations without modifying them.
- **JSON Diagnostic Output:** Reports remaining typecheck violations, casing issues (in `--no-fix` mode), and mismatches in machine-readable JSON format matching Mago's schema.

### 8. Native OpenTelemetry & Distributed Observability (`Trace` / `Telemetry`)

RHP eliminates legacy Zend C APM extensions (`ext-sentry`, `ddtrace`, `newrelic.so`) by embedding a zero-overhead native OpenTelemetry engine directly into the runtime:

- **W3C Distributed Trace Context:** Automatic propagation of `traceparent` (`00-{trace_id}-{span_id}-{flags}`) and `baggage` across incoming worker requests and outgoing HTTP/DB calls.
- **Hierarchical Spans:** `Trace::startSpan()`, `Trace::endSpan()`, and closure wrapper `Trace::span($name, fn() => ...)` with automatic exception capture.
- **High-Precision Metrics:** `Trace::counter()`, `Trace::gauge()`, and `Trace::histogram()` with exact percentiles ($p50$, $p90$, $p95$, $p99$).
- **Standard Exporters:** Canonical OTLP/HTTP JSON for `/v1/traces` and `/v1/metrics`, plus Sentry crash reporting and Datadog APM trace bridges:

```php
// Distributed span with attributes & metrics
Trace::span('checkout.process', function() {
    Trace::setAttribute('order.amount', 99.99);
    Trace::counter('orders.processed', 1);

    // Outgoing HTTP call auto-instruments client span & injects traceparent
    $payment = HTTP::post('https://api.gateway.internal/charge', ['amount' => 99.99]);

    Trace::addEvent('payment_received', Arr::dict(['gateway' => 'stripe']));
    return $payment->status();
});

// Export OTLP payload directly to collector
$otlpTracesJson = Trace::exportOtlp();
```

### 9. Web Request, Session & Cookie Facades

In RHP, procedural PHP superglobals (`$_GET`, `$_POST`, `$_SESSION`, `$_COOKIE`) are replaced with pure static facades:

- **`Request` Facade:**
  - `Request::get($key = null, $default = null)`: Read a request parameter (or all parameters).
  - `Request::all()`: Return all parameters as a `dict`.
  - `Request::only(['param1', 'param2'])` / `Request::only('param1', 'param2')`: Extract only specific parameters.
  - **Default String Values:** All incoming request values are strings by default.
  - **`Request::format(...)`:** Set and cast types on the request object. Supports both quoted types and bare type keywords (`int`, `bool`, `float`, `string`) without any parser ambiguity:
    ```php
    Request::format([
        'param1' => 'int',
        'param2' => bool,
        'rate'   => float,
    ]);

    $id = Request::get('param1');   // Typed int (e.g. 123)
    $flag = Request::get('param2'); // Typed bool (true/false)
    ```

- **`Session` Facade:**
  - `Session::get($key = null, $default = null)`, `Session::set($key, $value)` (or `put`), `Session::has($key)`, `Session::all()`, `Session::forget($key)`, `Session::clear()`.

- **`Cookie` Facade:**
  - `Cookie::get($key = null, $default = null)`, `Cookie::set($key, $value)`, `Cookie::has($key)`, `Cookie::all()`, `Cookie::forget($key)`, `Cookie::clear()`.

- **Linter Deprecation Warning:**
  Running `rhp run lint` flags legacy `$_GET`, `$_POST`, `$_SESSION`, and `$_COOKIE` variables with a `deprecated-superglobal` warning advising use of the facades.

### 10. Asynchronous Tokio Facades (`AFile`, `AHTTP`, `ADB`) & Future State Model

When non-blocking background I/O is required, RHP provides dedicated Tokio-backed facades that return `Future` handles immediately without blocking execution:

- **`AFile`**: Non-blocking filesystem operations (`AFile::read()`, `AFile::write()`, `AFile::exists()`, etc.).
- **`AHTTP`**: Non-blocking HTTP client (`AHTTP::get()`, `AHTTP::post()`, `AHTTP::withHeaders()`, etc.).
- **`ADB`**: Non-blocking database queries (`ADB::table()`, `ADB::find()`, `ADB::select()`, etc.).

#### Rust-Like `Future` State
RHP's `Future` object models Rust's `Poll::Pending` and `Poll::Ready` state:
- `$future->state` / `$future->getState()`: Returns string `"Pending"` or `"Ready"`.
- `$future->isPending()`: Returns `true` while the Tokio background task is active.
- `$future->isReady()` / `$future->isCompleted()`: Returns `true` when resolved.
- `$future->poll()`: Non-blocking probe returning `null` if pending, or the resolved value if ready.
- `$future->wait()`: Synchronously awaits completion and returns the resolved value.

```php
// Non-blocking asynchronous dispatch
$futFile = AFile::read('data.json');
$futHttp = AHTTP::get('https://api.example.com/v1/metrics');
$futDb   = ADB::table('users')->where('active', 1)->get();

// Probe state without blocking
if ($futFile->isPending()) {
    Sys::print("Task running in background, state={}", $futFile->state); // "Pending"
}

// Retrieve values when needed
$fileData = $futFile->wait();
$httpResp = $futHttp->wait();
$users    = $futDb->wait();
```

#### No `await` Keyword (Zero Function Coloring)
RHP intentionally omits the `await` keyword to eliminate the infectious function coloring problem (`async/await` virus):
- For synchronous, straightforward blocking execution, use standard facades: `File::read()`, `HTTP::get()`, `DB::table()`.
- For concurrent background execution, use `AFile`, `AHTTP`, `ADB`, or `Future::run()`, then call `->wait()` or join with `Future::wait([ ... ])`.
- Attempting to use `await` triggers a compile error directing developers to synchronous facades or `->wait()`.

### 11. Formatted Output Intrinsics (`echo` & `print`)

`Sys` provides two core output functions that compile directly to native Rust `print!` and `println!` routines with function parameters passed as-is:
- **`echo` / `Sys::echo(...)`**: Compiles directly to Rust `print!()` (outputs on the same line without a trailing newline).
- **`print` / `Sys::print(...)`**: Compiles directly to Rust `println!()` (outputs WITH a trailing newline `\n`).

Both functions work identically. RHP does **not** support C-language style formatting (`%d`, `%s`, `%f`, etc.). All template strings use native Rust formatting parameters directly:

```php
// Value placeholders
Sys::print("Age: {}, Name: {}", 30, "Bob"); // Outputs: Age: 30, Name: Bob\n

// 1. Floating-point precision
Sys::print("{:.2}", 3.141592); // Outputs: 3.14\n

// 2. Integer padding with zeros
Sys::print("{:05}", 42); // Outputs: 00042\n

// 3. Hexadecimal lowercase
Sys::print("{:x}", 255); // Outputs: ff\n

// 4. Binary formatting
Sys::print("{:b}", 5); // Outputs: 101\n

// Inline output on the same line with Sys::echo
Sys::echo("Age: {}, Name: {}", 30, "Bob"); // Outputs: Age: 30, Name: Bob (no newline)
Sys::echo(" [ready]");                     // Outputs:  [ready] (same line)
```

> **Note on `Sys::println`:**
> There is **no function like `Sys::println`** in RHP. Only `echo` and `print` exist (both as language constructs and via `Sys::echo` and `Sys::print`). Invocations of `Sys::println(...)` fail with an `UndefinedFacadeMethod` error.

### 12. Direct Rust File Import (`import "./Cache.rs"`) for Helper Classes

To keep the core language syntax minimal and unbloated while providing extreme native performance at the framework and application level, RHP allows importing `.rs` files directly into `.rhp` scripts:

```php
// Import Rust helper files directly
import "./Cache.rs";
import "./Log.rs" as Logger;
import FastCache from "./Cache.rs";
use "./Cache.rs";

// Use directly as static Tier 2 helper classes without instantiation
Cache::set("session_user_10", "Alice", 3600); // Key, Value, TTL (seconds)
if (Cache::has("session_user_10")) {
    $user = Cache::get("session_user_10");
}
Cache::increment("active_connections", 1);

Logger::info("User session verified", ["user_id" => 10]);
```

- **Framework-Level Performance:** Helpers like `Cache`, `Log`, `RateLimiter`, or `FastCrypto` do not need to be baked into core language syntax, yet execute with bare-metal Rust speed at the framework level.
- **AOT Compilation & SHA-256 Caching:** When `rustc` is available, `.rs` files are automatically compiled into native dynamic libraries (`cdylib` / `.so`) with SHA-256 caching in `target/tmp/rhp_cache/`.
- **Fast In-Process Fallback:** During development or testing without `rustc`, RHP transparently provides an in-process thread-safe native execution engine.
- **Thread-Safety & Multi-Worker Concurrency:** State stored across worker threads utilizes `FrozenValue` boundaries, ensuring zero data races across multi-threaded Tokio execution pools without GC pauses.

---

Have any questions or ready to test your first RHP script?


