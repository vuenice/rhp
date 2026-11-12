---
title: "Package"
description: "Learn how to use existing packages, create your own packages in pure RHP or Rust, and understand safety, sandboxing, and native drivers."
section: "Package"
order: 1
keywords: ["package", "packages", "rhp add", "rhp init", "rhp install", "rhp audit", "composer", "cargo", "native-rust", "wasm", "hybrid", "rhp.toml", "safety", "opaque handle"]
---

## Overview

RHP features a built-in, zero-configuration package manager inspired by **Cargo**, **Bun**, and **Composer**. 

You do not need to install separate tools like Composer or npm—everything is handled directly by the unified `rhp` binary. RHP packages can be written in:
- **Pure RHP:** Standard, familiar PHP/RHP code with zero compilation required.
- **Native Rust (Tier 2):** Raw machine-code dynamic libraries (`cdylib`) for high-throughput database drivers (MongoDB, Cassandra, Cosmos DB, Redis) and cryptography (JWT).
- **WebAssembly (Wasm):** Sandboxed, crash-proof plugins running inside an isolated VM.
- **Hybrid:** High-performance native Rust core wrapped in an elegant, fluent RHP script API.

---

## Part 1: How to Use a Package (In Simple Words)

Using a package in RHP takes just **two steps**: add it to your project, then use its classes in your code.

### Step 1: Add the Package

To add any package, open your terminal in your project directory and run:

```bash
# Add directly from GitHub (owner/repo)
rhp add vuenice/rhp-router

# Or specify a branch or tag
rhp add vuenice/rhp-router --branch=main
rhp add vuenice/rhp-router --tag=v1.2.0

# Or add from a full Git URL
rhp add https://github.com/vuenice/rhp-router.git

# Or add a local folder during development
rhp add ./packages/my-local-library
```

**What happens behind the scenes?**
1. RHP downloads the package into the `vendor/` directory.
2. It registers the package in your project's `rhp.toml` file.
3. It locks the exact Git commit in `rhp.lock` so your teammates and CI servers get the exact same code.
4. It pre-computes the static PSR-4 autoloader in `vendor/autoload.rhp`.

### Step 2: Use It in Your Code

In RHP, you **never** need to write `require 'vendor/autoload.php'` or `include`. Autoloading is handled automatically ahead of time.

Simply import the class with `use` and call it:

```php
<?php
use Vuenice\Router\Route;

Route::get('/users', function() {
    return ['status' => 'success', 'users' => ['Alice', 'Bob']];
});

Route::dispatch();
```

That's it!

### Common Package Commands

| What you want to do | Command to run |
| :--- | :--- |
| **Add a package** | `rhp add <vendor/package>` |
| **Add a dev-only package** | `rhp add <vendor/package> --dev` |
| **Install all packages (fresh clone / CI)** | `rhp install` |
| **Update packages to latest version** | `rhp update` or `rhp update <package>` |
| **Remove a package** | `rhp remove <package>` |
| **List installed packages** | `rhp list` |
| **Audit package safety** | `rhp audit` |

---

## Part 2: How to Create a Package (In Simple Words)

Creating a package in RHP is as easy as running a single command. RHP supports four different types of packages depending on your needs.

### Quick Start: Choose Your Package Type

```bash
# 1. Pure RHP Package (Most common: utilities, web routers, business logic)
rhp init my-package --type=rhp

# 2. Native Rust Package (For extreme speed: database drivers, JWT, cryptography)
rhp init my-mongo-driver --type=native-rust --safety=unsafe

# 3. WebAssembly Package (For community plugins: safe, sandboxed, cannot crash the server)
rhp init my-image-filter --type=wasm

# 4. Hybrid Package (Rust core engine + fluent RHP script wrapper)
rhp init my-cassandra-client --type=hybrid
```

---

### Walkthrough 1: Creating a Pure RHP Package

Let's create a reusable string and slug utility library called `my-slugger`:

```bash
rhp init my-slugger --type=rhp
cd my-slugger
```

RHP scaffolds the following clean structure:

```text
my-slugger/
├── rhp.toml                 # Package manifest
├── README.md                # Documentation
├── src/
│   └── MySlugger.rhp        # Your library code
└── tests/
    └── MySluggerTest.rhp    # Test suite
```

#### 1. Inspect `rhp.toml`
```toml
[package]
name = "my-slugger"
version = "0.1.0"
type = "rhp"
safety = "safe"              # Pure compute: no external network requests
rhp-version = ">=0.8.0"

[autoload]
psr-4 = { "Rhp\\MySlugger\\" = "src/" }

[exports]
facades = ["MySlugger"]
namespaces = ["Rhp\\MySlugger"]
```

#### 2. Write Your Code (`src/MySlugger.rhp`)
```php
<?php
namespace Rhp\MySlugger;

final class MySlugger {
    private function __construct() {}

    public static function slugify(string $text): string {
        return $text->lower()->replace(' ', '-');
    }
}
```

#### 3. Test It (`tests/MySluggerTest.rhp`)
```php
<?php
use Rhp\MySlugger\MySlugger;

$slug = MySlugger::slugify("Hello World RHP");
if ($slug !== "hello-world-rhp") {
    throw new \Exception("Test failed! Got: " . $slug);
}

echo "✅ All tests passed!\n";
```

Run the test:
```bash
rhp ./tests/MySluggerTest.rhp
```

#### 4. Share Your Package
Push your folder to GitHub:
```bash
git init && git add . && git commit -m "Initial commit"
git remote add origin https://github.com/your-username/my-slugger.git
git push -u origin main
```
Now anyone can install it with:
```bash
rhp add your-username/my-slugger
```

---

### Walkthrough 2: Creating a Native Rust Driver Package

When you need bare-metal performance, raw multi-threading, or direct TCP connection pools (e.g. for **MongoDB, Cassandra, Cosmos DB, or JWT**), create a **native Rust package**:

```bash
rhp init my-mongo --type=native-rust --safety=unsafe
cd my-mongo
```

RHP scaffolds:
```text
my-mongo/
├── rhp.toml                 # RHP Package manifest
├── Cargo.toml               # Standard Rust crate manifest
├── src/
│   └── lib.rs               # High-speed native Rust implementation
└── my_mongo.d.rhp           # Type declarations for IDE autocompletion
```

#### 1. How Rust Communicates with RHP (`src/lib.rs`)
The generated `src/lib.rs` exports a fast, memory-safe C-ABI entry point:

```rust
use std::collections::HashMap;
use std::sync::{Arc, RwLock, OnceLock};
use std::ffi::{CStr, CString};
use std::os::raw::c_char;

pub struct MongoConnection {
    pub endpoint: String,
    pub pool_size: u32,
}

static POOL: OnceLock<RwLock<HashMap<u64, Arc<MongoConnection>>>> = OnceLock::new();
static NEXT_HANDLE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

#[no_mangle]
pub extern "C" fn rhp_call(method_ptr: *const c_char, args_json_ptr: *const c_char) -> *mut c_char {
    let method = unsafe { CStr::from_ptr(method_ptr).to_str().unwrap_or("") };
    let args_json = unsafe { CStr::from_ptr(args_json_ptr).to_str().unwrap_or("[]") };

    let response = match method {
        "connect" => {
            // Keep connection pool inside Rust memory; return lightweight integer handle
            let handle = NEXT_HANDLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let conn = Arc::new(MongoConnection {
                endpoint: args_json.trim_matches('"').to_string(),
                pool_size: 16,
            });
            if let Ok(mut pool) = POOL.get_or_init(|| RwLock::new(HashMap::new())).write() {
                pool.insert(handle, conn);
            }
            format!("{{\"handle\": {}}}", handle)
        }
        "ping" => "{\"status\":\"ok\"}".to_string(),
        _ => format!("{{\"error\":\"Method '{}' not found\"}}", method),
    };

    CString::new(response).unwrap_or_default().into_raw()
}
```

#### 2. Generate Type Signatures for IDEs
Run:
```bash
rhp package gen-types
```
This generates `my_mongo.d.rhp` so code editors provide full autocomplete and type-checking without needing to compile Rust!

#### 3. Using the Native Driver in RHP Code
```php
<?php
use MyMongo;

// Receives an opaque handle to the persistent native connection pool
$conn = MyMongo::connect("mongodb://127.0.0.1:27017");
$status = MyMongo::ping();

echo "Connected! Handle ID: " . $conn['handle'] . "\n";
```

---

## Part 3: Extra Details & Advanced Architecture

### Package Archetypes Compared

| Feature | Pure RHP (`type = "rhp"`) | Native Rust (`type = "native-rust"`) | WebAssembly (`type = "wasm"`) | Hybrid (`type = "hybrid"`) |
| :--- | :--- | :--- | :--- | :--- |
| **Language** | Pure RHP / PHP syntax | Rust (compiled via Cargo) | Rust (compiled to `.wasm`) | Rust kernel + RHP script wrapper |
| **Best For** | Web apps, routers, helpers, business logic | High-speed DB drivers (Mongo, Cassandra), JWT, Crypto | Community plugins, templates, image filters | Full-featured ORMs and database query builders |
| **Execution** | RHP Rust VM / Fibers | Bare-metal machine code (`cdylib`) | Sandboxed `wasmtime` engine | Dual: Rust kernel + RHP VM |
| **Crash Risk** | **Zero.** Trapped as RHP Exceptions | Low (panics caught; unvetted segfaults possible) | **Zero.** Trapped inside Wasm sandbox | Low |
| **Performance** | High (JIT-compiled) | **100% Raw Native Hardware Speed** | High (~1.5x–2x of native) | 100% Native Core + Fast Script Layer |
| **Dependencies** | Other RHP packages | Full access to `crates.io` | `crates.io` (WASI compatible) | Full `crates.io` + RHP packages |

---

### Can a Package Crash Your Main Server Process?

This is a critical architectural guarantee in RHP:

* **Pure RHP Packages:** **CANNOT crash the main process.** Because they run inside RHP's memory-safe Rust VM, syntax mistakes or runtime errors throw standard RHP `Exception`s or `Error`s. The error terminates only that specific HTTP request fiber, while the server keeps running.
* **WebAssembly Packages:** **CANNOT crash the main process.** Wasm runs within an isolated linear memory sandbox. If a community Wasm package encounters a panic or memory trap, `wasmtime` cleanly terminates that specific sandbox instance. The host process is untouched.
* **Native Rust Packages:** Because they link dynamically as `.so` / `.dylib` for maximum database throughput, unvetted raw C pointers could in theory segfault. For this reason, RHP uses the **Package Safety Model** and verifies native packages.

---

### The Safety Model: `safe` vs. `unsafe`

Supply-chain attacks often happen when a seemingly harmless math or utility package quietly makes HTTP calls in the background to exfiltrate database credentials or `$_ENV` secrets.

RHP prevents this by declaring **package safety** in `rhp.toml`:

```toml
[package]
name = "vendor/text-formatter"
version = "1.0.0"
type = "rhp"
safety = "safe" # Pure compute: strictly forbidden from making network requests!
```

If a package is marked `safety = "safe"`, the RHP compiler and auditor enforce that it **cannot** invoke:
- `Http::*` facade
- `curl_*` handles
- `fsockopen` / raw TCP sockets
- Remote URL fetches (`file_get_contents('http://...')`)

For packages that genuinely need outbound network access (e.g. Sentry error reporting or Stripe APIs), declare `safety = "unsafe"` with explicit capabilities:

```toml
[package]
name = "vendor/sentry"
version = "1.0.0"
type = "wasm"
safety = "unsafe"

[capabilities]
network = ["*.ingest.sentry.io:443"] # Explicitly allowed endpoints
env = ["SENTRY_DSN"]
```

#### Run Security Audits
To inspect all your project's dependencies and source code for safety compliance, run:

```bash
rhp audit
```

If a package declared as `safe` attempts to make an unauthorized network call, `rhp audit` flags it immediately:
```text
🛡️  RHP Security & Safety Audit for 'my-app'
────────────────────────────────────────────────────────────
🚨 Found 1 SECURITY VIOLATION:
   ❌ src/Calculator.rhp:12: Illegal call to 'Http::' (Direct invocation of the HTTP facade)
```

---

### Opaque Handles: How Native Drivers Achieve Extreme Speed

Traditional runtimes suffer major performance bottlenecks when marshaling database records across language boundaries (e.g., converting C structs into PHP arrays and back).

RHP's native packages use **Opaque Handles**:
1. When you call `Mongo::connect("mongodb://...")`, the actual connection client and connection pool stay inside Rust memory.
2. The RHP runtime receives a lightweight integer handle ID (`u64`).
3. Subsequent operations pass this handle back to Rust. Rust looks up the connection in its global [`OpaqueHandleRegistry`](file:///home/yogesh/code/rhp/src/pkg/opaque_handle.rs) instantly.
4. **Zero intermediate serialization:** Result documents are written directly into RHP's internal memory arena without intermediate JSON conversion.

---

### Manifest Reference (`rhp.toml`)

Here is the complete reference of all configuration blocks supported in `rhp.toml`:

```toml
[package]
name = "vendor/my-package"
version = "1.0.0"
type = "rhp"                   # "rhp" | "native-rust" | "wasm" | "hybrid"
safety = "safe"                # "safe" | "unsafe"
description = "My package description"
license = "MIT"
rhp-version = ">=0.8.0"

# Pure RHP & Hybrid Autoloading (PSR-4 compliant)
[autoload]
psr-4 = { "Vendor\\MyPackage\\" = "src/" }
files = ["src/helpers.rhp"]

# Export Boundaries
[exports]
facades = ["MyPackage"]
namespaces = ["Vendor\\MyPackage"]

# Native Configuration (for native-rust, wasm, and hybrid)
[native]
crate-name = "my_package"
target-dir = "."
abi-version = 1
entry-symbol = "rhp_plugin_init"
execution-target = "native"     # "native" (verified) | "wasm" (sandboxed)
marshalling = "typed-c-abi"     # "typed-c-abi" | "msgpack"
facade = "MyPackage"

# Network & System Capabilities (for unsafe packages)
[capabilities]
network = ["api.example.com:443"]
filesystem = ["read:/tmp"]
env = ["API_KEY"]

# Production Dependencies
[dependencies]
"vuenice/rhp-router" = "^1.0"
"symfony/yaml" = { git = "https://github.com/symfony/yaml.git", branch = "main" }
"local/core" = { path = "../core-lib" }

# Development Dependencies
[dev-dependencies]
"rhp/test" = "^1.0"
```

---

## Summary Cheat Sheet

```bash
# === USING PACKAGES ===
rhp add <package>             # Add package to rhp.toml & vendor/
rhp add <package> --dev       # Add as development dependency
rhp install                   # Install from rhp.lock (CI / fresh clone)
rhp update                    # Update dependencies within semver constraints
rhp remove <package>          # Remove package and clean vendor/
rhp list                      # List installed dependencies
rhp audit                     # Audit dependencies for security & network egress

# === CREATING PACKAGES ===
rhp init my-lib --type=rhp              # Scaffold a pure RHP package
rhp init my-db --type=native-rust       # Scaffold a native Rust extension
rhp init my-filter --type=wasm          # Scaffold a sandboxed Wasm plugin
rhp init my-orm --type=hybrid           # Scaffold a hybrid Rust + RHP package
rhp package gen-types                   # Generate .d.rhp static type definitions
```
