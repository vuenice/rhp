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
| `Sys` | System intrinsics and process environment | `env()`, `print()`, `dd()`, `version()`, `timestamp()` |
| `Weak` | Ephemeral weak references for cycle prevention | `to()`, `get()`, `isValid()` |

---

## Example: Composing Facades

```php
<?rhp

// Read input, transform collection, and log telemetry
string $rawContent = File::read('metrics.json');
dict $data = json_decode($rawContent, true);

list $activeUsers = Arr::list($data->get('users'))
    ->filter(fn($u) => $u['is_active'] === true)
    ->map(fn($u) => Str::slug($u['username']));

Log::info("Found " . (string)$activeUsers->count() . " active users");
Sys::print("Current Memory Usage: " . (string)Memory::usage() . " bytes");
```
