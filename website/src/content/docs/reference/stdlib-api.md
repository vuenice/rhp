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

## `Future` Facade API

```php
Future::run(callable $fn): Future;
Future::wait(array $tasks): array;
Future::any(array $tasks): mixed;
Future::delayed(int $milliseconds, callable $fn = null): mixed;
Future::defer(callable $fn): void;
Future::value(mixed $value): Future;
Future::error(mixed $error): Future;
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
