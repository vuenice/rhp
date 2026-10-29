---
title: "Control Flow"
description: "Conditionals, pattern matching, loops, and iteration constructs in RHP."
section: "Language Guide"
order: 3
---

## Conditionals: `if`, `elseif`, `else`

Conditionals in RHP support standard boolean evaluations:

```php
int $statusCode = 200;

if ($statusCode === 200) {
    print("Success: Request completed");
} elseif ($statusCode >= 400 && $statusCode < 500) {
    print("Client Error");
} else {
    print("Server Error");
}
```

### Control Flow Normalization (`elseif` vs `else if`)

In RHP, chained conditionals are standardized strictly on the monolithic keyword **`elseif`**:
* **Standard Monolithic Keyword:** Always use `elseif` for secondary conditional branches.
* **Prohibition of Two-Word `else if`:** The separated two-word sequence `else if` is rejected at compile time to eliminate dangling-else ambiguities and ensure code formatting consistency.

```php
// ✅ Valid:
if ($status === 'active') {
    $code = 1;
} elseif ($status === 'pending') {
    $code = 2;
} else {
    $code = 0;
}

// ❌ Rejected by compiler:
// if ($status === 'active') {
//     $code = 1;
// } else if ($status === 'pending') { // ParseError
//     $code = 2;
// }
```

When `else if` is encountered, the compiler halts with an explicit error:
```text
SyntaxError: Use 'elseif' instead of 'else if'.
```

---

## Pattern Matching with `match` Expressions

RHP supports first-class `match` expressions. Unlike `switch`, `match` returns a value, enforces strict `===` type checking, and does not require `break` statements:

```php
int $roleId = 1;

string $roleTitle = match ($roleId) {
    1 => 'Administrator',
    2 => 'Editor',
    3 => 'Subscriber',
    default => 'Guest',
};

print("User Role: " . $roleTitle);
```

---

## Iteration: `foreach` Over Collections and Objects

The `foreach` construct iterates smoothly over all RHP collection subtypes (`list`, `dict`, `set`, `hashMap`), plain objects, and objects implementing PHP's `Traversable` interface (`IteratorAggregate` and `Iterator`).

### 1. Subtyped Collections (`list`, `dict`, `set`, `hashMap`)

For dictionaries (`Arr::dict`), `foreach` drives a zero-allocation streaming iterator (`ActiveIterator::Dict`) that accesses entries directly via internal index offsets without allocating temporary key/value arrays, strictly maintaining insertion order:

```php
// Iterating over a list (sequential 0-based index)
list $users = Arr::list(['Alice', 'Bob', 'Charlie']);
foreach ($users as $index => $name) {
    Sys::print("[{}]: {}", $index, $name);
}

// Iterating over an ordered dictionary (strict insertion order preserved)
dict $meta = Arr::dict([
    'region'  => 'us-east-1',
    'active'  => true,
    'cluster' => 'k8s-prod',
]);
foreach ($meta as $key => $value) {
    Sys::print("{} => {}", $key, (string)$value);
}
```

### 2. Object Iteration via `IteratorAggregate` & `Iterator`

RHP fully supports iterating over object instances using standard PHP iterator contracts:

#### `IteratorAggregate`
Objects implementing `IteratorAggregate` define a `getIterator()` method that returns a `Traversable` (such as `ArrayIterator` or another iterator):

```php
class TeamList implements IteratorAggregate {
    public function __construct(private array $members) {}

    public function getIterator(): Traversable {
        return new ArrayIterator($this->members);
    }
}

$team = new TeamList(['Alice', 'Bob', 'Charlie']);
foreach ($team as $key => $member) {
    print("Member {$key}: {$member}");
}
```

When `getIterator()` yields an `ArrayIterator` wrapping a `list` or `dict`, RHP applies an automated **fast-path optimization**: the VM unwraps the inner vector or map directly into a cache-friendly register stream, bypassing per-element method call overhead entirely.

#### Step-by-Step `Iterator`
For custom lazy generators or cursor-driven iterators, implement `Iterator` with step-by-step methods:

```php
class NumberRange implements Iterator {
    private int $current;

    public function __construct(private int $start, private int $end) {
        $this->current = $start;
    }

    public function rewind(): void {
        $this->current = $this->start;
    }

    public function valid(): bool {
        return $this->current <= $this->end;
    }

    public function current(): mixed {
        return $this->current * 10;
    }

    public function key(): mixed {
        return $this->current - $this->start;
    }

    public function next(): void {
        $this->current++;
    }
}

$range = new NumberRange(1, 5);
foreach ($range as $step => $value) {
    print("Step {$step}: {$value}");
}
```

The RHP VM safely executes `rewind()`, `valid()`, `current()`, `key()`, and `next()` synchronously within isolated VM call frames, preserving iterator state and allowing nested loops.

### 3. Plain Object Property Iteration

Iterating over standard class instances yields all visible and accessible object properties as `$key => $value` pairs in declaration order:

```php
class UserDTO {
    public string $name = "Alice";
    public string $role = "Engineer";
    public int $level = 7;
}

$dto = new UserDTO();
foreach ($dto as $property => $value) {
    print("{$property} = {$value}");
}
// Outputs:
// name = Alice
// role = Engineer
// level = 7
```

### 4. Laravel Collections, Paginators, & Query Results

RHP provides built-in native compatibility with common Laravel data structures:

```php
// 1. Eloquent / DB query results
$users = DB::table('users')->where('active', true)->get();
foreach ($users as $user) {
    print("User #{$user->id}");
}

// 2. LengthAwarePaginator
$paginator = DB::table('orders')->paginate(15);
foreach ($paginator as $order) {
    print("Order #{$order->id}");
}
print("Total orders: " . (string)$paginator->total());
print("Current page: " . (string)$paginator->currentPage());

// 3. Laravel Collections via collect()
$collection = collect(['apple', 'banana', 'cherry']);
foreach ($collection as $index => $fruit) {
    print("{$index}: {$fruit}");
}
```

### 5. `iterable` Parameter & Return Type Contracts

Functions and methods can declare `iterable` or `Traversable` type hints, accepting plain arrays, subtyped collections, or custom iterator objects interchangeably:

```php
function processItems(iterable $items): int {
    $count = 0;
    foreach ($items as $item) {
        $count++;
    }
    return $count;
}

processItems([1, 2, 3]);                           // Plain array
processItems(new ArrayIterator(['a', 'b']));       // ArrayIterator
processItems(collect([10, 20, 30]));               // Laravel Collection
```

Objects implementing `Iterator` or `IteratorAggregate` also satisfy `instanceof Traversable`, matching PHP 8 inheritance hierarchies.

### 6. Loop Control: `break` and `continue`

Both `break` and `continue` function cleanly within object and collection `foreach` loops, safely cleaning up iterator frames and register states:

```php
foreach ($team as $member) {
    if ($member === 'Guest') {
        continue;
    }
    if ($member === 'Admin') {
        break;
    }
    print("Processing {$member}");
}
```

---

## Loops: `for` and `while`

```php
// Standard for loop
for (int $i = 0; $i < 5; $i++) {
    if ($i === 2) continue;
    print("Step: " . (string)$i);
}

// While loop
int $attempts = 3;
while ($attempts > 0) {
    $attempts--;
}
```
