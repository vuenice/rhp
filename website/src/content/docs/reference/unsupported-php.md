---
title: "Deprecated PHP Features"
description: "Exhaustive guide to deprecated and unsupported PHP features (Reflection, Output Buffering, cURL, Serialization, Directives, list(), die(), error_reporting, Named Arguments, var_export, dynamic include/require), architectural rationale, modern RHP replacements, and migration guide."
section: "Reference"
order: 5
keywords: ["unsupported", "unsupported php", "deprecated php", "deprecated php features", "depriciated php features", "include", "require", "include_once", "require_once", "spl_autoload_register", "dynamic imports", "static imports", "psr-4 autoload", "named arguments", "named parameters", "skipped arguments", "var_export", "var_dump", "__set_state", "debug::format", "debug::dump", "bin::encode", "json::encode", "error_reporting", "error reporting", "set_error_handler", "@ error suppression", "reflection", "output buffering", "curl", "serialization", "php to rhp migration"]
---

# Deprecated PHP Features

This document details legacy PHP features that are deprecated and deliberately **not supported** in RHP, why each was rejected, what modern replacements exist in RHP, which code to change during migration, and why the RHP architecture is fundamentally superior to traditional PHP.

> [!IMPORTANT]
> **RHP is not PHP-compatible.** Running unmodified PHP or Laravel applications is explicitly out of scope. The `rhp migrate` tool automatically rewrites common idiomatic patterns and flags incompatible constructs for manual review.

---

## 1. Reflection API

**Not supported:** `ReflectionClass`, `ReflectionMethod`, `ReflectionProperty`, `ReflectionFunction`, `ReflectionParameter`, and the entire `Reflection*` family.

### What Was Bad About It in PHP

- **Huge surface, little actual use:** `ReflectionClass` alone has 60+ methods. Modern frameworks like Laravel only utilize a small fraction: constructor parameter types, method parameters, attributes, `newInstanceArgs()`, and property get/set. Supporting the entire reflection API forces a runtime to carry legacy engine metadata (doc comments, source line locations, generator state internals) indefinitely.
- **Runtime cost for a compile-time problem:** Dependency injection, method injection, and route parameter binding are fully resolvable at build time. Doing them via runtime introspection incurs heavy per-request CPU and memory allocations, while deferring dependency failures from build time to production.
- **Breaks optimization:** Private-member bypass (`$reflector->setAccessible(true)`) destroys JIT compiler invariants such as inlining, devirtualization, and optimal struct field layouts.
- **Blocks dead-code elimination (tree-shaking):** When any class or method can be dynamically invoked by string name at runtime, the compiler must preserve every symbol in the final binary, preventing binary-size reduction.
- **Rust interop gap:** Compiled Rust types, crates, and FFI bindings do not possess PHP-style runtime metadata. Reflection would be blind to them unless a costly derive macro were generated for every exported type.
- **Hides errors until runtime:** An unresolvable constructor parameter, missing interface binding, or circular dependency only triggers a crash when that specific code path is visited in production.

---

### What Replaces It in RHP

1. **Build-Time Dependency Injection (DI) Codegen:**
   When running `rhp run build`, the compiler analyzes the dependency graph from mandatory type signatures, detects circular dependencies, verifies bindings, and generates zero-overhead factory functions (`GeneratedClassFactory`). At request time, instantiating a controller or service is a direct function call with zero reflection overhead.
2. **Compile-Time Static Metadata Table:**
   Read-only static descriptors for classes, methods, and attributes are generated into a static program metadata table during compilation.
3. **Compile-Time Attribute Discovery:**
   Attributes such as `#[Route]`, `#[Command]`, `#[Listener]`, and `#[Policy]` are discovered and indexed at build time into static dispatch tables, eliminating runtime filesystem directory scanning and file-token reflection.
4. **Method Injection & Fast Handler Shims:**
   Route handlers inspect parameters once during registration/build time, synthesizing fast direct call shims (`HandlerShim`) for incoming `Request`, `RouteBindable` models, container services, and URL scalar parameters.
5. **Zero-Reflection Test Container Swapping:**
   Instead of using `setAccessible(true)` to overwrite private state, testing leverages explicit, type-safe container swapping via `App::swap()` or `$container->swap()`.

---

### Which Code to Change: Before & After

#### Example A: Dependency Injection & Service Instantiation

**Before (Legacy PHP with Runtime Reflection):**
```php
// In PHP: Runtime reflection inspects constructor parameters on every request
$reflector = new ReflectionClass(OrderController::class);
$constructor = $reflector->getConstructor();
$dependencies = [];

if ($constructor !== null) {
    foreach ($constructor->getParameters() as $param) {
        $type = $param->getType();
        if ($type instanceof ReflectionNamedType && !$type->isBuiltin()) {
            $dependencies[] = $container->get($type->getName());
        }
    }
}

$controller = $reflector->newInstanceArgs($dependencies);
```

**After (Modern RHP with Build-Time Codegen):**
```php
// In RHP: Write standard typed constructors.
// The compiler generates static factory dispatch during 'rhp run build'.
class OrderController {
    public function __construct(
        private OrderService $orderService,
        private PaymentGateway $gateway,
        private Logger $logger
    ) {}

    public function checkout(Request $request): Response {
        $order = $this->orderService->create($request->all());
        $this->gateway->charge($order);
        return Response::json(['status' => 'success']);
    }
}

// Factory emitted at build time (conceptually):
// $container->bind(OrderController::class, fn(Container $c) =>
//     new OrderController($c->get(OrderService::class), $c->get(PaymentGateway::class), $c->get(Logger::class))
// );
```

#### Example B: Attribute Inspection (Routing & Discovery)

**Before (Legacy PHP):**
```php
// In PHP: Iterates classes and methods at boot or runtime via reflection
$reflector = new ReflectionClass(UserController::class);
foreach ($reflector->getMethods() as $method) {
    $attributes = $method->getAttributes(Route::class);
    foreach ($attributes as $attribute) {
        $route = $attribute->newInstance();
        $router->add($route->path, [$controller, $method->getName()]);
    }
}
```

**After (Modern RHP):**
```php
// In RHP: Attributes are extracted at compile time into the ProgramMetadata registry
class UserController {
    #[Route('/users/{id}', methods: ['GET'])]
    public function show(int $id): Response {
        return Response::json(['id' => $id]);
    }
}

// No runtime scanning needed. The router boots directly from pre-compiled tables.
```

#### Example C: Mocking Dependencies & Private State in Tests

**Before (Legacy PHP):**
```php
// In PHP: Penetrating private visibility with setAccessible
$service = new BillingService();
$property = new ReflectionProperty(BillingService::class, 'stripeSecret');
$property->setAccessible(true);
$property->setValue($service, 'sk_test_fake_key');
```

**After (Modern RHP):**
```php
// In RHP: Use explicit constructor injection or container swapping
App::swap(BillingService::class, new MockBillingService('sk_test_fake_key'));
```

---

### Why RHP Is Better Than PHP

| Feature Dimension | Legacy PHP Reflection | RHP Build-Time Model | Why RHP Wins |
| :--- | :--- | :--- | :--- |
| **Execution Speed** | Reflection checks cost microseconds per request | Instantaneous direct function pointer call | **$10\times$ to $50\times$ faster** controller dispatch; zero allocation on request path |
| **Failure Detection** | Runtime exception on first execution | Compile-time diagnostic (`rhp run build`) | Catches unresolvable parameters and circular cycles before deploying to production |
| **JIT Optimization** | `setAccessible` breaks struct offsets & inlining | Immutable layouts preserve JIT invariants | Cranelift JIT can inline constructors and devirtualize method calls |
| **Binary Size** | Keeps all symbol names and dead classes | Tree-shaking strips unreachable code | Significantly smaller production binary footprint |
| **Rust FFI Interop** | Fails on native Rust structs without bindings | Seamless interface-based dispatch | Native Rust classes can be injected directly without reflection shims |

---

## 2. Output Buffering API

**Not supported:** `ob_start()`, `ob_get_clean()`, `ob_end_clean()`, `ob_get_contents()`, `ob_end_flush()`, `ob_get_level()`, output handler callbacks, and `ob_implicit_flush()`.

### What Was Bad About It in PHP

- **Unbalanced global stack:** A forgotten or exception-skipped `ob_end_clean()` leaves the buffer open. In long-running worker architectures (such as Octane or FrankenPHP), this leaks captured output into subsequent requests, necessitating cumbersome `while (ob_get_level() > 0) ob_end_clean();` cleanup loops.
- **Global state conflicts with concurrency:** Output buffers are process-global in PHP. In RHP's concurrent runtime, requests run in isolated worker arenas, and asynchronous tasks execute via `Future::run` and `Future::defer`. A global output stack would allow one concurrent task to capture or swallow another task's output.
- **Ambiguous deferred output:** Calling `echo` inside a `Future::defer` callback that runs after the HTTP response has already flushed to the socket has no sensible destination under PHP's shared stdout model.
- **Headers and output share one stream:** PHP's infamous `"Warning: Cannot modify header information - headers already sent"` error occurs because headers and standard output are coupled to the same procedural stream.
- **Wasteful for templates:** Capturing a template's `echo` output into a string allocates intermediate memory buffers, creates extra string copies, and requires manual exception cleanup.
- **Excessive complexity:** Chunking buffers (`chunk_size`), output handlers, and implicit flushing introduce complex state machines that modern web frameworks actively work around.

---

### What Replaces It in RHP

1. **`Output::capture(fn() => ...): string`:**
   A scoped, exception-safe capture mechanism. The capture sink is automatically released via RAII when the closure exits, even if an unhandled exception or panic occurs.
2. **Direct Writer-Based Templates:**
   Templates compile to functions that write directly into a response writer (`render(Writer $out, ...)`), bypassing string buffering completely.
3. **Explicit Streaming via `Response::stream(...)`:**
   HTTP streaming (Server-Sent Events, progressive downloads) is performed through explicit stream writers rather than procedural `flush()` calls.
4. **Request-Isolated Execution Arenas:**
   Output buffers belong exclusively to the current request or task context within the worker's `RequestArena`, never in a process-wide global stack.

---

### Which Code to Change: Before & After

#### Example A: Capturing Output to a String

**Before (Legacy PHP):**
```php
// In PHP: Error-prone global stack requiring manual cleanup
ob_start();
try {
    echo "<h1>Welcome, ";
    echo htmlspecialchars($username);
    echo "</h1>";
    $html = ob_get_clean();
} catch (Throwable $e) {
    ob_end_clean(); // Essential, or buffer leaks into next request
    throw $e;
}
```

**After (Modern RHP):**
```php
// In RHP: Exception-safe closure capture with automatic RAII release
string $html = Output::capture(function() use ($username): void {
    echo "<h1>Welcome, " . Html::specialChars($username) . "</h1>";
});
```

#### Example B: Template Rendering

**Before (Legacy PHP):**
```php
// In PHP: Procedural include with output buffering
function renderView(string $viewPath, array $data): string {
    extract($data);
    ob_start();
    include $viewPath;
    return ob_get_clean();
}
```

**After (Modern RHP):**
```php
// In RHP: Native template compilation or direct Writer streaming
function renderProfile(Writer $out, dict $data): void {
    $out->write("<h1>Profile: " . Html::specialChars($data['name']) . "</h1>");
    $out->write("<p>Email: " . Html::specialChars($data['email']) . "</p>");
}

// Or returning structured View response:
return View::render('users.profile', [
    'name' => 'Alice',
    'email' => 'alice@example.com',
]);
```

#### Example C: Real-Time HTTP Streaming (Server-Sent Events)

**Before (Legacy PHP):**
```php
// In PHP: Relying on side-effect-driven flush() and global buffer clearing
header('Content-Type: text/event-stream');
header('Cache-Control: no-cache');
ob_end_flush();
ob_implicit_flush(true);

for ($i = 0; $i < 5; $i++) {
    echo "data: " . json_encode(['tick' => $i]) . "\n\n";
    flush();
    usleep(200000);
}
```

**After (Modern RHP):**
```php
// In RHP: Explicit, non-blocking asynchronous response stream
return Response::stream(function(StreamWriter $stream): void {
    for (int $i = 0; $i < 5; $i++) {
        dict $tickData = ['tick' => $i];
        $payload = $tickData->toJson();
        $stream->write("data: {$payload}\n\n");
        Future::delayed(200); // Non-blocking Tokio fiber delay
    }
}, headers: [
    'Content-Type' => 'text/event-stream',
    'Cache-Control' => 'no-cache',
]);
```

---

### Why RHP Is Better Than PHP

| Feature Dimension | Legacy PHP Output Buffering | RHP Architecture | Why RHP Wins |
| :--- | :--- | :--- | :--- |
| **Worker State Safety** | Forgotten buffer corrupts subsequent requests | Scoped RAII closure releases automatically | **Zero buffer leakage** across requests in persistent worker pools |
| **Concurrency & Async** | Global buffer cross-contaminates concurrent tasks | Context-bound buffer per fiber/request | Safely execute asynchronous tasks (`Future::run`) without output collisions |
| **Header Safety** | `"Headers already sent"` fatal warnings | Structured `Response` object sent by server | Headers and body are strictly decoupled; impossibility of header send order bugs |
| **Memory Efficiency** | Buffers copy strings multiple times on heap | Direct streaming into `RequestArena` or socket | Zero intermediate string allocations; dramatic reduction in GC pressure |

---

## 3. cURL Extension

**Not supported:** `curl_init()`, `curl_setopt()`, `curl_setopt_array()`, `curl_exec()`, `curl_close()`, `curl_errno()`, `curl_error()`, `curl_getinfo()`, and all `curl_multi_*` functions.

### What Was Bad About It in PHP

- **Cannot be typed:** `curl_setopt($ch, CURLOPT_X, $value)` accepts completely different value types (strings, integers, booleans, arrays, file handles, closures) depending on an untyped integer constant. This fundamentally violates RHP's mandatory prefix typing system.
- **Opaque mutable handle with ~250 options:** Most of the ~250 `CURLOPT_*` options are obsolete, obscure, or hazardous. Navigating them requires continuous documentation lookups.
- **Hazardous defaults:** PHP's cURL extension defaults to **no timeout** (`CURLOPT_TIMEOUT = 0`). If an upstream service hangs, the worker process stalls indefinitely, exhausting server connection pools.
- **Pointless wrapper overhead:** Production PHP code almost never uses raw `curl_*`; applications rely on Laravel's `Http` facade, which wraps Guzzle, which wraps cURL, which wraps `libcurl`. Maintaining PHP's `curl_*` C extension API only preserves an outdated wrapper layer.
- **Clunky callback signatures:** Callback options such as `CURLOPT_WRITEFUNCTION` and progress callbacks require awkward, stateful closures that are difficult to verify statically.

---

### What Replaces It in RHP

1. **High-Performance Typed `Http` & `AHTTP` Engine in Core:**
   Built natively on top of Rust's premier networking crates (`reqwest`, `hyper`, `rustls`). Provides secure, sane defaults (5s connect timeout, 30s read timeout, maximum 5 redirects, TLS 1.3).
2. **Laravel Rocket `Http` Facade:**
   Provides an expressive, fluent API supporting authentication, headers, multipart uploads, retries with exponential backoff, JSON serialization, and request pooling (`Http::pool`).
3. **Native Tokio Async Concurrency:**
   Outbound HTTP requests integrate directly with RHP's Tokio fiber runtime via `AHTTP` and `Future::wait`, executing concurrent requests without blocking operating system threads.
4. **Seamless Testing & Mocking:**
   Native `Http::fake()` allows deterministic test mocking without monkey-patching socket handles or configuring external mock servers.
5. **Rust Interop as an Escape Hatch:**
   For specialized low-level networking (custom DNS resolution, mutual mTLS with custom hardware tokens, non-HTTP protocols), developers can directly import a Rust `.rs` helper file with zero C-FFI overhead.

---

### Which Code to Change: Before & After

#### Example A: GET Request with Bearer Token and JSON Parsing

**Before (Legacy PHP):**
```php
// In PHP: 25+ lines of procedural boilerplate with opaque constants
$ch = curl_init();
curl_setopt($ch, CURLOPT_URL, "https://api.example.com/v1/users/42");
curl_setopt($ch, CURLOPT_RETURNTRANSFER, true);
curl_setopt($ch, CURLOPT_TIMEOUT, 10);
curl_setopt($ch, CURLOPT_HTTPHEADER, [
    "Authorization: Bearer sec_tok_9918",
    "Accept: application/json",
]);

$raw = curl_exec($ch);
$statusCode = curl_getinfo($ch, CURLINFO_HTTP_CODE);

if (curl_errno($ch)) {
    $err = curl_error($ch);
    curl_close($ch);
    throw new RuntimeException("cURL failed: {$err}");
}
curl_close($ch);

$data = json_decode($raw, true);
if ($statusCode >= 400) {
    throw new RuntimeException("API error: {$statusCode}");
}
```

**After (Modern RHP):**
```php
// In RHP: Strongly-typed, chainable, safe facade
$response = Http::withToken("sec_tok_9918")
    ->timeout(10)
    ->get("https://api.example.com/v1/users/42");

if ($response->failed()) {
    throw new RuntimeException("API error: " . $response->status());
}

dict $data = $response->json();
```

#### Example B: POST Request with JSON Body & Basic Authentication

**Before (Legacy PHP):**
```php
// In PHP: Manual header formatting and payload serialization
$ch = curl_init("https://api.example.com/v1/orders");
curl_setopt($ch, CURLOPT_POST, true);
curl_setopt($ch, CURLOPT_POSTFIELDS, json_encode(['sku' => 'LAPTOP-01', 'qty' => 1]));
curl_setopt($ch, CURLOPT_USERPWD, "api_user:secret_pass");
curl_setopt($ch, CURLOPT_RETURNTRANSFER, true);
curl_setopt($ch, CURLOPT_HTTPHEADER, [
    'Content-Type: application/json',
]);
$res = curl_exec($ch);
curl_close($ch);
```

**After (Modern RHP):**
```php
// In RHP: Automatic JSON serialization and header setting
$response = Http::withBasicAuth("api_user", "secret_pass")
    ->post("https://api.example.com/v1/orders", [
        'sku' => 'LAPTOP-01',
        'qty' => 1,
    ]);
```

#### Example C: Concurrent Outbound Requests (Replacing `curl_multi`)

**Before (Legacy PHP with `curl_multi`):**
```php
// In PHP: Complex, manual non-blocking polling loop
$mh = curl_multi_init();
$ch1 = curl_init("https://api.billing.com/status");
$ch2 = curl_init("https://api.shipping.com/rates");
curl_setopt($ch1, CURLOPT_RETURNTRANSFER, true);
curl_setopt($ch2, CURLOPT_RETURNTRANSFER, true);
curl_multi_add_handle($mh, $ch1);
curl_multi_add_handle($mh, $ch2);

$active = null;
do {
    $status = curl_multi_exec($mh, $active);
    if ($active) {
        curl_multi_select($mh);
    }
} while ($active && $status == CURLM_OK);

$res1 = curl_multi_getcontent($ch1);
$res2 = curl_multi_getcontent($ch2);
curl_multi_remove_handle($mh, $ch1);
curl_multi_remove_handle($mh, $ch2);
curl_multi_close($mh);
```

**After (Modern RHP):**
```php
// In RHP: Concurrent request pool executed across Tokio fiber workers
$responses = Http::pool([
    'billing'  => fn() => Http::timeout(5)->get("https://api.billing.com/status"),
    'shipping' => fn() => Http::timeout(5)->get("https://api.shipping.com/rates"),
]);

dict $billingStatus = $responses['billing']->json();
dict $shippingRates = $responses['shipping']->json();

// Or using low-level asynchronous AHTTP with Future::wait:
list $results = Future::wait([
    AHTTP::get("https://api.billing.com/status"),
    AHTTP::get("https://api.shipping.com/rates"),
]);
```

---

### Why RHP Is Better Than PHP

| Feature Dimension | Legacy PHP `curl_*` | RHP `Http` / `AHTTP` Engine | Why RHP Wins |
| :--- | :--- | :--- | :--- |
| **Type Safety** | Untyped `curl_setopt($ch, INT, mixed)` | Strictly-typed fluent builder methods | Compile-time validation; no runtime option type mismatches |
| **Default Security** | No timeout by default (`timeout=0`) | Safe default timeouts & redirect caps | Prevents worker starvation and runaway outbound connections |
| **Async Concurrency** | Thread-blocking `curl_multi_select` polling | Tokio non-blocking event-driven I/O | Concurrently dispatch thousands of requests with near-zero memory |
| **Architecture** | App $\rightarrow$ Guzzle $\rightarrow$ cURL ext $\rightarrow$ libcurl | App $\rightarrow$ Native Rust HTTP (`reqwest` + `rustls`) | Minimal overhead; zero C-extension segfault risks |
| **Test Ergonomics** | Difficult mocking requiring C handle mocks | Native `Http::fake(['*' => ...])` | Fast, deterministic unit tests without network access |
---

## 4. Legacy Object Serialization & Deserialization

**Not supported:** `serialize()`, `unserialize()`, `__sleep()`, `__wakeup()`, PHP text-encoded serialization strings (`O:4:"User":2:{...}`), and unvalidated arbitrary class instantiation from byte streams.

### What Was Bad About It in PHP

- **Remote Code Execution (RCE) via Gadget Chains:** PHP's `unserialize()` instantiates arbitrary classes by string name and populates internal state without invoking the constructor (`__construct()`). When magic methods like `__destruct()`, `__wakeup()`, `__toString()`, or `__call()` run on attacker-controlled property graphs, they trigger interconnected method calls known as "gadget chains". Attackers leverage classes present anywhere in the vendor autoloader (Monolog, Guzzle, Symfony, Laravel) to achieve file writes, SQL injection, or remote shell execution (`eval()`, `system()`). Framework gadget generators (such as PHPGGC) catalog dozens of automated exploits for virtually every major PHP library.
- **Open-by-Default Insecurity:** For decades, `unserialize()` accepted any class in the autoloader. The `allowed_classes` option added in PHP 7 was an opt-in afterthought that is routinely omitted or misconfigured (for example, allowing polymorphic container classes that hold untrusted inner objects).
- **Destructors on Half-Initialized Objects:** If deserialization throws an exception midway (due to malformed payloads, missing classes, or type mismatches), PHP's engine still invokes `__destruct()` on the partially initialized object during garbage collection. Attackers deliberately craft payloads that fail late in deserialization specifically to trigger destructor-based gadget execution.
- **Bloated, Fragile Text Format with Null-Byte Mangling:** PHP's string format (`O:4:"User":2:{s:4:"name";s:5:"Alice";...}`) is slow to tokenize, parse, and allocate. For private and protected properties, PHP mangles visibility keys by injecting invisible null bytes (`\0*\0property` and `\0ClassName\0property`). This leads to silent string truncation bugs in MySQL/Postgres text columns, breaks when classes are renamed or moved across namespaces, and tightly couples data payloads to private variable identifiers.
- **Fragile State Management (`__sleep` / `__wakeup`):** The legacy `__sleep()` hook returned an array of property names as strings. Renaming a property during a code deployment rendered all previously serialized queue jobs and cache entries in Redis corrupt or missing attributes.
- **Subverting Invariants and JIT Optimizations:** Bypassing constructors allows objects to exist in invalid states that violate compile-time guarantees. In an optimizing JIT runtime, arbitrary property injection invalidates hidden class shapes, Cranelift unboxed slot layouts, and devirtualization assumptions.
- **Resource and Closure Serialization Traps:** Serializing database connections (`PDO`), sockets, or file handles yields useless dummy integers (`resource id #5`) that silently resurrect as dead handles. Userland attempts to serialize closures (by tokenizing PHP source or dumping opcodes) created severe security holes and broke across JIT and opcache versions.
- **Denial of Service (DoS):** Unbounded nesting depths, circular object bombs, and huge hash collisions trigger stack overflows and CPU/memory exhaustion with zero built-in limits.

---

### What Replaces It in RHP

1. **Compact, Versioned Binary Wire Format (MsgPack-Style with Magic Header):**
   RHP replaces PHP's text format with an efficient, tag-based binary format prefixed with a magic identifier and version byte (`[Magic 2B: "RH"][Format Version: 1B][Root Tag...]`). Scalars, UTF-8 strings, and collections (`list`, `dict`, `set`, `hashMap`) are encoded with 1-byte tags.
2. **Clean Object Protocol (`__serialize()` & `__unserialize(dict)` + `#[Transient]`):**
   RHP skips `__sleep()` and `__wakeup()` entirely. Classes explicitly implement:
   - `public function __serialize(): dict`: returns an explicit key-value dictionary of exportable state.
   - `public function __unserialize(dict $data): void`: receives the validated dictionary to rehydrate the instance.
   When hooks are omitted, RHP automatically serializes non-transient properties. Properties annotated with `#[Transient]` are automatically omitted.
3. **Deny-by-Default Security with Strict Allowlisting:**
   `unserialize($bytes, allowed_classes: [...])` is **deny-by-default for objects**. If a binary payload references a class not present in `allowed_classes` (or registered in the application's serializable registry), deserialization halts immediately before allocating the object, preventing gadget instantiation.
4. **Destructor Neutralization on Incomplete Restorations:**
   If deserialization fails or throws an exception, half-built objects are immediately marked invalid and discarded without running `__destruct()`, neutralizing destructor gadget chains completely.
5. **Graph-Preserving Memo Table for Cycles and Identity:**
   Shared references and circular object graphs are automatically preserved using variable-length index back-references (`Ref(index)`), preventing infinite recursion and preserving object identity.
6. **Built-in DoS Protections:**
   Configurable recursion depth caps (default 64) and byte size limits protect the runtime against decompression bombs and stack overflows.
7. **Safe Resources and Named Callables (No Closure Serialization):**
   Resources (database connections, file handles) and `Future` handles throw a `SerializationException` by default. Rather than attempting to serialize fragile closure bytecode, RHP serializes named callables (`[ClassName::class, 'methodName']`) and parameter lists.
8. **Laravel Rocket Queue & Model Integration (`SerializesModels`):**
   Queue jobs store model identity (`class` and primary key) rather than raw database entity state, re-querying the database upon worker pickup.
9. **Zero-Downtime Migration Reader (`unserialize_php`):**
   For seamless migration during cutover, an optional read-only PHP format parser (`unserialize_php($payload)`) allows reading legacy Redis sessions and cache data without rewriting existing datastores on day one.

---

### Which Code to Change: Before & After

#### Example A: Object Serialization Protocol (`__sleep`/`__wakeup` vs `__serialize`/`#[Transient]`)

**Before (Legacy PHP):**
```php
// In PHP: Fragile property string lists, null-byte mangling, and constructor bypass
class UserAccount {
    private int $id;
    private string $email;
    private string $passwordHash; // Sensitive, should not be serialized
    private ?PDO $db = null;     // Unserializable resource

    public function __sleep(): array {
        // String array breaks silently if properties are renamed or refactored
        return ['id', 'email'];
    }

    public function __wakeup(): void {
        // Executes automatically on untrusted object graph; vulnerable to gadget chains
        $this->db = Database::connect();
    }
}

$serialized = serialize($account); // Produces bulky text with null-byte mangled keys
```

**After (Modern RHP):**
```php
// In RHP: Explicit dictionary hooks or #[Transient] attribute
class UserAccount {
    public int $id;
    public string $email;

    #[Transient]
    public string $passwordHash; // Automatically skipped during serialization

    #[Transient]
    public ?DatabaseConnection $db = null;

    // Optional explicit hook for custom schema control
    public function __serialize(): dict {
        return [
            'version' => 1,
            'id' => $this->id,
            'email' => $this->email,
        ];
    }

    public function __unserialize(dict $data): void {
        $this->id = $data['id'];
        $this->email = $data['email'];
        // Safe: invoked only on allowlisted classes; destructor never runs if this throws
    }
}

// Compact binary wire format
string $wire = serialize($account);
```

#### Example B: Safe Deserialization with Deny-by-Default Allowlist

**Before (Legacy PHP):**
```php
// In PHP: Open-by-default; instantiates ANY class in the autoloader
// Vulnerable to RCE via gadget chains (Monolog, Guzzle, Symfony, etc.)
$data = unserialize($untrustedPayload);

// PHP 7 allowed_classes was opt-in, optional, and easily forgotten
$data = unserialize($untrustedPayload, ['allowed_classes' => [UserAccount::class]]);
```

**After (Modern RHP):**
```php
// In RHP: Deny-by-default for objects. Objects require explicit allowlist.
// Throws SecurityException if an unregistered class tag is encountered.
$account = unserialize($wireBytes, allowed_classes: [UserAccount::class]);

// Plain data (scalars, list, dict, set) deserializes safely without allowed_classes:
dict $config = unserialize($plainPayload);
```

#### Example C: Queued Jobs & Avoiding Closure Serialization

**Before (Legacy PHP):**
```php
// In PHP: Serializing closures via Opis/Closure or laravel/serializable-closure
// Extracts source code or opcodes via reflection and re-evals them; high security risk
dispatch(function() use ($orderId) {
    OrderProcessor::process($orderId);
});
```

**After (Modern RHP):**
```php
// In RHP: Explicit named job classes or named callables with typed parameters
class ProcessOrderJob {
    use SerializesModels;

    public function __construct(
        public Order $order // Re-queried by ID upon worker pickup via SerializesModels
    ) {}

    public function handle(): void {
        OrderProcessor::process($this->order);
    }
}

Queue::push(new ProcessOrderJob($order));
```

---

### Why RHP Is Better Than PHP

| Feature Dimension | Legacy PHP `serialize()` / `unserialize()` | RHP Native Object Serialization | Why RHP Wins |
| :--- | :--- | :--- | :--- |
| **Security & Gadget Chains** | Open-by-default; executes arbitrary class magic methods | Deny-by-default; explicit `allowed_classes` or registry | **Zero gadget chain RCE surface**; untrusted classes never instantiated |
| **Destructor Safety** | `__destruct()` runs even on failed/half-built objects | Destructor is suppressed if deserialization aborts | Eliminates destructor-based gadget chains and resource leaks |
| **Format & Performance** | Verbose text format with null bytes (`\0*\0`) | Compact binary format (MessagePack-style tags) | **$3\times$ to $8\times$ smaller payloads**; instantaneous binary decoding |
| **Format Versioning** | No built-in versioning; breaks across deployments | Embedded wire format version byte + schema versioning | Safe cross-deployment queue jobs and cache storage |
| **Object Graph & Cycles** | Verbose back-reference strings (`r:`, `R:`) | Memo table with variable-length index tags | Safely restores cyclic and DAG object graphs without stack overflow |
| **Type Integrity & JIT** | Injects unverified properties bypassing constructors | Enforces typed dictionary boundaries (`__unserialize(dict)`) | Preserves Cranelift hidden class shapes, unboxed slots, and devirtualization |
| **DoS Protection** | Vulnerable to memory/depth decompression bombs | Enforced recursion depth (64) and size limits | Prevents stack overflows and memory exhaustion attacks |
| **Closure Handling** | Unsafe runtime opcode/source eval (`eval()`) | Named callables (`[Class, 'method']`) + args | Zero code-eval vulnerability; deterministic compilation |

---

## 5. Strict Comparison Operators (`===` / `!==`) & Implicit Loose Coercion

**Not supported:** The triple-equals operators `===` and `!==`, as well as implicit type-coercing loose equality.

### What Was Bad About It in PHP

- **Security vulnerabilities & logic flaws:** PHP's legacy loose equality `==` was notoriously unpredictable. Operands of different types were coerced implicitly, leading to critical vulnerabilities (e.g. `'admin' == 0` evaluating to `true` in PHP 7, hash collision bypasses like `"0e123" == "0e456"`, and `42 == "42"`).
- **Redundant operator syntax:** Requiring developers to remember typing three equal signs (`===`) for basic safety added mental overhead and resulted in accidental bugs whenever someone typed `==` instead.
- **Float representation bugs:** Traditional float comparison with `==` failed on IEEE-754 rounding differences (such as `3.14f32` vs `3.14f64` or representation inaccuracies).

---

### What Replaces It in RHP

1. **Strict Type-Safe `==` and `!=` by Default:**
   In RHP, `==` and `!=` are strict by design. They compare both the types and the values.
2. **Compile-Time Type Mismatch Rejection:**
   If operands on either side of `==` or `!=` are not of compatible types (e.g. `42 == "42"` or `true == 1`), the compiler emits an immediate compile-time type error (`InvalidBinaryOperation`).
3. **Compiler Rejection of `===` and `!==`:**
   The parser rejects `===` and `!==` with diagnostic error `RHP0116`, pointing developers to `==` and `!=`.
4. **Automated Linter Conversion:**
   Running `rhp run lint` automatically updates all legacy `===` to `==` and `!==` to `!=` while preserving string literals and comments intact.
5. **Accurate Float Precision Comparison:**
   When comparing floats with `==`, RHP checks the maximum decimal point across both operands ($D_{\max} = \max(D_{\text{left}}, D_{\text{right}})$) and compares both sides to that exact decimal precision.
6. **Explicit Loose Comparison Facades:**
   When cross-type coercion is explicitly required, developers use intention-revealing facade helpers:
   - `Bool::compare($a, $b)`
   - `Str::compare($a, $b, $case_insensitive = false)`
   - `Int::compare($a, $b)`
   - `Float::compare($a, $b, $epsilon = null)`
   - `Array::compareJson($a, $b)`, `Array::compareRef($a, $b)`, `Array::compareJoined($a, $b, $sep)`, `Array::compareUnordered($a, $b)`

---

### Which Code to Change: Before & After

#### Example A: Equality and Identity Checks

**Before (Legacy PHP):**
```php
// In PHP: === was required to prevent dangerous loose coercion
if ($status === 200) {
    // ...
}
if ($raw_id == 100) {
    // Unsafe: converts "100-abc" or true to 100
}
```

**After (Modern RHP):**
```php
// In RHP: == is already strict and type-safe
if ($status == 200) {
    // ...
}

// Loose cross-type comparison is explicit:
if (Int::compare($raw_id, 100)) {
    // Intentionally loose comparison
}
```

---

## 6. Directive Statements (`declare(...)`)

**Not supported:** `declare(strict_types=1);`, `declare(ticks=1);`, `declare(encoding='...');`, and all block or statement forms of `declare(...)`.

### What Was Bad About It in PHP

- **Redundant per-file boilerplate:** PHP required developers to manually copy-paste `declare(strict_types=1);` onto line 1 of every single file. Forgetting it in a single file silently reverted that file to legacy loose typing, creating insidious bugs where invalid types were silently coerced.
- **Fragmented, caller-dependent semantics:** In PHP, whether a function call enforces type contracts depends on the caller file's `strict_types` pragma, not on the function declaration itself. If an un-strict legacy file invoked a strictly typed library function, PHP silently coerced invalid arguments at runtime, destroying cross-module type integrity.
- **Tick overhead cripples JIT compiler optimizations:** The `declare(ticks=N)` directive registers low-level tick callbacks that the PHP Zend Engine polls every $N$ opcodes. Polling tick counters between instructions severely degrades instruction pipelining, invalidates CPU branch prediction, and prevents modern optimizing JIT compilers (such as Cranelift) from inlining and optimizing hot loops.
- **Archaic script-encoding pragmas:** Directives like `declare(encoding=...);` were created before universal UTF-8 adoption. Modern web architectures require uniform UTF-8 encoding across all compilation units, making per-file source encoding declarations obsolete.

---

### What Replaces It in RHP

1. **Universal Compiler-Enforced Strict Typing:**
   In RHP, strict typing is an unalterable compiler guarantee applied uniformly across the entire project. There are no loose-type execution modes, and no per-file pragmas are needed. Function parameter types, property types, and return values are verified at compile time and enforced at runtime boundaries with zero boilerplate.
2. **Deterministic Cross-Module Contracts:**
   Type signatures in RHP are always enforced at the callee boundary, regardless of how or where a function is called.
3. **Tokio Async Fiber Runtime & OpenTelemetry:**
   Instead of archaic interpreter tick polling (`declare(ticks=1)`), RHP uses an asynchronous Tokio fiber runtime and native OpenTelemetry tracing (`Telemetry::trace()`), providing low-overhead task scheduling, profiling, signal interception, and execution tracing.
4. **Strict UTF-8 Source Encoding:**
   All RHP source files are strictly validated as UTF-8 during lexing, eliminating the need for encoding directives.

---

### Which Code to Change: Before & After

#### Example A: Eliminating `declare(strict_types=1);`

**Before (Legacy PHP):**
```php
<?php
// In PHP: Required on every single file to prevent silent coercion
declare(strict_types=1);

function calculateTax(int $amount, float $rate): float {
    return $amount * $rate;
}
```

**After (Modern RHP):**
```php
// In RHP: No declare directive needed.
// Strict type safety is active project-wide by default.
function calculateTax(int $amount, float $rate): float {
    return $amount * $rate;
}
```

#### Example B: Replacing `declare(ticks=1);` with Native Telemetry / Async

**Before (Legacy PHP):**
```php
// In PHP: Slow opcode-level tick handler for profiling/signals
declare(ticks=1);

register_tick_function(function() {
    echo "Tick executed\n";
});

doHeavyProcessing();
```

**After (Modern RHP):**
```php
// In RHP: Zero-overhead OpenTelemetry tracing or Tokio async tasks
use App\Telemetry;

Telemetry::trace('heavy_processing', function(): void {
    doHeavyProcessing();
});
```

---

### Why RHP Is Better Than PHP

| Feature Dimension | Legacy PHP Directives (`declare(...)`) | RHP Compiler Architecture | Why RHP Wins |
| :--- | :--- | :--- | :--- |
| **Typing Consistency** | Fragmented per-file; caller decides callee strictness | Universal compiler-enforced strict typing | Immune to caller-dependent type coercion bugs; zero per-file boilerplate |
| **Project Maintenance** | Must paste `declare(strict_types=1);` on every file | Zero declaration needed | Cleaner files; impossible to accidentally omit strict typing in new files |
| **JIT Optimization** | `ticks` forces opcode polling, destroying JIT loops | Clean loop IR with zero instruction polling | Cranelift JIT can fully inline, vectorize, and optimize hot compilation units |
| **Signal & Tracing** | Archaic synchronous tick callbacks | Non-blocking Tokio fiber runtime & OpenTelemetry | Enterprise-grade tracing, profiling, and cancellation with negligible overhead |

---

## 7. Classic `list()` Destructuring Assignment (`list($a, $b) = $array`)

**Not supported:** `list($a, $b) = $array`, `list($a, , $c) = $array`, `list('key' => $val) = $array`, and the `list(...)` statement construct for destructuring assignment.

### What Was Bad About It in PHP

- **Lexical keyword collision with RHP typed collections:** In RHP, `list` is a dedicated language keyword for sequential contiguous-memory collection subtypes (`list $items`, `list<string>`). Allowing `list(...)` as a destructuring construct introduces parsing ambiguity between variable type declarations, constructor-like expressions, and destructuring statements.
- **Pseudo-function syntactical illusion:** While `list()` resembled a built-in function or constructor, it was actually a special parser construct that could only appear on the left-hand side of an assignment operator (`=`). It could not be evaluated as an expression, passed to callables, or assigned directly, creating confusing language asymmetries.
- **Inconsistent error semantics:** Legacy PHP emitted undefined key warnings (`E_WARNING: Undefined array key`) or fatal type errors depending on PHP version when unpacking non-arrays or incomplete arrays, lacking type-safe guarantees.
- **Outdated and redundant:** Modern PHP (since 7.1) and all modern programming languages (Rust, JavaScript/TypeScript, Python) standardize on square-bracket or pattern destructuring (`[$a, $b] = $array;`), rendering the `list()` construct an obsolete syntax artifact.

---

### What Replaces It in RHP

1. **Clean Square-Bracket Destructuring (`[...]`):**
   RHP natively supports concise, modern square-bracket destructuring for sequential arrays, lists, and associative dictionaries:
   - Positional unpacking: `[$id, $name, $email] = $user;`
   - Skipping items: `[$first, , $third] = $items;`
   - Keyed dictionary unpacking: `['status' => $status, 'data' => $payload] = $response;`
2. **Dedicated `list` Collection Subtype:**
   The `list` keyword is cleanly repurposed as a first-class, cache-efficient sequential collection type: `list $nums = [1, 2, 3];`.
3. **Foreach Tuple Unpacking:**
   Foreach loops natively support clean tuple and list unpacking: `foreach ($entries as [$key, $value]) { ... }`.
4. **Type-Safe `match` Pattern Destructuring:**
   Pattern matching allows robust extraction with compile-time type guarantees and pattern guards.

---

### Which Code to Change: Before & After

#### Example A: Sequential Array Destructuring

**Before (Legacy PHP):**
```php
// In PHP: Clunky list() language construct
$coords = [100, 250];
list($x, $y) = $coords;

// Skipping elements with empty slots
list($r, , $b) = getRgbColors();
```

**After (Modern RHP):**
```php
// In RHP: Modern square-bracket destructuring
$coords = [100, 250];
[$x, $y] = $coords;

// Skipping elements with clean commas
[$r, , $b] = getRgbColors();

// list is reserved for typed collection declarations:
list $numbers = [100, 250];
```

#### Example B: Keyed Associative Destructuring

**Before (Legacy PHP):**
```php
// In PHP: Verbose list() with string keys
list('id' => $id, 'role' => $role) = $sessionData;
```

**After (Modern RHP):**
```php
// In RHP: Clean dictionary destructuring
['id' => $id, 'role' => $role] = $sessionData;
```

---

### Why RHP Is Better Than PHP

| Feature Dimension | Legacy PHP `list(...)` | RHP Destructuring Model | Why RHP Wins |
| :--- | :--- | :--- | :--- |
| **Grammar & Lexing** | Ambiguous pseudo-function construct | Unambiguous `[...]` destructuring syntax | Clean parser grammar without special-case lvalue pseudo-functions |
| **Language Consistency** | Two competing syntaxes (`list()` vs `[]`) | Single unified `[...]` syntax | Standardized modern destructuring identical to modern JS, Rust, and PHP 8+ |
| **Keyword Clarity** | `list` hijacked as destructuring keyword | `list` reserved for sequential collection types | Enables high-performance typed arrays (`list<T>`) with optimal memory layout |
| **Type Safety** | Silent warnings on missing elements | Compile-time pattern checks and exception boundaries | Prevents undefined offset bugs during record extraction |

---

## 8. `die()` Language Construct

**Not supported:** `die()` and `die($message)`.

### What Was Bad About It in PHP

- **Terminating persistent workers:** In modern persistent worker runtimes (such as RHP's Tokio fiber pool, RoadRunner, FrankenPHP, and Laravel Octane), invoking `die()` aborts or crashes the worker execution thread. Abrupt termination destroys worker recycling, drops uncommitted database transactions, abandons connection pools, and can terminate concurrent tasks running inside the same process arena.
- **Bypasses structured exception handling:** `die()` terminates execution immediately without unwinding the call stack. It bypasses `try / catch / finally` blocks and prevents application cleanup hooks and destructors from executing cleanly.
- **Unformatted, unescaped raw output:** Calling `die("Something went wrong")` flushes an unformatted, unescaped plain string directly to stdout/HTTP response with a default `200 OK` status header. In REST APIs, GraphQL, and microservices, this breaks client JSON parsers and masks severe errors as successful HTTP responses.
- **Redundant Perl artifact:** `die()` was originally inherited from Perl idioms (`open(...) or die(...)`). In a typed, modern language with structured exceptions, having an alias for abnormal script termination encourages fragile and untestable control-flow anti-patterns.

---

### What Replaces It in RHP

1. **Structured Typed Exceptions:**
   Throw descriptive, domain-specific exceptions that unwind the stack safely, guarantee execution of `finally` blocks, and allow centralized error middleware to handle failure:
   `throw new RuntimeException("Database unreachable");` or `throw new NotFoundException("User not found");`.
2. **Standardized HTTP Aborts (`abort()` / `Response::json()`):**
   In web requests, terminate execution cleanly by returning structured HTTP error responses with appropriate HTTP status codes (e.g. 400, 403, 404, 500):
   `abort(404, "User not found");` or `return Response::json(['error' => 'Unauthorized'], 401);`.
3. **Explicit CLI Process Termination (`Sys::exit($code)`):**
   When intentionally exiting a command-line script or daemon task with a specific OS return code, use the explicit, intention-revealing `Sys::exit($code)` facade method.
4. **Scope-Guaranteed Teardown (`defer`):**
   Use RHP's native `defer` statements to guarantee that file handles, database locks, and tracing spans are cleaned up before any exit path.

---

### Which Code to Change: Before & After

#### Example A: Web Request Error Handling

**Before (Legacy PHP):**
```php
// In PHP: Raw termination breaks API JSON format and sends 200 OK
$order = Order::find($orderId);
if (!$order) {
    die("Order not found"); // Unformatted plain text sent to client!
}
```

**After (Modern RHP):**
```php
// In RHP: Proper HTTP abort with correct status code and JSON format
$order = Order::find($orderId);
if (!$order) {
    abort(404, "Order not found");
    // Or throw structured exception:
    // throw new NotFoundException("Order not found");
}
```

#### Example B: CLI Tool Termination

**Before (Legacy PHP):**
```php
// In PHP: Abrupt die() call without clear exit code
if (!File::exists($configPath)) {
    die("Fatal: missing configuration file\n");
}
```

**After (Modern RHP):**
```php
// In RHP: Explicit Sys facade with standard POSIX exit code
if (!File::exists($configPath)) {
    Log::error("Fatal: missing configuration file");
    Sys::exit(1);
}
```

---

### Why RHP Is Better Than PHP

| Feature Dimension | Legacy PHP `die(...)` | RHP Error Architecture | Why RHP Wins |
| :--- | :--- | :--- | :--- |
| **Worker Stability** | Hard crash disrupts persistent worker pools | Clean stack unwinding via exceptions / abort | **Zero worker process crashes**; safe in Tokio concurrent fiber runtimes |
| **API Contract Integrity** | Emits raw text with misleading `200 OK` | Structured JSON responses with proper HTTP codes | Preserves API response contracts; clients receive standard JSON error bodies |
| **Resource Cleanup** | Skips `finally` blocks and deferred callbacks | Guaranteed execution of `finally` and `defer` | Locks, transactions, and temporary files are always cleaned up safely |
| **Code Maintainability** | Uncontrolled side effects; impossible to unit test | Testable, catchable exceptions and mockable aborts | Deterministic error handling testable via `assertThrows()` and HTTP assertions |

---

## 9. Error Reporting Configuration (`error_reporting()`)

**Not supported / Deprecated:** `error_reporting()`, `set_error_handler()`, `restore_error_handler()`, `@` error suppression operator, `E_ALL`, `E_NOTICE`, `E_WARNING`, `E_DEPRECATED`, `E_STRICT`, `E_USER_*` error level bitmasks.

### What Was Bad About It in PHP

- **Process-global mutable state in concurrent runtimes:** In PHP, `error_reporting()` modifies the runtime error filtering bitmask globally for the process or worker thread. In RHP's concurrent, zero-bootstrap architecture where asynchronous tasks execute inside worker pools (Tokio fibers) across long-running daemon processes, dynamically mutating global error bitmasks causes severe race conditions, cross-request state bleeding, and unpredictable error masking across concurrent requests.
- **Silent bug masking & production outages:** Traditional PHP code heavily relies on `error_reporting(0)` or `error_reporting(E_ALL & ~E_NOTICE)` to suppress warnings, notices, and deprecations. Suppressing warnings allows undefined array keys, uninitialized variables, null dereferences, and implicit type conversion bugs to silently pass through execution. Instead of halting immediately at the source of defect, invalid data propagates deeper into business logic, resulting in corrupted database records or financial calculation bugs.
- **The hazardous `@` error-suppression operator:** PHP's `@` operator dynamically disables error reporting for a single expression by toggling the engine's error reporting level to 0 and restoring it afterwards. This introduces runtime CPU overhead (allocating opcode handlers and modifying engine state) while dangerously masking fatal or unexpected errors (such as file permission denials, socket timeouts, or database disconnections occurring within the suppressed expression).
- **Conflation of compiler diagnostics with runtime logging:** PHP's integer bitmask system conflates three distinct concerns: compiler/type diagnostics, runtime unrecoverable panics/exceptions, and operational logging. In modern engineering practices, syntax and type issues are compile-time errors, operational events belong in structured loggers with OpenTelemetry integration, and unexpected failures belong in typed exception handlers.
- **Impediment to compiler optimizations:** When an engine must support dynamic runtime error level changes and custom procedural warning callbacks (`set_error_handler`), the compiler cannot aggressively optimize type assertions, inline array lookups, or eliminate dead code branches.

---

### What Replaces It in RHP

1. **Compiler-Enforced Strict Diagnostics:**
   Undeclared variables, missing array keys, and invalid types are detected at build time via `rhp run lint` and `rhp run build`. There are no silent runtime notices or warnings that can be ignored.
2. **Declarative Logging Configuration (`rhp.toml`):**
   Logging thresholds are declared statically in configuration files (e.g. `[logging] level = "warn"`) or overridden via environment variables (`RHP_LOG_LEVEL=info`). Infrastructure concerns are completely decoupled from application business logic.
3. **Structured `Log` Facade with OpenTelemetry:**
   Diagnostic and operational messages are emitted using the type-safe `Log` facade (`Log::debug()`, `Log::info()`, `Log::warning()`, `Log::error()`) with structured contextual key-value dictionaries and automatic OpenTelemetry span propagation.
4. **Deterministic Typed Exceptions & `Result` Types:**
   Runtime operations that may fail (such as file I/O or network requests) either throw typed exceptions (`FileNotFoundException`, `IOException`) that unwind the stack safely or return explicit `Result<T, E>` / `Option<T>` types.
5. **Null-Safe Operators Instead of `@` Suppression:**
   Instead of silencing undefined access with `@`, RHP provides idiomatic language constructs: null coalescing (`??`), null-safe chaining (`?->`), and dictionary methods (`$dict->get('key', $default)`).

---

### Which Code to Change: Before & After

#### Example A: Configuring Error and Logging Thresholds

**Before (Legacy PHP):**
```php
// In PHP: Procedural mutation of global error bitmask at runtime
error_reporting(E_ALL & ~E_NOTICE & ~E_DEPRECATED);
ini_set('display_errors', '0');
ini_set('log_errors', '1');
```

**After (Modern RHP):**
```toml
# In RHP: Statically declared in rhp.toml (or via RHP_LOG_LEVEL env)
[logging]
level = "info" # "debug" | "info" | "warn" | "error"
format = "json" # "json" | "pretty"
tracing = true
```
```php
// In RHP application code: Use structured logging
Log::info("Application started", [
    'environment' => App::environment(),
    'version' => App::version(),
]);
```

#### Example B: Suppressing Errors on Risky Operations

**Before (Legacy PHP):**
```php
// In PHP: Using @ suppression to swallow filesystem or parse warnings
$contents = @file_get_contents('/tmp/config.json');
if ($contents === false) {
    $contents = '{}';
}

$data = @json_decode($contents, true);
$apiKey = @$data['api_key'] ?? 'default_key';
```

**After (Modern RHP):**
```php
// In RHP: Explicit Result handling or safe facade lookups
$contents = File::tryRead('/tmp/config.json')->unwrapOr('{}');
$data = Json::decode($contents);
$apiKey = $data->get('api_key', 'default_key');
```

#### Example C: Custom Error Handling and Exception Translation

**Before (Legacy PHP):**
```php
// In PHP: Procedural global error handler converting warnings to exceptions
set_error_handler(function(int $severity, string $message, string $file, int $line): bool {
    if (!(error_reporting() & $severity)) {
        return false;
    }
    throw new ErrorException($message, 0, $severity, $file, $line);
});
```

**After (Modern RHP):**
```php
// In RHP: All runtime failures are native typed exceptions by default.
// Centralized error handling is registered cleanly via application middleware:
App::onError(function(Throwable $e, Request $req): Response {
    Log::error("Unhandled exception: " . $e->getMessage(), [
        'exception' => $e::class,
        'path' => $req->path(),
    ]);

    return Response::json([
        'error' => 'Internal server error',
        'code' => 500,
    ], 500);
});
```

---

### Why RHP Is Better Than PHP

| Feature Dimension | Legacy PHP `error_reporting()` | RHP Declarative Architecture | Why RHP Wins |
| :--- | :--- | :--- | :--- |
| **Concurrency & Thread Safety** | Modifies process-global bitmask; corrupts concurrent requests | Pure configuration + worker-isolated context | **Zero race conditions**; worker threads never cross-contaminate error levels |
| **Code Reliability & Bug Detection** | Bitmasks mask undefined variables and type errors | Strict compile-time checks + explicit exceptions | Eliminates silent data corruption; bugs caught at compile time |
| **Performance Overhead** | Dynamic error level checks and `@` opcode handler overhead | Direct compiled execution; zero runtime bitmask queries | **Measurably faster execution**; JIT compiler can optimize paths freely |
| **Modern Observability** | Raw text logs or unformatted stdout dumps | Structured JSON logging + OpenTelemetry tracing | Production-ready logs with correlation IDs, traces, and span attributes |
| **Safe Error Suppression** | `@` operator indiscriminately swallows fatal errors | Idiomatic `??`, `?->`, and `Result` types | Explicit, localized, and safe handling of optional or fallible values |

---

## 10. Named Arguments (`func(param: $val)`)

**Not supported:** PHP 8.0+ named argument call syntax (`func(name: $val)`).

### What Was Bad About It in PHP

- **Brittle Coupling to Private Implementation Details:** Named arguments turn internal parameter variable names into part of the public API contract. Renaming a parameter during routine refactoring (e.g., from `$n` to `$name` or fixing a typo) silently breaks all external callers that passed named arguments, even though the parameter type, position, and semantics did not change.
- **Inheritance and Interface Fragility:** In PHP, if an interface defines `function save(array $data)` and an implementing class names it `public function save(array $payload)`, calling `$obj->save(data: $arr)` fails at runtime with a fatal error if invoked on the concrete class instance rather than through an interface variable. This creates subtle, hard-to-detect runtime bugs across polymorphic codebases.
- **Runtime Argument Reconciliation Tax:** Supporting named arguments forces the VM to perform dynamic dictionary lookups and argument-to-parameter slot reconciliation on every function invocation. This prevents optimal register allocation, hinders Cranelift JIT devirtualization, and inflates call-frame initialization overhead.
- **Syntactic Bloat:** Call sites become excessively verbose and duplicate method documentation without providing type safety.

---

### What Replaces It in RHP

1. **Positional Arguments with Skipped Parameters (`func($a, , $c)`):**
   When calling functions or methods with multiple optional parameters, callers can skip intermediate optional arguments while preserving their default values simply by using extra commas:
   ```php
   function createToken(string $user, int $ttl = 3600, bool $refreshable = false, string $scope = 'read'): Token {
       // ...
   }

   // Skip $ttl (3600) and $refreshable (false), but customize $scope:
   $token = createToken('alice', , , 'admin');
   ```
2. **Strict Parameter Ordering Invariant:**
   All required parameters must strictly precede optional parameters. The RHP linter (`rhp run lint`) automatically reorders non-conforming signatures so required parameters always come first.
3. **Structured Options with `dict` or Configuration Objects:**
   For functions taking dozens of optional flags or configuration settings, pass a typed dictionary (`dict $options`) or a typed configuration struct (`UserOptions $options`), which provides compile-time property safety and autocomplete without fragile parameter-name coupling.

---

### Which Code to Change: Before & After

#### Example: Skipping Optional Parameters

**Before (PHP 8 Named Arguments):**
```php
// In PHP: Couples call sites to parameter names '$apiKey' and '$debug'
$client = new ApiClient(
    apiKey: 'secret_key',
    debug: true
);
```

**After (Modern RHP Positional & Skipped Parameters):**
```php
// In RHP: Method defined with required parameter before optional parameters:
// public function __construct(string $apiKey, int $timeout = 30, bool $debug = false)

// Call site skips $timeout (uses default 30) with an extra comma:
$client = new ApiClient('secret_key', , true);
```

---

## 11. Code Export & State Restorers (`var_export()`, `__set_state()`)

**Not supported:** `var_export()`, `Debug::export()`, `__set_state()`, and writing executable PHP source code files to disk for application caching.

### What Was Bad About It in PHP

- **Name Misleads Intent ("Export as PHP Code"):** The function name `var_export` literally means "export as PHP source code". In PHP, this was primarily used as a workaround to cache configuration arrays or container manifests to disk (e.g. `bootstrap/cache/config.php`) so opcode caches (OPcache) could parse and execute them. RHP is a compiled native binary language &mdash; a compiled binary cannot and should not `include` generated source files at runtime.
- **The `__set_state` Boilerplate Tax:** In PHP, exporting an object via `var_export` produces `\ClassName::__set_state([...])`. If a class author forgets to manually implement this static hook, evaluating the exported code fails with a fatal error.
- **Silent Failures and Mangled Output:** `var_export` fails silently on unsupported values. Closures output invalid syntax (`\Closure::__set_state(...)` which throws fatal errors), stream and database resources emit `NULL`, and circular object references produce unresolvable recursive structures or runtime crashes.
- **Conflated Responsibilities:** PHP historically conflated three distinct programming jobs under `var_dump`, `print_r`, and `var_export`:
  1. Inspecting a value for human eyes.
  2. Formatting that inspection string into a variable.
  3. Storing and rehydrating structured data across processes.

---

### What Replaces It in RHP: The Three-Job Architecture

RHP completely separates these three distinct jobs into purpose-built, zero-overhead primitives:

| Job | Modern RHP API | Output & Purpose | Under the Hood |
| :--- | :--- | :--- | :--- |
| **1. Show a value to a human** | `Debug::dump($x)`<br>`Debug::dd($x)` | Pretty prints to stdout/stderr (`dd` halts execution) | Rust `std::fmt::Debug` machinery |
| **2. Get inspection text as a string** | `Debug::format($x)` | Formatted diagnostic string (the `var_export($x, true)` / `print_r($x, true)` use case) | Rust `format!("{:?}", ...)` engine |
| **3. Save and reload structured data** | `Json::encode($x)` / `Json::decode($str)`<br>`Bin::encode($x)` / `Bin::decode($bytes)` | Structured JSON text for human-readable files, or compact binary for maximum cache performance | Rust `serde`, `serde_json`, and `bincode` / `postcard` |

> [!NOTE]
> **What About Code Generation?**
> If an application or compiler tooling genuinely requires generating RHP source code literals from in-memory data (e.g., writing static lookup tables), RHP reserves `Code::literal($x)` for future introduction. Laravel internals and application caches have zero need for `Code::literal` once configuration and route manifests are persisted via `Bin::encode`.

---

### Why RHP Is Better Than PHP

1. **Mandatory Types Eliminate `__set_state` Boilerplate:**
   Because property types are strictly mandatory in RHP, the compiler automatically generates debug printing and serialization routines for every class (analogous to Rust's `#[derive(Debug, Serialize, Deserialize)]`). There is zero manual `__set_state()` hook to write or maintain.
2. **Compiled Binaries Don't Need Loadable Source Code:**
   In PHP, caching configuration as `.php` code was necessary because OPcache cached ASTs. In RHP, compiled binaries cannot and should not invoke a PHP parser at runtime. Caching manifests as compact binary (`Bin::encode`) or JSON loads via memory-mapped deserialization at least $10\times$ faster than tokenizing and executing PHP source files.
3. **Clear, Fail-Fast Boundaries for Unsupported Values:**
   - On serialization (`Json::encode`, `Bin::encode`), unsupported types (closures, active database handles, circular references) fail fast with a compile-time or explicit runtime `SerializationException` rather than silently corrupting the cache.
   - On human inspection (`Debug::dump`, `Debug::format`), diagnostic tokens safely depict these constructs (e.g., `<closure: fn(int): string>`, `<resource #fd:3>`, or `<recursion: *1>`) without crashing.
4. **Zero-Cost Engine Integration via Rust `serde`:**
   RHP leverages Rust's industry-standard `serde` framework. JSON formatting and compact binary packing incur virtually zero engine overhead, producing compact, portable, and verifiable payloads.

---

### Which Code to Change: Before & After

#### Example A: Formatted String Inspection (Replacing `var_export($val, true)`)

**Before (Legacy PHP):**
```php
// In PHP: var_export with $return = true was used to format values as string
$data = ['status' => 'pending', 'retry' => 3];
$logText = var_export($data, true);
file_put_contents('/tmp/log.txt', $logText);
```

**After (Modern RHP):**
```php
// In RHP: Explicit Debug::format() produces clean inspection string
dict $data = ['status' => 'pending', 'retry' => 3];
string $logText = Debug::format($data);
File::write('/tmp/log.txt', $logText);
```

#### Example B: Configuration & Manifest Caching (Replacing Generated PHP Arrays)

**Before (Legacy PHP - Laravel Config Cache):**
```php
// In PHP: Writes executable PHP script containing array return
$config = ['app' => ['name' => 'Rocket', 'env' => 'production']];
$phpCode = '<?php return ' . var_export($config, true) . ';' . PHP_EOL;
file_put_contents('bootstrap/cache/config.php', $phpCode);

// Loading:
$loaded = require 'bootstrap/cache/config.php';
```

**After (Modern RHP):**
```php
// In RHP: Blazing-fast binary or readable JSON cache
dict $config = ['app' => ['name' => 'Rocket', 'env' => 'production']];

// Fast binary cache for production boot:
bytes $cache = Bin::encode($config);
File::write('bootstrap/cache/config.bin', $cache);

// Loading (instant zero-copy memory mapping):
dict $loaded = Bin::decode(File::read('bootstrap/cache/config.bin'));

// Or readable JSON:
File::write('bootstrap/cache/config.json', Json::encode($config, pretty: true));
dict $loadedJson = Json::decode(File::read('bootstrap/cache/config.json'));
```

---

## 12. Dynamic File Inclusion & Runtime Autoloading (`include`, `require`, `include_once`, `require_once`, `spl_autoload_register`)

**Not supported:** `include`, `require`, `include_once`, `require_once`, `include $path`, `require $file`, `spl_autoload_register()`, `spl_autoload_unregister()`, `__autoload()`, and runtime evaluation of source code files.

### What Was Bad About It in PHP

- **Type Checking Breakdown:** `require $path` pulls in code the compiler cannot inspect during compilation. When a path is dynamically resolved from a variable or expression, the compiler cannot know which classes, interfaces, traits, functions, or types will exist after execution. That undermines mandatory static typing, symbol resolution, and whole-program compile-time verification.
- **Incompatible with Single Compiled Binary Distribution (`rhp run build`):** A single compiled native binary must know the complete program up front to build the closed-world dependency graph, devirtualize method tables, perform dead-code elimination (tree-shaking), and link machine code. Runtime-loaded source files force an engine to bundle an entire parser, AST builder, compiler, or VM interpreter and dynamic file loader inside every production binary.
- **Scope Injection & Untyped Semantics:** In PHP, `include` executes code directly in the caller's variable scope, leaking variables (`extract($data)`) or overwriting local state. Furthermore, an include statement evaluates to an untyped return value (or arbitrary scalar). Clean lexical scoping and deterministic type safety cannot be maintained in a statically typed language when external scripts can dynamically pollute stack frames.
- **Runtime I/O & Concurrency Bottlenecks:** PHP's `spl_autoload_register()` defers class resolution until an uninstantiated symbol is first referenced at runtime. Every class access triggers filesystem stat calls (`file_exists`, `is_file`) and dynamic `require_once` loops during request handling, penalizing latency and creating concurrency contention in persistent workers.

---

### What Replaces It in RHP

1. **Static Imports Only (`use` / `import`):**
   Imports are strictly declarative and resolved at compile time using literal paths or fully-qualified namespace symbols:
   ```php
   use App\Services\OrderService;
   use App\Models\User;
   import "./Config.rhp";
   import "./Cache.rs" as NativeCache;
   ```
   Computed expressions (e.g., `require $path;`, `import $mod;`, `include $view . '.rhp';`) trigger fatal compile-time syntax errors (`RHP0120`).
2. **Build-Time Class Map & Implicit Prelude:**
   Package and dependency autoloading is moved entirely to build/install time. `vendor/autoload.rhp` is generated ahead of time by `rhp add` / `rhp install` / `rhpm` as a static class map and package manifest, not executed dynamically at startup. The compiler reads the PSR-4 namespace-to-directory mapping declared in `rhp.toml` / `composer.json`, resolves all imports, and builds the complete module dependency graph. The autoloader functions as an implicit prelude prepended to the compilation root. There is **no `spl_autoload_register()`**, zero runtime filesystem probing, and zero lazy class loading in production.
3. **Deterministic Go/Rust-Style Module Initialization Semantics:**
   Legacy PHP relied on `require_once` and `include_once` to guard against duplicate symbol declarations. In RHP, module execution follows Go/Rust semantics: each module initializes **exactly once**, in topological dependency order, inside the parent process during boot before child workers are forked. Top-level statements in an imported file execute once during master initialization; worker processes inherit this initialized immutable state via Copy-on-Write (CoW).
4. **Incremental Dev Mode Hot Reloading:**
   In development (`rhp run dev`), the server hot-reloads changed files incrementally without sacrificing static type safety. When a file changes, the file-watcher recompiles only the modified module and incrementally re-checks it against the pre-computed static dependency graph. This provides sub-millisecond dev turnaround while preserving strict compile-time type verification.

---

### Which Code to Change: Before & After

#### Example A: Dynamic Configuration Loading

**Before (Legacy PHP with Dynamic `require`):**
```php
// In PHP: Runtime file loading with dynamic string path
function loadConfig(string $environment): array {
    $path = __DIR__ . '/config/' . $environment . '.php';
    if (!file_exists($path)) {
        throw new InvalidArgumentException("Missing config: {$path}");
    }
    return require $path;
}
```

**After (Modern RHP with Typed Caching / Static Imports):**
```php
// In RHP: Static structured binary/JSON loading with explicit typing
function loadConfig(string $environment): dict {
    string $path = "config/{$environment}.bin";
    if (!File::exists($path)) {
        throw new InvalidArgumentException("Missing config: {$path}");
    }
    return Bin::decode(File::read($path));
}
```

#### Example B: Template Rendering & Scope Injection

**Before (Legacy PHP with Scope-Injecting `include`):**
```php
// In PHP: Procedural include executing in caller scope with output buffering
function renderView(string $viewPath, array $data): string {
    extract($data);
    ob_start();
    include $viewPath;
    return ob_get_clean();
}
```

**After (Modern RHP with Typed Views & Scoped Captures):**
```php
// In RHP: Scoped capture or dedicated typed template function
use App\Views\UserDashboardView;

function renderView(User $user, list $orders): string {
    return UserDashboardView::render($user, $orders);
}
```

#### Example C: Package & Class Autoloading

**Before (Legacy PHP with Runtime Autoloader):**
```php
// In PHP: Runtime autoloader registration and dynamic require_once
require_once __DIR__ . '/vendor/autoload.php';

$client = new GuzzleHttp\Client();
```

**After (Modern RHP with Build-Time Module Resolution):**
```php
// In RHP: Zero runtime require_once; symbols resolved at compile time
use GuzzleHttp\Client;

$client = new Client();
```

---

## Comprehensive Feature Matrix

| Legacy PHP Feature | Status in RHP | Primary Architectural Reason | Recommended RHP Replacement | Key Win Over PHP |
| :--- | :--- | :--- | :--- | :--- |
| **Reflection API** (`Reflection*`) | ❌ **Rejected** | Runtime overhead for compile-time tasks; prevents JIT inlining and dead-code stripping | Build-time DI codegen, static `ProgramMetadata`, `#[Route]` attributes | $10\times$ to $50\times$ faster controller dispatch; errors caught during `rhp run build` |
| **Output Buffering** (`ob_*`) | ❌ **Rejected** | Process-global state leaks across persistent workers; unsafe in async fibers | `Output::capture(fn() => ...)`, direct `Writer` templates, `Response::stream` | Exception-safe scoped captures; impossible to corrupt subsequent requests |
| **cURL Extension** (`curl_*`) | ❌ **Rejected** | Untypable integer constant API; missing safe timeouts; redundant wrapper layer | Fluent `Http` facade, async `AHTTP`, Rust `.rs` interop for exotic protocols | Strictly-typed builder; secure defaults; native Tokio asynchronous concurrency |
| **Legacy Serialization** (`serialize()`, `unserialize()`, `__sleep()`, `__wakeup()`) | ❌ **Rejected** | Pervasive RCE gadget chains; open-by-default class loading; slow text format with null-byte mangling | Compact binary format, `__serialize()`/`__unserialize(dict)`, `#[Transient]`, deny-by-default `allowed_classes` | Immune to gadget chains; $3\times$ to $8\times$ smaller payloads; destructor suppressed on failure |
| **Strict Operators (`===`, `!==`) & Loose `==`** | ❌ **Rejected** | Implicit type coercion causes security vulnerabilities; redundant `===` syntax | Strict `==`/`!=`, compile-time mismatch errors, `Bool/Str/Int/Float/Array::compare*` | Eradicates loose comparison exploits; `rhp lint` auto-converts `===` to `==` |
| **Implicit Arithmetic Coercion & Silent `+` Overflow** | ❌ **Rejected** | Strings coerced to int (`"5 apples" + 3`), lossy float promotion on overflow, array union | Checked `+` (traps), wrapping `+%`, saturating `+\|`, explicit casts, `.` for concat, `[...$a, ...$b]` for merge | Immune to silent numeric overflow exploits, no data loss from array union |
| **Directive Statements** (`declare(strict_types=1);`, `declare(ticks=1);`) | ❌ **Rejected** | Redundant per-file boilerplate; caller-dependent type semantics; ticks cripple JIT optimization | Universal compiler-enforced strict typing; Tokio fiber runtime & OpenTelemetry | Zero per-file boilerplate; strict typing enforced across entire project; uncompromised JIT compilation |
| **Classic `list()` Destructuring** (`list($a, $b) = $array`) | ❌ **Rejected** | Lexical keyword collision with `list` collection type; archaic pseudo-function syntax | Square-bracket destructuring `[$a, $b] = $array;`, dedicated `list` collection type | Clean grammar; `list` keyword dedicated to high-performance sequential collections; unified syntax |
| **`die()` Language Construct** (`die()`) | ❌ **Rejected** | Abrupt process termination crashes persistent workers; bypasses exception unwinding; emits raw text with `200 OK` | Typed exceptions (`throw new ...`), `abort(status, msg)`, `Response::json(...)`, `Sys::exit($code)` | Zero worker crashes; structured HTTP error payloads; guaranteed `finally` and `defer` resource cleanup |
| **Error Reporting Configuration** (`error_reporting()`, `set_error_handler()`, `@`) | ❌ **Rejected / Deprecated** | Process-global mutable error bitmask causes race conditions in concurrent workers; masks silent bugs | Compile-time checks, `rhp.toml` logging, structured `Log` facade, `try-catch` | Thread-safe; zero silent production bugs; native OpenTelemetry tracing integration |
| **Named Arguments** (`func(param: $val)`) | ❌ **Rejected** | Couples call sites to internal variable names; breaks refactoring and interface variance; runtime dictionary binding tax | Positional arguments with skipped optional arguments via extra comma (`func($a, , $c)`), typed `dict $options` | High-performance positional register calling; safe parameter renaming; clean skipped-argument syntax |
| **Code Export & Caching** (`var_export()`, `__set_state()`) | ❌ **Rejected** | Generating executable source code for caching is an anti-pattern in compiled binaries; silent failure on closures/cycles | `Debug::dump/dd/format` for inspection; `Json`/`Bin::encode/decode` for caching | Zero `__set_state` boilerplate; $10\times$ faster binary cache loading; fail-fast serialization |
| **Dynamic Include & Runtime Autoloading** (`include`, `require`, `*_once`, `spl_autoload_register`) | ❌ **Rejected** | Runtime file loading undermines compile-time types; prevents AOT single-binary distribution (`rhp run build`); leaks scope | Static `import`/`use` with literal paths/namespaces, build-time PSR-4 module graph, Go/Rust module init | Predictable whole-program typing; self-contained AOT native binaries; zero runtime autoloading I/O |

---

## Migration Tooling: Using `rhp migrate`

When porting an existing PHP codebase to RHP, run the automated migration command:

```bash
rhp migrate ./app --write
```

### What `rhp migrate` Automatically Rewrites

- **Strict Comparison Operators:** Automatically converts `===` to `==` and `!==` to `!=`.
- **Output Buffering:** Simple `ob_start(); ... $html = ob_get_clean();` blocks are automatically transformed into `Output::capture(function() { ... })`.
- **cURL Calls:** Standard `curl_init()` and `curl_setopt()` chains configured with common options (`CURLOPT_RETURNTRANSFER`, `CURLOPT_HTTPHEADER`, `CURLOPT_POSTFIELDS`, `CURLOPT_TIMEOUT`, `CURLOPT_USERPWD`) are rewritten into expressive `Http::` facade chains.
- **Dependency Injection:** Standard constructor injection and container bindings are formatted to leverage RHP's build-time DI generator.
- **Serialization Hooks:** Legacy `__sleep()` returning property arrays is rewritten to automatic serialization with `#[Transient]` attributes placed on unlisted properties; simple `__wakeup()` logic is moved into `__unserialize(dict $data)`.
- **Directive Statements:** Strips redundant `declare(strict_types=1);`, `declare(ticks=1);`, and `declare(encoding=...);` statements automatically.
- **Classic `list()` Destructuring:** Automatically converts `list($a, $b) = $expr` to modern square-bracket destructuring `[$a, $b] = $expr`.
- **`die()` Calls:** Simple `die("message")` or `die()` statements in web requests are converted to `abort(500, "message")` or `throw new RuntimeException("message")`, and in CLI scripts to `Sys::exit(1)`.
- **Error Reporting Statements:** Strips legacy `error_reporting(...)` and `ini_set('display_errors', ...)` calls; rewrites `@` error suppression on dictionary/array lookups and property accesses to modern null-coalescing `??` or null-safe `?->` operators.
- **Inspection & Code Export:** Automatically rewrites `var_dump(...)` and `dump(...)` to `Debug::dump(...)`, `dd(...)` to `Debug::dd(...)`, and `var_export($x, true)` or `print_r($x, true)` to `Debug::format($x)`.

### What `rhp migrate` Flags for Manual Review

- **Unbalanced Buffers:** Dynamic or unbalanced `ob_start()` calls without matching `ob_get_clean()` in the same lexical scope.
- **Complex `curl_multi` Loops:** Custom non-blocking event loops built with `curl_multi_exec()` and `curl_multi_select()`. These should be refactored into `Http::pool()` or `Future::wait([AHTTP::get(...)])`.
- **Private Member Reflection:** Instances of `ReflectionProperty::setAccessible(true)` or `ReflectionMethod::invokeArgs()`. These should be refactored to explicit interfaces, public testing helpers, or `App::swap()`.
- **Insecure `unserialize()` Calls:** Any call to `unserialize()` lacking an explicit `allowed_classes` parameter is flagged as a high-severity security risk and must specify allowed classes or be migrated to typed JSON / binary serialization.
- **Closure Serialization:** Usages of `Opis\Closure` or `Laravel\SerializableClosure` are flagged to be refactored into named job classes or named callables with typed parameters.
- **Legacy PHP Format Data:** Calls consuming serialized PHP string data (`O:4:...`) are flagged to use the migration reader `unserialize_php($payload)` during transition or to migrate cache stores to the versioned RHP binary format.
- **Tick Callbacks:** Any usage of `register_tick_function()` combined with `declare(ticks=1)` is flagged to be migrated to Tokio asynchronous event handlers or OpenTelemetry tracing spans.
- **Complex Control-Flow `die()` Statements:** Usages of `... or die(...)` guards or expressions where `die()` return values are conditionally evaluated are flagged for refactoring into typed exceptions or explicit `abort()` handlers.
- **Custom Error Handlers (`set_error_handler`):** Flags custom procedural `set_error_handler()` and `restore_error_handler()` calls to be refactored into centralized application error middleware or `App::onError()`.
- **File-Based Code Generation / Config Caching with `var_export`:** Code patterns that write `var_export` output to `.php` files for dynamic `require`/`include` are flagged to be migrated to `Bin::encode()` / `Bin::decode()` or `Json::encode()` / `Json::decode()`.
- **Dynamic File Inclusions (`require`, `include`, `require_once`, `include_once`):** Flagged as fatal errors (`RHP0120`) to be migrated to static `import` or `use` statements with literal paths or namespaces, with runtime package loading converted to build-time PSR-4 manifests.


