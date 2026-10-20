---
title: "Classes, Structs & Singletons"
description: "Object-oriented programming in RHP: classes, visibility modifiers, interfaces, traits, and persistent singleton classes."
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

## Facade Tiering & Extensibility via `::macro`

Native facades (`Arr`, `Str`, `Future`, `Memory`) wrap optimized Rust memory and cannot be extended via standard inheritance (`class CustomArr extends Arr` is forbidden).

Instead, extend them dynamically at application boot using `::macro()`:

```php
// Register custom helper dynamically
Arr::macro('toCsv', function(string $delimiter = ','): string {
    return implode($delimiter, $this->all());
});

// Accessible on all Arr collection instances
string $csv = Arr::list([1, 2, 3])->toCsv();
```
