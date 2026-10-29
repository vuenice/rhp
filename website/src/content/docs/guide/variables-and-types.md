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

| Type Keyword | Description | Default Rust Storage |
| :--- | :--- | :--- |
| `int` | Signed integer — **defaults to `i32`** | `i32` |
| `float` | Floating-point number — **defaults to `i32`** (32-bit float) | `i32` |
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

### Binary-Safe Strings & Multi-Byte Encodings

In RHP, strings are fundamentally raw byte buffers (`RhpString` storing $\le 23$ bytes inline on the stack via Small-String-Optimization and larger buffers via `Rc<Vec<u8>>`):
* **Binary Safety:** Strings can hold arbitrary binary data, image headers, network payloads, or null bytes (`\0`) without triggering Rust UTF-8 panics.
* **Legacy Japanese Support:** Legacy multi-byte character sets such as **Shift-JIS (SJIS / CP932)**, **EUC-JP**, and **ISO-2022-JP (JIS)** are preserved as raw bytes. Developers use `Str::fromEncoding($raw, 'Shift-JIS')` or `Str::utf8($raw)` for safe conversions.
* **Immutable Helpers:** Chained helper methods like `$text->trim()->upper()` are immutable by default, leaving the original variable untouched. Variable updates use explicit reassignment (`$text = $text->trim()->upper();`) with Copy-On-Write (COW) semantics.

### String Quoting Strategies & Escaping

RHP provides versatile quoting strategies to simplify string handling, multi-line formatting, and template safety:

* **Nested Opposing Quotes:** Single quotes can enclose double quotes without escaping (`'Hello "world"'`), and double quotes can enclose single quotes (`"It's working!"`).
* **Escaped Quotes:** Backslash escapes `\'` and `\"` are supported inside single- and double-quoted strings.
* **Raw Backtick Strings:** Enclosing strings in backticks (`` `...` ``) creates raw string literals where escape sequences (like `\n` or `\t`) and quotes are treated as literal characters without interpretation.
* **Line Ending Normalization:** All newline variations (CRLF `\r\n`, LF `\n`, CR `\r`) are tracked by the lexer and normalized cleanly across platforms.

```php
string $single  = 'He said, "Hello"';
string $double  = "It's a great day";
string $escaped = "Line 1\nLine 2 with \"quotes\"";
string $rawPath = `C:\new_folder\test.txt`; // Literal backslashes preserved
```

---

## Variable Declarations (Prefix Type Syntax)

RHP supports statically locked variable declarations using **prefix type notation**:

```php
int $x = 10;
string $name = "Alice";
float $rate = 0.05;
bool $isActive = true;
int $uninitialized; // Uninitialized typed declaration
```

> [!IMPORTANT]
> **No Colon Type Syntax (`$x: int = 10;`):**
> RHP **only** supports prefix type declarations (`int $x = 10;`). TypeScript/Hack-style colon declarations (`$x: int = 10;` or `$x: int;`) are strictly forbidden and trigger a compile-time parse error:
> ```text
> ParseError: Colon type syntax '$x: Type' is not supported. Use prefix type syntax 'Type $x' instead (e.g., 'int $x = 10;').
> ```
> This rule applies uniformly across local variables, class properties (`public int $id = 1;`), and parameter lists (`function calc(int $id)`).

### Variable Identifier Validation Rules

Variable identifiers must strictly adhere to lexical naming rules:
1. **Sigil (`$`) Requirement:** All variables must begin with a leading `$` sigil.
2. **Identifier Character Rules:** The character immediately following `$` must be an ASCII letter (`a-z`, `A-Z`) or underscore (`_`), followed by any combination of letters, digits (`0-9`), or underscores.
3. **Prohibited Patterns:**
   - Bare sigils (`$`) without an identifier name are rejected.
   - Variable-variables (`$$var`, `$$$name`) are strictly prohibited in RHP.
   - Variables starting with digits (`$0`, `$123`) are forbidden.

When an invalid variable name is detected, the lexer produces a clear diagnostic error:
```text
ParseError: Variable names must start with a letter or underscore, followed by alphanumeric characters (e.g., $varName).
```

```php
// ✅ Valid variable identifiers:
int $count = 10;
string $_temp = "cache";
float $data_v2 = 3.14;

// ❌ Invalid variable identifiers (trigger compile-time error):
// $$variable; // Error: Variable names must start with a letter or underscore...
// $0;         // Error: Variable names must start with a letter or underscore...
// $;          // Error: Variable names must start with a letter or underscore...
```

---

## Type Conversion & Casting (Functional Syntax)

Type conversions in RHP use function-style conversion syntax rather than C-style or PHP-style parenthesized casts:

```php
$raw = "42";

// Supported: functional type conversions
$count = int($raw);        // 42 (int)
$text  = string(123);      // "123" (string)
$pi    = float("3.14159"); // 3.14159 (float)
$flag  = bool(1);          // true (bool)
```

> [!WARNING]
> **C-Style / PHP-Style Casts (`(int)$x`) Are Not Supported:**
> Parenthesized type casting like `(int)$x`, `(string)$x`, `(float)$x`, or `(bool)$x` is rejected by the parser with a clear diagnostic:
> ```text
> ParseError: Type cast syntax '(int)' is not supported in RHP. Use functional conversion 'int(...)' instead (e.g. 'int($x)').
> ```
> This guarantees consistent, unambiguous expression parsing and aligns scalar casting with functional conversions.

---

## Default Storage: `int` and `float` are both `i32`

> [!IMPORTANT]
> **Both `int` and `float` default to `i32` (32-bit) storage.**
> The `float` primitive does **not** map to Rust's `f64` by default. It stores a 32-bit integer under the hood just like `int`. Use `Float::f64(...)` or `Float::f32(...)` via the `Float` facade when you explicitly need IEEE-754 precision.

```php
int   $count = 42;       // i32 internally
float $score = 9;        // also i32 internally — NOT f64

// Explicit 64-bit or IEEE float via the Float facade:
$precise = Float::f64(3.141592653589793);  // f64 precision
$big     = Int::i64(9223372036854775807);  // i64 max value
```

---

## `Int` and `Float` — Supporting Facade Classes (NOT Base Types)

`Int` and `Float` are **static facade classes** — they are *not* instances of `int` or `float` and can never be instantiated with `new`. They provide factory methods and fluent utilities for working with extended Rust numeric types.

| Item | Kind | Example |
| :--- | :--- | :--- |
| `int` | Primitive keyword (base datatype) | `int $x = 5;` |
| `float` | Primitive keyword (base datatype) | `float $y = 3;` |
| `Int` | Static facade class (identifier) | `Int::i64(...)` |
| `Float` | Static facade class (identifier) | `Float::f64(...)` |

```php
// ✅ Correct — use as type keywords (primitive base types)
int   $age    = 30;
float $amount = 100;

// ✅ Correct — use as static facade (extended numeric utilities)
$bigId   = Int::i64(9223372036854775807);
$precise = Float::f64(3.141592653589793)->toDecimal(6);
$random  = Int::i64()->random()->add()->subtract();

// ❌ Fatal Error — Int and Float cannot be instantiated
// $obj = new Int();
// => Fatal Error: Cannot instantiate static facade class 'Int'. Use static methods directly.
```

> [!CAUTION]
> Calling `new Int()`, `new Float()`, `new Arr()`, or `new Str()` always produces a **fatal compile-time error**. These are purely static facades.

---

## Extended Rust Numeric Types via Facade Methods

For low-level operations (cryptography, network protocols, large data), use the `Int` and `Float` facades to access native Rust numeric variants with full fluent chaining:

```php
// --- Int facade ---
$small  = Int::i8(127);               // i8  (-128 to 127)
$medium = Int::i32(2_147_483_647);    // i32 (same as plain `int` default)
$large  = Int::i64(9_223_372_036_854_775_807);  // i64
$huge   = Int::i128(170141183460469231731687303715884105727); // i128
$ptr    = Int::usize(1024);           // usize (pointer-width)
$chain  = Int::i64()->random()->add(10)->subtract(3); // fluent

// --- Float facade ---
$single = Float::f32(3.14);           // f32  (same as plain `float` default storage)
$double = Float::f64(3.141592653589793); // f64 full IEEE-754 64-bit
$fmt    = Float::f64(1234567.89)->toDecimal(2); // "1234567.89"
$pct    = Float::f64(0.75)->toPercent();        // "75.00%"
```

---

## Explicit Subtyped Collections (`Arr`)

Instead of standard, ambiguous hash tables for every list or map, RHP provides four specialized collection subtypes backed directly by Rust heap structures:

### 1. `Arr::list` (Sequential Lists)
Optimized for strictly sequential numerical indices and maximum CPU cache locality. Backed by contiguous Rust `Rc<RefCell<Vec<PhpValue>>>` with Copy-On-Write (COW) semantics.

* **CPU Cache Locality & Zero-Allocation Iteration:** During `foreach ($list as $key => $val)` iteration, the runtime uses a dedicated `ActiveIterator::List` stream that points directly to the underlying `Vec<PhpValue>` contiguous memory buffer with an internal cursor index. It streams elements without allocating intermediate key-value pair vectors or cloning keys.
* **Strict Sequential Indexing:** Supports numeric index reading (`$list[0]`), updating (`$list[1] = 999`), and sequential appending (`$list[] = $val`).
* **String Key Rejection:** Attempting to assign non-numeric string keys (such as `$list['name'] = 'val'`) is rejected with an `UndefinedIndex` error, preventing sequential lists from degrading into hash tables.
* **Inline Type Annotations & Implicit Factory Lowering:** Local variable declarations using `list $arr = [];` are compiled directly into optimized Rust-backed `Arr::list([])` allocations under the hood.

```php
// Inline variable type annotation and implicit factory initialization
list $numbers = [10, 20, 30, 40]; // Lowers directly to Arr::list([10, 20, 30, 40])

// Fluent higher-order transformations
list $doubled = $numbers
    ->map(fn($n) => $n * 2)
    ->filter(fn($n) => $n > 30);

print($doubled[0]); // 40

// Sequential operations
$doubled[] = 100;    // Appends 100
$doubled[1] = 999;   // Updates index 1
print($doubled->count()); // 4

// Attempting to assign a non-numeric string key throws an error:
// $doubled['invalid_key'] = 42; // Error: Undefined index or key 'invalid_key'
```

### Native Lowercase Collection Keywords & Inline Declarations

RHP aligns with PHP native conventions by using **strictly lowercase keywords** for collection types: `list`, `dict`, `set`, and `hashmap` (matching `array`, `string`, `int`, and `bool`).

This design provides two major advantages:
1. **PHP Native Alignment:** Avoids confusion with userland classes or external object models. Collection hints feel like first-class language primitives.
2. **Inline Variable Declarations:** Allows developers to declare and initialize statically typed collections in local scopes:
   ```php
   list $my_new_scoped_arr = [];
   dict $my_new_scoped_dict = [];
   ```

#### Compiler & Parser Lowering Strategy
When RHP processes collection types, the lexer, parser, and IR lowerer collaborate through a three-stage lowering pipeline:
* **Token Recognition:** The lexer recognizes `list`, `dict`, `set`, and `hashmap` as dedicated built-in type keywords (`Token::TypeList`, `Token::TypeDict`, etc.), clearly separating them from user-defined class identifiers.
* **Implicit Lowering to Factory Calls:** When the parser encounters local variable declarations or assignments like `list $my_arr = [];`, it compiles them directly into the optimized Rust-backed factory initialization:
  $$\text{list } \$my\_arr = []; \quad \longrightarrow \quad \$my\_arr = \text{Arr::list}([]); \quad \text{(backed by contiguous Vec<PhpValue>)}$$
  $$\text{dict } \$my\_dict = []; \quad \longrightarrow \quad \$my\_dict = \text{Arr::dict}([]); \quad \text{(backed by IndexMap)}$$
  $$\text{set } \$my\_set = []; \quad \longrightarrow \quad \$my\_set = \text{Arr::set}([]); \quad \text{(backed by HashSet)}$$
  $$\text{hashmap } \$my\_map = []; \quad \longrightarrow \quad \$my\_map = \text{Arr::hashMap}([]); \quad \text{(backed by HashMap)}$$
* **Parameter Type Safety:** At function boundaries, type contracts enforce compile-time verification and runtime guards (`OpCode::AssertType`), ensuring only the correct high-performance Rust variant enters the function.


### 2. `Arr::dict` (Insertion-Ordered Maps)
String-keyed map retaining strict deterministic insertion order and amortized $O(1)$ lookups, updates, and removals. Backed directly by Rust's `Rc<RefCell<IndexMap<String, PhpValue>>>`.

* **Strict Insertion Ordering:** Unlike standard unordered hash tables, `Arr::dict` preserves exact key insertion order across all iterations, serializations, and mutations. Updating the value of an existing key preserves its original index slot.
* **Zero-Allocation Streaming Iteration:** `foreach ($dict as $key => $val)` drives an `ActiveIterator::Dict` that indexes directly into the underlying `IndexMap` via an internal cursor, eliminating intermediate vector allocations.
* **Laravel Collection Methods:** `Arr::dict` exposes the full suite of Laravel-style collection instance methods for fluent, functional data pipelines.

```php
dict $product = Arr::dict([
    'sku'      => 'PROD-100',
    'name'     => 'Ergonomic Desk',
    'price'    => 250,
    'discount' => 25,
    'tax'      => 15,
]);

// 1. Fluent Inspection & Access
$name = $product->get('name');                   // 'Ergonomic Desk'
$stock = $product->get('stock', 0);               // Fallback default: 0
$hasSku = $product->has('sku');                   // true
$hasAny = $product->hasAny(['color', 'weight']); // false
list $keys = $product->keys();                   // ['sku', 'name', 'price', 'discount', 'tax']
list $values = $product->values();               // ['PROD-100', 'Ergonomic Desk', 250, 25, 15]

// 2. In-Place Mutations & Detachment (Copy-On-Write)
$product->put('in_stock', true);
$discount = $product->pull('discount', 0);        // 25 (removes 'discount' from $product)
$product->forget('tax');                          // Removes 'tax'

// 3. Higher-Order Transforms (Immutable Pipelines)
dict $prices = Arr::dict(['item1' => 10, 'item2' => 20])
    ->map(fn($price, $key) => $price * 2);        // ['item1' => 20, 'item2' => 40]

dict $filtered = Arr::dict(['a' => 10, 'b' => 5, 'c' => 25])
    ->filter(fn($val, $k) => $val >= 10);         // ['a' => 10, 'c' => 25]

dict $rejected = Arr::dict(['a' => 10, 'b' => 5, 'c' => 25])
    ->reject(fn($val, $k) => $val >= 10);         // ['b' => 5]

int $sum = Arr::dict(['x' => 10, 'y' => 20, 'z' => 30])->sum(); // 60
float $avg = Arr::dict(['x' => 10, 'y' => 20, 'z' => 30])->avg(); // 20.0

// 4. Filtering, Flipping, Merging & JSON Serialization
dict $subset = $product->only(['sku', 'name']);
dict $without = $product->except(['sku']);
dict $flipped = Arr::dict(['role' => 'admin'])->flip(); // ['admin' => 'role']
dict $merged = Arr::dict(['a' => 1])->merge(Arr::dict(['b' => 2, 'a' => 99])); // ['a' => 99, 'b' => 2]
string $json = $product->toJson(); // '{"sku":"PROD-100","name":"Ergonomic Desk",...}'

// 5. Zero-Allocation Streaming Iteration
foreach ($product as $prop => $val) {
    Sys::print("{}: {}", $prop, (string)$val);
}
```

### 3. `Arr::set` (Unique Sets - Step 10)
Unique collection mapped directly to native Rust `HashSet<PhpValue>`. Automatically deduplicates values, provides guaranteed $O(1)$ set membership, and implements mathematical set algebra:

```php
set $tags = Arr::set(['fast', 'rust', 'php', 'rust', 'backend', 'php']);

// 1. Automatic Deduplication
print($tags->count()); // 4 (unique: 'fast', 'rust', 'php', 'backend')

// 2. Element Insertion & Bracket Push Syntax
$tags->add('frontend'); // Appends 'frontend'; returns $tags
$tags[] = 'systems';    // Bracket push syntax (deduplicated automatically)

// 3. O(1) Membership Checking
bool $hasRust = $tags->contains('rust'); // true
bool $hasJava = $tags->has('java');       // false

// Bracket Indexing for Membership
bool $isMember = $tags['rust']; // true (bracket indexing checks set membership)
$tags['go'] = true;             // Inserts 'go' into set
$tags['go'] = false;            // Removes 'go' from set

// 4. Element Removal
bool $removed = $tags->remove('php'); // true (was present)
$tags->delete('nonexistent');         // false (alias for remove())

// 5. Mathematical Set Algebra
$setA = Arr::set([1, 2, 3, 4]);
$setB = Arr::set([3, 4, 5, 6]);

set $diff = $setA->diff($setB);            // Relative complement: {1, 2}
set $intersect = $setA->intersect($setB);  // Intersection: {3, 4}
set $union = $setA->union($setB);          // Set union: {1, 2, 3, 4, 5, 6}
set $diffArr = $setA->diff([2, 4]);        // Cross-collection algebra with plain array

// 6. Higher-Order Transforms & Reductions
set $evens = Arr::set([1, 2, 3, 4, 5, 6])
    ->filter(fn($n) => $n % 2 === 0);      // {2, 4, 6}

set $doubled = $evens->map(fn($n) => $n * 10); // {20, 40, 60}
int $sum = $doubled->reduce(fn($acc, $n) => $acc + $n, 0); // 120

// 7. Streaming Foreach & Export
list $asList = $tags->all();               // Converts to sequential list
string $json = $tags->toJson();            // '["fast","rust",...]'

foreach ($tags as $tag) {
    // Visits each unique element exactly once
}
```

### 4. Collection Architecture Roadmap: `list`, `dict`, and `set`

To guarantee maximum CPU cache density, deterministic ordering, and SIMD/AES-accelerated lookups, RHP concentrates on three optimized collection primitives while deferring arbitrary `hashMap` and `tuple` structures:

* **`list`** $\to$ Contiguous `Rc<Vec<Value>>`: Sequential flat array indexed by integer `0..n`.
* **`dict`** $\to$ `Rc<IndexMap<Key, Value, ahash::RandomState>>`: Insertion-ordered dictionary keyed by strings or atoms, powered by AES-hardware accelerated `ahash`.
* **`set`** $\to$ `Rc<IndexSet<Key, ahash::RandomState>>`: Unique value set preserving insertion order with $O(1)$ set algebra.
* **`Rc<>` & Copy-On-Write (COW):** Collections use reference counting for zero-allocation parameter passing and assignment. Detachments and mutations only allocate or clone when shared (`Rc::strong_count > 1`).

> [!NOTE]
> **Why `tuple` and `hashMap` are on hold:**
> 1. **Zero Complex Key Overhead:** In web applications and APIs, associative keys are almost exclusively strings or integers. Avoiding universal complex keys (arrays or objects as keys) keeps hash routines free of expensive structural hashing and maintains high cache locality.
> 2. **Focused Ergonomics:** `list` and `dict` cover virtually all array use cases in modern PHP and Laravel applications without type confusion or multiple competing dictionary abstractions.

---

## The Permissive `array` Parent Type vs. Explicit Subtypes

RHP achieves the best of both worlds: full backward compatibility for dynamic PHP codebases alongside Rust-grade performance and memory layout optimization.

### 1. Unified Parent Wrapper (`RhpType::Array`)
Under the hood, `RhpType::Array` serves as the **permissive parent datatype** for all collection variants (`list`, `dict`, and `set`):

| Syntax | Compiled Type | Underlying Rust Structure | Type Flexibility |
| :--- | :--- | :--- | :--- |
| `$arr = [];` | `RhpType::Array` | Dynamic `PhpValue::Array` | **Dynamic:** Freely reassignable across list, dict, and set |
| `array $arr = [];` | `RhpType::Array` | Unified collection parent | **Permissive:** Accepts any collection variant interchangeably |
| `list $arr = [];` | `CollectionSubtype::List` | Contiguous `Rc<Vec<Value>>` | **Strict:** Sequential integer-indexed lists only |
| `dict $arr = [];` | `CollectionSubtype::Dict` | `Rc<IndexMap<Key, Value, ahash::RandomState>>` | **Strict:** Insertion-ordered string-keyed dictionaries only |
| `set $arr = [];` | `CollectionSubtype::Set` | `Rc<IndexSet<Key, ahash::RandomState>>` | **Strict:** Unique value sets only |

### 2. Dynamic Typing via `PhpValue` & Flexible Reassignment
Local variables initialized without a strict type constraint (or declared with the permissive `array` type hint) store values dynamically in a unified `PhpValue` enum handle. 

Because they are not rigidly locked to a single data structure type for their entire lifetime, you can easily reassign `$arr` from a sequential list structure to a dictionary or map structure without throwing a type error:

```php
// Dynamic flexibility on untyped variables:
$arr = [1, 2, 3];            // Sequential list
$arr = ['name' => 'yogesh']; // Seamlessly reassigns to dictionary

// Permissive parent type hint for backward compatibility:
array $data = [1, 2, 3];
$data = ['name' => 'yogesh'];           // Allowed
$data = Arr::list([10, 20]);            // Allowed
$data = Arr::dict(['env' => 'prod']);   // Allowed
```

### 3. Strict Type Bounds vs. Dynamic Variables
This dynamic flexibility applies to untyped variables and generic `array`-hinted variables. However, once a variable or parameter is explicitly type-hinted with a **strict collection subtype** (like `list $arr` or `dict $arr`) or a primitive scalar (like `int $x`), RHP enforces strict type boundaries and throws a Rust-style compile-time or runtime error on incompatible assignments:

```php
// 1. Strict List rejects string-keyed dictionaries:
list $numbers = [1, 2, 3];
// $numbers = ['name' => 'yogesh']; 
// Error: VariableTypeMismatch: Cannot assign value of type 'Dict' to typed variable '$numbers' of type 'List'

// 2. Strict Dict rejects sequential integer lists:
dict $user = ['name' => 'yogesh'];
// $user = [1, 2, 3];
// Error: VariableTypeMismatch: Cannot assign value of type 'List' to typed variable '$user' of type 'Dict'

// 3. Strict scalar primitives reject incompatible types:
int $x = 0;
// $x = 'hello'; 
// Error: VariableTypeMismatch: Cannot assign value of type 'string' to typed variable '$x' of type 'int'
```

---

## Immutable Helper Methods by Default

* **Immutability by Default:** Calling array and collection helper methods (such as `$my_array->sort()` or `Arr::list($my_array)->sort()`) **never mutates the original array in place**.
* **Zero Hidden Side Effects:** Every transformation returns a brand-new collection instance, keeping data flows clean, predictable, and thread-safe.

```php
$original = [3, 1, 2];

// $sorted is a new sorted instance; $original remains [3, 1, 2]
$sorted = $original->sort();

print($original[0]); // 3 (untouched!)
print($sorted[0]);   // 1
```

### Explicit Variable Reassignment

If a developer wants to update a variable with the transformed or sorted array, they use **explicit reassignment**:

```php
$my_array = $my_array->sort();
// or
$my_array = Arr::list($my_array)->sort();
```

Under the hood, this leverages RHP's internal **Copy-On-Write (COW)** and pointer mechanics, making variable replacement lightning-fast without unnecessary memory duplication or garbage collection overhead.

---

## Copy-On-Write (COW) Semantics

To prevent unexpected side effects and eliminate unnecessary heap clones, collections and strings in RHP use Copy-On-Write (COW) semantics:

```php
list $original = Arr::list([1, 2, 3]);

// Shares the underlying buffer with zero memory copy (O(1) pointer reference)
list $copy = $original;

// Modifying $copy detaches it into a new buffer transparently
$copy[] = 4;

print($original->count()); // 3 (untouched)
print($copy->count());     // 4
```

---

## Undefined Variables, Missing Keys, and Null Semantics

RHP implements a deliberate hybrid strategy combining Rust-level compile-time guarantees with PHP's developer ergonomics:

### 1. First-Class `null` (No `None` Keyword)

RHP uses **`null`** as a first-class scalar value and type keyword (unlike Rust, which represents absence through `None` of the `Option<T>` enum). RHP does not have a separate `undefined` type or value.

### 2. Core Execution Invariants

* **Undefined Variables $\to$ Compile Error:**
  Reading an undeclared or uninitialized local variable is strictly rejected at compile time:
  ```php
  echo $x; // Compile Error: undefined variable $x
  ```
  Unlike PHP (which emits a runtime warning and evaluates to `null`), RHP ensures all variables are lexically defined before execution.

* **Missing Array / Collection Keys $\to$ Warning Evaluates to `null`:**
  Accessing a missing key or out-of-bounds offset on an array, list, or dictionary emits a runtime warning and evaluates safely to `null`:
  ```php
  $arr = ['a' => 1];
  echo $arr['b']; // Warning: Undefined array key "b" -> evaluates to null
  ```

* **Missing Struct Property $\to$ Structs Not Present in RHP:**
  The `struct` keyword is **not present in RHP**. Structured domain models are built using typed `class` declarations or native collection subtypes like `dict` (`Arr::dict`). On class instances, accessing undeclared properties triggers a compile-time or runtime `UndefinedProperty` error.

* **Uninitialized Fields $\to$ Compile Error (Must Initialize at Creation):**
  Typed class properties cannot be left in an uninitialized state. All properties without default values must be initialized upon object creation (e.g., in `__construct`):
  ```php
  class User {
      public string $name; // typed, no default
  }

  $u = new User(); // Compile Error: field $name of class User must be initialized at creation
  ```
  If a property is allowed to be empty, declare it as explicitly nullable: `public ?string $name = null;`.

### 3. Telling Missing Keys Apart from `null`

Because reading a missing key evaluates to `null`, you cannot distinguish "key does not exist" from "key exists with a `null` value" by value alone. Use explicit existence checks:

```php
$arr = ['a' => null];

isset($arr['a']);            // false (isset treats null as unset, identical to PHP)
array_key_exists('a', $arr); // true  (key exists, value is null)
array_key_exists('b', $arr); // false (key undefined)

// With native RHP dict collections:
dict $d = Arr::dict(['a' => null]);
$d->has('a');                // true  (key exists)
$d->has('b');                // false (key undefined)
$d->get('a');                // null
```

### 4. Safe Access Operators (Work as in PHP)

RHP implements PHP's null-coalescing and nullsafe operators, which cleanly suppress missing-key warnings:

* **Safe Fallback (`??`):**
  ```php
  $v = $arr['b'] ?? 'default'; // Evaluates to 'default' (no warning emitted)
  ```
* **Safe Assign (`??=`):**
  ```php
  $arr['b'] ??= 'default'; // Assigns 'default' only if missing or null
  ```
* **Nullsafe Operator (`?->`):**
  ```php
  $name = $user?->profile?->name; // Evaluates safely to null if $user or $profile is null
  ```

### 5. Strategy Comparison Matrix

| Scenario | PHP | Rust | **RHP** |
| :--- | :--- | :--- | :--- |
| **Value representation** | `null` | `None` (`Option<T>`) | **`null`** |
| **Undefined variable** | Warning $\rightarrow$ evaluates to `null` | Compile Error | **Compile Error** |
| **Missing array/dict key** | Warning $\rightarrow$ evaluates to `null` | Returns `None` | **Warning $\rightarrow$ evaluates to `null`** |
| **Missing struct property** | Warning $\rightarrow$ evaluates to `null` | Compile Error | **N/A (Structs not present in RHP)** |
| **Uninitialized field** | Runtime `Error` when accessed | Compile Error | **Compile Error (must initialize at creation)** |
| **Safe fallback (`??`)** | `$arr['b'] ?? 'default'` | `.unwrap_or("default")` | **`$arr['b'] ?? 'default'`** |
| **Safe assign (`??=`)** | `$arr['b'] ??= 'default'` | `entry().or_insert()` | **`$arr['b'] ??= 'default'`** |
| **Existence check** | `array_key_exists('b', $arr)` | `map.contains_key("b")` | `array_key_exists('b', $arr)` / `$d->has('b')` |


