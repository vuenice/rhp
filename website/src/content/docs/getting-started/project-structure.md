---
title: "Project Structure"
description: "Recommended directory layouts and project structure conventions for RHP applications."
section: "Getting Started"
order: 4
---

## Standard Application Layout

A typical RHP project follows a modular, separation-of-concerns layout designed for clean auto-loading and fast compiler discovery.

```text
my-rhp-app/
├── server.rhp               # Boot entrypoint: runs once at startup (config, routes, container, Arc)
├── index.rhp                # Request entrypoint: runs once per HTTP request (bump arena, zero-GC)
├── rhp.json                 # Project configuration & dependencies
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

## The Boot & Request Split: `server.rhp` vs `index.rhp`

RHP separates application lifecycle into two distinct execution phases:

### 1. Boot Phase (`server.rhp`)
Runs **once** when the server process starts up. It evaluates configuration, reads environment variables, registers routes, sets up the dependency container, and compiles application classes. The result is frozen into an immutable boot heap (`Arc<EngineState>`), shared read-only across all worker threads.

```php
// server.rhp: Bootstraps global state once at startup
use App\Services\PaymentGateway;

// Register routes & container
Router::get('/health', fn() => ['status' => 'healthy', 'gc_runs' => 0]);
Router::get('/users', fn() => DB::table('users')->all());
Router::post('/orders', 'OrderController@store');

Log::info("Application boot complete. Immutable heap frozen.");
```

### 2. Request Phase (`index.rhp`)
Runs **once per HTTP request** across worker threads. Each request receives its own bump arena (`RequestArena`). Every object created during request processing is allocated in this arena. When the response finishes, deferred work (`Future::defer`) runs, and the arena is dropped in one step ($O(1)$), eliminating memory leaks without a garbage collector.

```php
// index.rhp: Executes per request in an isolated arena
$response = Router::dispatch(Request::current());
$response->send();

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
