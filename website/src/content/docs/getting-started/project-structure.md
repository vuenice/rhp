---
title: "Project Structure"
description: "Recommended directory layouts and project structure conventions for RHP applications."
section: "Getting Started"
order: 4
---

## Standard Application Layout

A typical RHP project follows a clean, modular layout. The runtime owns the server process and worker pool, so your application needs only a single entrypoint (`index.rhp`):

```text
my-rhp-app/
├── index.rhp                # Application entrypoint: boot hooks + request handlers
├── rhp.toml                 # Server & runtime configuration (port, workers, timeouts, watch)
├── rhp.json                 # Project metadata, compilation options & linter settings
├── src/
│   ├── Controllers/         # HTTP request handlers
│   │   ├── UserController.rhp
│   │   └── OrderController.rhp
│   ├── Models/              # Domain entities & ORM models
│   │   └── User.rhp
│   └── Services/            # Core business logic & singleton services
│       └── PaymentGateway.rhp
├── storage/                 # Persistent SQLite database & application logs
│   ├── logs/
│   └── app.db
└── tests/                   # Integration and unit test suites
    ├── UserTest.rhp
    └── OrderTest.rhp
```

---

## Single Entrypoint (`index.rhp`) & Lifecycle Phases

In RHP, user code never has to write a server event loop or socket listen statement. The runtime supervisor (`rhp run dev` / `rhp run build`) manages the TCP socket and worker processes automatically.

Application lifecycle is divided into two phases managed via lifecycle hooks:

### 1. Boot Phase (Master Process, Runs Once)
Runs **once** in the parent supervisor process before worker processes are forked. Top-level registrations configure routing tables, dependency containers, and pre-compile bytecode/JIT. 

> **Strict Boot Discipline:** The master process remains strictly single-threaded during boot. Spawning Tokio runtimes, threads, or opening database connections before forking is strictly prohibited to prevent POSIX `fork()` deadlocks.

### 2. Request Phase (Worker Processes, Repeated)
The master pre-forks $N$ worker processes (configured in `rhp.toml`). Each worker initializes its Tokio runtime and database connection pools post-fork. When requests arrive, they execute inside isolated, zero-GC bump arenas (`RequestArena`), which are dropped in $O(1)$ time upon response completion.

```php
// index.rhp: Single entrypoint
use App\Controllers\OrderController;
use RHP\Http\Request;
use RHP\Http\Response;
use RHP\Routing\Router;

// ─── 1. BOOT PHASE (Runs once in Master before worker fork) ───
Router::get('/health', fn() => ['status' => 'healthy', 'gc_runs' => 0]);
Router::post('/orders', [OrderController::class, 'store']);

Router::get('/users', function(Request $req): Response {
    // ─── 2. REQUEST PHASE (Runs inside Worker on RequestArena) ───
    $users = DB::table('users')->all();
    return Response::json($users);
});
```

---

## Server & Runtime Configuration (`rhp.toml`)

Operational settings like host interfaces, port numbers, worker pool sizing, and watch paths are specified in `rhp.toml`:

```toml
[server]
host = "127.0.0.1"
port = 8080
workers = "auto" # or integer, e.g. 4
max_arena_mb = 32
timeout_ms = 5000

[dev]
watch = ["src/", "index.rhp"]
```

---

## Configuration File (`rhp.json`)

Project metadata, compilation options, and linter settings are defined in `rhp.json`:

```json
{
  "name": "my-rhp-app",
  "version": "0.1.0",
  "entry": "src/main.rhp",
  "compiler": {
    "opt_level": 2,
    "jit": true
  },
  "lint": {
    "indent": 4,
    "enforce_casing": true,
    "auto_type": true
  },
  "dependencies": {
    "TODO: package manager manifest specification in progress": "v0.3.0"
  }
}
```
