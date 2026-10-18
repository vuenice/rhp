---
title: "Package Manager"
description: "Package ecosystem roadmap, Composer interop bridge, and native Rust crate linking."
section: "Tooling"
order: 1
---

## Current Status (v0.1.0 Alpha)

> **Honest Trade-off:** RHP is currently in Alpha. A standalone package registry and automated dependency resolver (`rhp install`) are under active development for the v0.3.0 milestone.

During the v0.1 milestone, projects utilize:
1. **Tier 1 Native Facades:** Shipped pre-compiled in the `rhp` binary.
2. **Tier 2 Rust Plugins:** Linked statically or compiled as shared dynamic libraries (`.so` / `.dylib`).
3. **Local Class Modules:** Auto-loaded from project source directories.

---

## Composer Interoperability Bridge (Roadmap)

To enable friction-free adoption for teams upgrading Laravel applications, RHP includes a planned **Composer Bridge**:

```bash
# Roadmap for v0.3.0:
rhp composer install
```

The bridge reads `composer.json` and `composer.lock`, generates native AST autoload lookup tables, and maps PSR-4 namespaced vendor packages into the persistent engine state without relying on runtime PHP file scans.

---

## Linking Native Rust Crates (Tier 2 Plugins)

Developers can write high-performance custom extensions in Rust and link them at build time:

```toml
# In Cargo.toml
[dependencies]
rhp-plugin-sdk = "0.1"
image = "0.25"
```

```rust
use rhp_plugin_sdk::prelude::*;

#[rhp_macro("Image::blur")]
pub fn native_blur(args: &[PhpValue]) -> Result<PhpValue, RuntimeError> {
    // Zero-copy Rust image processing
    Ok(PhpValue::Bool(true))
}
```
