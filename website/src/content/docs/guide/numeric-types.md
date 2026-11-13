---
title: "Numeric Types"
description: "Complete reference for int, float, Int::, and Float:: — RHP's 64-bit numeric primitives and supporting utility facades."
section: "Language Guide"
order: 2
---

## Overview

RHP features a streamlined, high-performance numeric model directly lowered to bare-metal Rust primitives:

| Item | Category | Native Storage | Notes |
| :--- | :--- | :--- | :--- |
| `int` | Primitive keyword | **`i64`** | Signed 64-bit integer. Base whole number type. Strict lowercase. |
| `float` | Primitive keyword | **`f64`** | IEEE-754 64-bit double precision float. Base decimal type. Strict lowercase. |
| `Int` | Static utility facade | — | Utility class for random generation, variadic min/max, and comparisons. Cannot be instantiated. |
| `Float` | Static utility facade | — | Utility class for random generation, variadic min/max, formatting, and epsilon comparisons. Cannot be instantiated. |

> [!IMPORTANT]
> **Native 64-Bit Performance Without Boxing:**
> Both `int` and `float` are lowered directly to Rust `i64` and `f64`. At the bytecode engine level, they are stored inline (unboxed) in register cells (`PhpValue::Int(i64)` and `PhpValue::Float(f64)`). They incur **zero heap allocations** and execute raw CPU arithmetic instructions.

---

## `int` — 64-Bit Signed Integer

`int` is a reserved lowercase keyword representing a **signed 64-bit integer** (`i64`), with a range from $-9,223,372,036,854,775,808$ to $9,223,372,036,854,775,807$.

```php
int $count    = 42;
int $negative = -1000;
int $hex      = 0xFF;         // Hex literal (255)
int $bin      = 0b1010_0101;  // Binary literal (165)
int $oct      = 0o755;        // Octal literal (493)
int $bigId    = 9_223_372_036_854_775_807; // Maximum 64-bit integer
```

### High-Performance Arithmetic Operators

Always use native arithmetic operators for mathematical calculations. They compile directly into optimized IR bytecode opcodes (`OpCode::Binary`, `OpCode::Unary`) that run at hardware speed without method dispatch overhead:

```php
int $a = 10;
int $b = 3;

int $sum  = $a + $b;   // 13
int $diff = $a - $b;   // 7
int $prod = $a * $b;   // 30
int $div  = $a / $b;   // 3  (integer division — truncates)
int $mod  = $a % $b;   // 1
int $pow  = $a ** $b;  // 1000
```

### Conversions & Fluent Instance Methods

Numeric variables support immutable fluent methods for conversions and sign inspection:

```php
int $val = -42;

int $magnitude = $val->abs();       // 42
string $str    = $val->toString();  // "-42"
float $decimal = $val->toFloat();   // -42.0

// Explicit functional type casting:
int $parsed    = int("42");         // 42
int $fromBool  = int(true);         // 1
int $fromFloat = int(3.99);         // 3 (truncates)
```

> [!NOTE]
> **No Method Calls on Bare Literals:**
> The compiler restricts calling member methods directly on raw numeric literals (e.g. `0->abs()` or `10->toString()`). Always invoke methods on variables or parenthesized expressions:
> ```php
> // ❌ Syntax error:
> // 42->abs();
> 
> // ✅ Valid:
> int $n = 42;
> $n->abs();
> ```

---

## `float` — 64-Bit Floating-Point

`float` is a reserved lowercase keyword representing an **IEEE-754 64-bit double precision float** (`f64`).

```php
float $rate    = 9.99;
float $pi      = 3.141592653589793;
float $exp     = 1.5e10;    // Scientific notation
float $precise = 0.000001;
```

### Arithmetic & Decimal Formatting

```php
float $a = 10.5;
float $b = 2.5;

float $sum  = $a + $b;   // 13.0
float $diff = $a - $b;   // 8.0
float $prod = $a * $b;   // 26.25
float $div  = $a / $b;   // 4.2
```

### Fluent Formatting & Rounding Methods

Float variables support clean method chaining for financial and scientific presentation:

```php
float $price = 1234.5678;

// Fixed decimal representation
string $formatted = $price->toDecimal(2); // "1234.57"

// Rounding operations
float $rounded = $price->round(2); // 1234.57
float $floored = $price->floor();   // 1234.0
float $ceiled  = $price->ceil();    // 1235.0

// Conversions
int $truncated = $price->toInt();      // 1234
string $asStr  = $price->toString();  // "1234.5678"
```

---

## `Int` — Static Utility Facade

`Int` is a **static utility class** (PascalCase identifier), not a type keyword. It cannot be instantiated with `new Int()`.

It provides static utility methods that do not operate on an existing single variable:

```php
// 1. CSPRNG Cryptographically Secure Random Integer
int $dice = Int::random(1, 6);
int $token = Int::random(100000, 999999);

// 2. Variadic Min and Max
int $lowest  = Int::min(10, 20, 5, 40); // 5
int $highest = Int::max(10, 20, 5, 40); // 40

// 3. Intentionally Loose Type Comparison
bool $match = Int::compare(42, "42"); // true
bool $flag  = Int::compare(1, true);  // true
```

> [!CAUTION]
> `new Int()` is a **fatal compile-time error**:
> ```text
> Fatal Error: Cannot instantiate static facade class 'Int'. Use static methods directly.
> ```

---

## `Float` — Static Utility Facade

`Float` is a **static utility class** (PascalCase identifier), not a type keyword. Like `Int`, it can never be instantiated with `new Float()`.

```php
// 1. CSPRNG Random Float in Range [min, max]
float $chance = Float::random(0.0, 1.0);

// 2. Variadic Min and Max
float $minVal = Float::min(1.5, 2.8, 0.4); // 0.4
float $maxVal = Float::max(1.5, 2.8, 0.4); // 2.8

// 3. Loose & Epsilon Tolerance Comparisons
bool $close = Float::compare(1.0001, 1.0002, 0.001); // true (within epsilon)
bool $loose = Float::compare(3.14, "3.14");           // true

// 4. Number Formatting with Thousands Separator
string $display = Float::format(1234567.89, 2, '.', ','); // "1,234,567.89"
```

---

## Numeric Literal Edge Cases & Strict Validation

RHP's lexer enforces rigorous syntax verification on numeric literals to prevent ambiguities with member access (`->`) or range operators (`..`):

### 1. No Trailing Separators or Decimal Points
Numeric literals cannot end with an underscore (`_`) or a bare decimal point (`.`):
```php
// ❌ Rejected by compiler:
// int $a = 10_;   // Error: Numeric literal cannot end with a separator or decimal point.
// float $b = 10.; // Error: Numeric literal cannot end with a separator or decimal point.

// ✅ Valid:
int $a = 10;
float $b = 10.0;
int $c = 1_000_000;
```

### 2. Leading Dots Disallowed
Omitting the leading zero in fractional numbers is strictly prohibited. You must write `0.5`, not `.5`:
```php
// ❌ Rejected:
// float $half = .5;

// ✅ Valid:
float $half = 0.5;
```
This eliminates parsing collisions and enforces unambiguous, readable syntax.

### 3. Incomplete Base Prefixes & Exponents
Base prefixes (`0x`, `0b`, `0o`) and scientific exponent markers (`1e`, `2E`) must be followed by valid digits:
```php
// ❌ Rejected:
// $badHex = 0x;  // SyntaxError: invalid syntax.
// $badExp = 1e;  // SyntaxError: invalid syntax.

// ✅ Valid:
$hex = 0xFF;
$bin = 0b1010;
$sci = 1e6;
```

---

## Summary: Idiomatic Numeric Style

| Task | Recommended Syntax | Discouraged / Obsolete Syntax |
| :--- | :--- | :--- |
| **Integer declaration** | `int $id = 42;` | `Int::i64(42)` (obsolete wrapper) |
| **Float declaration** | `float $rate = 3.14;` | `Float::f64(3.14)` (obsolete wrapper) |
| **Basic arithmetic** | `$a + $b`, `$a * $b`, `$a / $b` | `$a->add($b)` (unnecessary method overhead) |
| **Absolute value** | `$val->abs()` | `Int::abs($val)` |
| **Decimal formatting** | `$price->toDecimal(2)` | `Float::format(...)` for simple rounding |
| **Random numbers** | `Int::random($min, $max)` | — |
| **Epsilon comparison** | `Float::compare($a, $b, $eps)` | — |
