---
title: "Classes, Objects & Singletons"
description: "Object-oriented programming in RHP: classes, property initialization contracts, interfaces, traits, and persistent singleton classes."
section: "Language Guide"
order: 5
---

## Object-Oriented Architecture

Classes in RHP represent structured types with explicit property typing and visibility modifiers:

```php
class UserProfile {
    public int $id;
    public string $name;
    private string $email;

    public function __construct(int $id, string $name, string $email) {
        $this->id = $id;
        $this->name = $name;
        $this->email = $email;
    }

    public function getEmail(): string {
        return $this->email;
    }
}

$user = new UserProfile(1, "Alice", "alice@example.com");
print("User: " . $user->name);
```

### Structs Are Not Present in RHP
Unlike Rust or C++, RHP intentionally omits the `struct` construct. Structured entity models and composite domain data are represented via typed `class` declarations or high-performance native collections (`dict` / `Arr::dict`).

### Field Initialization Guarantee (Compile-Time Strictness)
In PHP, reading an uninitialized typed property throws a runtime `Error`. In RHP, this safety guarantee is moved to compile time: **all properties without default values must be initialized upon object creation** (e.g. within `__construct`). Leaving non-nullable properties uninitialized triggers a **Compile Error**.

If a property can be empty at instantiation, declare it explicitly as nullable:
```php
class User {
    public string $name;         // Must be initialized in __construct
    public ?string $bio = null;  // Explicitly nullable, defaults to null
}
```

---

## Interfaces and Traits

```php
interface Loggable {
    public function logMessage(string $message): void;
}

trait Timestampable {
    public string $createdAt;

    public function touch(): void {
        $this->createdAt = Sys::timestamp();
    }
}

class AuditLog implements Loggable {
    use Timestampable;

    public function logMessage(string $message): void {
        Log::info("[Audit] " . $message);
    }
}
```

---

## Persistent Parent Process: `singleton class`

One of RHP's signature innovations is the `singleton class`. Singletons are instantiated once in the persistent parent process and remain shared across all worker threads via `Arc<EngineState>`:

```php
singleton class DatabaseManager {
    private static int $connectionCount = 0;

    public static function connect(): void {
        self::$connectionCount++;
        Log::info("Connected to shared pool");
    }

    public static function getPoolSize(): int {
        return self::$connectionCount;
    }
}
```

### Invariants for Singleton Invocation:
1. **Inside Non-Singleton Classes:** You can **only invoke static methods** on a singleton (e.g. `DatabaseManager::connect()`). Calling `new DatabaseManager()` or attempting `class Child extends DatabaseManager` throws a fatal `NonStaticSingletonMethodCallForbidden` compile error.
2. **Inside Other Singleton Classes:** Developers may call both static and instance methods directly.

---

## Facade Tiering & Extensibility: Runtime Macros vs. Inheritance

### Why Traditional Class Extension (`extends Arr`) is Forbidden

Native facades (`Arr`, `Str`, `Future`, `Memory`, `HTTP`, etc.) are static facades wrapping optimized Rust heap allocations (`Box<Vec<PhpValue>>`, UTF-8 byte buffers, bump arenas) rather than standard user-land classes. Traditional inheritance (`class CustomArr extends Arr`) is strictly forbidden:

* **Tight Coupling to Rust Internals:** Allowing class inheritance would expose internal memory boundaries and struct representations, complicating the Cranelift JIT compiler and VM optimization pipeline.
* **Stateless Design Violations:** Core facades are intentionally designed as stateless utility wrappers and fluent pipeline builders. Subclassing them introduces state management confusion and severe thread-safety hazards in a multi-threaded Tokio runtime.

Attempting to extend a static facade fails with a fatal compile error:

```text
Fatal Error: Cannot extend static facade class 'Arr'. Use 'Arr::macro()' to register custom methods dynamically.
```

### Dynamic Runtime Macro Registration (`::macro`)

Instead of extending the class via inheritance, developers register custom methods dynamically:

```php
// Define a custom macro on the Arr facade
Arr::macro('toCsv', function ($delimiter = ',') {
    // Custom logic accessing the underlying collection
    return implode($delimiter, $this->all());
});

// Usage anywhere in your application
$csv = Arr::list([1, 2, 3])->toCsv();
```

### Why Macros Work Perfectly for RHP

* **Zero Performance Cost on Core Types:** Macros reside in a global thread-safe runtime registry rather than modifying core class definitions or Rust struct layouts.
* **Maintains Familiarity:** Matches the exact developer ergonomics of Laravel (`Macroable`), making migration painless for teams upgrading performance-critical applications.
* **Thread-Safe Registration:** Macros can be registered once during application bootstrap (`index.rhp` boot phase) in the parent process, making them safely accessible across all concurrent worker processes with zero mutex contention on request paths.

