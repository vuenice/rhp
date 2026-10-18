---
title: "Project Structure"
description: "Recommended directory layouts and project structure conventions for RHP applications."
section: "Getting Started"
order: 3
---

## Standard Application Layout

A typical RHP project follows a modular, separation-of-concerns layout designed for clean auto-loading and fast compiler discovery.

```text
my-rhp-app/
├── rhp.json                 # Project configuration & dependencies
├── src/
│   ├── main.rhp             # Application entrypoint & HTTP bootstrap
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

## The Entrypoint (`src/main.rhp`)

In RHP, the application entrypoint bootstraps the persistent parent process:

```php
<?rhp

namespace App;

use App\Services\PaymentGateway;

// Register application singleton services in parent process
singleton class AppEngine {
    public static function boot(): void {
        DB::pool(min: 10, max: 50);
        Log::info("Parent engine booted with shared connection pool.");
    }
}

AppEngine::boot();

// Route incoming requests against the pre-compiled parent state
HTTP::serve(port: 8080, routes: [
    'GET /health' => fn() => ['status' => 'healthy', 'gc_runs' => Memory::gcRuns()],
    'GET /users'  => fn() => DB::table('users')->all(),
]);
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
