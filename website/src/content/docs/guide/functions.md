---
title: "Functions and Closures"
description: "Define named functions, short arrow closures, typed parameters, and learn about reference restrictions in RHP."
section: "Language Guide"
order: 4
---

## Named Functions

Functions require explicit type signatures on parameters and return values:

```php
function calculateDiscount(float $subtotal, float $discountRate = 0.10): float {
    return $subtotal - ($subtotal * $discountRate);
}

float $finalPrice = calculateDiscount(100.0, 0.15);
print("Final price: " + string($finalPrice));
```

### Parameter Ordering: Required Parameters Must Precede Optional Parameters

In RHP, all **required parameters** (parameters without default values) must strictly precede **optional parameters** (parameters with default values) across all function signatures, class/trait/interface/enum methods, closures, and arrow functions:

```php
// Valid: Required parameters precede optional parameters
function query(string $sql, int $limit = 10, int $offset = 0): list {
    // ...
}

// Invalid: Optional parameter before required parameter
// function query(int $limit = 10, string $sql): list { ... }
// Compile Error (RHP0230): Required parameter '$sql' follows optional parameter '$limit'
```

If an existing codebase declares optional parameters before required ones, running the RHP linter (`rhp run lint`) will automatically reorder the parameter signatures so that required parameters appear first without changing parameter types or comments.

### No Named Parameters & Skipping Optional Parameters (`fn($req, , $opt)`)

RHP **deliberately does not support named arguments** (e.g. `query(sql: "SELECT ...")` is rejected with compile diagnostic `RHP0206`). Call sites strictly use positional arguments to ensure high-performance execution, eliminate runtime argument-dictionary binding overhead, and prevent brittle coupling to internal parameter variable names.

To skip optional parameters at call time while retaining their default values, provide an **extra comma** at the call site:

```php
function sendNotification(
    string $userId,
    string $channel = 'email',
    string $priority = 'normal',
    bool $retry = true
): bool {
    // ...
}

// 1. Standard call providing all arguments:
sendNotification('u123', 'sms', 'urgent', false);

// 2. Skip $channel (uses default 'email') and $priority (uses default 'normal'), while setting $retry = false:
sendNotification('u123', , , false);

// 3. Skip $channel (uses default 'email'), but specify $priority as 'high':
sendNotification('u123', , 'high');
```

> [!IMPORTANT]
> You **cannot** skip required parameters. Attempting to skip a parameter without a default value (e.g., `sendNotification(, 'sms')`) triggers compile-time and runtime error `RHP0231` (`Cannot skip required parameter '$userId'`).

### Parameter & Return Type Hints: `array` vs. `list`, `dict`, `set`, `hashmap`

RHP supports both the permissive `array` parent type and strict collection subtypes in function signatures:

```php
// 1. Permissive parent return type: accepts any collection variant
function myfuncWhichreturnsArray(): array {
    return [1, 2, 3]; // or return ['name' => 'yogesh'];
}

// 2. Permissive array parameter: accepts List, Dict, Set, HashMap, or standard Array
function countElements(array $items): int {
    return $items->count();
}

$c1 = countElements([1, 2, 3]);                  // Standard array
$c2 = countElements(['a' => 1, 'b' => 2]);       // Dictionary
list $typedList = [10, 20, 30];
$c3 = countElements($typedList);                 // Subtyped List
set $typedSet = ['a', 'b'];
$c4 = countElements($typedSet);                  // Subtyped Set

// 3. Strict collection subtypes: enforce precise memory boundaries
function process_items(list $my_arr, dict $my_dict) {
    list $my_new_scoped_arr = [];
    dict $my_new_scoped_dict = [];
    
    // Internal operations with cache-locality
    $my_new_scoped_arr[] = 100;
    $my_new_scoped_dict['status'] = 'processed';
}

function processUsers(list $userIds, dict $options): list {
    return $userIds->map(fn($id) => $id * 2);
}

// Incompatible arguments throw compile-time & runtime type errors:
// process_items('not a list', []); 
// Error: Argument #1 ('$my_arr') has incompatible type: expected 'list', found 'string'
```

* **Compile-Time Verification:** RHP's two-pass type checker validates arguments against declared parameter signatures.
* **Runtime Guard (`OpCode::AssertType`):** The bytecode compiler injects `OpCode::AssertType` boundary assertions into the function prologue, validating the underlying collection subtype before executing the function body.
* **Return Types Come from the Linter:** `rhp run lint` inserts the return type. A body with no value return becomes `: void`. When every path returns the same collection or scalar, that type is written (`: list`, `: dict`, `: int`). A function that already has a return type is left unchanged. Constructors and destructors are not given one.

---

## Short Arrow Closures (`fn`)

Arrow functions provide concise syntax for higher-order collection transformations. Variables from the enclosing lexical scope are captured by value:

```php
float $taxRate = 0.08;

list $prices = [10.0, 20.0, 30.0];
list $taxed = $prices->map(fn(float $price) => $price * (1 + $taxRate));
```

---

## Anonymous Functions & Automatic Lexical Scope Capture of `$this`

Non-static anonymous functions (`function(...) { ... }`) created within instance methods automatically capture `$this` from their enclosing lexical scope without manual binding:

```php
class OrderService {
    private TaxCalculator $tax;

    public function processOrders(List $orders): List {
        // Automatically captures $this from the enclosing method scope
        return $orders->map(function(Order $order): ProcessedOrder {
            return $this->tax->calculate($order);
        });
    }
}
```

- **Zero Manual Binding:** Access `$this->prop` or `$this->method()` directly without calling `->bindTo()` or passing `$this`.
- **Lexical Scope Propagation:** Non-static closures capture `$this` transitively across nested closure boundaries.
- **Static Closures (`static function`):** Mark closures with `static` (`static function(...) { ... }`) when `$this` should not be captured or to avoid strong cyclic captures into properties.

---

## Global Function Lowering (`Glob::helperFunctions`)

RHP eliminates PHP's global namespace performance degradation. Global calls are statically resolved and compiled at parse-time:

- **System Intrinsics (`Sys::*`):** Standard intrinsics like `print()`, `env()`, and `dd()` are lowered to `Sys::print()`, `Sys::env()`, and `Sys::dd()`.
- **PHP Global Functions (`Glob::*`):** Standard and custom procedural helper functions are compiled directly to `Glob::helperFunctions` (such as `Glob::customHelper()`).
- **User Functions:** Functions declared via `function name(...)` in your application are linked directly without global symbol collisions.

---

## Pass-by-Reference Semantics (`&$var`)

RHP supports explicit pass-by-reference for function and method parameters while ensuring memory safety and thread isolation:

1. **Explicit Parameter Declaration:** Declare reference parameters using `&$name`:
   ```php
   function addTen(int &$val): void {
       $val = $val + 10;
   }
   
   function appendItem(array &$list, int $item): void {
       $list[] = $item;
   }
   ```
2. **Explicit Call-Site Ampersand Required:** To prevent subtle bugs and hidden side effects, callers **must** explicitly pass variables with `&` at the call site:
   ```php
   $num = 25;
   addTen(&$num); // $num is now 35
   
   $myList = [10, 20];
   appendItem(&$myList, 30); // $myList is now [10, 20, 30]
   
   // Passing without '&' or passing literals fails at compile time:
   // addTen($num); // Compile Error: Missing reference argument '&'
   // addTen(&99);  // Compile Error: Reference argument must be a variable
   ```
3. **Automated Linter Upgrades:** The `rhp run lint` tool automatically inspects function bodies: if an array parameter (`array`, `list`, `dict`, etc.) is mutated, the linter automatically modifies the parameter signature to `&$param` and synchronizes callers in the same file. See [Linter and Static Typing](/docs/tooling/linter#automated-copy-by-reference-for-mutated-arrays).
4. **Closure Capture Protection:** Closure capture-by-reference (`use (&$var)`) remains prohibited to prevent hidden cyclic memory graphs that would violate zero-GC memory safety.

---

## Callable Type Contract & Invokable Objects

The `callable` type hint accepts closures, arrow functions, global function names (`string`), and invokable class instances implementing `__invoke()`:

```php
function dispatch(callable $action, mixed $payload): mixed {
    return $action($payload);
}

// 1. Arrow closure
dispatch(fn($d) => print("Handled: " + $d), "event-1");

// 2. Invokable Object ($action($params))
class CreateSiteAction {
    public function __invoke(array $data): string {
        return "Created site: " + $data['domain'];
    }
}

$action = new CreateSiteAction();
$res = dispatch($action, ['domain' => 'rhp.dev']);
```

For detailed class-level invokable patterns (such as Laravel single-action controllers), see [Classes, Objects & Enums](/docs/guide/classes-and-objects#invokable-objects-__invoke).

