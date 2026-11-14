---
title: "Errors, Warnings, Notices & Exceptions"
description: "Comprehensive guide to compiler diagnostics, runtime errors, warnings, notices, structured try/catch, Go-style safe @ evaluation, and OpenTelemetry error tracking in RHP."
section: "Language Guide"
order: 6
---

In traditional PHP, error handling suffered from a historically fragmented model: dynamic error reporting bitmasks (`error_reporting(E_ALL & ~E_NOTICE)`), silent bug masking via global `@` suppression, procedural warnings/notices that polluted output, and separately introduced object-oriented exceptions.

RHP establishes a modern, strictly stratified error model divided into distinct phases:

1. **Compile-time vs. Runtime**: Errors caught before execution begins versus failures occurring during script execution.
2. **Fatal vs. Recoverable**: Unrecoverable engine invariant violations versus recoverable domain and I/O failures.
3. **Structured vs. Flat Evaluation**: Classical `try / catch / finally` blocks alongside **Go-style safe `@` tuple evaluation** (`[$result, $err] = @fn();`).
4. **First-Class Observability**: Automatic recording of errors, warnings, notices, and exceptions into distributed OpenTelemetry spans and APM backends.

---

## 1. Error Taxonomy in RHP

| Category | Phase | Halts Execution? | Catchable in Code? | Captured by Telemetry? | Handling Mechanism |
| :--- | :--- | :--- | :--- | :--- | :--- |
| **Compiler Error** | Compile-Time | **Yes** (Build Fails) | No (Pre-execution) | CI / Build Logs | Fix code syntax, type mismatches, or invalid keywords |
| **Compiler Warning** | Compile-Time | **No** (Build Succeeds) | No (Diagnostic) | CI / Linter Logs | Address deprecations or casing suggestions |
| **Runtime Error (Panic)** | Runtime | **Yes** (Request Aborted) | No (Engine Fault) | **Yes** (Span Status: `ERROR`) | Worker process isolates panic; arena resets in $O(1)$ |
| **Exception (`Throwable`)** | Runtime | **Yes** (If Uncaught) | **Yes** (`try/catch` or `@`) | **Yes** (Span Exception Event) | Handle with `try / catch` or Go-style `[$val, $err] = @expr` |
| **Runtime Warning / Notice** | Runtime | **No** (Continues) | No (Non-halting) | **Yes** (Span Log Event) | Structured `Log::warning()`, `Log::notice()`, `??`, `?->` |

---

## 2. Compiler Errors & Warnings

### Compiler Errors (Fatal Diagnostics)

Compiler errors occur during lexical analysis, parsing, or static type checking (`rhp check`, `rhp run build`, `rhp run dev`). When a compiler error occurs, **no bytecode is generated and execution never begins**.

Common causes include:
- **Syntax violations:** Unclosed strings, mismatched braces, unsupported PHP operators (`===`, `!==`).
- **Strict type contract violations:** Passing a `string` to a parameter declared `int`, or returning incompatible types.
- **Architectural violations:** Instantiating static facade classes (`new Str()`), or circular dependency cycles.

```text
[RHP Compile Error] RHP0116: Unsupported equality operator '==='
  --> src/AuthService.rhp:28:17
   |
28 |     if ($role === 'admin') {
   |               ^^^ Strict comparison '===' is rejected. In RHP, all comparisons are strict.
   |
   = help: Replace '===' with '==' (rhp explain RHP0116)
```

### Compiler Warnings (Non-Fatal Diagnostics)

Compiler warnings inform developers about potential issues that do not prevent bytecode generation or execution:
- Deprecated legacy functions or methods.
- Naming convention warnings (e.g. non-camelCase local variables, when enabled).
- Unused local variables or redundant imports.

### Interactive Diagnostic Catalog (`rhp explain`)

RHP assigns a stable, permanent diagnostic code (`RHP0001` through `RHP9999`) to every compiler error and warning. Rather than maintaining an unwieldy static list in documentation, RHP provides a built-in, searchable CLI diagnostic engine:

```bash
# List all registered compiler diagnostic codes
rhp explain --list

# Get an in-depth explanation and runnable code fix for a specific code
rhp explain RHP0102
```

For complete CLI usage, see the [CLI Reference](/docs/getting-started/cli-reference).

---

## 3. Runtime Errors, Warnings, & Notices

### Runtime Errors (Fatal Engine Invariants & Panics)

A runtime error represents an unrecoverable engine violation or environmental catastrophe:
- Unhandled arithmetic faults (e.g., division by zero outside a catch/suppression block).
- Native memory exhaustion or fiber stack overflow.
- Corrupted bytecode chunks or native FFI boundary crashes.

#### Worker Process Isolation

In production server mode (`rhp run build`), runtime errors trigger an isolated worker panic. The parent supervisor process instantly catches the failure, records the error with active distributed trace metadata, returns an HTTP `500 Internal Server Error` to the client, and resets the worker's bump-allocated request arena in $O(1)$ time. Sibling requests, concurrent fibers, and the master process remain completely unaffected.

### Runtime Warnings & Notices

Traditional PHP emitted runtime warnings and notices for undefined array keys (`$arr['missing']`), uninitialized variables, and implicit type coercions. These dynamic notices often silently corrupted data or filled production log disks with noise.

In RHP:
1. **No Silent Notices:** Undefined array indexing is checked at compile-time or handled safely with the null-coalescing operator (`$arr['key'] ?? $default`).
2. **Explicit Null Safety:** Optional properties use nullsafe navigation (`$user?->profile?->avatar`).
3. **Structured Operational Logging:** When your code needs to emit non-halting diagnostic information, use the high-performance `Log` facade rather than procedural `trigger_error()`:

```php
// Structured operational warning with contextual metadata
Log::warning("Payment gateway latency elevated", [
    'gateway' => 'stripe',
    'latency_ms' => 1250,
]);

// Operational notice
Log::notice("User triggered secondary 2FA verification", [
    'user_id' => $user->id,
]);
```

---

## 4. Recoverable Exceptions & `try / catch / finally`

For expected, recoverable runtime failures (database disconnects, missing files, validation failures), RHP provides full object-oriented exception handling under the standard `Throwable` hierarchy.

### The Throwable Hierarchy

```text
Throwable (root interface)
├── Exception (recoverable application exceptions)
│   ├── RuntimeException
│   ├── InvalidArgumentException
│   └── ... (custom domain exceptions)
└── Error (recoverable engine errors)
    ├── TypeError
    ├── ArithmeticError (e.g. DivisionByZeroError)
    └── ValueError
```

### Classic `try / catch / finally` Example

```php
class InsufficientFundsException extends Exception {}

function transferFunds(Account $from, Account $to, float $amount): void {
    if ($from->balance < $amount) {
        throw new InsufficientFundsException("Account {$from->id} has insufficient balance.");
    }
    $from->balance -= $amount;
    $to->balance += $amount;
}

try {
    transferFunds($accountA, $accountB, 500.00);
} catch (InsufficientFundsException $e) {
    Log::warning("Transfer declined: " + $e->getMessage());
} catch (Throwable $e) {
    Log::error("Unexpected fatal transfer error: " + $e->getMessage());
} finally {
    // Guarantees execution regardless of whether an exception was thrown
    DB::closePendingTransactions();
}
```

---

## 5. The `@` Operator: Go-Style Safe Evaluation

While `try / catch` works well for coarse-grained boundary blocks, wrapping multiple consecutive operations in nested `try / catch` blocks creates deeply indented "pyramids of doom" that obscure business logic.

RHP introduces a **compiler-friendly, zero-keyword, Go-style error handling model** by repurposing the unary `@` operator.

### Syntax & Tuple Return Format

Instead of dynamically modifying global engine error masks or silently swallowing defects, prepending `@` to an expression evaluates it safely and produces a structured 2-element tuple:

$$\text{@expr} \longrightarrow (T \mid \text{null},\; \text{Throwable} \mid \text{null})$$

```php
[$result, $err] = @helperFun();
```

### The Go vs. RHP Workflow

| Concept | Go Pattern | RHP Equivalent |
| :--- | :--- | :--- |
| **Evaluation** | `val, err := helperFun()` | `[$val, $err] = @helperFun();` |
| **Error Verification** | `if err != nil { ... }` | `if ($err != null) { ... }` |
| **Ignore Error Slot** | `val, _ := helperFun()` | `[$val] = @helperFun();` |
| **Void Operation** | `_, err := saveRecords()` | `[$val, $err] = @saveRecords();` (returns `[null, $err]`) |
| **Direct Tuple** | `tuple := helperFun()` | `$tuple = @helperFun();` (inspect `$tuple[0]`, `$tuple[1]`) |

### Practical Example: Flat, Linear Control Flow

Compare classical nested `try / catch` against flat Go-style evaluation:

#### Before (Classical Nested `try / catch`)
```php
try {
    $user = fetchUser($id);
    try {
        $orders = fetchOrders($user);
        try {
            $report = generateReport($orders);
        } catch (ReportException $e) {
            return Response::error("Report failed: " + $e->getMessage());
        }
    } catch (OrderException $e) {
        return Response::error("Orders failed: " + $e->getMessage());
    }
} catch (UserException $e) {
    return Response::error("User failed: " + $e->getMessage());
}
```

#### After (Modern RHP Flat Flow)
```php
[$user, $err] = @fetchUser($id);
if ($err != null) {
    return Response::error("User failed: " + $err->getMessage());
}

[$orders, $err] = @fetchOrders($user);
if ($err != null) {
    return Response::error("Orders failed: " + $err->getMessage());
}

[$report, $err] = @generateReport($orders);
if ($err != null) {
    return Response::error("Report failed: " + $err->getMessage());
}

return Response::json($report);
```

### Key Semantics of `@`

1. **Failure Slot (`$err`)**:
   - On exception: Populated with the thrown `Throwable` or `Exception` instance.
   - On success: Evaluates to `null`.
2. **Success Slot (`$result`)**:
   - On success: Contains the expression's return value.
   - On failure: Evaluates to `null`.
3. **Functions Returning `void`**:
   - If a `void` function succeeds without throwing, slot 0 returns `null` (type `null`), and slot 1 returns `null`.
4. **Single-Slot Destructuring**:
   - You are never forced to check the second slot if you only care about the result:
     ```php
     [$ok] = @unlink("cache.tmp");
     ```
5. **No Interference with Non-Exception Returns**:
   - If a function returns `false` or `null` without throwing an exception (e.g. `unlink()` returning `false`), `$err` remains `null`. Checking for boolean failure remains the caller's explicit responsibility.
6. **Captures Only Thrown Exceptions**:
   - `@` intercepts thrown `Throwable` instances. Compile-time type errors, syntax issues, and operational notices are not captured in slot 1.
7. **Zero Callee Rewriting**:
   - Functions do not need to be refactored to declare tuple returns or `Result` types. Callee code can use standard `throw new Exception()`, and the caller decides whether to catch via `try/catch` or safely evaluate via `@`.

---

## 6. Capturing Errors via OpenTelemetry

RHP includes an embedded, native OpenTelemetry engine that captures and correlates errors, warnings, notices, and exceptions automatically across distributed microservices.

### Automatic Exception Recording on Spans

When an exception is thrown inside an instrumented span (`Trace::span`), RHP automatically:
1. Marks the span status as `ERROR`.
2. Adds an OpenTelemetry `exception` event containing the exact exception class, message, and symbolic backtrace.
3. Propagates the active W3C `traceparent` context so downstream APM systems (Datadog, Sentry, Honeycomb, Jaeger) link the crash to the incoming HTTP request.

```php
$result = Trace::span('order.process', function() {
    [$order, $err] = @chargeCustomer();
    if ($err != null) {
        // Log attaches directly to active span trace context
        Log::error("Failed to charge customer", [
            'error' => $err->getMessage(),
            'trace_id' => Trace::traceId(),
        ]);
        throw $err;
    }
    return $order;
});
```

### Exporting to APM Backends

- **OTLP JSON Exporter:** `Trace::exportOtlp()` streams all error spans and exception events directly to an OpenTelemetry Collector or compatible APM.
- **Sentry Bridge:** `Trace::exportSentry($e)` exports Sentry-compliant event payloads with full stack traces.
- **Datadog Bridge:** `Trace::exportDatadog()` exports Datadog trace spans with error flags.

For full architectural details, span lifecycles, and metric exporters, read the dedicated **[Observability & OpenTelemetry Guide](/docs/guide/observability-opentelemetry)**.
