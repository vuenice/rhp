---
title: "Standard Library Overview"
description: "Overview of Tier 1 native static facades: Arr, Str, Future, Memory, File, HTTP, Log, and Sys."
section: "Language Guide"
order: 9
---

## Facade Architecture Philosophy

RHP replaces procedural legacy functions (`array_map`, `strlen`, `var_dump`) with expressive, static class facades wrapping optimized Rust heap primitives.

---

## Tier 1 Native Facades Summary

| Facade | Responsibility | Key Methods |
| :--- | :--- | :--- |
| `Request` | HTTP input parameters, filtering & in-place type formatting | `get()`, `all()`, `only()`, `format()`, `has()`, `set()`, `merge()` |
| `Session` | Session state management across requests | `get()`, `set()`, `put()`, `has()`, `all()`, `forget()`, `clear()` |
| `Cookie` | HTTP cookie reading and setting | `get()`, `set()`, `put()`, `has()`, `all()`, `forget()`, `clear()` |
| `Arr` | Subtyped collection creation & fluent operations | `list()`, `dict()`, `set()`, `hashMap()`, `macro()` |
| `Str` | Binary-safe raw byte & UTF-8 string manipulation | `slug()`, `contains()`, `upper()`, `fromEncoding()` |
| `Future` | Tokio fiber concurrency & async coordination | `run()`, `wait()`, `any()`, `delayed()`, `defer()` |
| `Memory` | Zero-GC telemetry & scoped request arena inspection | `usage()`, `peak()`, `isGcFree()`, `gcPauseMs()`, `scope()` |
| `File` | Synchronous filesystem I/O operations | `read()`, `write()`, `exists()`, `delete()` |
| `AFile` | Non-blocking async filesystem I/O returning `Future` | `read()`, `write()`, `exists()`, `delete()`, `append()`, `size()` |
| `HTTP` | Synchronous HTTP client and server kernel | `get()`, `post()`, `withHeaders()`, `serve()` |
| `AHTTP` | Non-blocking async HTTP client returning `Future` | `get()`, `post()`, `put()`, `delete()`, `patch()`, `withHeaders()` |
| `DB` | Synchronous database queries and transactions | `table()`, `find()`, `get()`, `select()`, `insert()` |
| `ADB` | Non-blocking async database queries returning `Future` | `table()`, `find()`, `get()`, `select()`, `insert()`, `update()`, `delete()` |
| `Log` | Lock-free mpsc async structured logger | `info()`, `warning()`, `error()`, `debug()` |
| `Sys` | System intrinsics, formatted output & process environment | `env()`, `echo()`, `print()`, `dd()`, `version()`, `timestamp()` |
| `Weak` | Ephemeral weak references for cycle prevention | `to()`, `get()`, `isValid()` |
| `Trace` / `Telemetry` | Native OpenTelemetry distributed tracing, metrics & APM bridges | `startSpan()`, `endSpan()`, `span()`, `counter()`, `gauge()`, `histogram()`, `exportOtlp()`, `exportSentry()` |

---

## Arr Facade & Subtyped Collections Architecture

RHP abandons PHP's legacy monolithic "everything-is-a-hash-table" array in favor of **four high-performance collection subtypes** mapped directly to native Rust heap structures with Copy-On-Write (COW) semantics:

| Subtype | Constructor | Underlying Rust Structure | Primary Use Case & Characteristics |
| :--- | :--- | :--- | :--- |
| `list` | `Arr::list([...])` | `Rc<RefCell<Vec<PhpValue>>>` | Contiguous numerical index sequence, maximum CPU cache locality, $O(1)$ append |
| `dict` | `Arr::dict([...])` | `Rc<RefCell<IndexMap<String, PhpValue>>>` | String-keyed dictionary retaining strict insertion order, $O(1)$ key lookups |
| `set` | `Arr::set([...])` | `Rc<RefCell<HashSet<PhpValue>>>` | Unique value collection, auto-deduplication, $O(1)$ membership checks, set algebra |
| `hashmap` | `Arr::hashMap([...])` | `Rc<RefCell<HashMap<PhpValue, PhpValue>>>` | Universal dictionary supporting arbitrary complex keys (arrays, objects, sets, scalars) |

---

### 1. `Arr::list` (Sequential Lists - Step 8)

`Arr::list` maps directly to a contiguous Rust `Vec<PhpValue>` wrapped in `Rc<RefCell<...>>`. It guarantees high CPU cache locality, sequential zero-based integer indexing, and fast append operations:

```php
// 1. Instantiation
list $numbers = Arr::list([10, 20, 30, 40]);

// 2. Functional pipelines
list $doubled = $numbers
    ->map(fn($n) => $n * 2)
    ->filter(fn($n) => $n > 30); // [40, 60, 80]

// 3. Math & Search
int $sum = $doubled->sum();      // 180
float $avg = $doubled->avg();    // 60.0
int $first = $doubled->first();  // 40
```

---

### 2. `Arr::dict` (Insertion-Ordered Dictionaries & Laravel Collection Methods - Step 9)

`Arr::dict` maps key-value dictionaries directly to Rust's `Rc<RefCell<IndexMap<String, PhpValue>>>`. It combines $O(1)$ amortized lookups, updates, and removals with **strict deterministic insertion ordering** and a full suite of Laravel-inspired fluent collection methods:

* **Strict Insertion Ordering:** Unlike unordered hash tables, `IndexMap` retains the exact chronological insertion order of keys across all loops, serializations, and transforms. Mutating the value of an existing key preserves its original index slot.
* **Zero-Allocation Streaming Iteration:** `foreach ($dict as $key => $val)` operates via `ActiveIterator::Dict` indexing directly into the `IndexMap` without creating temporary key or value vectors.
* **Laravel Collection Instance Methods:** Offers a rich, expressive functional API for manipulating key-value data with zero side effects.

```php
// 1. Instantiation
dict $product = Arr::dict([
    'sku'      => 'DESK-2026',
    'name'     => 'Standing Desk',
    'price'    => 350,
    'discount' => 30,
    'tax'      => 20,
]);

// 2. Inspection & Access
string $name = $product->get('name');                   // 'Standing Desk'
int $stock = $product->get('stock', 0);                 // Fallback default: 0
bool $hasSku = $product->has('sku');                    // true
bool $hasAny = $product->hasAny(['color', 'weight']);   // false
list $keys = $product->keys();                          // ['sku', 'name', 'price', 'discount', 'tax']
list $values = $product->values();                      // ['DESK-2026', 'Standing Desk', 350, 30, 20]
int $count = $product->count();                         // 5
bool $isEmpty = $product->isEmpty();                    // false

// 3. In-Place Mutations & Detachments (Copy-On-Write)
$product->put('in_stock', true);
$discount = $product->pull('discount');                 // 30 (retrieves and removes 'discount')
$product->forget('tax');                                // Removes 'tax' key

// 4. Higher-Order Transforms (Immutable Pipelines)
dict $prices = Arr::dict(['item1' => 20, 'item2' => 40])
    ->map(fn($price, $key) => $price * 1.1);

dict $keyed = Arr::dict(['first' => 10, 'second' => 20])
    ->mapWithKeys(fn($val, $key) => ["key_" . $key => $val * 2]);

dict $filtered = Arr::dict(['a' => 10, 'b' => 5, 'c' => 20])
    ->filter(fn($val, $key) => $val >= 10);             // ['a' => 10, 'c' => 20]

dict $rejected = Arr::dict(['a' => 10, 'b' => 5, 'c' => 20])
    ->reject(fn($val, $key) => $val >= 10);             // ['b' => 5]

int $reduced = Arr::dict(['x' => 1, 'y' => 2, 'z' => 3])
    ->reduce(fn($carry, $val) => $carry + $val, 0);     // 6

// 5. Sorting, Reversing & Slicing
dict $byKeyAsc = Arr::dict(['c' => 30, 'a' => 10, 'b' => 20])->sortKeys();
dict $byKeyDesc = Arr::dict(['c' => 30, 'a' => 10, 'b' => 20])->sortKeysDesc();
dict $byValAsc = Arr::dict(['c' => 30, 'a' => 10, 'b' => 20])->sort();
dict $reversed = Arr::dict(['a' => 1, 'b' => 2])->reverse(); // ['b' => 2, 'a' => 1]
list $chunks = Arr::dict(['a' => 1, 'b' => 2, 'c' => 3, 'd' => 4])->chunk(2);
dict $taken = Arr::dict(['a' => 1, 'b' => 2, 'c' => 3])->take(2);
dict $skipped = Arr::dict(['a' => 1, 'b' => 2, 'c' => 3])->skip(1);
dict $sliced = Arr::dict(['a' => 1, 'b' => 2, 'c' => 3])->slice(1, 1);

// 6. Navigation & Aggregates
string $firstVal = $product->first();
string $lastVal = $product->last();
string $firstKey = $product->firstKey();                // 'sku'
string $lastKey = $product->lastKey();                  // 'in_stock'
int $sum = Arr::dict(['a' => 10, 'b' => 20])->sum();    // 30
float $avg = Arr::dict(['a' => 10, 'b' => 20])->avg();  // 15.0
int $min = Arr::dict(['a' => 10, 'b' => 20])->min();    // 10
int $max = Arr::dict(['a' => 10, 'b' => 20])->max();    // 20

// 7. Partitioning, Flipping, Merging & Set Operations
dict $onlySku = $product->only(['sku', 'name']);
dict $without = $product->except(['sku']);
dict $flipped = Arr::dict(['role' => 'admin'])->flip(); // ['admin' => 'role']
dict $merged = Arr::dict(['a' => 1])->merge(Arr::dict(['b' => 2, 'a' => 99])); // ['a' => 99, 'b' => 2]
dict $diff = Arr::dict(['a' => 1, 'b' => 2])->diff(Arr::dict(['b' => 2])); // ['a' => 1]
dict $intersect = Arr::dict(['a' => 1, 'b' => 2])->intersect(Arr::dict(['b' => 2])); // ['b' => 2]

// 8. JSON Serialization
string $json = $product->toJson(); // '{"sku":"DESK-2026","name":"Standing Desk",...}'

// 9. Zero-Allocation Streaming Iteration
foreach ($product as $key => $value) {
    Sys::print("{}: {}", $key, (string)$value);
}
```

---

### 3. `Arr::set` (Unique Collections & Set Algebra - Step 10)

`Arr::set` maps directly to Rust's native `HashSet<PhpValue>`. It eliminates duplicate values automatically on construction and mutations, providing $O(1)$ set membership lookups and mathematical set algebra:

```php
// 1. Automatic Deduplication on Construction
$tags = Arr::set(['fast', 'rust', 'php', 'rust', 'backend', 'php']);
print($tags->count()); // 4 (unique: 'fast', 'rust', 'php', 'backend')

// 2. Adding Elements (Deduplicated automatically)
$tags->add('backend'); // Duplicate: count remains 4
$tags[] = 'frontend';  // Bracket push syntax: count becomes 5

// 3. O(1) Membership Checking
bool $hasRust = $tags->contains('rust'); // true
bool $hasGo   = $tags->has('go');        // false

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

// Relative Complement (diff: elements in A not in B -> {1, 2})
$diff = $setA->diff($setB);

// Set Intersection (elements in both A and B -> {3, 4})
$intersect = $setA->intersect($setB);

// Set Union (elements in A or B -> {1, 2, 3, 4, 5, 6})
$union = $setA->union($setB);

// Cross-Collection Algebra with Plain Arrays
$diffArr = $setA->diff([2, 4]); // {1, 3}

// 6. Higher-Order Transforms & Reductions
$evens = Arr::set([1, 2, 3, 4, 5, 6])
    ->filter(fn($n) => $n % 2 === 0); // {2, 4, 6}

$doubled = $evens->map(fn($n) => $n * 10); // {20, 40, 60}

$sum = $doubled->reduce(fn($acc, $n) => $acc + $n, 0); // 120

// 7. Streaming Native foreach Iteration
foreach ($tags as $tag) {
    // Visits each unique element exactly once
}
foreach ($tags as $index => $tag) {
    // Iterates with a 0-based sequence counter
}
```

---

### 4. `Arr::hashMap` / `Arr::map` (Universal Complex Key Dictionaries - Step 11)

In standard PHP, array keys are strictly coerced to integers or strings. `Arr::hashMap` breaks this limitation by mapping directly to Rust's `HashMap<PhpValue, PhpValue>`, enabling **any data structure** to serve as a key:

```php
class Point {
    public int $x;
    public int $y;
    public function __construct(int $x, int $y) {
        $this->x = $x;
        $this->y = $y;
    }
}

// Instantiate universal map
$map = Arr::hashMap();

// 1. Arrays / Lists as Complex Keys
$map[[10, 20]] = 'Alpha Base';
$map[[30, 40]] = 'Beta Outpost';

// Lookups use structural equality
$location = $map[[10, 20]]; // 'Alpha Base'

// 2. Objects / Class Instances as Keys
$p1 = new Point(100, 200);
$p2 = new Point(300, 400);
$map[$p1] = 'First Waypoint';
$map[$p2] = 'Second Waypoint';

$waypoint = $map[$p1]; // 'First Waypoint'

// 3. Sets as Keys
$map[Arr::set(['read', 'write'])] = 'ReadWriteRole';

// 4. Scalar Keys (Int, Float, String, Bool)
$map[42] = 'integer key';
$map[3.1415] = 'float key';
$map['tenant_id'] = 'string key';
$map[true] = 'boolean key';

// 5. Fluent CRUD & Helper Operations
$map->put('timeout', 5000);
$timeout = $map->get('timeout', 1000); // 5000
$fallback = $map->get('missing', 'default_val'); // 'default_val'

bool $hasTimeout = $map->has('timeout'); // true (also: contains(), hasKey())
$pulled = $map->pull('timeout', 0);      // Retrieves 5000 and deletes key
$map->remove('tenant_id');               // Removes key (also: forget())

// 6. Projections: Keys, Values & Entries
list $allKeys = $map->keys();            // Sequential list of all keys (including complex keys)
list $allVals = $map->values();          // Sequential list of all values
list $pairs   = $map->entries();         // List of [[$key, $value], ...] pairs

// 7. Higher-Order Transforms & Combinators
$doubled = $map
    ->filter(fn($v, $k) => is_numeric($v))
    ->map(fn($v, $k) => $v * 2);

// mapWithKeys transforms both key and value
$reindexed = $map->mapWithKeys(fn($v, $k) => ["transformed_" . (string)$k => $v]);

// Diff, Intersect, and Merge
$other = Arr::hashMap(['a' => 1, 'b' => 2]);
$diffMap = $map->diff($other);           // Keys in $map not in $other
$merged  = $map->merge($other);          // Combines maps into new instance

// 8. Native foreach Iteration with Complex Keys
foreach ($map as $key => $val) {
    // $key yields the complex key directly (array, object, set, or scalar)!
}
```

---

### 5. Static `Arr` Facade & Dot Notation Helpers

The static `Arr` facade exposes Laravel-inspired utilities for navigating deeply nested associative dictionaries, restructuring data, and inspecting collection shapes:

#### Dot Notation Traversal & Deep Mutation
Access and mutate arbitrarily nested dictionaries or object trees using intuitive dot-delimited key paths:

```php
$data = Arr::dict([
    'user' => Arr::dict([
        'profile' => Arr::dict([
            'name'  => 'Alice',
            'email' => 'alice@example.com',
        ]),
        'role' => 'admin',
    ]),
]);

// 1. Safe Deep Read with Arr::get
$name = Arr::get($data, 'user.profile.name');               // 'Alice'
$age  = Arr::get($data, 'user.profile.age', 25);            // 25 (default fallback)

// 2. Deep Membership with Arr::has and Arr::hasAny
bool $hasEmail = Arr::has($data, 'user.profile.email');     // true
bool $hasAny   = Arr::hasAny($data, ['user.missing', 'user.profile.email']); // true

// 3. Deep Mutation with Arr::set
// Sets intermediate dictionaries automatically if not already present:
$modified = Arr::set($data, 'user.profile.city', 'Amsterdam');
$city = Arr::get($modified, 'user.profile.city');           // 'Amsterdam'

// 4. Deep Removal with Arr::forget and Arr::pull
$withoutRole = Arr::forget($modified, 'user.role');
bool $hasRole = Arr::has($withoutRole, 'user.role');        // false

$pulledName = Arr::pull($withoutRole, 'user.profile.name'); // 'Alice' (retrieves & removes)

// 5. Flattening & Unflattening (Arr::dot / Arr::undot)
// Arr::dot collapses multidimensional dicts into single-level dot keys:
dict $flat = Arr::dot(Arr::dict([
    'db' => Arr::dict([
        'host' => '127.0.0.1',
        'port' => 3306,
    ]),
])); // ['db.host' => '127.0.0.1', 'db.port' => 3306]

// Arr::undot reconstructs full nested dictionaries from dot keys:
dict $nested = Arr::undot($flat);
// Returns: ['db' => ['host' => '127.0.0.1', 'port' => 3306]]
```

#### Structural Transformations & Shape Inspection

```php
// 1. Arr::divide: Splits a dictionary into a pair of [keys, values]
list $divided = Arr::divide(Arr::dict(['x' => 10, 'y' => 20]));
// Result: [['x', 'y'], [10, 20]]

// 2. Arr::keyBy: Indexes a list of dictionaries by a key field
list $users = Arr::list([
    Arr::dict(['id' => 'u1', 'name' => 'John']),
    Arr::dict(['id' => 'u2', 'name' => 'Jane']),
]);
dict $keyed = Arr::keyBy($users, 'id');
// ['u1' => ['id' => 'u1', 'name' => 'John'], 'u2' => ['id' => 'u2', 'name' => 'Jane']]

// 3. Arr::pluck: Plucks a field into a list or keyed dictionary
list $names = Arr::pluck($users, 'name');          // ['John', 'Jane']
dict $keyedNames = Arr::pluck($users, 'name', 'id'); // ['u1' => 'John', 'u2' => 'Jane']

// 4. Arr::collapse: Collapses an array of arrays into a single flat list
list $collapsed = Arr::collapse([[1, 2], [3, 4], [5]]); // [1, 2, 3, 4, 5]

// 5. Shape Inspection: Arr::isAssoc and Arr::isList
bool $isAssoc = Arr::isAssoc(Arr::dict(['k' => 'v']));  // true
bool $isList  = Arr::isList(Arr::list([1, 2, 3]));       // true
```

---

### 6. Collection Type Boundaries & Permissive `array` Acceptance

RHP combines strict type annotations with flexible parent compatibility:

```php
// Strict subtype type hints
function processUniqueTags(set $tags): int {
    return $tags->count();
}

function processCoordinates(hashmap $coords): void {
    // Accepts hashmap only
}

// Permissive parent type 'array' accepts ALL collection subtypes:
function countAnyCollection(array $items): int {
    return $items->count();
}

countAnyCollection(Arr::list([1, 2, 3]));       // OK
countAnyCollection(Arr::dict(['a' => 1]));      // OK
countAnyCollection(Arr::set(['x', 'y']));       // OK
countAnyCollection(Arr::hashMap([[1, 2] => 3]));// OK
```

---

### 7. Dynamic Runtime Macro Registration (`Arr::macro`)

Extend collection instances with custom domain methods at runtime without subclassing `Arr`:

```php
Arr::macro('joinCommas', function(): string {
    return $this->values()->join(', ');
});

$tags = Arr::set(['php', 'rust', 'rhp']);
print($tags->joinCommas()); // "php, rust, rhp"
```

---

## Web State Facades (`Request`, `Session`, `Cookie`)

In RHP, legacy PHP superglobals (`$_GET`, `$_POST`, `$_SESSION`, `$_COOKIE`) are deprecated. Instead, use static facades:

### 1. `Request` Facade & Type Formatting

All incoming request parameters are strings by default. You can inspect, filter, or cast them on the request object using `Request::format()`:

```php
// Retrieve raw string values by default
$pageStr = Request::get('page'); // e.g. "1"

// Format and cast types in-place on the request object:
// Both quoted types ('int', 'bool') and bare type keywords (int, bool) are supported!
Request::format([
    'page'   => 'int',
    'active' => bool,
    'rate'   => float,
]);

int $page = Request::get('page');     // 1
bool $active = Request::get('active'); // true

// Filter specific keys via array list or variadic args:
dict $public = Request::only(['page', 'active']);
dict $pageOnly = Request::only('page');
```

> **Why bare type keywords work without ambiguity:**
> In RHP's Pratt parser, type keywords in statement positions (`int $x = 10;`) are validated by the presence of a variable identifier `$var`. When a type keyword appears as a value in an array literal (`'active' => bool`), it parses cleanly as a value token resolving to the type name string without any grammar ambiguity.

### 2. `Session` Facade

```php
// Store session state
Session::set('user_id', 42);

// Retrieve session value with fallback default
$userId = Session::get('user_id', 0);
$hasUser = Session::has('user_id');

// Clear specific key or all session data
Session::forget('user_id');
Session::clear();
```

### 3. `Cookie` Facade

```php
// Set a cookie (stored as string)
Cookie::set('theme', 'dark');

// Read cookie with fallback default
string $theme = Cookie::get('theme', 'light');

// Check and remove cookie
bool $hasTheme = Cookie::has('theme');
Cookie::forget('theme');
```

---

## Arr Facade & Subtyped Collection Architecture

RHP provides the static `Arr` facade to replace PHP's procedural array functions with high-performance, object-oriented collection builders:

### 1. Four High-Performance Collection Subtypes
* **`Arr::list(array $items = []): list`** - Sequential integer-indexed lists backed by contiguous `Box<Vec<PhpValue>>` for maximum CPU cache locality.
* **`Arr::dict(array $pairs = []): dict`** - Insertion-ordered string-keyed dictionaries backed by `Box<IndexMap<String, PhpValue>>`.
* **`Arr::set(array $values = []): set`** - Unique value collections backed by `Box<HashSet<PhpValue>>` with $O(1)$ membership checks and set algebra.
* **`Arr::hashMap(): hashmap`** - Universal key-value maps backed by `Box<HashMap<PhpValue, PhpValue>>`, supporting arbitrary complex keys (arrays, objects).

### 2. Permissive Parent `array` Type
The `array` keyword acts as the **permissive parent type** for all four collection subtypes. Functions can accept `array $items` or return `array` to work seamlessly across lists, dicts, sets, and hashmaps without strict boundary errors:

```php
function transform(array $input): array {
    // Works with [1, 2, 3], ['k' => 'v'], Arr::list(), or Arr::dict()
    return $input->sort();
}
```

### 3. Immutable Helper Methods by Default
Calling collection helper methods (such as `$arr->sort()`, `$arr->filter()`, or `Arr::list($arr)->sort()`) **never mutates the original collection in place**. Every transformation returns a brand-new instance, guaranteeing clean, side-effect-free data flows.

### 4. Explicit Variable Reassignment
To update a variable with a transformed or sorted collection, use explicit reassignment:

```php
$numbers = [3, 1, 2];

// $numbers is unchanged; $sorted holds a new sorted array
$sorted = $numbers->sort();

// Explicit reassignment leverages Copy-On-Write (COW) with zero unnecessary clones:
$numbers = $numbers->sort();
```

### 5. Runtime Macro Registration (`Arr::macro`)
Traditional class inheritance (`extends Arr`) is forbidden to preserve Rust heap boundaries and thread safety. Developers register custom macros dynamically at runtime:

```php
Arr::macro('toCsv', function($delimiter = ',') {
    return implode($delimiter, $this->all());
});

$csv = Arr::list([1, 2, 3])->toCsv(); // "1,2,3"
```

---

## Str Facade & Binary-Safe String Architecture

In RHP, strings are fundamentally **binary-safe raw byte buffers** (`RhpString` storing $\le 23$ bytes inline on the stack via Small-String-Optimization (SSO) and larger buffers via `Rc<Vec<u8>>`). This design matches the Zend Engine's binary safety while maintaining Rust's strict memory safety:

### 1. Underlying Representation: `PhpValue::String(Vec<u8>)`
* **Binary Safety:** Safely store and process arbitrary binary payloads, network buffers, images (e.g. PNG/JPEG byte headers), and strings containing null bytes (`\0`) without triggering Rust UTF-8 panics or validation errors.
* **Legacy Japanese Multi-Byte Support:** Safely accommodates legacy multi-byte Japanese encodings like **Shift-JIS (SJIS / CP932)**, **EUC-JP**, and **ISO-2022-JP (JIS)** natively as raw bytes without crashing.

### 2. Static Facade Constructors (No `new Str()`)
Following RHP's design philosophy—where developers do not instantiate helper classes with `new Str()`—developers use **static factory methods and explicit encoding facades** to handle text safely:

```php
// 1. Creating a standard string (treated as raw bytes / UTF-8 safe)
$greeting = "こんにちは世界"; // Stored natively as raw UTF-8 bytes in Vec<u8>

// 2. Explicitly declaring or converting from a specific encoding (e.g., Shift-JIS)
$jisText = Str::fromEncoding($rawBytes, 'Shift-JIS');

// 3. Ensuring UTF-8 compliance via explicit facade constructors
$safeText = Str::utf8($userInput);

// 4. Converting back to raw encoding bytes (e.g. Shift-JIS)
$rawJis = Str::toEncoding($greeting, 'Shift-JIS');
```

### 3. Immutable Helpers & Explicit Variable Reassignment
Consistent with array and collection design rules, string helper methods are **immutable by default** (returning a new instance rather than mutating buffers in place), ensuring clean data flows. Developers update variables via **explicit reassignment** leveraging RHP’s Copy-On-Write (COW) and pointer semantics:

```php
$text = "  hello world  ";

// Method chaining returns a new instance; original $text is untouched
$trimmed = $text->trim()->upper(); 

// Explicit reassignment to update the variable in place cleanly
$text = $text->trim()->upper(); 
```

### 4. Summary of String Architecture

| Feature | Implementation Strategy | Benefit |
| :--- | :--- | :--- |
| **Internal Storage** | `Vec<u8>` with 23-byte SSO inside `PhpValue::String` | Binary safe; handles arbitrary bytes, images, network streams, and legacy Japanese encodings natively without panics. |
| **Instantiation** | Static facades (`Str::utf8()`, `Str::fromEncoding()`) | Avoids `new Str()` object header overhead; keeps syntax clean and performant. |
| **Mutations** | Immutable helpers + explicit reassignment (`$str = $str->method()`) | Predictable side-effect-free data manipulation combined with zero-garbage memory replacement. |

---

## Sys Facade: Formatted Output (`echo` & `print`)

`Sys` contains two primary output functions that compile directly to native Rust `print!` and `println!` functions with function parameters passed as-is:
- **`Sys::echo(...)`**: Prints on the same line without a trailing newline (identical to Rust's `print!()`).
- **`Sys::print(...)`**: Prints and automatically adds a trailing newline `\n` after the line (identical to Rust's `println!()`).

Both functions work identically with format strings and arguments. RHP does **not** support C-language style formatting (such as `%d`, `%s`, `%f`, or `%x`). All templates use native Rust formatting parameters directly:

| Format Specifier | Description | Example | Output |
| :--- | :--- | :--- | :--- |
| `{}` | Value placeholder | `Sys::print("Age: {}, Name: {}", 30, "Bob")` | `Age: 30, Name: Bob\n` |
| `{:.2}` | Floating-point precision | `Sys::print("{:.2}", 3.141592)` | `3.14\n` |
| `{:05}` | Integer padding with zeros | `Sys::print("{:05}", 42)` | `00042\n` |
| `{:x}` | Hexadecimal lowercase | `Sys::print("{:x}", 255)` | `ff\n` |
| `{:b}` | Binary formatting | `Sys::print("{:b}", 5)` | `101\n` |

```php
// 1. Value placeholders
Sys::print("Age: {}, Name: {}", 30, "Bob"); // Outputs: Age: 30, Name: Bob\n

// 2. Floating-point precision
Sys::print("{:.2}", 3.141592); // Outputs: 3.14\n

// 3. Integer padding with zeros
Sys::print("{:05}", 42); // Outputs: 00042\n

// 4. Hexadecimal lowercase
Sys::print("{:x}", 255); // Outputs: ff\n

// 5. Binary formatting
Sys::print("{:b}", 5); // Outputs: 101\n

// Same-line printing with Sys::echo (no newline)
Sys::echo("Age: {}, Name: {}", 30, "Bob"); // Outputs: Age: 30, Name: Bob (same line)
Sys::echo(" | Binary: {:b}", 5);            // Outputs:  | Binary: 101 (same line)
Sys::print(" [done]");                     // Outputs:  [done]\n
```

> **Important: There is no `Sys::println` function.**
> In RHP, only `echo` and `print` exist (both as language constructs `echo ...` / `print(...)` and via `Sys::echo(...)` / `Sys::print(...)`). There is **no** `Sys::println` function. Calling `Sys::println(...)` will result in an `UndefinedFacadeMethod` error.

---

## Synchronous Facades (`File`, `HTTP`, `DB`) vs Async Facades (`AFile`, `AHTTP`, `ADB`)

RHP eliminates function coloring by providing pairs of synchronous and asynchronous facades rather than requiring `async/await` keywords:

- **Use Synchronous Facades (`File`, `HTTP`, `DB`)** for straightforward, sequential blocking execution:
  ```php
  string $data = File::read('report.csv');
  $response = HTTP::get('https://api.example.com/status');
  $user = DB::table('users')->find(10);
  ```
- **Use Async Facades (`AFile`, `AHTTP`, `ADB`)** to launch non-blocking background tasks on Rust Tokio, returning `Future` handles that can be checked with `$fut->isPending()` or resolved via `$fut->wait()`:
  ```php
  $fut1 = AFile::read('report.csv');
  $fut2 = AHTTP::get('https://api.example.com/status');
  $fut3 = ADB::table('users')->find(10);

  // Synchronize results without 'await':
  $data = $fut1->wait();
  $response = $fut2->wait();
  $user = $fut3->wait();
  ```

---

## Observability & Distributed Tracing (`Trace` & `Telemetry`)

RHP embeds a zero-cost OpenTelemetry-compliant tracing and metrics engine directly into the runtime, replacing heavy Zend C extensions:

```php
// Automatic closure-based span with metrics and exception capture
$result = Trace::span('order.process', function() {
    Trace::setAttribute('order.amount', 149.99);
    Trace::counter('orders.processed', 1);

    // Outbound HTTP call automatically injects W3C traceparent header
    $res = HTTP::post('https://api.payment.internal/charge', ['amount' => 149.99]);

    Trace::addEvent('payment_settled', Arr::dict(['provider' => 'stripe']));
    return $res->status();
});

// Record high-precision latency percentiles (p50, p90, p95, p99)
Trace::histogram('db.query_latency_ms', 4.2);
$stats = Trace::metricSummary('db.query_latency_ms');

// Export canonical OpenTelemetry Protocol (OTLP/HTTP JSON)
$tracesJson = Trace::exportOtlp();
$metricsJson = Trace::exportOtlpMetrics();
```

For full architecture details, W3C context propagation, Sentry/Datadog bridges, and worker auto-instrumentation, see the [Observability & OpenTelemetry Guide](/docs/guide/observability-opentelemetry).


---

## Tier 2 Native Helper Classes (`Cache`, `Log`) via Rust File Imports

While Tier 1 facades are pre-compiled into the RHP binary, framework-level helper utilities like `Cache` or high-performance `Log` are designed as **Tier 2 Native Helper Classes**.

### Why Tier 2 Rust Imports?
1. **Core Language Minimalism:** Helper classes are not required in the core programming language syntax, keeping RHP lean, fast to parse, and architecturally uncluttered.
2. **Framework-Level Speed:** Frameworks need high-speed caching with TTLs, atomics, and structured logging. By implementing these as native Rust files (`.rs`) imported directly (`import "./Cache.rs"`), applications achieve bare-metal Rust performance without engine forks or C FFI overhead.
3. **Thread Safety & Multi-Worker Concurrency:** Native helper stores use `FrozenValue` boundaries, ensuring lock-free or read-optimized thread safety across all Tokio worker threads with zero GC pauses.

```php
import "./Cache.rs";
import "./Log.rs" as Logger;

// Cache operations (in-memory atomic L1 store with TTL)
Cache::set("session:user_10", "Alice", 3600);
if (Cache::has("session:user_10")) {
    $user = Cache::get("session:user_10");
}
Cache::increment("hits", 1);
int $hitCount = Cache::get("hits");

// Structured logging
Logger::info("User session verified", ["user_id" => 10]);
Logger::warn("Rate limit threshold nearing", ["ip" => "127.0.0.1"]);
```

---

## Example: Composing Facades


```php
// Read input, transform collection, and log telemetry
string $rawContent = File::read('metrics.json');
dict $data = json_decode($rawContent, true);

list $activeUsers = Arr::list($data->get('users'))
    ->filter(fn($u) => $u['is_active'] === true)
    ->map(fn($u) => Str::slug($u['username']));

Log::info("Found " . (string)$activeUsers->count() . " active users");
Sys::print("Active users: {}, Peak Memory: {} bytes", $activeUsers->count(), Memory::peak());
```
