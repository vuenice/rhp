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
print("Final price: " . (string)$finalPrice);
```

### Parameter & Return Type Hints: `array` vs. `list`, `dict`, `set`, `hashmap`

RHP supports both the permissive `array` parent type and strict collection subtypes in function signatures:

```php
// 1. Permissive parent return type: accepts any collection variant
function myfuncWhichreturnsArray(): array {
    return [1, 2, 3]; // or return ['name' => 'yogesh']; or return Arr::list(...);
}

// 2. Permissive array parameter: accepts List, Dict, Set, HashMap, or standard Array
function countElements(array $items): int {
    return $items->count();
}

$c1 = countElements([1, 2, 3]);                  // Standard array
$c2 = countElements(['a' => 1, 'b' => 2]);       // Dictionary
$c3 = countElements(Arr::list([10, 20, 30]));    // Subtyped List
$c4 = countElements(Arr::set(['a', 'b']));       // Subtyped Set

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
// process_items('not a list', Arr::dict([])); 
// Error: Argument #1 ('$my_arr') has incompatible type: expected 'list', found 'string'
```

* **Compile-Time Verification:** RHP's two-pass type checker validates arguments against declared parameter signatures.
* **Runtime Guard (`OpCode::AssertType`):** The bytecode compiler injects `OpCode::AssertType` boundary assertions into the function prologue, validating the underlying collection subtype before executing the function body.
* **Return Type Inference for Void Functions:** When a function omits an explicit return type and contains no value returns (`return <expr>;`), the type checker automatically infers `void`. Functions that return a value must specify an explicit return type hint.

---

## Short Arrow Closures (`fn`)

Arrow functions provide concise syntax for higher-order collection transformations. Variables from the enclosing lexical scope are captured by value:

```php
float $taxRate = 0.08;

list $prices = Arr::list([10.0, 20.0, 30.0]);
list $taxed = $prices->map(fn(float $price) => $price * (1 + $taxRate));
```

---

## Global Function Lowering (`Glob::helperFunctions`)

RHP eliminates PHP's global namespace performance degradation. Global calls are statically resolved and compiled at parse-time:

- **System Intrinsics (`Sys::*`):** Standard intrinsics like `print()`, `env()`, and `dd()` are lowered to `Sys::print()`, `Sys::env()`, and `Sys::dd()`.
- **PHP Global Functions (`Glob::*`):** Standard and custom procedural helper functions are compiled directly to `Glob::helperFunctions` (such as `Glob::customHelper()`).
- **User Functions:** Functions declared via `function name(...)` in your application are linked directly without global symbol collisions.

---

## Reference Restrictions: Pass-By-Reference Forbidden

To ensure absolute memory safety and eliminate cyclic reference leaks across multi-threaded workers, **pass-by-reference (`&$var`) is strictly forbidden** at the parser and type-checker levels:

```php
// COMPILE ERROR: Pass-by-reference is forbidden
// function modifyValue(&$value) { ... }

// IDIOMATIC RHP: Return explicit values or wrap in mutable collections
function updateCounter(int $value): int {
    return $value + 1;
}

int $counter = 0;
$counter = updateCounter($counter);
```

Similarly, closure capture-by-reference (`use (&$var)`) is rejected to ensure closures cannot construct hidden circular memory graphs that would require a tracing garbage collector.

---

## Callable Type Contract & Invokable Objects

The `callable` type hint accepts closures, arrow functions, global function names (`string`), and invokable class instances implementing `__invoke()`:

```php
function dispatch(callable $action, mixed $payload): mixed {
    return $action($payload);
}

// 1. Arrow closure
dispatch(fn($d) => print("Handled: " . $d), "event-1");

// 2. Invokable Object ($action($params))
class CreateSiteAction {
    public function __invoke(array $data): string {
        return "Created site: " . $data['domain'];
    }
}

$action = new CreateSiteAction();
$res = dispatch($action, ['domain' => 'rhp.dev']);
```

For detailed class-level invokable patterns (such as Laravel single-action controllers), see [Classes, Objects & Enums](/docs/guide/classes-and-objects#invokable-objects-__invoke).

