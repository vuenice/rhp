---
title: "Functions and Closures"
description: "Define named functions, short arrow closures, typed parameters, and learn about reference restrictions in RHP."
section: "Language Guide"
order: 4
---

## Named Functions

Functions require explicit type signatures on parameters and return values:

```php
<?rhp

function calculateDiscount(float $subtotal, float $discountRate = 0.10): float {
    return $subtotal - ($subtotal * $discountRate);
}

float $finalPrice = calculateDiscount(100.0, 0.15);
Sys::print("Final price: " . (string)$finalPrice);
```

---

## Short Arrow Closures (`fn`)

Arrow functions provide concise syntax for higher-order collection transformations. Variables from the enclosing lexical scope are captured by value:

```php
<?rhp

float $taxRate = 0.08;

list $prices = Arr::list([10.0, 20.0, 30.0]);
list $taxed = $prices->map(fn(float $price) => $price * (1 + $taxRate));
```

---

## Reference Restrictions: Pass-By-Reference Forbidden

To ensure absolute memory safety and eliminate cyclic reference leaks across multi-threaded workers, **pass-by-reference (`&$var`) is strictly forbidden** at the parser and type-checker levels:

```php
<?rhp

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
