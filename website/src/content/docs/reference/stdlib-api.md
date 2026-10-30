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
Arr::first(mixed $array): mixed;                               // Returns first element
Arr::last(mixed $array): mixed;                                // Returns last element
Arr::isAssoc(mixed $array): bool;                              // Checks if collection is associative dict
Arr::isList(mixed $array): bool;                               // Checks if collection is sequential list

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
Arr::sort(mixed $items): list;

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
$list->sort(?callable $comparator = null): list;
$list->sortBy(string|callable $key): list;
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
$dict->sortKeys(): dict;                                        // Sorts keys ascending (natural ordering)
$dict->sortKeysDesc(): dict;                                    // Sorts keys descending
$dict->sort(?callable $comparator = null): dict;                // Sorts by values ascending
$dict->sortBy(string|callable $key): dict;                      // Sorts by custom closure or subkey
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

// Set-Like Differences & Intersections
$dict->diff(dict|array $other): dict;                           // Entries whose values are not in $other
$dict->diffKeys(dict|array $other): dict;                       // Entries whose keys are not in $other
$dict->intersect(dict|array $other): dict;                      // Entries whose values exist in $other
$dict->intersectKeys(dict|array $other): dict;                  // Entries whose keys exist in $other

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
$set->diff(set|array $other): set;      // Relative complement (elements in $this not in $other)
$set->intersect(set|array $other): set; // Intersection (elements present in both)
$set->union(set|array $other): set;     // Set union (elements present in either)

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
Str::repeat(string $text, int $times): string;
Str::reverse(string $text): string;
Str::random(int $length = 16): string;
Str::join(string $glue, array $pieces): string;
Str::split(string $delimiter, string $string): list;
Str::replace(string $search, string $replace, string $subject): string;

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
File::mimeType(string $path): string|false;         // Sniff magic bytes & detect MIME type (like Laravel)
File::guessExtension(string $pathOrMime): ?string;  // Guess extension from MIME type or file content
File::extension(string $path): string;              // Extract file extension from path
File::type(string $path): string;                   // Get file type: "file", "dir", or "link"
File::size(string $path): int;                      // Get file size in bytes
File::name(string $path): string;                   // Get filename without extension
File::basename(string $path): string;               // Get trailing name component
File::dirname(string $path): string;                // Get directory name
File::isFile(string $path): bool;                   // Check if path is a regular file
File::isDirectory(string $path): bool;              // Check if path is a directory
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

---

## `AFile` Facade API (Asynchronous Tokio Filesystem)

```php
AFile::read(string $path): Future;                  // Resolves to string (file contents)
AFile::write(string $path, string $content): Future; // Resolves to bool
AFile::exists(string $path): Future;                 // Resolves to bool
AFile::delete(string $path): Future;                 // Resolves to bool
AFile::append(string $path, string $content): Future;// Resolves to bool
AFile::size(string $path): Future;                   // Resolves to int (byte size)
AFile::mimeType(string $path): Future;               // Resolves to string|false (detected MIME type)
AFile::guessExtension(string $path): Future;         // Resolves to ?string (guessed extension)
AFile::extension(string $path): Future;              // Resolves to string (file extension)
AFile::type(string $path): Future;                   // Resolves to string ("file" | "dir")
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

## `Carbon` Facade API (Date & Time Services)

```php
// Factory Constructors
Carbon::now(?string $tz = null): Carbon;
Carbon::today(?string $tz = null): Carbon;
Carbon::yesterday(?string $tz = null): Carbon;
Carbon::tomorrow(?string $tz = null): Carbon;
Carbon::parse(string $time, ?string $tz = null): Carbon;
Carbon::create(int $year = 0, int $month = 1, int $day = 1, int $hour = 0, int $minute = 0, int $second = 0, ?string $tz = null): Carbon;
Carbon::createFromDate(int $year, int $month, int $day, ?string $tz = null): Carbon;
Carbon::createFromTime(int $hour, int $minute, int $second, ?string $tz = null): Carbon;
Carbon::createFromTimestamp(int $timestamp, ?string $tz = null): Carbon;
Carbon::createFromTimestampMs(int $timestampMs, ?string $tz = null): Carbon;
Carbon::createFromFormat(string $format, string $time, ?string $tz = null): Carbon;
Carbon::maxValue(): Carbon;
Carbon::minValue(): Carbon;

// Mock Test Time
Carbon::setTestNow(mixed $testNow = null): void;
Carbon::getTestNow(): ?Carbon;
Carbon::hasTestNow(): bool;

// Class Constants
Carbon::SUNDAY;             // 0
Carbon::MONDAY;             // 1
Carbon::TUESDAY;            // 2
Carbon::WEDNESDAY;          // 3
Carbon::THURSDAY;           // 4
Carbon::FRIDAY;             // 5
Carbon::SATURDAY;           // 6
Carbon::DAYS_PER_WEEK;      // 7
Carbon::HOURS_PER_DAY;      // 24
Carbon::MINUTES_PER_HOUR;   // 60
Carbon::SECONDS_PER_MINUTE; // 60

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

// Static Shortcut Invocations (Evaluated on Carbon::now())
Carbon::toDateTimeString(): string;
Carbon::toDateString(): string;
Carbon::toDatString(): string;
Carbon::toTimeString(): string;
Carbon::toISOString(): string;
Carbon::format(string $format): string;

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
$date->addDay(int $count = 1): Carbon;
$date->addDays(int $count): Carbon;
$date->subDay(int $count = 1): Carbon;
$date->subDays(int $count): Carbon;
$date->addWeek(int $count = 1): Carbon;
$date->addWeeks(int $count): Carbon;
$date->subWeek(int $count = 1): Carbon;
$date->subWeeks(int $count): Carbon;
$date->addMonth(int $count = 1): Carbon;
$date->addMonths(int $count): Carbon;
$date->subMonth(int $count = 1): Carbon;
$date->subMonths(int $count): Carbon;
$date->addQuarter(int $count = 1): Carbon;
$date->addQuarters(int $count): Carbon;
$date->subQuarter(int $count = 1): Carbon;
$date->subQuarters(int $count): Carbon;
$date->addYear(int $count = 1): Carbon;
$date->addYears(int $count): Carbon;
$date->subYear(int $count = 1): Carbon;
$date->subYears(int $count): Carbon;
$date->addHour(int $count = 1): Carbon;
$date->addHours(int $count): Carbon;
$date->subHour(int $count = 1): Carbon;
$date->subHours(int $count): Carbon;
$date->addMinute(int $count = 1): Carbon;
$date->addMinutes(int $count): Carbon;
$date->subMinute(int $count = 1): Carbon;
$date->subMinutes(int $count): Carbon;
$date->addSecond(int $count = 1): Carbon;
$date->addSeconds(int $count): Carbon;
$date->subSecond(int $count = 1): Carbon;
$date->subSeconds(int $count): Carbon;
$date->addUnit(string $unit, int $count = 1): Carbon;
$date->subUnit(string $unit, int $count = 1): Carbon;

// Period Boundaries
$date->startOfDay(): Carbon;
$date->endOfDay(): Carbon;
$date->startOfMonth(): Carbon;
$date->firstOfMonth(): Carbon;
$date->endOfMonth(): Carbon;
$date->lastOfMonth(): Carbon;
$date->startOfWeek(): Carbon;
$date->endOfWeek(): Carbon;
$date->startOfQuarter(): Carbon;
$date->endOfQuarter(): Carbon;
$date->startOfYear(): Carbon;
$date->endOfYear(): Carbon;
$date->lastOfYear(): Carbon;
$date->startOfHour(): Carbon;
$date->endOfHour(): Carbon;
$date->startOfMinute(): Carbon;
$date->endOfMinute(): Carbon;

// Modifiers & Setters
$date->setYear(int $year): Carbon;
$date->setMonth(int $month): Carbon;
$date->setDay(int $day): Carbon;
$date->setHour(int $hour): Carbon;
$date->setMinute(int $minute): Carbon;
$date->setSecond(int $second): Carbon;
$date->setTime(int $hour, int $minute, int $second = 0, int $micro = 0): Carbon;
$date->setDate(int $year, int $month, int $day): Carbon;
$date->setTimezone(string $tz): Carbon;
$date->next(?int $dayOfWeek = null): Carbon;
$date->previous(?int $dayOfWeek = null): Carbon;
$date->copy(): Carbon;
$date->clone(): Carbon;

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
$date->min(mixed $other = null): Carbon;
$date->max(mixed $other = null): Carbon;

// Dynamic Macros
Carbon::macro(string $name, callable $closure): bool;
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


