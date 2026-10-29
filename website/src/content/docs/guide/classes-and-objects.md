---
title: "Classes, Objects, Enums & Singletons"
description: "Object-oriented programming in RHP: classes, property initialization contracts, enumerations, interfaces, traits, and persistent singleton classes."
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

### Automatic Lexical Scope Capture of `$this` in Closures

Anonymous callbacks (`function(...) { ... }`) created within non-static instance methods automatically capture `$this` from their enclosing lexical scope:

```php
class CartService {
    private DiscountManager $discounts;

    public function applyDiscounts(List $items): List {
        // Automatically captures $this from the enclosing method scope
        return $items->map(function(CartItem $item): CartItem {
            return $this->discounts->apply($item);
        });
    }
}
```

Methods and properties can be invoked directly inside closures without manual binding (`$closure->bindTo($this)`). For details, see [Anonymous Functions & Lexical Scope Capture of $this](/docs/guide/functions#anonymous-functions--automatic-lexical-scope-capture-of-this).

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

## Invokable Objects (`__invoke`)

Classes in RHP can implement the magic `__invoke()` method to allow instances of the class to be directly called as functions (`$obj(...)`). This pattern is ubiquitous in modern PHP frameworks such as Laravel for single-action controllers, action services, and invokable commands.

```php
class CreateSite {
    public function __construct(private string $environment) {}

    public function __invoke(array $data): string {
        return "Created site [" . $data['domain'] . "] in " . $this->environment;
    }
}

$createSite = new CreateSite("production");

// Directly invoke the object instance:
$result = $createSite(["domain" => "laravel.com"]);
print($result); // "Created site [laravel.com] in production"
```

### Callable Type Contract Acceptance

Invokable objects fully satisfy PHP's `callable` type hint at both compile-time and runtime boundary assertions:

```php
function executeAction(callable $action, array $params): mixed {
    return $action($params);
}

$createSite = new CreateSite("staging");
executeAction($createSite, ["domain" => "test.local"]);
```

If an object without an `__invoke()` method is called as a function, RHP throws a `TypeError: Value of type 'ClassName' is not callable`, matching standard PHP 8 semantics.

---

## Enumerations (`enum`)

RHP provides first-class support for pure (unit) enums and backed enums (`string` or `int`), matching PHP 8.1+ semantics while integrating directly into RHP's high-performance memory model and gradual type system.

### Declaring Enums

#### 1. Pure (Unit) Enums
Pure enums define a finite set of discrete case objects without an underlying primitive value:

```php
enum PlanType {
    case Free;
    case Pro;
    case Enterprise;
}

$plan = PlanType::Pro;
print("Selected plan: " . $plan->name); // "Pro"
```

#### 2. Backed Enums (`string` or `int`)
Backed enums associate each case with an immutable scalar backing value (`string` or `int`):

```php
enum SiteStatus: string {
    case Draft = 'draft';
    case Published = 'published';
    case Archived = 'archived';
}

enum Priority: int {
    case Low = 1;
    case Medium = 2;
    case High = 3;
}

$status = SiteStatus::Published;
print($status->name);  // "Published"
print($status->value); // "published"
```

Each enum case is initialized as an immutable singleton object in memory. Mutating `$status->name` or `$status->value` triggers a runtime `ReadonlyPropertyModification` error.

---

### Built-in Static Methods

RHP automatically synthesizes standard enum static methods:

#### 1. `Enum::cases(): list<Enum>`
Available on **all** enums (both pure and backed). Returns a packed list (`Arr::list`) containing all declared cases in declaration order:

```php
$allPlans = PlanType::cases();
// [$allPlans[0] === PlanType::Free, $allPlans[1] === PlanType::Pro, ...]

foreach (SiteStatus::cases() as $case) {
    print($case->name . " => " . $case->value . "\n");
}

$count = count(PlanType::cases()); // 3
```

#### 2. `Enum::from(int|string $value): static`
Available exclusively on **backed enums**. Maps a scalar value to its corresponding enum case instance:

```php
// Retrieve from request and map to enum:
$status = SiteStatus::from($request->status); // e.g. SiteStatus::Published

$priority = Priority::from(2); // Priority::Medium
$isMedium = ($priority === Priority::Medium); // true
```

* **Invalid Value Handling (`ValueError`):** If no matching case exists, `from()` raises a standard PHP-compatible `ValueError`:
  ```php
  try {
      SiteStatus::from('invalid_status');
  } catch (ValueError $e) {
      print("Caught error: " . $e->message);
      // 'Caught error: "invalid_status" is not a valid backing value for enum SiteStatus'
  }
  ```
* **Strict Type Safety:** Calling `SiteStatus::from(123)` on a `string`-backed enum triggers a `TypeError` (boundary contract violation).
* **Pure Enums:** Calling `from()` on a non-backed enum produces an `UndefinedMethod` error (`Call to undefined method PlanType::from()`).

#### 3. `Enum::tryFrom(int|string|null $value): ?static`
Available exclusively on **backed enums**. Maps a scalar value to its enum instance, or returns `null` if the value is not valid or if `null` is passed:

```php
// Returns SiteStatus::Published
$status = SiteStatus::tryFrom($request->status);

// Returns null safely without throwing ValueError:
$unknown = SiteStatus::tryFrom('nonexistent'); // null

// Convenient for optional or missing query parameters:
$optional = SiteStatus::tryFrom(null); // null
```

---

### Custom Methods & Interfaces

Enums can implement interfaces and declare custom instance or static methods:

```php
interface HasBadge {
    public function badgeColor(): string;
}

enum SiteStatus: string implements HasBadge {
    case Draft = 'draft';
    case Published = 'published';
    case Archived = 'archived';

    public function badgeColor(): string {
        return match ($this) {
            self::Draft => 'yellow',
            self::Published => 'green',
            self::Archived => 'gray',
        };
    }

    public static function defaultStatus(): self {
        return self::Draft;
    }
}

$status = SiteStatus::from('published');
print($status->badgeColor()); // "green"
```

### Subtyping & `instanceof` Contracts

Enums participate fully in RHP's nominal type hierarchy:
* All enums implement the built-in `UnitEnum` interface contract:
  ```php
  $isUnit = (PlanType::Free instanceof UnitEnum); // true
  $isUnit = (SiteStatus::Draft instanceof UnitEnum); // true
  ```
* Backed enums additionally implement `BackedEnum`:
  ```php
  $isBacked = (SiteStatus::Draft instanceof BackedEnum); // true
  $isBacked = (PlanType::Free instanceof BackedEnum);    // false
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

