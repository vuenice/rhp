---
title: "Unsupported PHP"
description: "Exhaustive guide to PHP features deliberately omitted from RHP, their modern replacements, step-by-step code migration examples, and architectural advantages over PHP."
section: "Reference"
order: 5
---

# Unsupported PHP Features

This document details legacy PHP features that RHP deliberately does **not** support, why each was rejected, what modern replacements exist in RHP, which code to change during migration, and why the RHP architecture is fundamentally superior to traditional PHP.

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
        $payload = Arr::dict(['tick' => $i])->toJson();
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

## Comprehensive Feature Matrix

| Legacy PHP Feature | Status in RHP | Primary Architectural Reason | Recommended RHP Replacement | Key Win Over PHP |
| :--- | :--- | :--- | :--- | :--- |
| **Reflection API** (`Reflection*`) | ❌ **Rejected** | Runtime overhead for compile-time tasks; prevents JIT inlining and dead-code stripping | Build-time DI codegen, static `ProgramMetadata`, `#[Route]` attributes | $10\times$ to $50\times$ faster controller dispatch; errors caught during `rhp run build` |
| **Output Buffering** (`ob_*`) | ❌ **Rejected** | Process-global state leaks across persistent workers; unsafe in async fibers | `Output::capture(fn() => ...)`, direct `Writer` templates, `Response::stream` | Exception-safe scoped captures; impossible to corrupt subsequent requests |
| **cURL Extension** (`curl_*`) | ❌ **Rejected** | Untypable integer constant API; missing safe timeouts; redundant wrapper layer | Fluent `Http` facade, async `AHTTP`, Rust `.rs` interop for exotic protocols | Strictly-typed builder; secure defaults; native Tokio asynchronous concurrency |
| **Legacy Serialization** (`serialize()`, `unserialize()`, `__sleep()`, `__wakeup()`) | ❌ **Rejected** | Pervasive RCE gadget chains; open-by-default class loading; slow text format with null-byte mangling | Compact binary format, `__serialize()`/`__unserialize(dict)`, `#[Transient]`, deny-by-default `allowed_classes` | Immune to gadget chains; $3\times$ to $8\times$ smaller payloads; destructor suppressed on failure |

---

## Migration Tooling: Using `rhp migrate`

When porting an existing PHP codebase to RHP, run the automated migration command:

```bash
rhp migrate ./app --write
```

### What `rhp migrate` Automatically Rewrites

- **Output Buffering:** Simple `ob_start(); ... $html = ob_get_clean();` blocks are automatically transformed into `Output::capture(function() { ... })`.
- **cURL Calls:** Standard `curl_init()` and `curl_setopt()` chains configured with common options (`CURLOPT_RETURNTRANSFER`, `CURLOPT_HTTPHEADER`, `CURLOPT_POSTFIELDS`, `CURLOPT_TIMEOUT`, `CURLOPT_USERPWD`) are rewritten into expressive `Http::` facade chains.
- **Dependency Injection:** Standard constructor injection and container bindings are formatted to leverage RHP's build-time DI generator.
- **Serialization Hooks:** Legacy `__sleep()` returning property arrays is rewritten to automatic serialization with `#[Transient]` attributes placed on unlisted properties; simple `__wakeup()` logic is moved into `__unserialize(dict $data)`.

### What `rhp migrate` Flags for Manual Review

- **Unbalanced Buffers:** Dynamic or unbalanced `ob_start()` calls without matching `ob_get_clean()` in the same lexical scope.
- **Complex `curl_multi` Loops:** Custom non-blocking event loops built with `curl_multi_exec()` and `curl_multi_select()`. These should be refactored into `Http::pool()` or `Future::wait([AHTTP::get(...)])`.
- **Private Member Reflection:** Instances of `ReflectionProperty::setAccessible(true)` or `ReflectionMethod::invokeArgs()`. These should be refactored to explicit interfaces, public testing helpers, or `App::swap()`.
- **Insecure `unserialize()` Calls:** Any call to `unserialize()` lacking an explicit `allowed_classes` parameter is flagged as a high-severity security risk and must specify allowed classes or be migrated to typed JSON / binary serialization.
- **Closure Serialization:** Usages of `Opis\Closure` or `Laravel\SerializableClosure` are flagged to be refactored into named job classes or named callables with typed parameters.
- **Legacy PHP Format Data:** Calls consuming serialized PHP string data (`O:4:...`) are flagged to use the migration reader `unserialize_php($payload)` during transition or to migrate cache stores to the versioned RHP binary format.
