---
title: "Blocking Native/FFI Extensions Escape Hatch"
description: "Handling synchronous C/Rust libraries (libvips, argon2id, synchronous DNS) with #[blocking], tokio::task::spawn_blocking offloading, and fiber suspend_for_io."
section: "Advanced"
order: 9
---

## Overview & The Concurrency Problem

RHP executes up to **2,000 concurrent requests per worker OS thread** using lightweight, cooperative local execution fibers. Under RHP's synchronous-by-default execution model, developers write linear, procedural code without `async`/`await` boilerplate. The runtime transparently suspends and resumes fibers when performing standard I/O (database queries, network requests, filesystem reads).

However, real-world applications frequently integrate external C and Rust libraries through FFI:
- **Image Processing:** High-resolution resizing and WebP/AVIF encoding with `libvips` or `gd`.
- **Password Hashing:** Computationally intensive, memory-hard key derivation with `argon2id` or `scrypt`.
- **Legacy System Calls:** Synchronous DNS lookups (`getaddrinfo`), synchronous socket operations, or hardware crypto dongles.

```text
┌────────────────────────────────────────────────────────────────────────┐
│             THE THREAD-STALLING PROBLEM WITHOUT AN ESCAPE HATCH        │
├────────────────────────────────────────────────────────────────────────┤
│ Worker OS Thread (Runs 2,000 Fibers)                                   │
│  ├── Fiber 1: Calls argon2id_hash() (Synchronous C/Rust FFI)           │
│  │    └── BLOCKS OS THREAD FOR 80ms!                                   │
│  ├── Fiber 2: PAUSED (Waiting for CPU)                                 │
│  ├── Fiber 3: PAUSED (Waiting for CPU)                                 │
│  └── ... Fiber 2,000: ALL PAUSED UNTIL ARGON2ID COMPLETES              │
└────────────────────────────────────────────────────────────────────────┘
```

Because external C/Rust libraries perform synchronous, CPU-intensive work directly on the caller's thread, invoking them directly on an RHP worker thread would **block the worker OS thread and pause all 2,000 fibers**.

---

## The Solution: `#[blocking]` Escape Hatch

RHP solves this architectural challenge with the native **`#[blocking]` attribute escape hatch**. 

When a native or FFI function is decorated with `#[blocking]` (or `#[rhp::blocking]`), the RHP compiler and native loader register it as a blocking extension. When invoked at runtime:

1. **Fiber Suspension:** The runtime immediately suspends the calling fiber via `suspend_for_io(IoKind::NativeBlocking { extension, function })`.
2. **Background Dispatch:** The heavy native computation is automatically offloaded into Tokio's dedicated blocking thread pool (`tokio::task::spawn_blocking`).
3. **Zero Worker Thread Blocking:** The worker OS thread yields immediately to other ready fibers. The remaining 1,999 fibers continue servicing requests, executing queries, and handling HTTP traffic with zero interruption.
4. **Seamless Resumption:** When the background worker thread finishes, Tokio notifies RHP, the fiber is marked ready, and execution resumes seamlessly on the worker thread with the returned result.

```text
┌────────────────────────────────────────────────────────────────────────┐
│           RHP #[blocking] NATIVE/FFI ESCAPE HATCH DISPATCH             │
├────────────────────────────────────────────────────────────────────────┤
│ Worker OS Thread                                                       │
│  ├── Fiber 1 calls #[blocking] NativeCrypto::argon2id()                │
│  │    ├── 1. Fiber 1 Suspends: suspend_for_io(NativeBlocking)          │
│  │    └── 2. Offloads task to Tokio spawn_blocking pool                │
│  ├── Fiber 2: Runs uninterrupted (No Latency Spike)                    │
│  ├── Fiber 3: Handles HTTP GET /api/v1/users                           │
│  ├── ... Fibers 4-2,000: Execute concurrently at full speed           │
│                                                                        │
│ Background Tokio Thread Pool (spawn_blocking)                          │
│  └── Worker Thread: Computes argon2id memory-hard derivation (80ms)    │
│       └── On Complete: Notifies RHP Engine                             │
│                                                                        │
│ Worker OS Thread                                                       │
│  └── Fiber 1 Resumes: Receives computed hash without worker stalls     │
└────────────────────────────────────────────────────────────────────────┘
```

---

## Syntax & Usage

RHP supports function-level and module-level blocking attributes on native Rust files imported via `import "..."`:

### 1. Function-Level `#[blocking]` Attribute

Decorate individual CPU-heavy or synchronous C-binding functions with `#[blocking]` or `#[rhp::blocking]`. Fast or in-memory functions in the same module remain non-blocking:

```rust
// NativeCrypto.rs
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

// Fast in-memory CRC32: runs synchronously on the fiber thread
pub fn fast_crc32(data: &str) -> u32 {
    crc32fast::hash(data.as_bytes())
}

// Heavy CPU key derivation: automatically offloaded to spawn_blocking
#[blocking]
pub fn argon2id_hash(password: &str, salt: &str) -> String {
    // Synchronous CPU-bound argon2 computation (e.g., 64MB memory, 3 passes)
    argon2::hash_encoded(password.as_bytes(), salt.as_bytes(), &argon2::Config::default())
        .unwrap_or_default()
}

#[no_mangle]
pub extern "C" fn rhp_call(method: *const c_char, args_json: *const c_char) -> *mut c_char {
    let m = unsafe { CStr::from_ptr(method).to_str().unwrap_or("") };
    // JSON dispatch implementation...
    CString::new("\"ok\"").unwrap().into_raw()
}
```

In `.rhp` script-land, you call both methods synchronously with zero boilerplate:

```php
import "./NativeCrypto.rs" as Crypto;

// 1. Non-blocking: executes instantly on worker fiber
$checksum = Crypto::fast_crc32("quick_header");

// 2. Blocking: fiber suspends, offloads to spawn_blocking, resumes seamlessly
$hash = Crypto::argon2id_hash($userPassword, $userSalt);

// 3. Execution continues linearly
return Arr::dict([
    "checksum" => $checksum,
    "hash"     => $hash,
]);
```

### 2. Module-Level `#![blocking]` Attribute

For modules where every exported function performs heavy synchronous operations (such as an image processing or PDF generation extension), annotate the module header with `#![blocking]`:

```rust
// HeavyImage.rs
#![blocking]

use std::ffi::{CStr, CString};
use std::os::raw::c_char;

pub fn resize(image_path: &str, width: i32, height: i32) -> String {
    // Synchronous libvips resize operation
    format!("resized_{}_{}x{}", image_path, width, height)
}

pub fn compress_avif(image_path: &str, quality: i32) -> String {
    // Synchronous AVIF encoding
    format!("compressed_{}_q{}", image_path, quality)
}

#[no_mangle]
pub extern "C" fn rhp_call(method: *const c_char, args_json: *const c_char) -> *mut c_char {
    // All methods in this file inherit #[blocking] behavior
    CString::new("\"ok\"").unwrap().into_raw()
}
```

---

## Memory Safety & Cross-Thread Data Transfer (`FrozenValue`)

When dispatching execution across thread boundaries into Tokio's `spawn_blocking` pool, data structures with thread-local reference counters (`Rc<RefCell<...>>`) cannot be safely transferred between threads (`!Send`).

RHP solves this through the **`FrozenValue`** architectural bridge:

1. **Freezing on Caller Fiber:** When entering a `#[blocking]` native call, arguments (`&[PhpValue]`) are deeply converted into immutable, thread-safe `FrozenValue` instances (`Send + Sync`).
2. **Execution in Background Worker:** Inside the Tokio blocking thread pool, values are safely accessed or thawed into local representations.
3. **Thawing on Fiber Resume:** The computed result is returned as `FrozenValue` and thawed back into a standard `PhpValue` on the fiber's thread upon resumption.

```text
┌─────────────────┐       FrozenValue (Send + Sync)       ┌──────────────────────┐
│ Calling Fiber   │ ────────────────────────────────────> │ Blocking Worker Pool │
│ (Worker Thread) │ <──────────────────────────────────── │ (tokio threadpool)   │
└─────────────────┘       FrozenValue (Send + Sync)       └──────────────────────┘
```

---

## Tier 2 Rust Plugins & In-Process Extensions

In addition to `.rs` file imports, RHP supports the `#[blocking]` escape hatch for compiled Tier 2 plugins and in-process native extensions:

### 1. `PluginFacade::is_blocking` Hook

Custom Rust plugins implementing the `PluginFacade` trait can declare which methods should be dispatched to the blocking worker pool:

```rust
use rhp::{PluginFacade, PhpValue, Span, RuntimeError};

pub struct ImageMagickPlugin;

impl PluginFacade for ImageMagickPlugin {
    fn name(&self) -> &str {
        "ImageMagick"
    }

    // Declare heavy operations as blocking
    fn is_blocking(&self, method: &str) -> bool {
        matches!(method, "resize" | "blur" | "optimize" | "convert")
    }

    fn call(&self, method: &str, args: &[PhpValue], span: Span) -> Result<PhpValue, RuntimeError> {
        match method {
            "resize" => {
                // Runs safely inside tokio::task::spawn_blocking
                Ok(PhpValue::string("image_resized.webp"))
            }
            "dimensions" => {
                // Fast in-memory check
                Ok(PhpValue::Int(1080))
            }
            _ => Err(RuntimeError::custom("Method not found")),
        }
    }
}
```

### 2. Programmatic In-Process Native Functions

Register in-process native functions directly from engine extensions or embedding applications:

```rust
use rhp::{register_in_process_native_function, FrozenValue};

register_in_process_native_function(
    "NativeDns",
    "resolveMx",
    true, // is_blocking = true -> dispatched to spawn_blocking
    |args| {
        let domain = match args.first() {
            Some(FrozenValue::String(s)) => s,
            _ => "example.com",
        };
        // Synchronous system libc getaddrinfo / resolv lookup
        let records = vec![FrozenValue::String("mail.example.com".to_string())];
        Ok(FrozenValue::List(records))
    },
);
```

---

## Fiber Telemetry & OpenTelemetry Metrics

Whenever a `#[blocking]` native function executes, RHP automatically instruments the operation in the calling fiber's telemetry:

* **I/O Category:** Logged as `"NativeBlocking"`.
* **I/O Description:** Formatted as `"{ExtensionName}::{FunctionName}[blocking]"`.
* **Metrics Tracked:**
  - `stats.io_count`: Incremented by 1 for each blocking invocation.
  - `stats.io_wait_duration`: High-resolution wall-clock duration the fiber was suspended waiting for the background thread.
  - `stats.io_breakdown["NativeBlocking"]`: Specific counter tracking total blocking native calls.

Inspect telemetry directly in script-land or through OpenTelemetry collectors:

```php
// Query active fiber execution stats
$stats = Fiber::stats();

// Inspect telemetry breakdown
$blockingIoWaitMs = $stats["io_breakdown"]["NativeBlocking"] ?? 0;
$totalWaitDuration = $stats["io_wait_duration_ms"];
```

---

## Summary of Guarantees

| Capability | Standard Native FFI | `#[blocking]` Native FFI |
| :--- | :--- | :--- |
| **Execution Thread** | Worker OS Thread | Background Tokio Worker Pool (`spawn_blocking`) |
| **Worker Fiber Impact** | Pauses all 2,000 fibers during execution | Calling fiber suspended; other 1,999 fibers run without pause |
| **Fiber State During Wait** | `Running` (wasting CPU ticks) | `WaitingIo(NativeBlocking)` |
| **Thread Safety** | Process-local | Cross-thread safe via `FrozenValue` (`Send + Sync`) |
| **Telemetry & Observability** | Raw CPU time | Tracked under `io_breakdown["NativeBlocking"]` & `io_wait_duration` |
| **Ideal For** | Fast math, string parsing, CRC32, lookups | Image processing (`libvips`), `argon2id`, synchronous DNS, PDF render |
