---
title: "Standard Library API"
description: "Exhaustive method signatures and specifications for core Tier 1 native facades."
section: "Reference"
order: 4
---

## `Arr` Facade API

```php
// Instantiation
Arr::list(array $items = []): list;
Arr::dict(array $pairs = []): dict;
Arr::set(array $values = []): set;
Arr::hashMap(): hashmap;

// Extension
Arr::macro(string $name, callable $closure): void;

// List Instance Methods
$list->map(callable $fn): list;
$list->filter(callable $fn): list;
$list->reduce(callable $fn, mixed $initial = null): mixed;
$list->find(callable $fn): mixed;
$list->count(): int;
$list->push(mixed ...$values): void;
$list->pop(): mixed;

// Dict Instance Methods
$dict->get(string $key, mixed $default = null): mixed;
$dict->put(string $key, mixed $value): void;
$dict->has(string $key): bool;
$dict->keys(): list;
$dict->values(): list;
$dict->diff(dict $other): dict;
```

---

## `Str` Facade & String Instance API

```php
// Static Facade Methods & Constructors
Str::slug(string $text, string $separator = '-'): string;
Str::contains(string $haystack, string $needle): bool;
Str::startsWith(string $haystack, string $prefix): bool;
Str::endsWith(string $haystack, string $suffix): bool;
Str::upper(string $text): string;
Str::lower(string $text): string;
Str::trim(string $text, string $chars = " \t\n\r\0\x0B"): string;
Str::ltrim(string $text, string $chars = " \t\n\r\0\x0B"): string;
Str::rtrim(string $text, string $chars = " \t\n\r\0\x0B"): string;
Str::substr(string $text, int $start, ?int $length = null): string;
Str::repeat(string $text, int $times): string;
Str::reverse(string $text): string;
Str::random(int $length = 16): string;
Str::join(string $glue, array $pieces): string;
Str::split(string $delimiter, string $string): list;
Str::replace(string $search, string $replace, string $subject): string;

// Multi-Byte Encodings & Binary Constructors
Str::fromEncoding(string|array $bytes, string $encoding): string; // Decodes Shift-JIS, EUC-JP, ISO-2022-JP, UTF-8
Str::toEncoding(string $text, string $encoding): string;           // Encodes to target encoding raw bytes
Str::utf8(string|array $input): string;                            // Ensures UTF-8 compliance from raw bytes
Str::isUtf8(string $text): bool;                                   // Checks if bytes are valid UTF-8
Str::fromBytes(array|list $bytes): string;                         // Constructs binary string from byte values [0..255]
Str::bytes(string $text): list;                                    // Returns list of byte integers [0..255]
Str::byteLength(string $text): int;                                // Returns byte length (binary length)
Str::encoding(string $text): string;                               // Detects probable encoding
Str::macro(string $name, callable $closure): void;                 // Registers dynamic macro bound to string instances

// Fluent Immutable Instance Methods ($text->trim()->upper())
$str->trim(?string $chars = null): string;
$str->ltrim(?string $chars = null): string;
$str->rtrim(?string $chars = null): string;
$str->upper(): string;
$str->lower(): string;
$str->slug(): string;
$str->contains(string $needle): bool;
$str->startsWith(string $prefix): bool;
$str->endsWith(string $suffix): bool;
$str->length(): int;
$str->byteLength(): int;
$str->split(string $delimiter): list;
$str->replace(string $search, string $replace): string;
$str->substr(int $start, ?int $length = null): string;
$str->reverse(): string;
$str->repeat(int $times): string;
$str->toEncoding(string $encoding): string;
$str->utf8(): string;
$str->bytes(): list;
$str->isUtf8(): bool;
$str->isEmpty(): bool;
```

---

## `Future` Facade & Instance API

```php
// Static Methods
Future::run(callable $fn): Future;
Future::wait(array $tasks): array;
Future::any(array $tasks): mixed;
Future::delayed(int $milliseconds, callable $fn = null): mixed;
Future::defer(callable $fn): void;
Future::value(mixed $value): Future;
Future::error(mixed $error): Future;

// Instance Properties & State Inspection (Matches Rust Poll::Pending / Poll::Ready)
$future->state;                 // string: "Pending" | "Ready"
$future->status;                // string: "Pending" | "Ready"
$future->getState(): string;    // "Pending" | "Ready"
$future->isPending(): bool;     // true if background Tokio task is running
$future->isReady(): bool;       // true if completed
$future->isCompleted(): bool;   // true if completed
$future->poll(): mixed;         // Non-blocking poll: returns null if pending, or resolved value
$future->wait(): mixed;         // Synchronously waits for completion and returns resolved value
```

---

## `AFile` Facade API (Asynchronous Tokio Filesystem)

```php
AFile::read(string $path): Future;                  // Resolves to string (file contents)
AFile::write(string $path, string $content): Future; // Resolves to int (bytes written)
AFile::exists(string $path): Future;                 // Resolves to bool
AFile::delete(string $path): Future;                 // Resolves to bool
AFile::append(string $path, string $content): Future;// Resolves to int (bytes written)
AFile::size(string $path): Future;                   // Resolves to int (byte size)
```

---

## `AHTTP` Facade API (Asynchronous Tokio HTTP Client)

```php
AHTTP::get(string $url): Future;                    // Resolves to HTTPResponse
AHTTP::post(string $url, mixed $body = null): Future;
AHTTP::put(string $url, mixed $body = null): Future;
AHTTP::delete(string $url): Future;
AHTTP::patch(string $url, mixed $body = null): Future;
AHTTP::withHeaders(dict $headers): AHTTPClient;     // Returns fluent client
```

---

## `ADB` Facade API (Asynchronous Tokio Database)

```php
ADB::table(string $table): ADBQuery;                // Returns fluent async query builder
ADB::find(string $table, mixed $id): Future;
ADB::get(string $table): Future;
ADB::select(string $query, array $bindings = []): Future;
ADB::insert(string $table, dict $values): Future;
ADB::update(string $table, dict $values): Future;
ADB::delete(string $table): Future;
ADB::query(string $sql, array $bindings = []): Future;
ADB::raw(string $sql): Future;

// Fluent ADBQuery Instance Methods (all terminal operations return Future)
$query->where(string $col, mixed $val): ADBQuery;
$query->orderBy(string $col, string $dir = 'ASC'): ADBQuery;
$query->limit(int $limit): ADBQuery;
$query->find(mixed $id): Future;
$query->first(): Future;
$query->get(): Future;
$query->count(): Future;
$query->insert(dict $data): Future;
$query->update(dict $data): Future;
$query->delete(): Future;
```

---

## `Memory` Facade API

```php
Memory::usage(): int;            // Current memory allocations in bytes
Memory::peak(): int;             // Peak memory allocation in bytes
Memory::isGcFree(): bool;        // Strictly returns true
Memory::gcRuns(): int;           // Strictly returns 0
Memory::gcPauseMs(): float;      // Strictly returns 0.0
Memory::cowDetachments(): int;   // Total COW detachment count
Memory::scope(callable $fn): mixed;
Memory::leakCheck(): dict;
Memory::arenaStats(): dict;
Memory::resetArena(): dict;
```

---

## `Request` Facade API

```php
// Parameter Inspection
Request::get(?string $key = null, mixed $default = null): mixed;
Request::all(): dict;
Request::only(array|string ...$keys): dict;
Request::has(string $key): bool;
Request::set(string $key, mixed $value): string;
Request::merge(array|dict $data): dict;
Request::clear(): void;

// In-Place Type Formatting (supports both quoted types and bare type keywords)
Request::format(array|dict $typeMap): dict;
```

---

## `Session` Facade API

```php
Session::get(?string $key = null, mixed $default = null): mixed;
Session::set(string $key, mixed $value): mixed;
Session::put(string $key, mixed $value): mixed;
Session::has(string $key): bool;
Session::all(): dict;
Session::forget(string $key): void;
Session::clear(): void;
```

---

## `Cookie` Facade API

```php
Cookie::get(?string $key = null, mixed $default = null): mixed;
Cookie::set(string $key, mixed $value, ...$options): string;
Cookie::put(string $key, mixed $value, ...$options): string;
Cookie::has(string $key): bool;
Cookie::all(): dict;
Cookie::forget(string $key): void;
Cookie::clear(): void;
```

---

## `Sys` Facade API

```php
Sys::echo(string $format, mixed ...$args): void;  // Directly compiles to Rust print!() (no trailing newline)
Sys::print(string $format, mixed ...$args): void; // Directly compiles to Rust println!() (adds trailing \n)
Sys::env(string $key, mixed $default = null): mixed;
Sys::os(): string;                               // Host OS name (e.g. "linux")
Sys::version(): string;                          // RHP engine version
Sys::timestamp(): int;                           // Current Unix epoch in seconds
Sys::dd(mixed ...$vars): void;                   // Dump variables and terminate

// NOTE: There is NO Sys::println function. Only echo and print exist in RHP.
```

### Formatting Syntax & Direct Parameter Mapping

Both `Sys::echo` and `Sys::print` directly map to Rust's native `print!` and `println!` routines with function parameters passed as-is. C-style format specifiers (`%d`, `%s`, `%f`, etc.) are **not** supported.

```php
// Value placeholder
Sys::print("Age: {}, Name: {}", 30, "Bob"); // Age: 30, Name: Bob\n

// Floating-point precision
Sys::print("{:.2}", 3.141592);              // 3.14\n

// Integer padding with zeros
Sys::print("{:05}", 42);                    // 00042\n

// Hexadecimal lowercase
Sys::print("{:x}", 255);                    // ff\n

// Binary formatting
Sys::print("{:b}", 5);                      // 101\n

// Inline output on the same line
Sys::echo("Age: {}, Name: {}", 30, "Bob");  // Age: 30, Name: Bob (same line)
```

---

## `Trace` & `Telemetry` Facade API (OpenTelemetry)

```php
// Span Lifecycle
Trace::startSpan(string $name, ?array $options = null): dict;
Trace::endSpan(?array $status = null): ?dict;
Trace::span(string $name, callable $closure, ?array $attributes = null): mixed;
Trace::setAttribute(string $key, mixed $value): void;
Trace::addEvent(string $name, ?array $attributes = null): void;
Trace::recordException(string $type, string $message, ?string $stackTrace = null): void;

// Context & Identifier Inspection
Trace::traceId(): ?string;          // Active 32-hex character TraceId
Trace::spanId(): ?string;           // Active 16-hex character SpanId
Trace::parentSpanId(): ?string;     // Active 16-hex parent SpanId (if any)
Trace::isRecording(): bool;         // true if active span is recording

// Metrics & Histograms
Trace::counter(string $name, float $delta = 1.0, ?array $attributes = null): void;
Trace::gauge(string $name, float $value, ?array $attributes = null): void;
Trace::histogram(string $name, float $value, ?array $attributes = null): void;
Trace::metricSummary(string $name): ?dict; // Returns [count, sum, min, max, avg, p50, p90, p95, p99]

// OTLP & APM Exporters
Trace::exportOtlp(): string;        // Canonical OTLP/HTTP JSON (/v1/traces)
Trace::exportOtlpMetrics(): string; // Canonical OTLP/HTTP JSON (/v1/metrics)
Trace::exportSentry(string $type, string $message): string; // Sentry crash event JSON
Trace::exportDatadog(): string;     // Datadog APM trace JSON
```

---

## Tier 2 Native Helper Classes API (`Cache`, `Log`)

Imported via `import "./Cache.rs";` or `import "./Log.rs";`. These helper classes are not built into the core language syntax to avoid language bloat, but provide framework-level performance using thread-safe Rust stores with zero garbage collection overhead.

### `Cache` Helper Class API

```php
// In-Memory L1 Cache with Optional TTL
Cache::get(string $key, mixed $default = null): mixed;
Cache::set(string $key, mixed $value, int $ttlSeconds = 0): bool;
Cache::has(string $key): bool;
Cache::delete(string $key): bool;
Cache::increment(string $key, int $step = 1): int;
Cache::decrement(string $key, int $step = 1): int;
Cache::all(): dict;
Cache::keys(): list;
Cache::count(): int;
Cache::clear(): void;
```

### `Log` Helper Class API

```php
// Structured High-Speed In-Memory & File Logging
Log::info(string $message, array|dict $context = []): void;
Log::warn(string $message, array|dict $context = []): void;
Log::error(string $message, array|dict $context = []): void;
Log::debug(string $message, array|dict $context = []): void;
Log::history(): list; // Returns list of log entries with timestamps, levels, and messages
Log::clear(): void;
```


