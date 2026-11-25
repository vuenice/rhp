---
title: "Modules and Imports"
description: "Organize code with namespaces, use statements, and automated symbol resolution in RHP."
section: "Language Guide"
order: 7
---

## Namespaces

RHP groups classes, interfaces, and functions under structured namespaces. RHP supports **convention over configuration**:

### 1. Automatic Convention-Based Namespaces

In an RHP project (with `rhp.toml`), writing `namespace ...;` in every single file is **not required**. If you omit the `namespace` header, the compiler automatically derives the namespace from the file's relative path from `rhp.toml`:

```php
// File: src/Services/Auth/User.rhp
// No namespace declaration needed! Automatically in Src\Services\Auth

class User {
    public function getRole(): string {
        return "admin";
    }
}
```

* **Direct Path Mapping (Default):** Folder names are converted to PascalCase relative to `rhp.toml`:
  $$\text{src/Services/Auth/User.rhp} \longrightarrow \mathbf{Src\backslash Services\backslash Auth\backslash User}$$
* **PSR-4 Mapping (if declared in `rhp.toml`):** If your `rhp.toml` maps `[autoload.psr-4]`:
  ```toml
  [autoload]
  psr-4 = { "App\\" = "src/" }
  ```
  Then `src/Services/Auth/User.rhp` resolves to `App\Services\Auth\User`.
* **Root Files:** Files located directly at the project root (like `index.rhp` or `app.rhp`) automatically belong to the **global root namespace** (`\`).

### 2. Explicit Global Scope (`namespace;`)

If a file inside a subdirectory needs to declare functions or classes in the **global root scope** (`\`), use an explicit empty `namespace;` (or `namespace ~;`):

```php
// File: src/Helpers/GlobalFunctions.rhp
namespace; // Opts out of path-based namespace, placing symbols into global scope \

function dd(...$vars): void {
    print_r($vars);
    exit(1);
}
```

### 3. Explicit Namespace Override

You can always explicitly override the folder-derived namespace by declaring a custom namespace at the top of the file:

```php
namespace ThirdParty\Custom\Auth;

class Gateway {}
```

---

## The `use` Statement

Import classes and namespaces into your current scope using `use`:

```php
namespace App\Controllers;

use App\Services\Payment\StripeGateway;
use App\Models\User;

class CheckoutController {
    public function handle(int $userId, int $cents): bool {
        $gateway = new StripeGateway();
        return $gateway->charge($cents);
    }
}
```

---

## Aliasing with `as`

To resolve naming collisions between different modules, alias imported identifiers with `as`:

```php
use App\Services\Http\Client as HttpClient;
use External\Network\Client as ExternalClient;

$local = new HttpClient();
$ext = new ExternalClient();
```

---

## Auto-Loading and Symbol Lowering

Unlike traditional PHP which scans the filesystem on every request or relies on heavy Composer ClassLoader lookups, RHP's **Pratt Parser** resolves module symbols during initial parsing in the parent process.

- Built-in facades (`Arr`, `Str`, `Future`, `Sys`) are resolved in $O(1)$ time via compile-time DFA tokens.
- Application classes are loaded and compiled into the persistent engine AST index once.

---

## Importing Rust Files (`.rs`) for High-Performance Helper Classes

To keep the core programming language lightweight and minimal while providing extreme native performance at the framework and application level, RHP allows importing `.rs` files directly using `import` or `use`:

```php
// Import Rust files directly into your script
import "./Cache.rs";
import "./Log.rs";

// Or import with an explicit alias
use "./FastCache.rs" as Cache;
import CustomLog from "./Log.rs";
```

### Direct Usage as Static Helper Classes

Once imported, the Rust file is automatically registered as a **Tier 2 Static Facade**. You can invoke its methods directly without instantiation:

```php
// Cache helper methods
Cache::set("session_user_42", "Alice", 3600); // Key, value, optional TTL (seconds)
if (Cache::has("session_user_42")) {
    $user = Cache::get("session_user_42");
}

Cache::increment("page_views", 1);
$views = Cache::get("page_views");

Cache::delete("session_user_42");
Cache::clear();

// Log helper methods
Log::info("User signed in successfully", ["user_id" => 42]);
Log::warn("Cache miss for resource", ["key" => "product_123"]);
Log::error("Database connection timeout", ["retry_after_ms" => 500]);

$recentLogs = Log::history();
```

### Architectural Benefits: Framework Performance Without Core Bloat

- **Zero Core Language Bloat**: Framework-level helpers like `Cache`, `Log`, `RateLimiter`, or `FastHasher` do not need to be hardcoded into the language core or standard library syntax.
- **Native Rust Speed**: When `rustc` is available, the engine automatically compiles `.rs` files into native machine-code dynamic shared libraries (`cdylib` / `.so`) with SHA-256 caching.
- **Fast In-Process Fallback**: For rapid iteration during development (`rhp run dev`) or test environments, RHP features a built-in thread-safe in-process engine providing instant zero-compilation execution.
- **Thread-Safety & CoW Protection**: Data stored across requests utilizes `FrozenValue` state boundaries, guaranteeing strict thread safety across multi-threaded worker pools and Tokio execution fibers without garbage collection pauses.

