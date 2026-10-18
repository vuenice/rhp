---
title: "FFI and Rust Native Interop"
description: "Authoring native Tier 2 Rust crates, invoking external C-ABI libraries, and building high-performance plugins."
section: "Advanced"
order: 2
---

## Three-Tier Facade Architecture

RHP organizes execution libraries into three performance tiers:

- **Tier 1 (Built-in Native Facades):** Core runtime facades (`Str`, `Arr`, `Future`, `Memory`, `DB`, `Sys`) pre-compiled inside the engine binary.
- **Tier 2 (Custom Rust Plugins):** Crates authored in Rust and linked into the engine at build time.
- **Tier 3 (Script-land Facades):** User-defined static classes written in `.rhp` for application-specific logic.

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
<?rhp

// Load dynamic shared object
$lib = FFI::load("libcrypto.so");

// Declare C symbol signature
$lib->def("int SHA256(const char* d, size_t n, unsigned char* md);");

// Invoke directly from RHP
$hash = $lib->SHA256("password", 8);
```
