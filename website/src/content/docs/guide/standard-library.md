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
| `Request` | HTTP input parameters, filtering & in-place type formatting | `get()`, `all()`, `only()`, `format()`, `has()`, `set()`, `merge()` |
| `Session` | Session state management across requests | `get()`, `set()`, `put()`, `has()`, `all()`, `forget()`, `clear()` |
| `Cookie` | HTTP cookie reading and setting | `get()`, `set()`, `put()`, `has()`, `all()`, `forget()`, `clear()` |
| `Arr` | Subtyped collection creation & fluent operations | `list()`, `dict()`, `set()`, `hashMap()`, `macro()` |
| `Str` | Binary-safe raw byte & UTF-8 string manipulation | `slug()`, `contains()`, `upper()`, `fromEncoding()` |
| `Future` | Tokio fiber concurrency & async coordination | `run()`, `wait()`, `any()`, `delayed()`, `defer()` |
| `Memory` | Zero-GC telemetry & scoped request arena inspection | `usage()`, `peak()`, `isGcFree()`, `gcPauseMs()`, `scope()` |
| `File` | Synchronous filesystem I/O operations | `read()`, `write()`, `exists()`, `delete()` |
| `AFile` | Non-blocking async filesystem I/O returning `Future` | `read()`, `write()`, `exists()`, `delete()`, `append()`, `size()` |
| `HTTP` | Synchronous HTTP client and server kernel | `get()`, `post()`, `withHeaders()`, `serve()` |
| `AHTTP` | Non-blocking async HTTP client returning `Future` | `get()`, `post()`, `put()`, `delete()`, `patch()`, `withHeaders()` |
| `DB` | Synchronous database queries and transactions | `table()`, `find()`, `get()`, `select()`, `insert()` |
| `ADB` | Non-blocking async database queries returning `Future` | `table()`, `find()`, `get()`, `select()`, `insert()`, `update()`, `delete()` |
| `Log` | Lock-free mpsc async structured logger | `info()`, `warning()`, `error()`, `debug()` |
| `Sys` | System intrinsics, formatted output & process environment | `env()`, `echo()`, `print()`, `dd()`, `version()`, `timestamp()` |
| `Weak` | Ephemeral weak references for cycle prevention | `to()`, `get()`, `isValid()` |

---

## Web State Facades (`Request`, `Session`, `Cookie`)

In RHP, legacy PHP superglobals (`$_GET`, `$_POST`, `$_SESSION`, `$_COOKIE`) are deprecated. Instead, use static facades:

### 1. `Request` Facade & Type Formatting

All incoming request parameters are strings by default. You can inspect, filter, or cast them on the request object using `Request::format()`:

```php
// Retrieve raw string values by default
$pageStr = Request::get('page'); // e.g. "1"

// Format and cast types in-place on the request object:
// Both quoted types ('int', 'bool') and bare type keywords (int, bool) are supported!
Request::format([
    'page'   => 'int',
    'active' => bool,
    'rate'   => float,
]);

int $page = Request::get('page');     // 1
bool $active = Request::get('active'); // true

// Filter specific keys via array list or variadic args:
dict $public = Request::only(['page', 'active']);
dict $pageOnly = Request::only('page');
```

> **Why bare type keywords work without ambiguity:**
> In RHP's Pratt parser, type keywords in statement positions (`int $x = 10;`) are validated by the presence of a variable identifier `$var`. When a type keyword appears as a value in an array literal (`'active' => bool`), it parses cleanly as a value token resolving to the type name string without any grammar ambiguity.

### 2. `Session` Facade

```php
// Store session state
Session::set('user_id', 42);

// Retrieve session value with fallback default
$userId = Session::get('user_id', 0);
$hasUser = Session::has('user_id');

// Clear specific key or all session data
Session::forget('user_id');
Session::clear();
```

### 3. `Cookie` Facade

```php
// Set a cookie (stored as string)
Cookie::set('theme', 'dark');

// Read cookie with fallback default
string $theme = Cookie::get('theme', 'light');

// Check and remove cookie
bool $hasTheme = Cookie::has('theme');
Cookie::forget('theme');
```

---

## Sys Facade: Formatted Output (`echo` & `print`)

`Sys` provides two output functions with Rust-style template string formatting:
- **`Sys::echo(...)`**: Prints on the same line without a trailing newline (identical to Rust's `print!()`).
- **`Sys::print(...)`**: Prints and automatically adds a trailing newline `\n` (identical to Rust's `println!()`).

> **Important: There is no `Sys::println` function.**
> In RHP, only `echo` and `print` exist (both as language keywords `echo ...` / `print(...)` and via `Sys::echo(...)` / `Sys::print(...)`). There is **no** `Sys::println` function. Calling `Sys::println(...)` will result in an `UndefinedFacadeMethod` error.

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

## Synchronous Facades (`File`, `HTTP`, `DB`) vs Async Facades (`AFile`, `AHTTP`, `ADB`)

RHP eliminates function coloring by providing pairs of synchronous and asynchronous facades rather than requiring `async/await` keywords:

- **Use Synchronous Facades (`File`, `HTTP`, `DB`)** for straightforward, sequential blocking execution:
  ```php
  string $data = File::read('report.csv');
  $response = HTTP::get('https://api.example.com/status');
  $user = DB::table('users')->find(10);
  ```
- **Use Async Facades (`AFile`, `AHTTP`, `ADB`)** to launch non-blocking background tasks on Rust Tokio, returning `Future` handles that can be checked with `$fut->isPending()` or resolved via `$fut->wait()`:
  ```php
  $fut1 = AFile::read('report.csv');
  $fut2 = AHTTP::get('https://api.example.com/status');
  $fut3 = ADB::table('users')->find(10);

  // Synchronize results without 'await':
  $data = $fut1->wait();
  $response = $fut2->wait();
  $user = $fut3->wait();
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
