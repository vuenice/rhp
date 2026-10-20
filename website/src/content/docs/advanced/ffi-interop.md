---
title: "FFI and Rust Native Interop"
description: "Authoring native Tier 2 Rust crates, invoking external C-ABI libraries, and building high-performance plugins."
section: "Advanced"
order: 2
---

## Three-Tier Facade Architecture

RHP organizes execution libraries into three performance tiers:

- **Tier 1 (Built-in Native Facades):** Core runtime facades (`Str`, `Arr`, `Future`, `Memory`, `DB`, `Sys`) pre-compiled inside the engine binary.
- **Tier 2 (Custom Rust Plugins & `.rs` File Imports):** Crates or standalone `.rs` files imported directly into scripts (`import "./Cache.rs";`) and compiled via `rustc` AOT or evaluated via the in-process native engine.
- **Tier 3 (Script-land Facades):** User-defined static classes written in `.rhp` for application-specific logic.

---

## Direct Rust File Import (`import "./Cache.rs"`)

RHP eliminates the need to maintain boilerplate C-bindings or compile full engine forks just to run performance-critical utilities. You can author a standard Rust `.rs` file and import it directly into `.rhp` scripts:

```php
import "./Cache.rs";
import "./Log.rs" as CustomLogger;

// Direct static invocation
Cache::set("user_token", "sec_token_9999", 7200);
$token = Cache::get("user_token");

CustomLogger::info("User token cached successfully");
```

### Implementing a Rust Helper File

Your `.rs` file can implement helper methods as standalone functions:

```rust
// Cache.rs
use std::collections::HashMap;
use std::sync::{LazyLock, RwLock};

static STORE: LazyLock<RwLock<HashMap<String, String>>> = LazyLock::new(|| RwLock::new(HashMap::new()));

pub fn get(key: &str) -> Option<String> {
    STORE.read().unwrap().get(key).cloned()
}

pub fn set(key: &str, val: &str) {
    STORE.write().unwrap().insert(key.to_string(), val.to_string());
}

pub fn has(key: &str) -> bool {
    STORE.read().unwrap().contains_key(key)
}

pub fn delete(key: &str) -> bool {
    STORE.write().unwrap().remove(key).is_some()
}
```

Or implement the canonical C-ABI dynamic dispatch entrypoint:

```rust
// FastCrypto.rs
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

#[no_mangle]
pub extern "C" fn rhp_call(
    method: *const c_char,
    args_json: *const c_char,
) -> *mut c_char {
    let m = unsafe { CStr::from_ptr(method).to_str().unwrap_or("") };
    let result = match m {
        "hash" => "\"9f83c605e7162b2b3e1626cd39eb43ec\"",
        _ => "null",
    };
    CString::new(result).unwrap().into_raw()
}

#[no_mangle]
pub extern "C" fn rhp_free_string(ptr: *mut c_char) {
    if !ptr.is_null() {
        unsafe { drop(CString::from_raw(ptr)); }
    }
}
```

RHP caches compiled `.so` files in `target/tmp/rhp_cache/` using SHA-256 source hashing, ensuring that after initial compilation, subsequent requests boot in microseconds.

---

## Writing a Tier 2 Rust Extension

To create a high-speed native extension, create a standard Rust crate implementing the `rhp-core` FFI interfaces:

```rust
use rhp_core::vm::{PhpValue, VmContext};
use rhp_core::error::RuntimeError;

pub fn native_fast_hash(ctx: &mut VmContext, args: &[PhpValue]) -> Result<PhpValue, RuntimeError> {
    if let Some(PhpValue::String(s)) = args.first() {
        let hash = xxhash_rust::xxh3::xxh3_64(s.as_bytes());
        Ok(PhpValue::Int(hash as i32))
    } else {
        Err(RuntimeError::InvalidArgument("Expected string argument".into()))
    }
}
```

---

## C-ABI Foreign Function Interface (`FFI::load`)

For dynamic libraries authored in C, C++, or Zig, load shared symbols dynamically at runtime:

```php
// Load dynamic shared object
$lib = FFI::load("libcrypto.so");

// Declare C symbol signature
$lib->def("int SHA256(const char* d, size_t n, unsigned char* md);");

// Invoke directly from RHP
$hash = $lib->SHA256("password", 8);
```
