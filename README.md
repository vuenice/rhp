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

- **Dual-Engine Execution Modes:**
  - **Development Mode (`rhp run dev`):** Uses a direct tree-walking AST interpreter for sub-millisecond hot-reloading, instant cold starts, and immediate stack traces.
  - **Production Mode (`rhp run build`):** Lowers code into a register-based Bytecode VM with optional Cranelift JIT compilation, running on a multi-worker Tokio execution pool for maximum throughput.
- **Asynchronous & Synchronous Flexibility:** RHP is dynamically typed and supports both synchronous and asynchronous execution. Most database queries and file reads execute sequentially in local fibers without extra boilerplate, but you can explicitly offload tasks when needed.
- **Rust-Powered Reliability:** RHP is built on Rust to guarantee memory safety, eliminate crashes from isolated user requests, and provide top-tier request handling speeds.

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

### 2. Str Facade

Provides clean, UTF-8-safe string inspection and transformation utilities:

```php
$slug = Str::slug('Laravel Style Facades'); // 'laravel-style-facades'
$token = Str::random(32);
$hasWord = Str::contains('Runtime engine', 'engine'); // true
```

### 3. Asynchronous Execution (`async` / `defer`)

- **Use `async()` to spawn background tasks and return immediate futures:**

```php
$fileFuture = async(File::read('data.csv'));
$data = $fileFuture->wait();
```

- **Use `defer()` to execute post-response hooks after the payload has been flushed to the client, ensuring background logging or cleanup tasks never delay user response times.**

---

Have any questions or ready to test your first RHP script?
