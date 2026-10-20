---
title: "Variables and Types"
description: "Explore RHP's gradual type system, scalar primitives, native collection subtypes, and Copy-On-Write memory semantics."
section: "Language Guide"
order: 1
---

## Type System Strategy: Medium (Gradual Contracts)

RHP implements a **medium-strategy gradual type system**:

1. **Strict Signature Boundaries:** Function, method, and constructor signatures enforce explicit parameter and return types to ensure API predictability, compile-time safety, and IDE auto-completion.
2. **Dynamic Internal Inference:** Variable assignments inside function bodies are dynamically inferred and validated by the compiler, maintaining rapid developer ergonomics.
3. **No Implicit Type-Shifting:** Once a variable is declared with an explicit type signature (such as `int $x = 0;`), assigning an incompatible type throws a compile-time or runtime `VariableTypeMismatch` error.

---

## Scalar Primitives

All primitive type keywords are strictly lowercase:

| Type Keyword | Description | Underlying Rust Structure |
| :--- | :--- | :--- |
| `int` | Signed 32-bit integer | `i32` |
| `float` | Double precision floating-point | `f64` |
| `string` | Binary-safe UTF-8 byte buffer with 23-byte SSO | `RhpString` (`Vec<u8>`) |
| `bool` | Boolean value (`true` or `false`) | `bool` |
| `void` | Absence of a return value | Unit `()` |
| `mixed` | Permissive dynamic container | `PhpValue` |

Capitalized scalar variants (such as `String` or `Integer`) are strictly rejected by the parser.

```php
int $count = 42;
float $pi = 3.14159;
string $appName = "RHP Engine";
bool $isProduction = true;

// Attempting incompatible reassignment fails:
// $count = "forty-two"; // Error: VariableTypeMismatch
```

---

## Native Rust Numeric Types & Fluent APIs

For low-level system operations, cryptography, and large data structures, RHP provides fluent static wrappers for native Rust numeric types:

```php
// Construct and chain 64-bit integer calculations
$largeNumber = Int::i64(9223372036854775807);

// Decimal formatting
$formatted = Float::f64(1234567.89)->toDecimal(2);
```

---

## Explicit Subtyped Collections (`Arr`)

Instead of standard, ambiguous hash tables for every list or map, RHP provides four specialized collection subtypes backed directly by Rust heap structures:

### 1. `Arr::list` (Sequential Lists)
Optimized for sequential numerical indices and maximum CPU cache locality. Backed by `Box<Vec<PhpValue>>`.

```php
list $numbers = Arr::list([10, 20, 30, 40])
    ->map(fn($n) => $n * 2)
    ->filter(fn($n) => $n > 30);

print($numbers[0]); // 40
```

### 2. `Arr::dict` (Insertion-Ordered Maps)
String-keyed map retaining deterministic insertion order and $O(1)$ lookups. Backed by `Box<IndexMap<String, PhpValue>>`.

```php
dict $user = Arr::dict([
    'id' => 1,
    'name' => 'Alice',
    'role' => 'admin'
]);

print($user->get('name')); // 'Alice'
```

### 3. `Arr::set` (Unique Sets)
Unique collections mapped directly to `HashSet<PhpValue>` with $O(1)$ membership checking and set algebra:

```php
set $tags = Arr::set(['fast', 'rust', 'php', 'rust']);
$tags->add('backend');

// Set algebra
$diff = $tags->diff(Arr::set(['php']));
```

### 4. `Arr::hashMap` (Universal Complex Maps)
Universal key-value map mapped to `HashMap<PhpValue, PhpValue>`, allowing arbitrary complex keys such as arrays or object instances:

```php
$geoIndex = Arr::hashMap();
$geoIndex[[40.7128, -74.0060]] = 'New York City';
$geoIndex[[51.5074, -0.1278]]  = 'London';

print($geoIndex[[40.7128, -74.0060]]); // 'New York City'
```

---

## Copy-On-Write (COW) Semantics

To prevent unexpected side effects and eliminate unnecessary heap clones, collections and strings in RHP use Copy-On-Write (COW) semantics:

```php
list $original = Arr::list([1, 2, 3]);

// Shares the underlying buffer with zero memory copy
list $copy = $original;

// Modifying $copy detaches it into a new buffer transparently
$copy[] = 4;

print($original->count()); // 3 (untouched)
print($copy->count());     // 4
```
