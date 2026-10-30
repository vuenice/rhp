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

| Facade | Responsibility | Key Methods | Legacy Superglobal Status |
| :--- | :--- | :--- | :--- |
| `Request` | HTTP input parameters, filtering & in-place type formatting | `get()`, `all()`, `only()`, `format()`, `has()`, `set()`, `merge()` | `$_GET`, `$_POST` are deprecated |
| `Session` | Session state management across requests | `get()`, `set()`, `put()`, `has()`, `all()`, `forget()`, `clear()` | **`$_SESSION` is not supported** &mdash; use `Session::get()` |
| `Cookie` | HTTP cookie reading and setting | `get()`, `set()`, `put()`, `has()`, `all()`, `forget()`, `clear()` | **`$_COOKIE` is not supported** &mdash; use `Cookie::get()` |
| `Arr` | Subtyped collection creation & fluent operations | `list()`, `dict()`, `set()`, `hashMap()`, `macro()` | &mdash; |
| `Str` | Binary-safe raw byte & UTF-8 string manipulation | `slug()`, `contains()`, `upper()`, `fromEncoding()` | &mdash; |
| `Crypt` | Cryptographic hashing, HMAC, OpenSSL AES encryption & random bytes | `hash()`, `hashHmac()`, `opensslEncrypt()`, `opensslDecrypt()`, `randomBytes()`, `md5()` | &mdash; |
| `Html` | HTML entity escaping, tag stripping, sanitization & tag generation | `specialChars()`, `stripTags()`, `entities()`, `specialCharsDecode()`, `clean()`, `tag()` | &mdash; |
| `Url` | URL component parsing, query string building & Base64 encoding | `parseUrl()`, `httpBuildQuery()`, `base64Encode()`, `base64Decode()`, `to()`, `encode()` | &mdash; |
| `Carbon` | High-performance date & time manipulation modeled after Laravel Carbon | `now()`, `parse()`, `today()`, `toDateTimeString()`, `toDateString()`, `addDay()`, `lastOfMonth()`, `diffForHumans()` | &mdash; |
| `Future` | Tokio fiber concurrency & async coordination | `run()`, `wait()`, `any()`, `delayed()`, `defer()` | &mdash; |
| `Memory` | Zero-GC telemetry & scoped request arena inspection | `usage()`, `peak()`, `isGcFree()`, `gcPauseMs()`, `scope()` | &mdash; |
| `File` | Synchronous filesystem I/O operations & uploads | `read()`, `write()`, `exists()`, `delete()`, `fromRequest()` | **`$_FILES` is not supported** &mdash; use `File::fromRequest()` |
| `AFile` | Non-blocking async filesystem I/O returning `Future` | `read()`, `write()`, `exists()`, `delete()`, `append()`, `size()` | &mdash; |
| `HTTP` | Synchronous HTTP client and server kernel | `get()`, `post()`, `withHeaders()`, `serve()` | &mdash; |
| `AHTTP` | Non-blocking async HTTP client returning `Future` | `get()`, `post()`, `put()`, `delete()`, `patch()`, `withHeaders()` | &mdash; |
| `DB` | Synchronous database queries and transactions | `table()`, `find()`, `get()`, `select()`, `insert()` | &mdash; |
| `ADB` | Non-blocking async database queries returning `Future` | `table()`, `find()`, `get()`, `select()`, `insert()`, `update()`, `delete()` | &mdash; |
| `Log` | Lock-free mpsc async structured logger | `info()`, `warning()`, `error()`, `debug()` | &mdash; |
| `Sys` | System intrinsics, formatted output & process environment | `env()`, `echo()`, `print()`, `dd()`, `version()`, `timestamp()` | **`$_ENV` is not supported** &mdash; use `env()` / `Sys::env()` |
| `Glob` | Global helpers & fallback dynamic registry | `match()`, `getClass()`, `customHelper()` | **`$GLOBALS` is also not supported** &mdash; use `Glob::` |
| `Weak` | Ephemeral weak references for cycle prevention | `to()`, `get()`, `isValid()` | &mdash; |
| `Trace` / `Telemetry` | Native OpenTelemetry distributed tracing, metrics & APM bridges | `startSpan()`, `endSpan()`, `span()`, `counter()`, `gauge()`, `histogram()`, `exportOtlp()`, `exportSentry()` | &mdash; |

---

## Arr Facade & Subtyped Collections Architecture

RHP abandons PHP's legacy monolithic "everything-is-a-hash-table" array in favor of **three streamlined, high-performance collection primitives** mapped directly to native Rust heap structures with `Rc` Copy-On-Write (COW) semantics:

| Subtype | Constructor | Underlying Rust Structure | Primary Use Case & Characteristics |
| :--- | :--- | :--- | :--- |
| `list` | `Arr::list([...])` | `Rc<Vec<Value>>` | Contiguous numerical index sequence (`0..n`), maximum CPU cache locality, $O(1)$ append |
| `dict` | `Arr::dict([...])` | `Rc<IndexMap<Key, Value, ahash::RandomState>>` | String/atom-keyed dictionary retaining strict insertion order, hardware AES-accelerated `ahash` |
| `set` | `Arr::set([...])` | `Rc<IndexSet<Key, ahash::RandomState>>` | Unique value collection, auto-deduplication, $O(1)$ membership checks, insertion-ordered set algebra |

> [!NOTE]
> **Streamlined Architecture (`tuple` & `hashMap` on hold):**
> Universal `hashMap` (arbitrary complex keys like arrays or objects) and `tuple` structures are deferred for now. The vast majority of data structures in web applications map cleanly and more efficiently to `list` and `dict` without paying the hashing overhead of complex keys.

---

### 1. `Arr::list` (Sequential Lists - Step 8)

`Arr::list` maps directly to a contiguous Rust `Vec<PhpValue>` wrapped in `Rc<RefCell<...>>` with Copy-On-Write (COW) semantics. It guarantees maximum CPU cache locality, sequential zero-based integer indexing, and rapid append operations:

* **CPU Cache Locality During Iteration:** The runtime iteration engine uses a dedicated `ActiveIterator::List` stream that retains a direct reference handle to the contiguous vector with a cursor index. It streams elements sequentially without allocating intermediate `(PhpValue, PhpValue)` pairs or cloning keys.
* **Strict Sequential Index Integrity:** Lists support numeric indexing (`$list[0]`), numeric in-place updates (`$list[1] = 999`), and append operations (`$list[] = $val`). Non-numeric string keys (e.g. `$list['key'] = 42`) are strictly rejected with an `UndefinedIndex` runtime error.
* **Inline Declaration & Implicit Factory Lowering:** Declaring `list $arr = [];` is automatically lowered by the compiler into an optimized `Arr::list([])` allocation.

```php
// 1. Instantiation (Explicit Facade or Inline Declaration)
list $numbers = Arr::list([10, 20, 30, 40]);
list $items = []; // Lowers directly to Arr::list([])

// 2. Static Factory Helpers
list $seq = Arr::range(1, 10, 2);       // [1, 3, 5, 7, 9]
list $repeated = Arr::repeat('rhp', 3); // ['rhp', 'rhp', 'rhp']
list $wrapped = Arr::wrap(42);          // [42]
list $fromDict = Arr::from(Arr::dict(['a' => 1, 'b' => 2])); // [1, 2]

// 3. Higher-Order Transforms & Pipelines
list $doubled = $numbers
    ->map(fn($n) => $n * 2)
    ->filter(fn($n) => $n > 30); // [40, 60, 80]

list $rejected = $numbers->reject(fn($n) => $n % 20 === 0); // [10, 30]
int $reduced = $numbers->reduce(fn($acc, $n) => $acc + $n, 0); // 100

// 4. Math & Aggregations
int $sum = $doubled->sum();      // 180
float $avg = $doubled->avg();    // 60.0
int $min = $doubled->min();      // 40
int $max = $doubled->max();      // 80
int $count = $doubled->count();  // 3

// 5. Slicing, Chunks, and Flattening
list $chunks = $numbers->chunk(2);               // [[10, 20], [30, 40]]
list $flat = Arr::flatten(Arr::list([[1, 2], [3, 4]])); // [1, 2, 3, 4]
list $taken = $numbers->take(2);                // [10, 20]
list $skipped = $numbers->skip(2);              // [30, 40]
list $sliced = $numbers->slice(1, 2);           // [20, 30]

// 6. Searching, Verification & Uniqueness
int $first = $doubled->first();                 // 40
int $last = $doubled->last();                   // 80
int $found = $doubled->find(fn($n) => $n > 50); // 60
bool $hasForty = $doubled->contains(40);        // true
bool $isEmpty = $doubled->isEmpty();            // false
list $unique = Arr::list([1, 2, 2, 3])->unique(); // [1, 2, 3]

// 7. Partitioning & Zipping
list $parts = $numbers->partition(fn($n) => $n <= 20); // [[10, 20], [30, 40]]
list $zipped = Arr::list(['a', 'b'])->zip(Arr::list([1, 2])); // [['a', 1], ['b', 2]]

// 8. Dynamic Macro Registration (Arr::macro)
Arr::macro('toCsv', function ($delimiter = ',') {
    return implode($delimiter, $this->all());
});

string $csv = Arr::list([1, 2, 3])->toCsv();      // "1,2,3"
string $pipe = Arr::list(['a', 'b'])->toCsv('|'); // "a|b"
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

## Web State Facades & Helper Classes (`Request`, `Session`, `Cookie`, `File`, `env()`, `Glob`)

In RHP's concurrent, zero-bootstrap architecture, legacy PHP superglobals are deprecated or completely unsupported. Instead, RHP provides modern, type-safe static facades and helper functions:

| Helper Class / Function | Status of Legacy Superglobal | Recommended RHP Idiom |
| :--- | :--- | :--- |
| `Request::get()` | `$_GET` & `$_POST` are **Deprecated** | `Request::get('key')`, `Request::all()` |
| `env()` / `Sys::env()` | **`$_ENV` is not supported** | `env('KEY')` or `Sys::env('KEY')` |
| `File::fromRequest()` | **`$_FILES` is not supported** | `File::fromRequest('upload')` |
| `Session::get()` | **`$_SESSION` is not supported** | `Session::get('key')` / `Session::set('key', $val)` |
| `Cookie::get()` | **`$_COOKIE` is not supported** | `Cookie::get('name')` / `Cookie::set('name', $val)` |
| `Glob::` | **`$GLOBALS` is also not supported** | `Glob::` helper dispatch or explicit `global $var;` |

---

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

---

### 2. `Session` Facade (`Session::get()`) &mdash; `$_SESSION` is not supported

Legacy `$_SESSION` is **not supported** in RHP. Because RHP runs requests concurrently on Tokio worker threads, mutable global session superglobals would risk data corruption and memory leaks across fibers. All session access is handled through the thread-isolated `Session` facade:

```php
// Store session state
Session::set('user_id', 42);

// Retrieve session value with fallback default ($_SESSION is not supported)
$userId = Session::get('user_id', 0);
$hasUser = Session::has('user_id');

// Retrieve all session data
dict $sessionData = Session::all();

// Clear specific key or all session data
Session::forget('user_id');
Session::clear();
```

---

### 3. `Cookie` Facade (`Cookie::get()`) &mdash; `$_COOKIE` is not supported

Legacy `$_COOKIE` is **not supported** in RHP. HTTP cookies are accessed and manipulated cleanly via the static `Cookie` facade:

```php
// Set a cookie (stored as string)
Cookie::set('theme', 'dark');

// Read cookie with fallback default ($_COOKIE is not supported)
string $theme = Cookie::get('theme', 'light');

// Check existence and inspect all cookies
bool $hasTheme = Cookie::has('theme');
dict $allCookies = Cookie::all();

// Remove a cookie
Cookie::forget('theme');
```

---

### 4. `File` Facade (`File::fromRequest()`) &mdash; `$_FILES` is not supported

Legacy `$_FILES` is **not supported** in RHP. In multi-part HTTP requests, uploaded files are parsed directly into typed file representations using `File::fromRequest()`:

```php
// Inspect uploaded file from incoming HTTP request ($_FILES is not supported)
$uploaded = File::fromRequest('avatar');

if ($uploaded !== null) {
    // Read uploaded file metadata and contents
    string $tmpPath = $uploaded->path();
    string $origName = $uploaded->clientName();
    int $fileSize = $uploaded->size();

    // Persist to permanent storage using synchronous or async File facades
    File::write("uploads/{$origName}", File::read($tmpPath));
}
```

---

### 5. `env()` Helper & `Sys::env()` &mdash; `$_ENV` is not supported

Legacy `$_ENV` is **not supported** in RHP. Environment variables are read directly from the host process environment using the intrinsic `env()` helper function or `Sys::env()`:

```php
// Read environment variable with fallback default ($_ENV is not supported)
$dbHost = env('DB_HOST', '127.0.0.1');
$dbPort = env('DB_PORT', 5432);

// Equivalent static call via Sys facade
$apiKey = Sys::env('API_SECRET_KEY');
```

---

### 6. `Glob::` Helper Class &mdash; `$GLOBALS` is also not supported

Legacy `$GLOBALS` is **also not supported** in RHP. RHP eliminates global variable mutation arrays to maintain strict memory isolation across concurrent worker threads. To access global helpers or scoped global state, use the `Glob` facade or lexical `global` declarations:

```php
// 1. Dynamic global helper resolution ($GLOBALS is not supported)
$matched = Glob::match('/^[a-z]+$/', 'rhp');

// 2. Lexical global variable scoping in functions
$configCount = 10;

function incrementConfig(): void {
    global $configCount; // Scoped lexical global
    $configCount++;
}
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

### 5. Runtime Macro Registration (`Arr::macro`) vs. Class Extension

#### Why Traditional Class Extension (`extends Arr`) is a Bad Idea

In object-oriented programming, developers often instinctively subclass helper utilities (e.g., `class CustomArr extends Arr`). In RHP, **inheriting from static facades is strictly forbidden at compile time**:

* **Tight Coupling to Rust Internals:** Core helper classes like `Arr` and `Str` are not regular user-land classes—they are static facades wrapping optimized Rust heap allocations (such as `Box<Vec<PhpValue>>`, `Box<IndexMap<String, PhpValue>>`, or UTF-8 string buffers). Allowing inheritance would expose these internal memory boundaries and complicate the VM's JIT and optimization pipeline.
* **Stateless Design Violations:** `Arr` and `Str` are intentionally designed as stateless utility wrappers and fluent pipeline builders. Subclassing them introduces state management confusion and severe thread-safety risks in a multi-threaded Tokio execution environment.

Attempting to extend `Arr` or any other core static facade triggers a compile-time fatal diagnostic:

```text
Fatal Error: Cannot extend static facade class 'Arr'. Use 'Arr::macro()' to register custom methods dynamically.
```

#### Dynamic Runtime Macro Registration

Instead of extending the class via inheritance, developers can register custom methods dynamically using `::macro()`:

```php
// Define a custom macro on the Arr facade
Arr::macro('toCsv', function ($delimiter = ',') {
    // Custom logic accessing the underlying collection
    return implode($delimiter, $this->all());
});

// Usage anywhere in your application
$csv = Arr::list([1, 2, 3])->toCsv(); // "1,2,3"
```

#### Why Macros Work Perfectly for RHP

1. **Zero Performance Cost on Core Types:** Macros are stored in a global runtime registry rather than altering core class definitions or underlying Rust struct layouts.
2. **Maintains Familiarity:** Matches the exact developer ergonomics of Laravel's `Macroable` trait, making migration seamless for engineering teams upgrading high-traffic apps.
3. **Thread-Safe Registration:** Macros can be registered once during the initial application bootstrap phase in the parent process (`index.rhp` boot phase), making them safely accessible across all concurrent worker processes with zero locking overhead on hot paths.

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

## Crypt Facade & Cryptographic Services (`Crypt::*`)

The `Crypt` facade provides high-performance, constant-time cryptographic primitives, standard hashing, HMAC generation, OpenSSL AES encryption/decryption, and CSPRNG random byte generation implemented in pure Rust.

```php
// 1. Standard Hashing & Digest Computation
string $sha256 = Crypt::hash('sha256', 'secure-password');
string $md5 = Crypt::md5('content payload');
string $sha1 = Crypt::sha1('legacy token');
string $sha512 = Crypt::sha512('sensitive data');

// Optional raw binary digest ($binary = true)
string $rawBytes = Crypt::hash('sha256', 'payload', true);

// 2. Hash-based Message Authentication Code (HMAC - RFC 2104)
string $signature = Crypt::hashHmac('sha256', 'message payload', 'secret-key');

// 3. OpenSSL-Compatible AES Encryption & Decryption
string $key = "01234567890123456789012345678901"; // 32 bytes for AES-256
string $iv  = "1234567890123456";                 // 16 bytes IV
string $data = "Secret financial transaction data";

// Encrypts with PKCS#7 padding, returning Base64 by default:
string $cipherText = Crypt::opensslEncrypt($data, 'aes-256-cbc', $key, 0, $iv);

// Decrypts and strips PKCS#7 padding:
string|false $plainText = Crypt::opensslDecrypt($cipherText, 'aes-256-cbc', $key, 0, $iv);

// Supports AES-128, AES-192, AES-256 across CBC, CTR, and ECB modes:
string $ctrEnc = Crypt::opensslEncrypt($data, 'aes-128-ctr', '16bytekeyhere!', 0, $iv);
string $ctrDec = Crypt::opensslDecrypt($ctrEnc, 'aes-128-ctr', '16bytekeyhere!', 0, $iv);

// 4. Secure Random Byte & Hex Generation (CSPRNG)
string $bytes = Crypt::randomBytes(32); // 32 cryptographically secure random bytes
string $token = Crypt::randomHex(32);   // 32-character random hex string

// 5. Algorithm & Cipher Method Introspection
list $algos = Crypt::hashAlgos();              // ['md5', 'sha1', 'sha224', 'sha256', 'sha384', 'sha512']
list $ciphers = Crypt::opensslCipherMethods(); // ['aes-128-cbc', 'aes-256-cbc', 'aes-128-ctr', ...]
```

---

## Html Facade & Web Sanitization (`Html::*`)

The `Html` facade encapsulates modern HTML escaping, entity conversion, XSS sanitization, tag stripping, and tag generation:

```php
// 1. HTML Special Characters Escaping (XSS Prevention)
string $safe = Html::specialChars('<script>alert("XSS & \'quote\'");</script>');
// Result: "&lt;script&gt;alert(&quot;XSS &amp; &#039;quote&#039;&quot;);&lt;/script&gt;"

// Decode back to raw characters:
string $raw = Html::specialCharsDecode($safe);

// 2. HTML Entities Encoding & Decoding
string $encoded = Html::entities("Café & Bakery © 2026");
// Result contains: "&copy;", "&eacute;", "&amp;"

string $decoded = Html::entityDecode($encoded);
// Result: "Café & Bakery © 2026"

// 3. Tag Stripping with Tag Whitelisting
string $html = '<p>Hello <b>World</b>! <script>bad();</script> <a href="#">Link</a></p>';
string $stripped = Html::stripTags($html);             // "Hello World!  Link"
string $allowed  = Html::stripTags($html, ['b', 'a']); // "Hello <b>World</b>!  <a href="#">Link</a>"

// 4. Clean Sanitization Utility (strip tags + specialChars)
string $cleaned = Html::clean('<p>User input with & and "quotes"</p>');

// 5. HTML Tag & Attribute Generation
string $btn = Html::tag('button', ['class' => 'btn btn-primary', 'disabled' => true], 'Save');
// Result: '<button class="btn btn-primary" disabled>Save</button>'

string $img = Html::tag('img', ['src' => '/logo.png', 'alt' => 'Logo']);
// Result: '<img src="/logo.png" alt="Logo" />'

string $attrs = Html::attributes(['id' => 'main-box', 'class' => 'card shadow']);
// Result: 'id="main-box" class="card shadow"'
```

---

## Url Facade & URL / Base64 Manipulation (`Url::*` / `URL::*`)

The `Url` facade (aliased interchangeably as `Url` or `URL`) provides robust URL component parsing (both absolute and relative), query string generation with nested dictionary support, Base64 encoding/decoding, and route resolution:

```php
// 1. Parsing URLs into Dictionary Components
$url = "https://admin:secret@rhp.dev:8080/docs/helpers?version=2.0&lang=en#section-crypt";
dict $parts = Url::parseUrl($url);
// $parts['scheme']   => 'https'
// $parts['host']     => 'rhp.dev'
// $parts['port']     => 8080
// $parts['user']     => 'admin'
// $parts['pass']     => 'secret'
// $parts['path']     => '/docs/helpers'
// $parts['query']    => 'version=2.0&lang=en'
// $parts['fragment'] => 'section-crypt'

// Extract a single component directly:
string $host = Url::parseUrl($url, 1); // PHP_URL_HOST -> "rhp.dev"
int $port    = Url::parseUrl($url, 2); // PHP_URL_PORT -> 8080
string $path = Url::parseUrl($url, 5); // PHP_URL_PATH -> "/docs/helpers"

// Relative URLs are also cleanly parsed:
dict $rel = Url::parseUrl('/api/v1/items?sort=desc#top');
// $rel['path'] => '/api/v1/items', $rel['query'] => 'sort=desc'

// 2. HTTP Query String Construction (Nested Dict & RFC Support)
dict $params = [
    'search' => 'crypt & html',
    'page' => 1,
    'filter' => ['status' => 'active', 'tags' => ['rust', 'rhp']],
];

// RFC 1738 (default, '+' for spaces):
string $query = Url::httpBuildQuery($params);
// "search=crypt+%26+html&page=1&filter%5Bstatus%5D=active&filter%5Btags%5D%5B0%5D=rust&filter%5Btags%5D%5B1%5D=rhp"

// RFC 3986 ('%20' for spaces):
string $queryRfc = Url::httpBuildQuery($params, '', '&', 2);

// 3. Base64 & URL-Safe Base64 Encoding
string $b64 = Url::base64Encode("Hello from RHP!");
string $original = Url::base64Decode($b64);

string $urlSafeB64 = Url::base64UrlEncode("Data with +/-/=");
string $urlSafeDec = Url::base64UrlDecode($urlSafeB64);

// 4. URL Encoding & Decoding
string $encodedUrl = Url::encode("hello world & foo=bar"); // "hello+world+%26+foo%3Dbar"
string $decodedUrl = Url::decode($encodedUrl);              // "hello world & foo=bar"
string $rawEnc     = Url::rawUrlEncode("hello world");     // "hello%20world"

// 5. Route & App URL Generation
string $dashUrl = Url::to('dashboard'); // "https://example.com/dashboard"
```

---

## Carbon Facade & DateTime Manipulation (`Carbon::*`)

The `Carbon` facade brings Laravel's beloved fluent date and time manipulation library natively into RHP. Implemented in pure Rust over `chrono`, it provides high-throughput datetime arithmetic, RFC/ISO formatting, period boundary adjustments, and human-readable diffs with zero garbage collection overhead.

### Creating and Parsing Dates

```php
// Factory constructors
Carbon $now = Carbon::now();
Carbon $today = Carbon::today();         // Today at 00:00:00
Carbon $yesterday = Carbon::yesterday(); // Yesterday at 00:00:00
Carbon $tomorrow = Carbon::tomorrow();   // Tomorrow at 00:00:00

// Explicit component constructors
Carbon $d1 = Carbon::create(2024, 5, 20, 14, 30, 45);
Carbon $fromTs = Carbon::createFromTimestamp(1716206400);

// Parsing ISO 8601, RFC 3339, standard date strings & relative phrases
Carbon $parsed = Carbon::parse("2024-12-25 08:00:00");
Carbon $nextWeek = Carbon::parse("+1 week");
Carbon $firstNext = Carbon::parse("first day of next month");
```

### Output & Formatting

```php
Carbon $dt = Carbon::now();

string $dateTime = $dt->toDateTimeString();       // "2024-05-20 14:30:45"
string $dateOnly = $dt->toDateString();           // "2024-05-20"
string $dateAlias = $dt->toDatString();           // "2024-05-20" (alias for toDateString)
string $timeOnly = $dt->toTimeString();           // "14:30:45"
string $iso = $dt->toISOString();                 // "2024-05-20T14:30:45.000000Z"
string $formatted = $dt->toFormattedDateString(); // "May 20, 2024"
string $custom = $dt->format("Y/m/d H:i");        // "2024/05/20 14:30"

// Static shortcut: calling formatting methods on Carbon executes on Carbon::now()
string $currentDate = Carbon::toDateString();
string $currentDateTime = Carbon::toDateTimeString();

// String casting & echo automatically format as toDateTimeString()
echo "Event at: " . $dt; // "Event at: 2024-05-20 14:30:45"
```

### Fluent Date Arithmetic & Boundaries

```php
// Fluent modification (mutates and returns $this)
$dt->addDay();
$dt->addDays(5);
$dt->subDay();
$dt->subDays(3);
$dt->addWeek();
$dt->addMonth();
$dt->addYear();

// Period boundary adjustment
$lastDay = Carbon::parse("2024-02-10")->lastOfMonth()->toDateTimeString(); // "2024-02-29 23:59:59"
$startDay = Carbon::now()->startOfDay()->toDateTimeString();                // "2024-05-20 00:00:00"
$endDay = Carbon::now()->endOfDay()->toDateTimeString();                    // "2024-05-20 23:59:59"
$startWeek = Carbon::now()->startOfWeek()->toDateTimeString();
$endWeek = Carbon::now()->endOfWeek()->toDateTimeString();
$startMonth = Carbon::now()->startOfMonth()->toDateTimeString();
$endMonth = Carbon::now()->endOfMonth()->toDateTimeString();
$startYear = Carbon::now()->startOfYear()->toDateTimeString();
$endYear = Carbon::now()->endOfYear()->toDateTimeString();
```

### Comparisons and Differences

```php
Carbon $past = Carbon::parse("2024-05-10 12:00:00");
Carbon $future = Carbon::parse("2024-05-20 12:00:00");

bool $isPast = $past->isPast();
bool $isFuture = $future->isFuture();
bool $isToday = Carbon::now()->isToday();
bool $isWeekend = Carbon::now()->isWeekend();
bool $isLeap = Carbon::parse("2024-01-01")->isLeapYear();

int $daysDiff = $future->diffInDays($past);     // 10
int $hoursDiff = $future->diffInHours($past);   // 240
string $forHumans = $past->diffForHumans();     // "10 days ago"
string $forHumansFuture = $future->diffForHumans(); // "in 10 days"
```

### Test Time Mocking (`setTestNow`)

```php
// Mock current time in unit test suites
Carbon::setTestNow("2024-05-15 12:00:00");

if (Carbon::hasTestNow()) {
    Carbon $mocked = Carbon::now();
    assert($mocked->toDateString() === "2024-05-15");
}

// Reset mock fixture
Carbon::setTestNow(null);
```

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
