---
title: "Classes, Objects, Enums & Shared Classes"
description: "Object-oriented programming in RHP: classes, property initialization contracts, enumerations, interfaces, traits, and persistent shared classes."
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
Unlike Rust or C++, RHP intentionally omits the `struct` construct. Structured entity models and composite domain data are represented via typed `class` declarations or high-performance native collections (`dict`).

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

## VM Table Reduction & Compile-Time Static Dispatch

In traditional PHP runtimes, dynamic method dispatch on objects traverses runtime hash tables (`zend_function_entry`), causing cache misses, dynamic lookups, and branch mispredictions.

RHP drastically minimizes dynamic virtual method table (vtable) lookups and memory overhead by enforcing static dispatch at compile time—using **100% standard PHP syntax**:

### 1. Direct Static Dispatch via `final`

When a class or method is declared `final`, the compiler knows with mathematical certainty that it can never be extended or overridden:

```php
final class OrderCalculator {
    public function calculateTotal(int $price, int $quantity): int {
        return $price * $quantity;
    }
}

$calc = new OrderCalculator();
// Statically dispatched via OpCode::MethodCallDirect (Zero vtable overhead)
$total = $calc->calculateTotal(100, 2);
```

* **No Virtual Method Slot:** The compiler emits `OpCode::MethodCallDirect` pointing directly to the compiled function address, completely bypassing vtable lookups.
* **Extending Final Class is a Compile Error:** Attempting `class SubCalculator extends OrderCalculator` fails instantly at compile time with a fatal contract violation.

### 2. Whole-Program Class Hierarchy Analysis (CHA)

PHP classes are **open and extensible by default** without requiring non-standard keywords:

```php
class UserService {
    public function findUser(int $id): string {
        return "user_" . string($id);
    }
}
```

* If no subclass in the compiled program overrides `findUser()`, RHP's **Class Hierarchy Analysis (CHA)** automatically devirtualizes every call `$service->findUser($id)` to direct static execution (`MethodCallDirect`).
* If a derived subclass does override the method, only those specific calls use nominal vtable dispatch.

### 3. Contract Enforcement via PHP 8.3 `#[\Override]`

To prevent silent typos and broken inheritance hierarchies, RHP fully enforces the standard PHP 8.3 `#[\Override]` and `#[Override]` attribute at compile time:

```php
class BaseLogger {
    public function log(string $msg): void {
        print($msg);
    }
}

class FileLogger extends BaseLogger {
    #[\Override]
    public function log(string $msg): void {
        Log::info($msg);
    }

    #[\Override]
    public function nonExistent(): void {
        // Compile Error: Method FileLogger::nonExistent() has '#[Override]' attribute 
        // but no matching parent or interface method was found.
    }
}
```

* **Zero Runtime Overhead:** Attribute checks happen entirely during semantic type checking. The compiled bytecode contains no attribute reflection bloat.
* **Namespace Flexibility:** Both `#[\Override]` (fully-qualified) and `#[Override]` (imported/unqualified) are recognized.

### 4. Trait Conflict Resolution (`insteadof` & `as`)

Unlike runtime languages that rely on ambiguous method resolution orders (MRO), RHP enforces explicit trait composition:

```php
trait LogA {
    public function dump(): string { return "A"; }
}

trait LogB {
    public function dump(): string { return "B"; }
}

class Service {
    use LogA, LogB {
        LogA::dump insteadof LogB;
        LogB::dump as dumpFromB;
    }
}
```

* **Compile-Time Conflict Detection:** If two traits provide the same method and no resolution is specified, RHP refuses to compile with `TraitMethodCollision`.
* **Explicit Resolution:** Conflicts are cleanly resolved via standard PHP `insteadof`, aliased via `as`, or overridden by declaring the method directly on the class.

### 5. 100% PHP Syntax & Identifier Compatibility

RHP does not reserve non-standard keywords like `open` or `sealed`. Words such as `open`, `sealed`, and `override` remain valid identifiers in user-land code:

```php
class FileManager {
    public function open(): bool { return true; }
    public function sealed(): bool { return false; }
    public function override(): void {}
}
```

---

## Anonymous Classes

RHP fully supports anonymous classes via the `new class` expression syntax. Anonymous classes are ideal for lightweight, one-off objects such as mock services in unit tests, local strategy objects, disposable observers, and custom implementations of interfaces or abstract classes without creating a separate named file.

### Basic Syntax

An anonymous class is defined and instantiated in a single expression:

```php
$greeter = new class {
    public function greet(string $name): string {
        return "Hello, " . $name . "!";
    }
};

print($greeter->greet("World")); // "Hello, World!"
```

### Constructor Arguments & Properties

Anonymous classes can accept constructor arguments directly inside parentheses following the `class` keyword:

```php
$user = new class("Alice", 30) {
    public string $name;
    public int $age;

    public function __construct(string $name, int $age) {
        $this->name = $name;
        $this->age = $age;
    }

    public function describe(): string {
        return $this->name . " is " . string($this->age);
    }
};

print($user->describe()); // "Alice is 30"
```

### Inheritance (`extends`) and Interfaces (`implements`)

Anonymous classes participate fully in RHP's nominal type system. They can extend parent classes, implement interfaces, and satisfy `instanceof` checks:

```php
interface LoggerInterface {
    public function log(string $msg): void;
}

class BaseService {
    public function status(): string {
        return "online";
    }
}

$mockService = new class extends BaseService implements LoggerInterface {
    public function log(string $msg): void {
        Log::info("[Mock] " . $msg);
    }
};

$isBase = $mockService instanceof BaseService;       // true
$isLogger = $mockService instanceof LoggerInterface; // true
```

### Parent Constructor Forwarding

When extending a parent class, constructor arguments passed to `new class(...)` are forwarded to the parent constructor if the anonymous class does not declare its own `__construct`:

```php
class Vehicle {
    public string $type;
    public function __construct(string $type) {
        $this->type = $type;
    }
}

$car = new class("Sedan") extends Vehicle {
    public function getType(): string {
        return $this->type;
    }
};

print($car->getType()); // "Sedan"
```

### Direct Chained Invocations

You can instantiate and call an anonymous class method directly in an expression by wrapping it in parentheses:

```php
$result = (new class {
    public function calculate(): int {
        return 21 * 2;
    }
})->calculate(); // 42
```

### Composing Traits

Anonymous classes can incorporate traits using standard `use` statements:

```php
trait Formatter {
    public function formatText(string $raw): string {
        return trim(strtoupper($raw));
    }
}

$service = new class {
    use Formatter;

    public function run(string $input): string {
        return $this->formatText($input);
    }
};
```

### Readonly Anonymous Classes

RHP supports the `readonly` modifier on anonymous classes:

```php
$config = new readonly class("https://api.example.com", 30) {
    public string $endpoint;
    public int $timeout;

    public function __construct(string $endpoint, int $timeout) {
        $this->endpoint = $endpoint;
        $this->timeout = $timeout;
    }
};
```

### Internal Class Naming & Determinism

Under the hood, RHP synthesizes deterministic internal class names based on source file IDs and source byte offsets (e.g. `class@anonymous$0_142`). Calling `get_class($anon)` or using RHP's reflection returns this identifier. Because names are deterministically generated at compile time, anonymous class instances remain consistent across RHP's multi-process worker pool (`Arc<EngineState>`).

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

## Object Iteration: `Traversable`, `IteratorAggregate`, & `Iterator`

RHP supports standard object iteration interfaces, enabling custom collections and data domain objects to be looped over via `foreach`:

### 1. `IteratorAggregate`
The simplest and most efficient way to make a class iterable is by implementing `IteratorAggregate` and returning a `Traversable` (such as `ArrayIterator`):

```php
class OrderRepository implements IteratorAggregate {
    private array $orders;

    public function __construct(array $orders) {
        $this->orders = $orders;
    }

    public function getIterator(): Traversable {
        return new ArrayIterator($this->orders);
    }
}

$repo = new OrderRepository(['ORD-101', 'ORD-102', 'ORD-103']);
foreach ($repo as $order) {
    print("Processing order: " . $order);
}
```

> **Zero-Cost Vector Streaming:**
> When `getIterator()` returns an `ArrayIterator` wrapping a native `list` or `dict`, the RHP runtime directly unwraps the inner vector into an `ActiveIterator::List` or `ActiveIterator::Dict`. This streams elements with zero method call overhead, matching native Rust array iteration speeds.

### 2. Step-by-Step `Iterator`
For on-demand streaming, database pagination cursors, or generator patterns, classes can implement `Iterator` with granular step methods:

```php
class PagedCursor implements Iterator {
    private int $position = 0;
    private array $buffer = [];

    public function __construct(private int $totalRecords) {}

    public function rewind(): void {
        $this->position = 0;
    }

    public function valid(): bool {
        return $this->position < $this->totalRecords;
    }

    public function current(): mixed {
        return "Record #" . (string)$this->position;
    }

    public function key(): mixed {
        return $this->position;
    }

    public function next(): void {
        $this->position++;
    }
}
```

### 3. Nominal Type Hierarchy & `iterable` Contracts
RHP pre-seeds the standard PHP nominal hierarchy:
* Both `IteratorAggregate` and `Iterator` extend `Traversable`.
* Objects implementing either interface satisfy `instanceof Traversable` and the unified `iterable` parameter and return type hint.
* Built-in Laravel structures like `LengthAwarePaginator` and `Collection` (via `collect()`) implement `IteratorAggregate` out of the box.

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
Available on **all** enums (both pure and backed). Returns a packed sequential list (`list`) containing all declared cases in declaration order:

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

## Persistent Parent Process: `shared class`

One of RHP's signature innovations is the `shared class` (declared with the `shared` keyword or the `#[Shared]` attribute). Shared classes are instantiated in the persistent parent process during application boot and remain safely shared across all worker threads via `Arc<EngineState>`:

```php
shared class DatabasePool {
    public readonly string $driver;
    public readonly int $maxConnections;

    public function __construct(string $driver = "pgsql", int $maxConnections = 50) {
        $this->driver = $driver;
        $this->maxConnections = $maxConnections;
    }

    public function getConnectionString(): string {
        return "{$this->driver}://localhost:5432/app";
    }

    public static function version(): string {
        return "1.0.0";
    }
}
```

### Why `shared` instead of `singleton`?

* **Expresses Request Boundaries, Not Instance Count:** "Singleton" describes instance count (exactly one), but the property the compiler enforces is cross-request safety. Nothing in the architecture requires strictly one instance per class; you could have multiple shared config objects, routers, or connection pools of the same class.
* **Eliminates Global-State Baggage:** PHP developers associate "singleton" with `getInstance()` anti-patterns, hidden coupling, and mutable global state. `shared` clarifies that the compiler enforces strict boundaries across request cycles.
* **Natural Composition with Modifiers:** Much like `readonly` means a property cannot mutate, `shared` signifies that an object crosses request boundaries and is visible to multiple worker threads.

---

### Instance Methods Are First-Class & Fully Accessible

In RHP, **instance methods on shared classes can be invoked from anywhere** (controllers, middleware, service classes, and background jobs):

```php
class OrderController {
    public function handle(DatabasePool $pool, Router $router): Response {
        // Calling instance methods on shared services is completely valid and encouraged!
        $conn = $pool->getConnectionString();
        $target = $router->dispatch("/checkout");
        return new Response(200, "OK");
    }
}
```

> [!NOTE]
> In traditional PHP runtimes like Laravel Octane, shared services frequently suffer from state-leak bugs because methods mutate `$this`. In RHP, instance methods are safe because **deep immutability is enforced at compile time**.

---

### Unconditional Instantiation Rule

Direct instantiation of a shared class with `new` is **unconditionally forbidden** in user code:

```php
$pool = new DatabasePool();
// Fatal Error: Cannot instantiate shared class 'DatabasePool'. Shared instances are created by the container at boot.
```

**Why Unconditional?**
1. **Concurrency Primitives Safety:** Shared classes frequently manage stateful concurrency primitives such as `Atomic<T>` or `Pool<T>`. Instantiating a second copy bypasses the central pool or atomic counter, producing silent concurrency and logic bugs.
2. **Explicit Lifecycle:** Shared instances belong to the application lifecycle and must be provisioned and registered by the dependency injection container during the boot phase before worker requests are dispatched.

---

### Inheritance Boundaries

To prevent mutable state leaking across request boundaries, RHP enforces strict bidirectional inheritance rules:

1. **Non-Shared Classes Cannot Extend Shared Classes (`RHP0115`):**
   ```php
   shared class SecureVault {}

   // Compile Error: Cannot extend shared class 'SecureVault' from non-shared class 'CustomVault'.
   class CustomVault extends SecureVault {}
   ```
   Without this restriction, a mutable subclass could be substituted where a shared type is expected, violating immutability guarantees.

2. **Shared Classes Extending Non-Shared Classes Enforce Readonly Hierarchy (`RHP0114`):**
   ```php
   class BaseRepository {
       public string $table = "users"; // Mutable!
   }

   // Compile Error: Shared class 'UserRepository' cannot inherit mutable property '$table' from non-shared class 'BaseRepository'. All inherited properties must be readonly.
   shared class UserRepository extends BaseRepository {}
   ```
   When a shared class extends a non-shared parent, the compiler walks the entire inheritance chain and verifies that every inherited instance property is `readonly`.

---

### Preventing Octane State Leak Bugs (Deep Immutability Matrix)

Laravel Octane and long-running PHP runtimes are infamous for subtle request-pollution bugs where state leaks between consecutive requests. RHP statically eliminates this entire class of bugs at compile time:

| Octane Failure Mode | How It Leaks State | RHP Compile-Time Prevention | Error Code |
| :--- | :--- | :--- | :--- |
| **Constructor Request Injection** | Shared service captures `$request` or `$container` in a field | Constructors of shared classes cannot accept request-scoped services (`Request`, `Response`, `Container`, `Application`, `Session`, `WorkerRequest`) | `RHP0114` |
| **Non-Static Closure Fields** | Closure captures request variables or outer scope via `use ($req)` | Properties in shared classes cannot be initialized or assigned non-static closures; only `static fn()` is allowed | `RHP0114` |
| **Mutable Instance Properties** | Service mutates `$this->cachedUser = $user` across requests | All instance properties declared on a shared class must be `readonly` | `RHP0114` |
| **Mutable Static Properties** | Static variables or memoization caches bleed across requests | Static properties on shared classes must be `readonly` or concurrency-safe primitives (`Atomic`, `Pool`, `Lock`, `Channel`) | `RHP0114` |

---

### Concurrency & Runtime Memory Model: RHP vs. Swoole

| Feature | Swoole / OpenSwoole | RHP (Rust-Powered PHP) |
| :--- | :--- | :--- |
| **Execution Architecture** | Multi-process fork model with event loops | Persistent single parent process + multi-threaded Tokio/Rust worker pool |
| **Cross-Worker Memory** | Linux IPC, shared memory segments, `Swoole\Table` | Shared heap via `Arc<EngineState>` with zero-copy reads across threads |
| **Copy-on-Write (CoW)** | Memory pages shared via OS fork; any write triggers page faults and memory duplication | Compile-time deep immutability guarantees true zero-copy multi-threading without page duplication |
| **Atomic Primitives** | `Swoole\Atomic` (POSIX mutex / C atomic shm) | Hardware-native Rust atomics (`Atomic<int>`, `Atomic<bool>`, `AtomicInt`) |
| **Synchronization** | `Swoole\Lock`, `Swoole\Channel` | Zero-overhead Rust channels (`Channel<T>`), locks (`Lock`), and connection pools (`Pool<T>`) |
| **Developer Discipline** | Manual developer hygiene required to reset singletons in `Octane::tick()` | **Compiler-enforced**: Illegal mutations and request leaks fail compilation before execution |

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
list $nums = [1, 2, 3];
$csv = $nums->toCsv();
```

### Why Macros Work Perfectly for RHP

* **Zero Performance Cost on Core Types:** Macros reside in a global thread-safe runtime registry rather than modifying core class definitions or Rust struct layouts.
* **Maintains Familiarity:** Matches the exact developer ergonomics of Laravel (`Macroable`), making migration painless for teams upgrading performance-critical applications.
* **Thread-Safe Registration:** Macros can be registered once during application bootstrap (`index.rhp` boot phase) in the parent process, making them safely accessible across all concurrent worker processes with zero mutex contention on request paths.

---

## Method Dispatch & Dynamic Features (The 4-Tier Pipeline)

RHP bridges the gap between compiled native performance (Rust / Cranelift JIT) and expressive PHP/Laravel ergonomics through a **4-tier method dispatch pipeline**. This architecture ensures that 95%+ of typical business logic runs at direct CPU speed, while dynamic features (such as Eloquent scopes, `::class` dependency injection, and magic forwarding) remain fully supported.

```text
┌────────────────────────────────────────────────────────────────────────┐
│                      THE 4-TIER DISPATCH PIPELINE                      │
├────────┬──────────────────────────────────┬────────────────────────────┤
│ Tier 0 │ Direct Specialized Calls         │ User::create(), $this->x() │
│ Tier 1 │ Devirtualized Direct Calls (CHA) │ Single-implementation call │
│ Tier 2 │ Dense Slot-Indexed VTables       │ $animal->speak()           │
│ Tier 3 │ Escaped Classes & Magic Fallback │ Foo::class, __call, DI     │
└────────┴──────────────────────────────────┴────────────────────────────┘
```

### Tier 0: Direct Specialized Calls
When a static method or instance call target is 100% known at compile time (such as `User::create(...)` or private method calls on `$this`), RHP compiles it directly to machine code (`OpCode::CallFunction` or direct Cranelift call). There is zero hash map lookup, zero runtime resolution loop, and the function call is eligible for native LLVM/Cranelift inlining.

### Tier 1: Devirtualization via Class Hierarchy Analysis (CHA)
In traditional dynamic languages, calling `$model->save()` forces the runtime to dynamically resolve the method name via hash tables. In RHP:
1. During link time, RHP performs **Whole-Program Class Hierarchy Analysis (CHA)**.
2. If `BaseEntity::save()` is declared on a base class and no subclass overrides it, RHP detects that only a single implementation exists across the entire project.
3. The call is automatically **devirtualized** into a direct function call with zero vtable overhead.

### Tier 2: Polymorphic Calls via Slot-Indexed VTables
When an abstract method or interface method has multiple overriding subclasses:
```php
abstract class Animal {
    public abstract function speak(): string;
}

class Dog extends Animal {
    public function speak(): string { return "Woof"; }
}

class Cat extends Animal {
    public function speak(): string { return "Meow"; }
}

function makeItSpeak(Animal $a): string {
    return $a->speak();
}
```
1. At compile/link time, `Animal::speak` is assigned a deterministic integer slot index in the class table (e.g. `Slot #2`).
2. Both `Dog` and `Cat` populate `vtable[2]` with their respective compiled function pointers.
3. At runtime, calling `$a->speak()` executes an $O(1)$ array index lookup (`obj.vtable[2]`), matching the speed of C++ virtual method calls and Rust `dyn Trait` vtables.

### Tier 3: Escaped Classes (`Foo::class`) & Dependency Injection

Laravel relies extensively on passing class names as values for routing, events, and dependency injection:

```php
// Route registration with class reference
Route::get('/users', [UserController::class, 'index']);

// Direct resolution from the application container
$repo = app(UserRepository::class);

// Dynamic make via container instance
$app = app();
$service = $app->make(PaymentService::class);
```

In RHP:
1. **Compile-Time Constant Resolution:** When the compiler encounters `UserRepository::class`, it resolves the class name at compile time and emits a raw string constant (`OpCode::LoadConst`). There is zero runtime reflection or opcode overhead for `::class` expressions.
2. **Escape Analysis:** The compiler marks classes whose `::class` is taken as **escaped**. This guarantees that Dead Code Elimination (DCE) preserves their public constructors and methods.
3. **Zero-Reflection Container Resolution:** The built-in IoC container (`app()`) uses pre-compiled class descriptors to instantiate services instantly with constructor dependency resolution.

### Magic Methods & Dynamic Scopes (`__call` and `__callStatic`)

For expressive patterns like Eloquent dynamic scopes or query builder forwarding, RHP provides dedicated fast-fallback slots:

```php
class EloquentModel {
    public function __call(string $name, array $args): mixed {
        // Automatically invoked when calling undefined instance methods
        return "dynamic_call:" . $name . ":" . implode(",", $args);
    }

    public static function __callStatic(string $name, array $args): mixed {
        // Automatically invoked when calling undefined static methods
        return "static_call:" . $name;
    }
}

$model = new EloquentModel();

// Calls EloquentModel::__call("scopeActive", ["admin", "verified"])
$result = $model->scopeActive("admin", "verified");
```

* **Slot Caching:** `__call` and `__callStatic` are cached in dedicated slots on the class table (`call_slot` and `call_static_slot`).
* **Fast-Path Fallback:** When a method name is not present in the class vtable, the VM immediately invokes `__call` without traversing deep class hierarchies, giving Eloquent models and Macroable classes high-throughput performance.


