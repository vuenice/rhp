---
title: "Standard Library API"
description: "Exhaustive method signatures and specifications for core Tier 1 native facades."
section: "Reference"
order: 4
---

## `Arr` Facade & Collection Subtypes API

RHP provides four specialized collection subtypes backed by dedicated Rust heap structures, alongside a permissive `array` parent type:
- `array`: Permissive parent collection type for backward compatibility, matching any collection variant (`list`, `dict`, `set`, `hashmap`)
- `list`: Contiguous numerical index sequence (`Vec<PhpValue>`)
- `dict`: Insertion-ordered string-keyed dictionary (`IndexMap<String, PhpValue>`)
- `set`: Unique collection with $O(1)$ membership and set algebra (`HashSet<PhpValue>`)
- `hashmap` / `map`: Universal key-value dictionary supporting arbitrary complex keys (`HashMap<PhpValue, PhpValue>`)

> **Immutability by Default & Reassignment Contract:**
> Calling collection helper methods (such as `$arr->sort()`, `$arr->filter()`, or `Arr::list($arr)->sort()`) **never mutates the original collection in place**. Every transformation returns a brand-new instance. To update a variable, use explicit reassignment:
> ```php
> $my_array = $my_array->sort(); // Leverages Copy-On-Write (COW) with zero unnecessary clones
> ```

### Static `Arr` Facade Methods

```php
// Subtype Constructors
Arr::list(array $items = []): list;
Arr::dict(array $pairs = []): dict;
Arr::set(array $values = []): set;
Arr::hashMap(array $entries = []): hashmap;
Arr::map(array $entries = []): hashmap; // Alias for Arr::hashMap()

// Dot-Notation Deep Inspection & Mutation
Arr::get(mixed $target, string $key, mixed $default = null): mixed;
Arr::set(mixed &$target, string $key, mixed $value): mixed;
Arr::has(mixed $target, string $key): bool;
Arr::hasAny(mixed $target, array|list $keys): bool;
Arr::forget(mixed &$target, string ...$keys): mixed;
Arr::pull(mixed &$target, string $key, mixed $default = null): mixed;
Arr::dot(mixed $target, string $prepend = ''): dict;
Arr::undot(dict|array $dotted): dict;

// Structural Partitioning & Transformation
Arr::divide(dict|array $target): list;                         // Returns [keys, values]
Arr::keyBy(array|list $items, string $key): dict;              // Re-indexes items by field key
Arr::pluck(array|list $items, string $key, ?string $keyBy = null): list|dict;
Arr::collapse(array|list $array): list;                        // Collapses list of arrays into flat list
Arr::flatten(mixed $array): list;                              // Recursively flattens nested arrays
Arr::chunk(mixed $array, int $size): list;                     // Chunks array into list of lists
Arr::from(mixed $items): list;                                 // Converts any iterable/collection to list
Arr::first(mixed $array, mixed $default = null): mixed;        // Returns first element or default
Arr::last(mixed $array, mixed $default = null): mixed;         // Returns last element or default
Arr::firstKey(mixed $array): mixed;                            // Returns first key or null
Arr::lastKey(mixed $array): mixed;                             // Returns last key or null
Arr::isAssoc(mixed $array): bool;                              // Checks if collection is associative dict
Arr::isList(mixed $array): bool;                               // Checks if collection is sequential list
Arr::sum(mixed $array): int|float;                             // Computes sum of elements
Arr::product(mixed $array): int|float;                         // Computes product of elements
Arr::fill(int $start, int $count, mixed $value): array;        // Fills array with count entries of value
Arr::fillKeys(array $keys, mixed $value): dict;                // Fills array using keys with value
Arr::pad(array $array, int $size, mixed $value): array;        // Pads array to size with value
Arr::column(mixed $array, ?string $columnKey, ?string $indexKey = null): list|dict; // array_column()
Arr::flip(mixed $array): dict;                                 // array_flip(): swaps keys and values
Arr::combine(mixed $keys, mixed $values): dict;                // array_combine(): creates dict from keys and values
Arr::countValues(mixed $array): dict;                          // array_count_values(): counts frequency of each value

// Sorting (Pure Immutable: Returns New Collection of Respective Subtype)
Arr::sort(mixed $items): list|dict;                            // Ascending sort (preserves dict keys)
Arr::asort(mixed $items): list|dict;                           // Procedural alias for Arr::sort()
Arr::ksort(mixed $dict): dict;                                 // Sorts dictionary by key ascending
Arr::sortKeys(mixed $dict): dict;                              // Alias for ksort()
Arr::sortKeysDesc(mixed $dict): dict;                          // Sorts dictionary by key descending
Arr::usort(mixed $items, callable $cmp): list|dict;            // User comparator sort
Arr::uasort(mixed $items, callable $cmp): list|dict;           // User comparator sort preserving keys
Arr::uksort(mixed $dict, callable $cmp): dict;                 // User comparator sort on dictionary keys
Arr::natsort(mixed $items): list|dict|set;                     // Natural order sorting ('img1.png', 'img2.png', 'img10.png')
Arr::sortNatural(mixed $items): list|dict|set;                 // Alias for natsort()
Arr::natcasesort(mixed $items): list|dict|set;                 // Case-insensitive natural order sorting
Arr::sortNaturalCase(mixed $items): list|dict|set;             // Alias for natcasesort()

// Set-Like Difference & Intersection
Arr::diff(mixed $collection, mixed ...$others): list|dict|set; // array_diff(): values not in other collections
Arr::diffKey(mixed $collection, mixed ...$others): list|dict;  // array_diff_key(): keys not in other collections
Arr::diffKeys(mixed $collection, mixed ...$others): list|dict; // Alias for diffKey()
Arr::intersect(mixed $collection, mixed ...$others): list|dict|set; // array_intersect(): values present in all collections
Arr::intersectKey(mixed $collection, mixed ...$others): list|dict;  // array_intersect_key(): keys present in all collections
Arr::intersectKeys(mixed $collection, mixed ...$others): list|dict; // Alias for intersectKey()

// Randomization & Shuffling
Arr::shuffle(mixed $collection): list|dict|set;                // Returns randomly shuffled collection of same subtype
Arr::randomKey(mixed $collection, int $num = 1): mixed;        // array_rand(): picks 1 or $num random keys/indices
Arr::random(mixed $collection, ?int $num = null): mixed;       // Picks 1 random value or collection of $num random values

// Generators & Utilities
Arr::range(int $start, int $end, int $step = 1): list;
Arr::repeat(mixed $value, int $times): list;
Arr::wrap(mixed $value): list;
Arr::count(mixed $collection): int;
Arr::contains(mixed $collection, mixed $needle): bool;
Arr::keys(mixed $collection): list;
Arr::values(mixed $collection): list;
Arr::merge(mixed ...$arrays): list|dict;
Arr::unique(mixed $items): set;
Arr::reverse(mixed $target): list|dict;

// Static Cross-Collection Comparison (also available via Array::)
Arr::compareJson(mixed $a, mixed $b): bool;                   // Canonical JSON stringification and comparison across collection types
Arr::compareRef(mixed $a, mixed $b): bool;                    // Memory address / pointer identity check (same allocation)
Arr::compareJoined(mixed $a, mixed $b, string $sep = ','): bool; // Converts elements to joined string and compares
Arr::compareUnordered(mixed $a, mixed $b): bool;              // Unordered value comparison (strictly supports list and set only)

// Dynamic Macro Extension
Arr::macro(string $name, callable $closure): bool;
```

> **Why Class Inheritance (`extends Arr`) is Forbidden:**
> Core facades wrap optimized native Rust heap allocations (such as `Box<Vec<PhpValue>>` and `Box<IndexMap<String, PhpValue>>`). Subclassing static facades (`class MyArr extends Arr`) is strictly forbidden at compile time to prevent tight coupling to Rust internals and state management violations in multi-threaded Tokio fibers.
>
> Attempting `class CustomArr extends Arr` generates:
> ```text
> Fatal Error: Cannot extend static facade class 'Arr'. Use 'Arr::macro()' to register custom methods dynamically.
> ```
>
> Instead, register dynamic methods at boot via `Arr::macro('methodName', function(...) { ... })`. Macros are stored in a thread-safe global registry with zero overhead on core collection types.

---

### `list` Instance Methods (Sequential Contiguous List)

```php
// Inspection & State
$list->count(): int;                    // Also: size(), length(), len()
$list->isEmpty(): bool;
$list->isNotEmpty(): bool;
$list->contains(mixed $needle): bool;
$list->first(): mixed;
$list->last(): mixed;
$list->find(callable $fn): mixed;

// Mutation & Extraction
$list->push(mixed ...$values): list;    // Also: add()
$list->pop(): mixed;
$list->shift(): mixed;
$list->unshift(mixed ...$values): list;
$list->all(): list;                     // Also: toArray()
$list->slice(int $offset, ?int $length = null): list;
$list->take(int $count): list;
$list->skip(int $count): list;
$list->chunk(int $size): list;
$list->flatten(): list;
$list->unique(): list;
$list->reverse(): list;
$list->sort(?callable $comparator = null): list; // Also: asort(), usort(), uasort()
$list->sortBy(string|callable $key): list;
$list->natsort(): list;                 // Natural order sorting ('img1.png', 'img2.png', 'img10.png')
$list->sortNatural(): list;             // Alias for natsort()
$list->natcasesort(): list;             // Case-insensitive natural order sorting
$list->sortNaturalCase(): list;         // Alias for natcasesort()
$list->diff(mixed ...$others): list;    // array_diff()
$list->diffKey(mixed ...$others): list; // array_diff_key()
$list->intersect(mixed ...$others): list; // array_intersect()
$list->intersectKey(mixed ...$others): list; // array_intersect_key()
$list->column(?string $col, ?string $indexKey = null): list|dict; // array_column()
$list->flip(): dict;                    // array_flip(): values become keys, indices become values
$list->combine(mixed $values): dict;    // array_combine(): $list items become keys
$list->countValues(): dict;             // array_count_values()
$list->shuffle(): list;                 // Returns randomly shuffled list
$list->randomKey(int $num = 1): int|list; // array_rand(): returns 1 index or list of indices
$list->random(?int $num = null): mixed|list; // Returns 1 random item or list of $num random items
$list->concat(mixed $other): list;      // Also: merge()
$list->pluck(string $key): list;
$list->partition(callable $fn): list;
$list->zip(mixed $other): list;

// Higher-Order Transforms & Reductions
$list->map(callable $fn): list;         // fn($item, $index)
$list->filter(?callable $fn = null): list;
$list->reject(callable $fn): list;
$list->reduce(callable $fn, mixed $initial = null): mixed; // fn($acc, $item)
$list->each(callable $fn): list;        // Also: forEach()

// Mathematical Aggregations & Serialization
$list->sum(): int|float;
$list->avg(): float;                    // Also: average()
$list->min(): mixed;
$list->max(): mixed;
$list->toJson(): string;

// Operators & Iteration (Zero-Allocation ActiveIterator::List)
$val = $list[0];                        // 0-indexed integer lookup
$list[1] = 999;                         // In-place numeric update
$list[] = 42;                           // Appends value
// $list['str_key'] = 1;                // Throws UndefinedIndex error (strict sequential indexing)
foreach ($list as $item) {}             // Contiguous sequential traversal with CPU cache locality
foreach ($list as $idx => $item) {}     // Yields 0-based integer index without key allocations
```

---

### `dict` Instance Methods (Ordered String-Key Map)

```php
// Key-Value Retrieval, Projections & Mutation
$dict->get(string $key, mixed $default = null): mixed;
$dict->put(string $key, mixed $value): dict;
$dict->pull(string $key, mixed $default = null): mixed;
$dict->forget(string ...$keys): dict;
$dict->has(string $key): bool;                                  // Also: hasKey()
$dict->hasAny(array $keys): bool;
$dict->keys(): list;                                            // Returns keys in strict insertion order
$dict->values(): list;                                          // Returns values in strict insertion order
$dict->only(array|string ...$keys): dict;
$dict->except(array|string ...$keys): dict;
$dict->flip(): dict;                                            // Swaps keys with values
$dict->merge(dict|array ...$others): dict;                      // Merges multiple dictionaries into a new dict
$dict->all(): dict;                                             // Returns underlying dictionary (also: toArray())
$dict->toArray(): dict;

// Inspection & Search
$dict->count(): int;                                            // Also: size(), length(), len()
$dict->isEmpty(): bool;
$dict->isNotEmpty(): bool;
$dict->contains(mixed|callable $needleOrClosure): bool;
$dict->first(): mixed;                                          // First element in insertion order
$dict->last(): mixed;                                           // Last element in insertion order
$dict->firstKey(): ?string;                                     // First key in insertion order
$dict->lastKey(): ?string;                                      // Last key in insertion order

// Ordering, Slicing & Chunking
$dict->sortKeys(): dict;                                        // Sorts keys ascending (also: ksort())
$dict->sortKeysDesc(): dict;                                    // Sorts keys descending
$dict->sortKeysBy(callable $fn): dict;                          // Custom comparator key sort (also: uksort())
$dict->sort(?callable $comparator = null): dict;                // Sorts by values ascending (also: asort(), uasort(), usort())
$dict->sortBy(string|callable $key): dict;                      // Sorts by custom closure or subkey
$dict->natsort(): dict;                                         // Natural order sorting by values (preserves keys)
$dict->sortNatural(): dict;                                     // Alias for natsort()
$dict->natcasesort(): dict;                                     // Case-insensitive natural order sorting by values
$dict->sortNaturalCase(): dict;                                 // Alias for natcasesort()
$dict->reverse(): dict;                                         // Reverses key-value insertion order
$dict->chunk(int $size): list;                                  // Chunks dictionary into list of dictionaries
$dict->take(int $limit): dict;                                  // Takes first (or last if negative) N entries
$dict->skip(int $count): dict;                                  // Skips first N entries
$dict->slice(int $offset, ?int $length = null): dict;           // Slices dictionary entries

// Functional Higher-Order Transforms & Pipelines
$dict->map(callable $fn): dict;                                 // fn($val, $key) => $newVal
$dict->mapWithKeys(callable $fn): dict;                         // fn($val, $key) => [$newKey => $newVal]
$dict->filter(?callable $fn = null): dict;                      // Keeps entries where fn($val, $key) is truthy
$dict->reject(callable $fn): dict;                              // Drops entries where fn($val, $key) is truthy
$dict->reduce(callable $fn, mixed $initial = null): mixed;      // fn($carry, $val) => $newCarry
$dict->each(callable $fn): dict;                                // fn($val, $key) (short-circuit on false)
$dict->pluck(string $valueKey, ?string $keyBy = null): dict|list;
$dict->column(?string $columnKey, ?string $indexKey = null): dict; // array_column()
$dict->combine(mixed $values): dict;                            // array_combine(): dict values become keys
$dict->countValues(): dict;                                     // array_count_values()
$dict->shuffle(): dict;                                         // Returns dictionary with shuffled entries
$dict->randomKey(int $num = 1): string|list;                    // array_rand(): returns 1 key or list of keys
$dict->random(?int $num = null): mixed|dict;                    // Returns 1 random value or dict of $num random entries

// Set-Like Differences & Intersections
$dict->diff(dict|array ...$others): dict;                       // array_diff(): entries whose values are not in $others
$dict->diffKeys(dict|array ...$others): dict;                   // array_diff_key(): entries whose keys are not in $others
$dict->diffKey(dict|array ...$others): dict;                    // Alias for diffKeys()
$dict->intersect(dict|array ...$others): dict;                  // array_intersect(): entries whose values exist in all $others
$dict->intersectKeys(dict|array ...$others): dict;              // array_intersect_key(): entries whose keys exist in all $others
$dict->intersectKey(dict|array ...$others): dict;               // Alias for intersectKeys()

// Aggregations & Serialization
$dict->sum(): int|float;
$dict->avg(): float;
$dict->min(): mixed;
$dict->max(): mixed;
$dict->toJson(): string;                                        // Encodes to JSON object string '{"k":"v"}'

// Operators & Zero-Allocation Streaming Iteration
$val = $dict['key'];                                            // O(1) string key lookup
$dict['key'] = $value;                                          // Inserts or updates key in-place
foreach ($dict as $key => $val) {}                              // Zero-allocation streaming via ActiveIterator::Dict
```

---

### `set` Instance Methods (Unique Value Collection - Step 10)

Directly mapped to native Rust `HashSet<PhpValue>`. Guaranteed $O(1)$ set membership and duplicate elimination:

```php
// Membership & Insertion
$set->contains(mixed $needle): bool;    // O(1) membership check
$set->has(mixed $needle): bool;         // Alias for contains()
$set->add(mixed $value): set;           // Inserts unique value; returns $set
$set->push(mixed $value): set;          // Alias for add()
$set->remove(mixed $value): bool;       // Removes value; returns true if present
$set->delete(mixed $value): bool;       // Alias for remove()

// Inspection & Export
$set->count(): int;                     // Also: size(), length(), len()
$set->isEmpty(): bool;
$set->isNotEmpty(): bool;
$set->first(): mixed;                   // Retrieves first available element
$set->all(): list;                      // Converts to sequential list
$set->toArray(): list;                  // Alias for all()
$set->values(): list;                   // Alias for all()
$set->toJson(): string;                 // JSON array representation "[...]"

// Mathematical Set Algebra
$set->diff(set|array ...$others): set;  // Relative complement (elements in $this not in $others)
$set->intersect(set|array ...$others): set; // Intersection (elements present in both)
$set->union(set|array ...$others): set; // Set union (elements present in either)

// Natural Sorting & Randomization
$set->natsort(): set;                   // Returns new Set (immutable copy)
$set->natcasesort(): set;               // Returns new Set (immutable copy)
$set->countValues(): dict;              // array_count_values()
$set->shuffle(): set;                   // Returns new Set with shuffled elements
$set->randomKey(int $num = 1): mixed|set; // array_rand(): picks 1 or $num random values
$set->random(?int $num = null): mixed|set; // Picks 1 random value or Set of $num random values

// Higher-Order Transforms & Side Effects
$set->map(callable $fn): set;           // fn($item) => mapped item (deduplicated)
$set->filter(?callable $fn = null): set;// fn($item) => bool (keeps matching items)
$set->reject(callable $fn): set;        // fn($item) => bool (drops matching items)
$set->reduce(callable $fn, mixed $initial = null): mixed; // fn($acc, $item)
$set->each(callable $fn): set;          // fn($item) (side effects)
$set->forEach(callable $fn): set;       // Alias for each()

// Bracket Operators & Syntax
$isMember = $set['apple'];              // O(1) bracket membership check (returns bool)
$set['apple'] = true;                   // Inserts element into set
$set['apple'] = false;                  // Removes element from set
$set[] = 'banana';                      // Push syntax: inserts unique element

// Native foreach Iteration
foreach ($set as $item) {               // Iterates through each unique element
    // $item
}
foreach ($set as $idx => $item) {       // Iterates with 0-based sequence counter
    // $idx, $item
}
```

---

### `hashmap` / `map` Instance Methods (Universal Complex Key-Value Map - Step 11)

Directly mapped to native Rust `HashMap<PhpValue, PhpValue>`. Universal general key-value dictionary supporting arbitrary complex keys (arrays, objects, sets, scalars):

```php
// Key-Value Retrieval & Mutation
$map->get(mixed $key, mixed $default = null): mixed;      // Retrieves value by key
$map->has(mixed $key): bool;                              // O(1) key existence check
$map->contains(mixed $key): bool;                         // Alias for has()
$map->hasKey(mixed $key): bool;                           // Alias for has()
$map->put(mixed $key, mixed $value): hashmap;             // Inserts or replaces key-value
$map->pull(mixed $key, mixed $default = null): mixed;     // Removes key and returns value
$map->remove(mixed ...$keys): hashmap;                    // Removes one or more keys
$map->forget(mixed ...$keys): hashmap;                    // Alias for remove()

// Key/Value/Entry Projections
$map->keys(): list;                                       // List of all keys (including complex keys)
$map->values(): list;                                     // List of all values
$map->entries(): list;                                    // List of [[$key, $value], ...] pairs
$map->all(): list;                                        // Alias for entries()
$map->toArray(): list;                                    // Alias for entries()

// Inspection & Count
$map->count(): int;                                       // Also: size(), length(), len()
$map->isEmpty(): bool;
$map->isNotEmpty(): bool;
$map->first(): mixed;                                     // Retrieves first value in iteration order
$map->toJson(): string;                                   // JSON object representation

// Higher-Order Transforms & Combinators
$map->map(callable $fn): hashmap;                         // fn($value, $key) => transformed value
$map->mapWithKeys(callable $fn): hashmap;                 // fn($value, $key) => [$newK, $newV]
$map->filter(?callable $fn = null): hashmap;              // fn($value, $key) => bool
$map->reject(callable $fn): hashmap;                      // fn($value, $key) => bool
$map->reduce(callable $fn, mixed $initial = null): mixed; // fn($acc, $value, $key)
$map->diff(hashmap|dict|set|array $other): hashmap;       // Key-based diff
$map->intersect(hashmap|dict|set|array $other): hashmap;  // Key-based intersect
$map->merge(hashmap|dict ...$others): hashmap;            // Merges maps/dicts into new hashmap
$map->each(callable $fn): hashmap;                        // fn($value, $key) (side effects)
$map->forEach(callable $fn): hashmap;                     // Alias for each()

// Complex Key Types Supported
// 1. Array / List as key:
$map[[10, 20]] = 'Point A';
$pointA = $map[[10, 20]];

// 2. Object / Class instance as key:
$user = new User(1);
$map[$user] = 'User Session Data';
$data = $map[$user];

// 3. Set as key:
$map[Arr::set([1, 2])] = 'Coordinate Pair';

// 4. Scalar keys:
$map[42] = 'int';
$map[3.14] = 'float';
$map['slug'] = 'string';
$map[true] = 'bool';

// Native foreach Iteration with Complex Keys
foreach ($map as $key => $val) {                          // $key yields complex key directly!
    // $key can be an array [x, y], object, set, or scalar
}
foreach ($map as $val) {                                  // Iterates values only
    // $val
}
```

---

## `Str` Facade & String Instance API

```php
// Static Facade Methods & Constructors
Str::slug(string $text, string $separator = '-'): string;
Str::contains(string $haystack, string $needle): bool;
Str::startsWith(string $haystack, string $prefix): bool;
Str::endsWith(string $haystack, string $suffix): bool;
Str::upper(string $text): string;
Str::lower(string $text): string;
Str::trim(string $text, string $chars = " \t\n\r\0\x0B"): string;
Str::ltrim(string $text, string $chars = " \t\n\r\0\x0B"): string;
Str::rtrim(string $text, string $chars = " \t\n\r\0\x0B"): string;
Str::substr(string $text, int $start, ?int $length = null): string;
Str::pad(string $s, int $length, string $pad = " ", string|int $side = "right"): string; // mb-safe padding (left, right, both)
Str::repeat(string $text, int $times): string;
Str::countOccurrences(string $haystack, string $needle): int; // Count substring frequency
Str::replaceAt(string $s, string $replacement, int $start, ?int $length = null): string; // Replace by position
Str::reverse(string $text): string;
Str::random(int $length = 16): string;
Str::join(string $glue, array $pieces): string;
Str::split(string $delimiter, string $string): list;
Str::replace(string $search, string $replace, string $subject): string;
Str::find(string $haystack, string $needle, int $offset = 0): ?int;
Str::compare(mixed $a, mixed $b, bool $caseInsensitive = false): bool; // String coercion comparison (optional case-insensitive)
Str::natCompare(string $string1, string $string2): int;                    // Natural order string comparison (strnatcmp, < 0, 0, > 0)
Str::natCaseCompare(string $string1, string $string2): int;                // Case-insensitive natural order string comparison (strnatcasecmp)
Str::strcmp(string $string1, string $string2): int;                        // Binary-safe string comparison (< 0, 0, > 0)
Str::strcasecmp(string $string1, string $string2): int;                    // Binary-safe case-insensitive string comparison
Str::strncmp(string $string1, string $string2, int $length): int;          // Binary-safe string comparison of first n characters
Str::strncasecmp(string $string1, string $string2, int $length): int;      // Binary-safe case-insensitive comparison of first n characters
Str::digit(mixed $value): bool;                                        // Checks if string contains only digits [0-9]
Str::alpha(mixed $value): bool;                                        // Checks if string contains only alphabetic characters [a-zA-Z]
Str::alnum(mixed $value): bool;                                        // Checks if string contains only alphanumeric characters [a-zA-Z0-9]

// Multi-Byte Encodings & Binary Constructors
Str::fromEncoding(string|array $bytes, string $encoding): string; // Decodes Shift-JIS, EUC-JP, ISO-2022-JP, UTF-8
Str::toEncoding(string $text, string $encoding): string;           // Encodes to target encoding raw bytes
Str::utf8(string|array $input): string;                            // Ensures UTF-8 compliance from raw bytes
Str::isUtf8(string $text): bool;                                   // Checks if bytes are valid UTF-8
Str::fromBytes(array|list $bytes): string;                         // Constructs binary string from byte values [0..255]
Str::bytes(string $text): list;                                    // Returns list of byte integers [0..255]
Str::byteLength(string $text): int;                                // Returns byte length (binary length)
Str::encoding(string $text): string;                               // Detects probable encoding
Str::macro(string $name, callable $closure): void;                 // Registers dynamic macro bound to string instances

// Fluent Immutable Instance Methods ($text->trim()->upper())
$str->trim(?string $chars = null): string;
$str->ltrim(?string $chars = null): string;
$str->rtrim(?string $chars = null): string;
$str->upper(): string;
$str->lower(): string;
$str->slug(): string;
$str->contains(string $needle): bool;
$str->startsWith(string $prefix): bool;
$str->endsWith(string $suffix): bool;
$str->find(string $needle, int $offset = 0): ?int;
$str->length(): int;
$str->byteLength(): int;
$str->split(string $delimiter): list;
$str->replace(string $search, string $replace): string;
$str->substr(int $start, ?int $length = null): string;
$str->reverse(): string;
$str->repeat(int $times): string;
$str->toEncoding(string $encoding): string;
$str->utf8(): string;
$str->bytes(): list;
$str->isUtf8(): bool;
$str->isEmpty(): bool;
$str->compare(mixed $other, bool $caseInsensitive = false): bool;
$str->strcmp(string $other): int;
$str->strcasecmp(string $other): int;
$str->strncmp(string $other, int $length): int;
$str->strncasecmp(string $other, int $length): int;
$str->digit(): bool;
$str->alpha(): bool;
$str->alnum(): bool;
```

---

## Primitive Comparison Facades (`Bool`, `Int`, `Float`)

RHP provides dedicated facades for explicit, loose equality comparisons across differing scalar types:

```php
// Boolean truthiness comparison
Bool::compare(mixed $a, mixed $b): bool;
// Coerces both operands to boolean truthiness before comparing:
Bool::compare(1, true);       // true
Bool::compare("", false);     // true
Bool::compare("0", false);    // true
Bool::compare("hello", true); // true

// Integer numeric utility & comparison
Int::compare(mixed $a, mixed $b): bool;
// Coerces strings, floats, and booleans to 64-bit signed integers:
Int::compare("42", 42);       // true
Int::compare(true, 1);        // true
Int::compare(3.9, 3);         // true (truncates float)
Int::abs(int $n): int;                                         // Absolute value
Int::min(int ...$n): int;                                      // Variadic or array min: Int::min(1, 2) or Int::min([1, 2])
Int::max(int ...$n): int;                                      // Variadic or array max: Int::max(1, 2) or Int::max([1, 2])
Int::random(int $min = 0, int $max = 100): int;                // CSPRNG random integer (uses /dev/urandom internally)

// Integer Non-Static / Fluent Operations
int $n = 10;
$n->add(5);      // 15
$n->sub(3);      // 7
$n->mul(4);      // 40
$n->div(2);      // 5
$n->div(0);      // Throws \DivisionByZeroError (catchable in try-catch)

// Float numeric utility & precision comparison
Float::compare(mixed $a, mixed $b, ?float $epsilon = null): bool;
// If $epsilon is omitted or null, dynamically checks maximum decimal places between both operands
// and compares formatted representations. If $epsilon is supplied, checks abs($a - $b) <= $epsilon:
Float::compare("3.14", 3.14);             // true
Float::compare(10, 10.0);                 // true
Float::compare(0.1 + 0.2, 0.3);           // true (handles IEEE 754 precision)
Float::compare(1.0001, 1.0002, 0.001);    // true (within epsilon tolerance)
Float::format(float $num, int $decimals = 0, string $decPoint = '.', string $thousandsSep = ','): string; // Formats number
Float::abs(float $n): float;                                   // Absolute value
Float::min(float ...$n): float;                                // Variadic or array min
Float::max(float ...$n): float;                                // Variadic or array max
Float::random(float $min = 0.0, float $max = 1.0): float;      // CSPRNG random float in [min, max]

// Float Binary Format Operations (IEEE-754 64-bit Floating-Point)
float $x = 0.1;
$x->bAdd(0.2);   // float(0.30000000000000004)
$x->bSub(0.05);  // float(0.05)
$x->bMul(2.0);   // float(0.2)
$x->bDiv(2.0);   // float(0.05)
$x->bDiv(0.0);   // Throws \DivisionByZeroError

// Float Arbitrary-Precision Decimal Math (Exact Base-10 Arithmetic, BCMath-style)
$x->add(0.2);    // float(0.3) - exact decimal representation without binary precision drift
$x->sub(0.05);   // float(0.05)
$x->mul(0.2);    // float(0.02)
0.3->div(0.1);   // float(3)
$x->div(0.0);    // Throws \DivisionByZeroError

// Arbitrary-Precision Static & Procedural Math (bcmath bridge)
Float::add('0.1', '0.2');      // '0.3' (or bcadd('0.1', '0.2'))
Float::sub('0.3', '0.2');      // '0.1' (or bcsub('0.3', '0.2'))
Float::mul('0.1', '0.2');      // '0.02' (or bcmul('0.1', '0.2'))
Float::div('0.3', '0.1', 2);   // '3.00' (or bcdiv('0.3', '0.1', 2))
Float::bccomp('0.3', '0.30');  // 0 (or bccomp('0.3', '0.30'))
```

---

## `Future` Facade & Instance API

```php
// Static Methods
Future::run(callable $fn): Future;
Future::wait(array $tasks): array;
Future::any(array $tasks): mixed;
Future::delayed(int $milliseconds, callable $fn = null): mixed;
Future::defer(callable $fn): void;
Future::value(mixed $value): Future;
Future::error(mixed $error): Future;

// Instance Properties & State Inspection (Matches Rust Poll::Pending / Poll::Ready)
$future->state;                 // string: "Pending" | "Ready"
$future->status;                // string: "Pending" | "Ready"
$future->getState(): string;    // "Pending" | "Ready"
$future->isPending(): bool;     // true if background Tokio task is running
$future->isReady(): bool;       // true if completed
$future->isCompleted(): bool;   // true if completed
$future->poll(): mixed;         // Non-blocking poll: returns null if pending, or resolved value
$future->wait(): mixed;         // Synchronously waits for completion and returns resolved value
```

---

## `File` Facade API (Synchronous Filesystem & Uploads)

```php
File::read(string $path): string;                    // Read file contents as string
File::write(string $path, string $content): bool;    // Write string to file
File::exists(string $path): bool;                   // Check if file exists
File::delete(string $path): bool;                   // Remove file
File::copy(string $from, string $to): bool;         // Copy file from source to destination
File::rename(string $from, string $to): bool;       // Rename or move file (alias: move)
File::mkdir(string $path, string $mode = "0755", bool $recursive = false): bool; // Create directory (accepts "0755" or "rwxr-xr-x")
File::makeDirectory(string $path, string $mode = "0755", bool $recursive = false, bool $force = false): bool; // Create directory (Laravel alias)
File::parseMode(string $mode): int|false;           // Parse octal string ("0755") or symbolic string ("rwxr-xr-x") to integer mode
File::chmod(string $path, ?string $mode = null): mixed;// Set or inspect file permission mode (accepts "0755" or "rwxr-xr-x")
File::chown(string $path, string|int $user, string|int $group = null): bool; // Change file owner
File::touch(string $path, ?int $time = null, ?int $atime = null): bool;      // Create file or update timestamp
File::mimeType(string $path): string|false;         // Sniff magic bytes & detect MIME type (like Laravel)
File::guessExtension(string $pathOrMime): ?string;  // Guess extension from MIME type or file content
File::extension(string $path): string;              // Extract file extension from path
File::type(string $path): string;                   // Get file type: "file", "dir", or "link"
File::size(string $path): int;                      // Get file size in bytes
File::name(string $path): string;                   // Get filename without extension (file stem)
File::basename(string $path): string;               // Get trailing name component with extension
File::dirname(string $path, int $levels = 1): string; // Get directory name (supports traversal levels)
File::realpath(string $path): string|false;         // Canonicalize absolute path (alias: realPath, canonicalize)
File::info(string $path): dict;                     // Normalized pathinfo dict with dirname, basename, extension, filename
File::modifiedAt(string $path): int;                // File modification Unix timestamp (throws if missing)
File::isReadable(string $path): bool;               // Check if file is readable
File::isWritable(string $path): bool;               // Check if file is writable
File::isFile(string $path): bool;                   // Check if path is a regular file
File::isDirectory(string $path): bool;              // Check if path is a directory
File::sysGetTempDir(): string;                       // Get system temporary directory (alias: sys_get_temp_dir)
File::tempnam(?string $dir = null, ?string $prefix = null): string|false; // Create unique temporary file (alias: createTempFile, tempFile)
File::createTempDirectory(?string $dir = null, ?string $prefix = null, string $mode = "0700"): string|false; // Create temporary directory (alias: tempDirectory, createTempDir)
File::tmpfile(): resource|false;                     // Create temporary file opened in w+b mode (auto-deleted on close)
File::writeStream(resource $stream, string $data, ?int $length = null): int|false; // Write data to stream resource (alias: fwrite)
File::readStream(resource $stream, int $length): string|false; // Read bytes from stream resource (alias: fread)
File::fromRequest(?string $key = null): mixed;      // Retrieve uploaded file from request (UploadedFile)

// UploadedFile instance methods ($file = File::fromRequest('avatar')):
$file->path(): string;                              // Temporary uploaded file path
$file->clientName(): string;                        // Original client filename
$file->size(): int;                                 // File size in bytes
$file->mimeType(): string;                          // Detected MIME type via content sniffing
$file->clientMimeType(): string;                    // Client-provided MIME type
$file->extension(): string;                         // Client file extension
$file->guessExtension(): ?string;                   // Guessed extension from detected MIME type
$file->isValid(): bool;                             // Check if uploaded file is valid
$file->move(string $dir, ?string $name = null): string; // Move/persist uploaded file to destination
```

> **Note on `$_FILES`:**
> `$_FILES` is **not supported** in RHP. Retrieve uploaded files using `File::fromRequest('upload_field')` or `Request::file('upload_field')`.

### File Permissions & Directory Creation (`File::parseMode`, `File::mkdir`, `File::chmod`)

In RHP, file permission helper functions strictly enforce string formats to prevent security vulnerabilities and bugs caused by integer/octal confusion (such as passing a literal integer `0755` or accidental decimal `755`). Non-string arguments or invalid permission patterns return `false`.

#### Supported Permission String Formats

1. **Octal strings**:
   - `"0755"`, `"0777"`, `"0644"`, `"0700"` (standard 4-digit octal format)
   - `"755"`, `"777"`, `"644"` (standard 3-digit octal format)
   - `"0o755"`, `"0o644"` (prefixed octal string)
2. **POSIX symbolic strings**:
   - `"rwxr-xr-x"`, `"rwxrwxrwx"`, `"rw-r--r--"` (9-character format)
   - `"-rwxr-xr-x"`, `"drwxr-xr-x"` (10-character format with leading type marker)

#### `File::parseMode(string $mode): int|false`

Converts an octal or symbolic permission string into a numeric bitmask, returning `false` if the format is invalid or if a non-string argument is supplied:

```php
File::parseMode("0755");        // 493 (0o755)
File::parseMode("rwxr-xr-x");    // 493
File::parseMode("0644");        // 420 (0o644)
File::parseMode("rw-r--r--");    // 420
File::parseMode(0755);          // false (integers are strictly rejected)
File::parseMode("invalid");     // false
```

#### `File::mkdir` & `File::makeDirectory`

```php
File::mkdir(string $path, string $mode = "0755", bool $recursive = false): bool
File::makeDirectory(string $path, string $mode = "0755", bool $recursive = false, bool $force = false): bool
```

Creates a directory with the specified permissions. Internally delegates permission validation to `File::parseMode($mode)`:
- If `parseMode($mode)` fails (e.g. invalid string format or an integer is passed), `mkdir` returns `false`.
- If directory creation fails (e.g. directory already exists, or intermediate parent directory does not exist and `$recursive` is `false`), it returns `false`.
- Legacy procedural `mkdir($path, $mode = "0755", $recursive = false)` and `parse_mode($mode)` route directly to these facade implementations.

```php
// Octal string mode
File::mkdir("/tmp/cache", "0755"); // true

// Symbolic permission string
File::mkdir("/tmp/logs", "rwxr-xr-x"); // true

// Recursive directory creation
File::mkdir("/tmp/nested/app/dir", "0755", true); // true

// Integer mode or invalid syntax returns false
File::mkdir("/tmp/bad", 0755);       // false (integers are rejected)
File::mkdir("/tmp/bad", "invalid");  // false
```

#### `File::chmod` & Procedural `chmod`

```php
File::chmod(string $path, ?string $mode = null): mixed
```

- When `$mode` is supplied: sets the permission bitmask for `$path` using string format (`"0755"` or `"rwxr-xr-x"`). Returns `true` on success, or `false` if `parseMode` fails or permissions cannot be set.
- When `$mode` is omitted or `null`: inspects the current file permissions and returns an octal string representation (e.g. `"0755"`).

### Temporary Files & Directories (`File::tempnam`, `File::tmpfile`, `File::sysGetTempDir`, `File::createTempDirectory`)

RHP provides safe, high-performance temporary file and directory creation with cross-platform compatibility and automatic resource cleanup.

#### `File::sysGetTempDir(): string` (or procedural `sys_get_temp_dir()`)

Returns the normalized directory path used by the operating system for temporary files (e.g. `"/tmp"` on Linux/macOS or `C:\Users\...\AppData\Local\Temp` on Windows) without a trailing slash.

```php
string $tmpDir = File::sysGetTempDir(); // "/tmp"
string $dir = sys_get_temp_dir();       // Identical procedural helper
```

#### `File::tempnam(?string $dir = null, ?string $prefix = null): string|false` (or procedural `tempnam()`)

Creates a unique temporary file with restricted permissions (`0600` on Unix) and returns its absolute path.
- If `$dir` is omitted, empty, or does not exist, it automatically falls back to `sys_get_temp_dir()`.
- The filename begins with `$prefix` (defaults to `"tmp"`).
- Aliases: `File::createTempFile()`, `File::tempFile()`.

```php
// Create unique temp file in system temp dir
string $path = File::tempnam(null, "cache_"); // "/tmp/cache_5f8a12c0.tmp"

// Procedural tempnam with custom directory and prefix
string $logTmp = tempnam("/var/log/app", "worker_");

// File exists immediately and is ready for use
File::write($path, "payload");
```

#### `File::tmpfile(): resource|false` (or procedural `tmpfile()`)

Creates a temporary file with a unique name in read-write (`w+b`) mode and returns an active stream resource handle.
- The file is **automatically deleted** from disk when closed via `fclose($stream)` (or `File::close($stream)`), or when the script terminates.
- Aliases: `File::createTempStream()`, `File::tempStream()`.

```php
// Create temporary read/write stream
$stream = tmpfile(); // or File::tmpfile()

// Write and rewind
fwrite($stream, "temporary buffered data");
rewind($stream);

// Read back from stream
$content = fread($stream, 1024); // "temporary buffered data"

// Closing stream immediately removes the file from disk
fclose($stream);
```

#### `File::createTempDirectory(?string $dir = null, ?string $prefix = null, string $mode = "0700"): string|false`

Creates a unique temporary directory with secure permissions.
- Accepts standard string permissions (e.g. `"0700"`, `"0755"` or `"rwxr-xr-x"`). Integer modes return `false`.
- If `$dir` is omitted or does not exist, falls back to `sys_get_temp_dir()`.
- Aliases: `File::tempDirectory()`, `File::createTempDir()`.

```php
// Create temp directory with default "0700" mode
string $workspace = File::createTempDirectory(null, "build_"); // "/tmp/build_a9f1b2.dir"

// Create with explicit permission string
string $publicTmp = File::createTempDirectory("/tmp", "export_", "0755");
```

### Canonical Path Resolution (`File::realpath`, `File::realPath`, `File::canonicalize`, procedural `realpath`)

Resolves symbolic links, relative segments (`.` and `..`), and canonicalizes absolute pathnames. Returns the canonical absolute path as a `string`, or `false` if the target path or any intermediate component does not exist.

```php
string|false $real = File::realpath("./src/../Cargo.toml"); // "/home/user/project/Cargo.toml"
string|false $missing = realpath("/invalid/non_existent_path"); // false

// Resolving parent directories with optional traversal levels
string $parent = File::dirname("/var/www/html/index.php", 2); // "/var/www"
```

---

## `AFile` Facade API (Asynchronous Tokio Filesystem)

```php
AFile::read(string $path): Future;                  // Resolves to string (file contents)
AFile::write(string $path, string $content): Future; // Resolves to bool
AFile::exists(string $path): Future;                 // Resolves to bool
AFile::delete(string $path): Future;                 // Resolves to bool
AFile::copy(string $from, string $to): Future;        // Resolves to bool
AFile::rename(string $from, string $to): Future;      // Resolves to bool
AFile::append(string $path, string $content): Future;// Resolves to bool
AFile::size(string $path): Future;                   // Resolves to int (byte size)
AFile::mimeType(string $path): Future;               // Resolves to string|false (detected MIME type)
AFile::guessExtension(string $path): Future;         // Resolves to ?string (guessed extension)
AFile::extension(string $path): Future;              // Resolves to string (file extension)
AFile::type(string $path): Future;                   // Resolves to string ("file" | "dir")
AFile::realpath(string $path): Future;               // Resolves to string|false (canonicalized path)
AFile::mkdir(string $path, string $mode = "0755", bool $recursive = false): Future; // Resolves to bool
AFile::chmod(string $path, ?string $mode = null): Future; // Resolves to bool|string
AFile::sysGetTempDir(): Future;                      // Resolves to string (system temp directory)
AFile::tempnam(?string $dir = null, ?string $prefix = null): Future; // Resolves to string|false
AFile::createTempDirectory(?string $dir = null, ?string $prefix = null, string $mode = "0700"): Future; // Resolves to string|false
```

---

## `AHTTP` Facade API (Asynchronous Tokio HTTP Client)

```php
AHTTP::get(string $url): Future;                    // Resolves to HTTPResponse
AHTTP::post(string $url, mixed $body = null): Future;
AHTTP::put(string $url, mixed $body = null): Future;
AHTTP::delete(string $url): Future;
AHTTP::patch(string $url, mixed $body = null): Future;
AHTTP::withHeaders(dict $headers): AHTTPClient;     // Returns fluent client
```

---

## `ADB` Facade API (Asynchronous Tokio Database)

```php
ADB::table(string $table): ADBQuery;                // Returns fluent async query builder
ADB::find(string $table, mixed $id): Future;
ADB::get(string $table): Future;
ADB::select(string $query, array $bindings = []): Future;
ADB::insert(string $table, dict $values): Future;
ADB::update(string $table, dict $values): Future;
ADB::delete(string $table): Future;
ADB::query(string $sql, array $bindings = []): Future;
ADB::raw(string $sql): Future;

// Fluent ADBQuery Instance Methods (all terminal operations return Future)
$query->where(string $col, mixed $val): ADBQuery;
$query->orderBy(string $col, string $dir = 'ASC'): ADBQuery;
$query->limit(int $limit): ADBQuery;
$query->find(mixed $id): Future;
$query->first(): Future;
$query->get(): Future;
$query->count(): Future;
$query->insert(dict $data): Future;
$query->update(dict $data): Future;
$query->delete(): Future;
```

---

## `Memory` Facade API

```php
Memory::usage(): int;            // Current memory allocations in bytes
Memory::peak(): int;             // Peak memory allocation in bytes
Memory::isGcFree(): bool;        // Strictly returns true
Memory::gcRuns(): int;           // Strictly returns 0
Memory::gcPauseMs(): float;      // Strictly returns 0.0
Memory::cowDetachments(): int;   // Total COW detachment count
Memory::scope(callable $fn): mixed;
Memory::leakCheck(): dict;
Memory::arenaStats(): dict;
Memory::resetArena(): dict;
```

---

## `Request` Facade API

```php
// Parameter Inspection
Request::get(?string $key = null, mixed $default = null): mixed;
Request::all(): dict;
Request::only(array|string ...$keys): dict;
Request::has(string $key): bool;
Request::set(string $key, mixed $value): string;
Request::merge(array|dict $data): dict;
Request::clear(): void;

// In-Place Type Formatting (supports both quoted types and bare type keywords)
Request::format(array|dict $typeMap): dict;
```

---

## `Session` Facade API

```php
Session::get(?string $key = null, mixed $default = null): mixed;
Session::set(string $key, mixed $value): mixed;
Session::put(string $key, mixed $value): mixed;
Session::has(string $key): bool;
Session::all(): dict;
Session::forget(string $key): void;
Session::clear(): void;
```

> **Note on `$_SESSION`:**
> `$_SESSION` is **not supported** in RHP. Use `Session::get('key')` / `Session::set('key', $value)` to manage thread-isolated session state.

---

## `Cookie` Facade API

```php
Cookie::get(?string $key = null, mixed $default = null): mixed;
Cookie::set(string $key, mixed $value, ...$options): string;
Cookie::put(string $key, mixed $value, ...$options): string;
Cookie::has(string $key): bool;
Cookie::all(): dict;
Cookie::forget(string $key): void;
Cookie::clear(): void;
```

> **Note on `$_COOKIE`:**
> `$_COOKIE` is **not supported** in RHP. Use `Cookie::get('name')` / `Cookie::set('name', $value)` for HTTP cookie management.

---

## `Sys` Facade API

```php
Sys::echo(string $format, mixed ...$args): void;  // Directly compiles to Rust print!() (no trailing newline)
Sys::print(string $format, mixed ...$args): void; // Directly compiles to Rust println!() (adds trailing \n)
Sys::env(string $key, mixed $default = null): mixed;
Sys::os(): string;                               // Host OS name (e.g. "linux")
Sys::version(): string;                          // RHP engine version
Sys::timestamp(): int;                           // Current Unix epoch in seconds
Sys::clearstatcache(bool $clear_realpath_cache = false, string $filename = ""): void; // Clear stat and realpath cache
Sys::disk_free_space(string $directory): float|bool; // Free disk space in bytes (or false on failure)
Sys::disk_total_space(string $directory): float|bool; // Total disk space in bytes (or false on failure)
Sys::dd(mixed ...$vars): void;                   // Dump variables and terminate

// NOTE: There is NO Sys::println function. Only echo and print exist in RHP.
```

> **Note on `$_ENV`:**
> `$_ENV` is **not supported** in RHP. Read environment variables via `env('KEY')` or `Sys::env('KEY')`.

### Formatting Syntax & Direct Parameter Mapping

Both `Sys::echo` and `Sys::print` directly map to Rust's native `print!` and `println!` routines with function parameters passed as-is. C-style format specifiers (`%d`, `%s`, `%f`, etc.) are **not** supported.

```php
// Value placeholder
Sys::print("Age: {}, Name: {}", 30, "Bob"); // Age: 30, Name: Bob\n

// Floating-point precision
Sys::print("{:.2}", 3.141592);              // 3.14\n

// Integer padding with zeros
Sys::print("{:05}", 42);                    // 00042\n

// Hexadecimal lowercase
Sys::print("{:x}", 255);                    // ff\n

// Binary formatting
Sys::print("{:b}", 5);                      // 101\n

// Inline output on the same line
Sys::echo("Age: {}, Name: {}", 30, "Bob");  // Age: 30, Name: Bob (same line)
```

---

## `Debug` Facade API

The `Debug` facade provides zero-overhead diagnostic inspection for human consumption, backed directly by Rust's `std::fmt::Debug` machinery.

```php
// Pretty-prints values to stdout/stderr with syntax highlights and structure indentation
Debug::dump(mixed ...$vars): void;

// Dumps values and halts execution immediately (standard Laravel dd idiom)
Debug::dd(mixed ...$vars): void;

// Returns the formatted diagnostic inspection as a string (replaces var_export($x, true) and print_r($x, true))
Debug::format(mixed $value): string;
```

> **Why `var_export` Was Dropped in Favor of `Debug::format`:**
> `var_export` literally meant "export as PHP source code", which was historically used in PHP to generate loadable array cache files. RHP binaries cannot and do not parse PHP source at runtime. `Debug::format` cleanly isolates the string-formatting job, safely rendering non-serializable constructs (closures, resources, recursive cycles) as diagnostic tokens like `<closure: fn(int): string>`, `<resource #fd:3>`, or `<recursion: *1>`.

---

## `Json` Facade API

The `Json` facade provides high-performance, structured text serialization powered by Rust's `serde` and `serde_json` engines.

```php
// Serializes any data structure or typed class into a JSON string
Json::encode(mixed $value, bool $pretty = false): string;

// Deserializes a JSON string into native RHP scalars, lists, and dicts
Json::decode(string $json, bool $associative = true): mixed;

// Validates whether a given string is valid JSON without allocating memory
Json::validate(string $json): bool;
```

---

## `Bin` Facade API

The `Bin` facade provides compact, ultra-fast binary serialization and cache persistence powered by Rust's `serde` and `bincode` / `postcard` format.

```php
// Encodes any data structure or typed class into compact binary bytes
Bin::encode(mixed $value): bytes;

// Decodes compact binary bytes back into typed instances or collections
Bin::decode(bytes $data): mixed;

// Returns the raw byte length of a binary payload
Bin::size(bytes $data): int;
```

> **Why Binary Cache Wins Over PHP Source File Caching:**
> Because class property types are mandatory in RHP, the compiler automatically generates serialization routines for every class (like Rust's `#[derive(Serialize, Deserialize)]`). Persisting application manifests, configuration, and routes as compact binary allows worker processes to load cached state via instant zero-copy memory mapping &mdash; at least $10\times$ faster than tokenizing and executing generated source code.
>
> If source code literal generation is ever genuinely needed for compiler or code-generation tooling, RHP reserves `Code::literal($x)` for future release.

---

## `Validate` Facade API

```php
Validate::null(mixed $value): bool;       // Returns true if value === null
Validate::int(mixed $value): bool;        // Returns true if value is integer (i32 or RustInt)
Validate::bool(mixed $value): bool;       // Returns true if value is boolean
Validate::float(mixed $value): bool;      // Returns true if value is float (f32 or RustFloat)
Validate::numeric(mixed $value): bool;    // Returns true if value is number or valid numeric string
Validate::scalar(mixed $value): bool;     // Returns true if value is int, float, bool, or string
Validate::resource(mixed $value): bool;   // Returns true if value is an open stream or system resource
```

---

## `Csv` Facade API

The `Csv` facade provides high-performance, stream-oriented reading, writing, and parsing of comma-separated values (CSV) conforming to RFC 4180. It supports stream processing for large files, multi-line quoted fields, custom delimiters and enclosures, and in-memory streams.

```php
// Stream processing methods (fgetcsv & fputcsv)
Csv::fgetcsv(resource $stream, ?int $length = null, string $separator = ",", string $enclosure = "\"", string $escape = "\\"): array|false;
Csv::fputcsv(resource $stream, array $fields, string $separator = ",", string $enclosure = "\"", string $escape = "\\", string $eol = "\n"): int|false;

// Stream lifecycle & control
Csv::open(string $path, string $mode = "r"): resource|false;
Csv::close(resource $stream): bool;
Csv::feof(resource $stream): bool;
Csv::rewind(resource $stream): bool;
Csv::memory(?string $initialData = ""): resource;

// Batch string & file helpers
Csv::parse(string $content, string $separator = ",", string $enclosure = "\"", string $escape = "\\"): array;
Csv::parseRow(string $line, string $separator = ",", string $enclosure = "\"", string $escape = "\\"): array;
Csv::generate(array $rows, string $separator = ",", string $enclosure = "\"", string $escape = "\\", string $eol = "\n"): string;
Csv::formatRow(array $fields, string $separator = ",", string $enclosure = "\"", string $escape = "\\", string $eol = "\n"): string;
Csv::readFile(string $path, string $separator = ",", string $enclosure = "\"", string $escape = "\\"): array;
Csv::writeFile(string $path, array $rows, string $separator = ",", string $enclosure = "\"", string $escape = "\\", string $eol = "\n"): bool;
```

## `Glob` Facade API

```php
Glob::match(string $pattern, string $subject): bool;           // Regex pattern matching
Glob::getClass(mixed $object): string;                         // Inspect object class name
Glob::customHelper(string $helperName, mixed ...$args): mixed; // Dynamic fallback registry for unresolved global functions
```

> **Note on `$GLOBALS`:**
> `$GLOBALS` is **also not supported** in RHP. Unresolved global functions are dispatched through `Glob::customHelper()`, and global variable scope within functions is bound using lexical `global $var;` declarations.

---

## `Trace` & `Telemetry` Facade API (OpenTelemetry)

```php
// Span Lifecycle
Trace::startSpan(string $name, ?array $options = null): dict;
Trace::endSpan(?array $status = null): ?dict;
Trace::span(string $name, callable $closure, ?array $attributes = null): mixed;
Trace::setAttribute(string $key, mixed $value): void;
Trace::addEvent(string $name, ?array $attributes = null): void;
Trace::recordException(string $type, string $message, ?string $stackTrace = null): void;

// Context & Identifier Inspection
Trace::traceId(): ?string;          // Active 32-hex character TraceId
Trace::spanId(): ?string;           // Active 16-hex character SpanId
Trace::parentSpanId(): ?string;     // Active 16-hex parent SpanId (if any)
Trace::isRecording(): bool;         // true if active span is recording

// Metrics & Histograms
Trace::counter(string $name, float $delta = 1.0, ?array $attributes = null): void;
Trace::gauge(string $name, float $value, ?array $attributes = null): void;
Trace::histogram(string $name, float $value, ?array $attributes = null): void;
Trace::metricSummary(string $name): ?dict; // Returns [count, sum, min, max, avg, p50, p90, p95, p99]

// OTLP & APM Exporters
Trace::exportOtlp(): string;        // Canonical OTLP/HTTP JSON (/v1/traces)
Trace::exportOtlpMetrics(): string; // Canonical OTLP/HTTP JSON (/v1/metrics)
Trace::exportSentry(string $type, string $message): string; // Sentry crash event JSON
Trace::exportDatadog(): string;     // Datadog APM trace JSON
```

---

## `Crypt` Facade API (Cryptographic Services)

```php
// Standard Hashing
Crypt::hash(string $algo, string $data, bool $binary = false): string;
Crypt::md5(string $data, bool $binary = false): string;
Crypt::sha1(string $data, bool $binary = false): string;
Crypt::sha256(string $data, bool $binary = false): string;
Crypt::sha512(string $data, bool $binary = false): string;

// HMAC Message Authentication (RFC 2104)
Crypt::hashHmac(string $algo, string $data, string $key, bool $binary = false): string;

// OpenSSL AES Encryption / Decryption
Crypt::opensslEncrypt(string $data, string $cipherAlgo, string $passphrase, int $options = 0, string $iv = ''): string|false;
Crypt::opensslDecrypt(string $data, string $cipherAlgo, string $passphrase, int $options = 0, string $iv = ''): string|false;

// CSPRNG Random Generation
Crypt::randomBytes(int $length = 16): string;
Crypt::randomHex(int $length = 32): string;

// Introspection & Macros
Crypt::hashAlgos(): list;
Crypt::opensslCipherMethods(): list;
Crypt::macro(string $name, callable $closure): bool;
```

---

## `Html` Facade API (Web Sanitization & Helpers)

```php
// Escaping & Decoding
Html::specialChars(string $string, int $flags = 11, ?string $encoding = 'UTF-8', bool $doubleEncode = true): string;
Html::specialCharsDecode(string $string, int $flags = 11): string;
Html::escape(string $string): string; // Alias for specialChars()

// Tag Stripping & Sanitization
Html::stripTags(string $string, array|list|string|null $allowedTags = null): string;
Html::clean(string $string): string; // Strips tags and escapes special characters

// Entity Transformation
Html::entities(string $string, int $flags = 11, ?string $encoding = 'UTF-8', bool $doubleEncode = true): string;
Html::entityDecode(string $string, int $flags = 11, ?string $encoding = 'UTF-8'): string;

// Markup Generation
Html::tag(string $name, array|dict $attributes = [], string $content = ''): string;
Html::attributes(array|dict $attributes = []): string;

// Dynamic Macros
Html::macro(string $name, callable $closure): bool;
```

---

## `Url` Facade API (`Url` / `URL`)

```php
// Component Parsing & Query Strings
Url::parseUrl(string $url, int|string $component = -1): mixed;
Url::parseQuery(string $input): dict; // Parses full URL or bare query string into dictionary
Url::httpBuildQuery(mixed $data, string $numericPrefix = '', string $argSeparator = '&', int $encodingType = 1): string;

// Base64 Encoding & Decoding
Url::base64Encode(string $string): string;
Url::base64Decode(string $string, bool $strict = false): string|false;
Url::base64UrlEncode(string $string): string;
Url::base64UrlDecode(string $string): string|false;

// URL Encoding (RFC 1738 & RFC 3986)
Url::encode(string $string): string;       // Spaces encoded as '+'
Url::decode(string $string): string;
Url::rawUrlEncode(string $string): string; // Spaces encoded as '%20'
Url::rawUrlDecode(string $string): string;

// Route & App URL Generation
Url::to(string $path = ''): string;
Url::route(string $name, array $params = []): string;
Url::current(): string;
Url::full(): string;

// Dynamic Macros
Url::macro(string $name, callable $closure): bool;
```

---

## `Ip` Facade API

```php
Ip::toLong(string $ip): int;       // Converts IPv4 dotted string to integer (throws on invalid)
Ip::fromLong(int $long): string;   // Converts integer to IPv4 dotted string
```

---

## `Compress` Facade API

```php
Compress::zlib(string $data, int $level = -1): string;   // ZLIB compression (wraps gzcompress)
Compress::unzlib(string $data): string;                  // ZLIB decompression (wraps gzuncompress, throws on corrupt)
Compress::gzip(string $data, int $level = -1): string;   // GZIP compression (wraps gzencode)
Compress::gunzip(string $data): string;                  // GZIP decompression (wraps gzdecode, throws on corrupt)
```

---

## `Time` Facade API (Date & Time Services)

```php
// Factory Constructors
Time::now(?string $tz = null): Time;
Time::today(?string $tz = null): Time;
Time::yesterday(?string $tz = null): Time;
Time::tomorrow(?string $tz = null): Time;
Time::parse(string $time, ?string $tz = null): Time;
Time::create(int $year = 0, int $month = 1, int $day = 1, int $hour = 0, int $minute = 0, int $second = 0, ?string $tz = null): Time;
Time::createFromDate(int $year, int $month, int $day, ?string $tz = null): Time;
Time::createFromTime(int $hour, int $minute, int $second, ?string $tz = null): Time;
Time::createFromTimestamp(int $timestamp, ?string $tz = null): Time;
Time::createFromTimestampMs(int $timestampMs, ?string $tz = null): Time;
Time::createFromFormat(string $format, string $time, ?string $tz = null): Time;
Time::maxValue(): Time;
Time::minValue(): Time;

// Mock Test Time
Time::setTestNow(mixed $testNow = null): void;
Time::getTestNow(): ?Time;
Time::hasTestNow(): bool;

// Class Constants
Time::SUNDAY;             // 0
Time::MONDAY;             // 1
Time::TUESDAY;            // 2
Time::WEDNESDAY;          // 3
Time::THURSDAY;           // 4
Time::FRIDAY;             // 5
Time::SATURDAY;           // 6
Time::DAYS_PER_WEEK;      // 7
Time::HOURS_PER_DAY;      // 24
Time::MINUTES_PER_HOUR;   // 60
Time::SECONDS_PER_MINUTE; // 60

// Instance Output & Formatting
$date->toDateTimeString(): string;
$date->toDateString(): string;
$date->toDatString(): string; // Alias for toDateString()
$date->toTimeString(): string;
$date->toISOString(): string;
$date->toIso8601String(): string;
$date->toAtomString(): string;
$date->toRfc2822String(): string;
$date->toRfc822String(): string;
$date->toRfc3339String(): string;
$date->toFormattedDateString(): string;
$date->toDayDateTimeString(): string;
$date->format(string $format): string;
$date->toJson(): string;

// Static Shortcut Invocations (Evaluated on Time::now())
Time::toDateTimeString(): string;
Time::toDateString(): string;
Time::toDatString(): string;
Time::toTimeString(): string;
Time::toISOString(): string;
Time::format(string $format): string;

// Public Properties & Queries
$date->year: int;
$date->month: int;
$date->day: int;
$date->hour: int;
$date->minute: int;
$date->second: int;
$date->micro: int;
$date->timestamp: int;
$date->timezone: string;
$date->dayOfWeek: int;
$date->dayOfYear: int;
$date->weekOfYear: int;
$date->quarter: int;
$date->daysInMonth: int;
$date->age(): int;
$date->getTimestamp(): int;
$date->getTimestampMs(): int;
$date->getTimezone(): string;

// Fluent Addition & Subtraction (Mutates & Returns $this)
$date->addDay(int $count = 1): Time;
$date->addDays(int $count): Time;
$date->subDay(int $count = 1): Time;
$date->subDays(int $count): Time;
$date->addWeek(int $count = 1): Time;
$date->addWeeks(int $count): Time;
$date->subWeek(int $count = 1): Time;
$date->subWeeks(int $count): Time;
$date->addMonth(int $count = 1): Time;
$date->addMonths(int $count): Time;
$date->subMonth(int $count = 1): Time;
$date->subMonths(int $count): Time;
$date->addQuarter(int $count = 1): Time;
$date->addQuarters(int $count): Time;
$date->subQuarter(int $count = 1): Time;
$date->subQuarters(int $count): Time;
$date->addYear(int $count = 1): Time;
$date->addYears(int $count): Time;
$date->subYear(int $count = 1): Time;
$date->subYears(int $count): Time;
$date->addHour(int $count = 1): Time;
$date->addHours(int $count): Time;
$date->subHour(int $count = 1): Time;
$date->subHours(int $count): Time;
$date->addMinute(int $count = 1): Time;
$date->addMinutes(int $count): Time;
$date->subMinute(int $count = 1): Time;
$date->subMinutes(int $count): Time;
$date->addSecond(int $count = 1): Time;
$date->addSeconds(int $count): Time;
$date->subSecond(int $count = 1): Time;
$date->subSeconds(int $count): Time;
$date->addUnit(string $unit, int $count = 1): Time;
$date->subUnit(string $unit, int $count = 1): Time;

// Period Boundaries
$date->startOfDay(): Time;
$date->endOfDay(): Time;
$date->startOfMonth(): Time;
$date->firstOfMonth(): Time;
$date->endOfMonth(): Time;
$date->lastOfMonth(): Time;
$date->startOfWeek(): Time;
$date->endOfWeek(): Time;
$date->startOfQuarter(): Time;
$date->endOfQuarter(): Time;
$date->startOfYear(): Time;
$date->endOfYear(): Time;
$date->lastOfYear(): Time;
$date->startOfHour(): Time;
$date->endOfHour(): Time;
$date->startOfMinute(): Time;
$date->endOfMinute(): Time;

// Modifiers & Setters
$date->setYear(int $year): Time;
$date->setMonth(int $month): Time;
$date->setDay(int $day): Time;
$date->setHour(int $hour): Time;
$date->setMinute(int $minute): Time;
$date->setSecond(int $second): Time;
$date->setTime(int $hour, int $minute, int $second = 0, int $micro = 0): Time;
$date->setDate(int $year, int $month, int $day): Time;
$date->setTimezone(string $tz): Time;
$date->next(?int $dayOfWeek = null): Time;
$date->previous(?int $dayOfWeek = null): Time;
$date->copy(): Time;
$date->clone(): Time;

// Comparisons & Boolean Checks
$date->isToday(): bool;
$date->isYesterday(): bool;
$date->isTomorrow(): bool;
$date->isFuture(): bool;
$date->isPast(): bool;
$date->isCurrentYear(): bool;
$date->isCurrentMonth(): bool;
$date->isWeekend(): bool;
$date->isWeekday(): bool;
$date->isMonday(): bool;
$date->isTuesday(): bool;
$date->isWednesday(): bool;
$date->isThursday(): bool;
$date->isFriday(): bool;
$date->isSaturday(): bool;
$date->isSunday(): bool;
$date->isLeapYear(): bool;
$date->isStartOfDay(): bool;
$date->isEndOfDay(): bool;
$date->isSameDay(mixed $other): bool;
$date->isSameMonth(mixed $other): bool;
$date->isSameYear(mixed $other): bool;
$date->isBirthday(mixed $other = null): bool;
$date->eq(mixed $other): bool;
$date->equalTo(mixed $other): bool;
$date->ne(mixed $other): bool;
$date->notEqualTo(mixed $other): bool;
$date->gt(mixed $other): bool;
$date->greaterThan(mixed $other): bool;
$date->gte(mixed $other): bool;
$date->greaterThanOrEqualTo(mixed $other): bool;
$date->lt(mixed $other): bool;
$date->lessThan(mixed $other): bool;
$date->lte(mixed $other): bool;
$date->lessThanOrEqualTo(mixed $other): bool;
$date->between(mixed $start, mixed $end, bool $equal = true): bool;

// Differences & Humans
$date->diffInSeconds(mixed $other = null, bool $abs = true): int;
$date->diffInMinutes(mixed $other = null, bool $abs = true): int;
$date->diffInHours(mixed $other = null, bool $abs = true): int;
$date->diffInDays(mixed $other = null, bool $abs = true): int;
$date->diffInWeeks(mixed $other = null, bool $abs = true): int;
$date->diffInMonths(mixed $other = null, bool $abs = true): int;
$date->diffInQuarters(mixed $other = null, bool $abs = true): int;
$date->diffInYears(mixed $other = null, bool $abs = true): int;
$date->diffForHumans(mixed $other = null): string;

// Min / Max Helpers
$date->min(mixed $other = null): Time;
$date->max(mixed $other = null): Time;

// Dynamic Macros
Time::macro(string $name, callable $closure): bool;
```

---

## Tier 2 Native Helper Classes API (`Cache`, `Log`)

Imported via `import "./Cache.rs";` or `import "./Log.rs";`. These helper classes are not built into the core language syntax to avoid language bloat, but provide framework-level performance using thread-safe Rust stores with zero garbage collection overhead.

### `Cache` Helper Class API

```php
// In-Memory L1 Cache with Optional TTL
Cache::get(string $key, mixed $default = null): mixed;
Cache::set(string $key, mixed $value, int $ttlSeconds = 0): bool;
Cache::has(string $key): bool;
Cache::delete(string $key): bool;
Cache::increment(string $key, int $step = 1): int;
Cache::decrement(string $key, int $step = 1): int;
Cache::all(): dict;
Cache::keys(): list;
Cache::count(): int;
Cache::clear(): void;
```

### `Log` Helper Class API

```php
// Structured High-Speed In-Memory & File Logging
Log::info(string $message, array|dict $context = []): void;
Log::warn(string $message, array|dict $context = []): void;
Log::error(string $message, array|dict $context = []): void;
Log::debug(string $message, array|dict $context = []): void;
Log::history(): list; // Returns list of log entries with timestamps, levels, and messages
Log::clear(): void;
```

---

### `Uniq` Helper Class API

The `Uniq` static facade provides high-performance unique identifier generation, including PHP-compatible `uniqid()` (down to microsecond precision with Combined LCG entropy) and modern RFC-compliant UUIDs (v4, v5, v7, v3):

```php
// PHP-compatible microsecond unique ID (Default: 13-char hex; With entropy: 23-char string)
Uniq::uniqid(?string $prefix = "", bool $more_entropy = false): string;
Uniq::id(?string $prefix = "", bool $more_entropy = false): string; // Alias

// Modern RFC 4122 / RFC 9562 UUID Generators
Uniq::uuid4(): string;                                             // Version 4 random UUID
Uniq::v4(): string;                                                // Alias for uuid4
Uniq::uuid(): string;                                              // Alias for uuid4

Uniq::uuid5(string $namespace, string $name): string;              // Version 5 SHA-1 name-based UUID ('dns', 'url', 'oid', 'x500', 'nil', or custom UUID)
Uniq::v5(string $namespace, string $name): string;                 // Alias for uuid5

Uniq::uuid7(?int $timestamp_ms = null): string;                    // Version 7 Unix Epoch millisecond time-ordered UUID
Uniq::v7(?int $timestamp_ms = null): string;                       // Alias for uuid7

Uniq::uuid3(string $namespace, string $name): string;              // Version 3 MD5 name-based UUID
Uniq::v3(string $namespace, string $name): string;                 // Alias for uuid3

// Inspection, validation, and parsing helpers
Uniq::isValid(string $uuid): bool;                                 // Returns true if valid 36-char hyphenated or 32-char hex UUID
Uniq::version(string $uuid): ?int;                                 // Returns UUID version integer (1..8) or null
Uniq::parse(string $uuid): dict;                                   // Parses UUID into dict ['hex', 'version', 'variant', 'timestamp_ms']
Uniq::nil(): string;                                               // Returns "00000000-0000-0000-0000-000000000000"
Uniq::max(): string;                                               // Returns "ffffffff-ffff-ffff-ffff-ffffffffffff"
```



