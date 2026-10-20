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
| `Arr` | Subtyped collection creation & fluent operations | `list()`, `dict()`, `set()`, `hashMap()`, `macro()` |
| `Str` | Binary-safe raw byte & UTF-8 string manipulation | `slug()`, `contains()`, `upper()`, `fromEncoding()` |
| `Future` | Tokio fiber concurrency & async coordination | `run()`, `wait()`, `any()`, `delayed()`, `defer()` |
| `Memory` | Zero-GC telemetry & scoped request arena inspection | `usage()`, `peak()`, `isGcFree()`, `gcPauseMs()`, `scope()` |
| `File` | Synchronous and asynchronous file I/O operations | `read()`, `write()`, `exists()`, `delete()` |
| `HTTP` | Non-blocking HTTP client and server kernel | `get()`, `post()`, `withHeaders()`, `serve()` |
| `Log` | Lock-free mpsc async structured logger | `info()`, `warning()`, `error()`, `debug()` |
| `Sys` | System intrinsics, formatted output & process environment | `env()`, `echo()`, `print()`, `dd()`, `version()`, `timestamp()` |
| `Weak` | Ephemeral weak references for cycle prevention | `to()`, `get()`, `isValid()` |

---

## Sys Facade: Formatted Output (`echo` & `print`)

`Sys` provides two output functions with Rust-style template string formatting:
- **`Sys::echo(...)`**: Prints on the same line without a trailing newline (identical to Rust's `print!()`).
- **`Sys::print(...)`**: Prints and automatically adds a trailing newline `\n` (identical to Rust's `println!()`).

```php
// Rust-style formatting templates
Sys::print("Age: {}, Name: {}", 30, "Bob"); // Outputs: Age: 30, Name: Bob\n

// 1. Floating-point precision (C: %.2f)
Sys::print("{:.2}", 3.141592); // Outputs: 3.14\n

// 2. Integer padding with zeros (C: %05d)
Sys::print("{:05}", 42); // Outputs: 00042\n

// 3. Hexadecimal lowercase (C: %x)
Sys::print("{:x}", 255); // Outputs: ff\n

// 4. Binary formatting
Sys::print("{:b}", 5); // Outputs: 101\n

// Same line printing with Sys::echo
Sys::echo("Counting: ");
Sys::echo("{:02}.. ", 1);
Sys::echo("{:02}.. ", 2);
Sys::print("Done!");
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
