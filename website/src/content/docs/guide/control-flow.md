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

## Iteration: `foreach` Over Subtyped Collections

The `foreach` construct iterates smoothly over all RHP collection subtypes (`list`, `dict`, `set`, `hashMap`).

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
