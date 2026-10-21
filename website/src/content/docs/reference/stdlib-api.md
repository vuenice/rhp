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

## `Str` Facade API

```php
Str::slug(string $text, string $separator = '-'): string;
Str::contains(string $haystack, string $needle): bool;
Str::startsWith(string $haystack, string $prefix): bool;
Str::endsWith(string $haystack, string $suffix): bool;
Str::upper(string $text): string;
Str::lower(string $text): string;
Str::trim(string $text): string;
Str::fromEncoding(string $bytes, string $encoding): string;
Str::toEncoding(string $text, string $encoding): string;
Str::byteLength(string $text): int;
Str::macro(string $name, callable $closure): void;
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
Sys::echo(string $format, mixed ...$args): void;  // Formatted output without trailing newline
Sys::print(string $format, mixed ...$args): void; // Formatted output WITH trailing newline (\n)
Sys::env(string $key, mixed $default = null): mixed;
Sys::os(): string;                               // Host OS name (e.g. "linux")
Sys::version(): string;                          // RHP engine version
Sys::timestamp(): int;                           // Current Unix epoch in seconds
Sys::dd(mixed ...$vars): void;                   // Dump variables and terminate

// NOTE: There is NO Sys::println function. Only echo and print exist in RHP.
```

